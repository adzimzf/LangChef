use crate::db::open_db;
use crate::locale::effective_detail_sql;
use crate::quiz::{cards_missing_meaning, clear_task, issue_task, QuizTask};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Serialize, Deserialize, Clone)]
#[allow(non_snake_case)]
pub struct ReviewItem {
    pub id: String,
    pub content: String,
    pub EF: f64,
    pub I: i64,
    pub R: i64,
    pub state: String,
    pub step: i64,
    pub lapse_count: i64,
    pub next_review: Option<i64>,
    pub language: Option<String>,
    pub detail: Option<String>,
    pub mnemonic: Option<String>,
    pub fsrs_stability: Option<f32>,
    pub fsrs_difficulty: Option<f32>,
    pub difficulty_bg_class: String,
    pub difficulty_label: String,
    pub state_label: String,
    pub next_review_formatted: String,
}

/// Columns read by `row_to_review_item`. `detail` is only returned while it is in the learner's
/// explanation language; a meaning in another language reads as no meaning.
pub fn review_cols(prefix: &str) -> String {
    format!(
        "{p}id, {p}content, {p}EF, {p}I, {p}R, {p}state, {p}step, {p}lapse_count, {p}next_review, {p}language, {d}, {p}mnemonic, {p}fsrs_stability, {p}fsrs_difficulty",
        p = prefix,
        d = effective_detail_sql(prefix)
    )
}

pub fn row_to_review_item(row: &rusqlite::Row) -> rusqlite::Result<ReviewItem> {
    let ef: f64 = row.get(2)?;
    let r: i64 = row.get(4)?;
    let state: String = row.get(5)?;
    let next_review: Option<i64> = row.get(8)?;
    let fsrs_stability: Option<f32> = row.get(12).unwrap_or(None);
    let fsrs_difficulty: Option<f32> = row.get(13).unwrap_or(None);

    let difficulty_bg_class = match fsrs_difficulty {
        Some(d) => compute_difficulty_bg_class_from_fsrs(d),
        None => compute_difficulty_bg_class(ef, r).to_string(),
    };
    let difficulty_label: String = match fsrs_difficulty {
        Some(d) => compute_tag_from_fsrs_difficulty(d).to_string(),
        None => if ef >= 2.7 && r >= 5 { "Easy" } else if ef < 2.0 { "Hard" } else { "Medium" }.to_string(),
    };

    #[allow(non_snake_case)]
    Ok(ReviewItem {
        id: row.get(0)?,
        content: row.get(1)?,
        EF: ef,
        I: row.get(3)?,
        R: r,
        state: state.clone(),
        step: row.get(6)?,
        lapse_count: row.get(7)?,
        next_review,
        language: row.get(9)?,
        detail: row.get(10)?,
        mnemonic: row.get(11)?,
        fsrs_stability,
        fsrs_difficulty,
        difficulty_bg_class,
        difficulty_label,
        state_label: compute_state_label(&state).to_string(),
        next_review_formatted: compute_next_review_formatted(next_review),
    })
}

fn get_fsrs() -> std::sync::MutexGuard<'static, fsrs::FSRS> {
    use std::sync::OnceLock;
    static FSRS_INSTANCE: OnceLock<Mutex<fsrs::FSRS>> = OnceLock::new();
    FSRS_INSTANCE.get_or_init(|| {
        Mutex::new(fsrs::FSRS::new(Some(&[])).expect("Failed to create FSRS instance with default parameters"))
    }).lock().expect("Failed to lock FSRS instance")
}

const DESIRED_RETENTION: f32 = 0.9;

fn memory_state_from_db(stability: Option<f32>, difficulty: Option<f32>) -> Option<fsrs::MemoryState> {
    match (stability, difficulty) {
        (Some(s), Some(d)) => Some(fsrs::MemoryState { stability: s, difficulty: d }),
        _ => None,
    }
}

fn compute_days_elapsed(next_review: Option<i64>) -> u32 {
    match next_review {
        Some(nr) => {
            let now = chrono::Utc::now().timestamp();
            let elapsed_secs = now.saturating_sub(nr);
            if elapsed_secs <= 0 {
                0
            } else {
                ((elapsed_secs as f64) / 86400.0).ceil() as u32
            }
        }
        None => 0,
    }
}

fn fsrs_difficulty_to_ef(difficulty: f32) -> f64 {
    let d = difficulty.clamp(1.0, 10.0);
    let normalized = (d - 1.0) / 9.0;
    (3.0 - normalized * 1.7) as f64
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SRSConfig {
    pub learning_steps: Vec<i64>,
    pub relearning_steps: Vec<i64>,
    pub graduating_interval: i64,
    pub easy_interval: i64,
    pub new_interval: f64,
    pub minimum_lapse_interval: i64,
    pub easy_bonus: f64,
    pub interval_modifier: f64,
    pub maximum_interval: i64,
    pub grace_period_multiplier: f64,
}

impl Default for SRSConfig {
    fn default() -> Self {
        SRSConfig {
            learning_steps: vec![1, 10],
            relearning_steps: vec![10],
            graduating_interval: 1,
            easy_interval: 4,
            new_interval: 0.0,
            minimum_lapse_interval: 1,
            easy_bonus: 1.3,
            interval_modifier: 1.0,
            maximum_interval: 36500,
            grace_period_multiplier: 0.5,
        }
    }
}

#[derive(Debug, Serialize)]
#[allow(non_snake_case)]
pub struct SRSResult {
    pub EF: f64,
    pub I: i64,
    pub R: i64,
    pub state: String,
    pub step: i64,
    pub lapse_count: i64,
    pub next_review: i64,
    pub next_minutes: i64,
    pub lapsed: bool,
    pub graduated: bool,
    pub fsrs_stability: Option<f32>,
    pub fsrs_difficulty: Option<f32>,
}

fn compute_ef(ef: f64, q: i64) -> f64 {
    let qf = q as f64;
    f64::max(1.3, ef + (0.1 - (5.0 - qf) * (0.08 + (5.0 - qf) * 0.02)))
}

fn grace_bonus(next_review: Option<i64>, now: i64, multiplier: f64) -> f64 {
    match next_review {
        Some(nr) => {
            let now_ms = now * 1000;
            let nr_ms = nr * 1000;
            let days_late = f64::max(0.0, (now_ms - nr_ms) as f64 / (24.0 * 60.0 * 60.0 * 1000.0));
            days_late * multiplier
        }
        None => 0.0,
    }
}

pub(crate) fn format_next_review_minutes(minutes: i64) -> String {
    if minutes <= 0 { return "<1m".to_string(); }
    if minutes < 60 { return format!("{}m", minutes); }
    if minutes < 24 * 60 { return format!("{}h", minutes / 60); }
    let days = (minutes as f64 / (24.0 * 60.0)).round() as i64;
    if days < 30 { return format!("{}d", days); }
    if days < 365 { return format!("{}mo", (days as f64 / 30.0).round() as i64); }
    format!("{}y", (days as f64 / 365.0).round() as i64)
}

pub(crate) fn end_of_today() -> i64 {
    let now_local = chrono::Local::now();
    now_local
        .date_naive()
        .and_hms_opt(23, 59, 59)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap()
        .with_timezone(&chrono::Utc)
        .timestamp()
}

fn ef_normalized(ef: f64) -> f64 {
    ((ef - 1.3) / 2.0).clamp(0.0, 1.0)
}

fn compute_difficulty_bg_class(ef: f64, _r: i64) -> &'static str {
    let n = ef_normalized(ef);
    if n < 0.15 { "bg-red-500" }
    else if n < 0.30 { "bg-orange-500" }
    else if n < 0.40 { "bg-amber-500" }
    else if n < 0.50 { "bg-yellow-500" }
    else if n < 0.65 { "bg-lime-500" }
    else if n < 0.80 { "bg-green-500" }
    else { "bg-emerald-500" }
}

fn compute_difficulty_bg_class_from_fsrs(n: f32) -> String {
    if n < 1.5 { "bg-lime-500".to_string() }
    else if n < 4.0 { "bg-green-500".to_string() }
    else if n < 5.0 { "bg-emerald-500".to_string() }
    else if n < 8.0 { "bg-yellow-500".to_string() }
    else if n < 9.0 { "bg-amber-500".to_string() }
    else if n < 9.8 { "bg-orange-500".to_string() }
    else { "bg-red-500".to_string() }
}

fn compute_tag_from_fsrs_difficulty(n: f32) -> &'static str {
    if n < 1.5 { "Trivial" }
    else if n < 4.0 { "Easy" }
    else if n < 5.0 { "Mild" }
    else if n < 8.0 { "Medium" }
    else if n < 9.0 { "Hard" }
    else if n < 9.8 { "Tough" }
    else { "Brutal" }
}

fn compute_state_label(state: &str) -> &'static str {
    match state {
        "new" => "New",
        "learning" => "Learning",
        "review" => "Review",
        "relearning" => "Relearning",
        _ => "New",
    }
}

fn compute_next_review_formatted(next_review: Option<i64>) -> String {
    match next_review {
        None => "Now".to_string(),
        Some(nr) => {
            let now = chrono::Utc::now().timestamp();
            let diff_ms = (nr - now) * 1000;
            let is_past = diff_ms < 0;
            let abs_diff = diff_ms.abs() as f64;
            let minute = 60.0 * 1000.0;
            let hour = 60.0 * minute;
            let day = 24.0 * hour;
            let week = 7.0 * day;
            let month = 30.0 * day;
            let year = 365.0 * day;

            if is_past { return "overdue".to_string(); }

            if abs_diff < minute {
                format!("in {}s", (abs_diff / 1000.0) as i64)
            } else if abs_diff < hour {
                format!("in {}m", (abs_diff / minute) as i64)
            } else if abs_diff < day {
                format!("in {}h", (abs_diff / hour) as i64)
            } else if abs_diff < week {
                format!("in {}d", (abs_diff / day) as i64)
            } else if abs_diff < month {
                format!("in {}w", (abs_diff / week) as i64)
            } else if abs_diff < year {
                format!("in {}mo", (abs_diff / month) as i64)
            } else {
                format!("in {}y", (abs_diff / year) as i64)
            }
        }
    }
}

#[allow(non_snake_case)]
fn schedule_card(
    ef: f64, i: i64, r: i64, state: &str, step: i64, lapse_count: i64,
    next_review: Option<i64>, grade: &str, config: &SRSConfig,
) -> SRSResult {
    let q = match grade {
        "again" => 1,
        "hard" => 3,
        "good" => 4,
        "easy" => 5,
        _ => 4,
    };
    let new_ef = compute_ef(ef, q);
    let now = chrono::Utc::now().timestamp();
    let mut lapsed = false;

    let (new_i, new_r, new_state, new_step, new_lapse_count, next, next_minutes) = match state {
        "new" | "learning" => {
            let steps = &config.learning_steps;
            match grade {
                "again" => {
                    let ns = 0;
                    let nm = steps.get(ns as usize).copied().unwrap_or(1);
                    let nr = now + nm * 60;
                    (1, 0, "learning".to_string(), ns, lapse_count, nr, nm)
                }
                "hard" => {
                    let ns = i64::max(0, step);
                    let nm = steps.get(ns as usize).copied().unwrap_or(1);
                    let nr = now + nm * 60;
                    (1, 0, "learning".to_string(), ns, lapse_count, nr, nm)
                }
                "good" => {
                    let next_step = step + 1;
                    if (next_step as usize) >= steps.len() {
                        let gi = config.graduating_interval;
                        let nm = gi * 24 * 60;
                        let nr = now + gi * 24 * 60 * 60;
                        (gi, 1, "review".to_string(), 0, lapse_count, nr, nm)
                    } else {
                        let nm = steps.get(next_step as usize).copied().unwrap_or(1);
                        let nr = now + nm * 60;
                        (1, 0, "learning".to_string(), next_step, lapse_count, nr, nm)
                    }
                }
                _ => {
                    let ei = config.easy_interval;
                    let nm = ei * 24 * 60;
                    let nr = now + ei * 24 * 60 * 60;
                    (ei, 1, "review".to_string(), 0, lapse_count, nr, nm)
                }
            }
        }
        "review" => {
            let bonus = grace_bonus(next_review, now, config.grace_period_multiplier);
            if grade == "again" {
                lapsed = true;
                let lapse_i = i64::max(
                    config.minimum_lapse_interval,
                    (i as f64 * config.new_interval).round() as i64,
                );
                if !config.relearning_steps.is_empty() {
                    let nm = config.relearning_steps[0];
                    let nr = now + nm * 60;
                    (lapse_i, 0, "relearning".to_string(), 0, lapse_count + 1, nr, nm)
                } else {
                    let nm = lapse_i * 24 * 60;
                    let nr = now + lapse_i * 24 * 60 * 60;
                    (lapse_i, 0, "review".to_string(), 0, lapse_count + 1, nr, nm)
                }
            } else {
                let new_i_val = match grade {
                    "hard" => i64::max(i + 1, (i as f64 * 1.2 * config.interval_modifier).round() as i64),
                    "good" => {
                        let raw = ((i as f64 * new_ef) + bonus) * config.interval_modifier;
                        i64::max(i + 1, raw.round() as i64)
                    }
                    _ => {
                        let raw = ((i as f64 * new_ef * config.easy_bonus) + bonus) * config.interval_modifier;
                        i64::max(i + 1, raw.round() as i64)
                    }
                };
                let capped = i64::min(config.maximum_interval, new_i_val);
                let nm = capped * 24 * 60;
                let nr = now + capped * 24 * 60 * 60;
                (capped, r + 1, "review".to_string(), 0, lapse_count, nr, nm)
            }
        }
        "relearning" => {
            let steps = &config.relearning_steps;
            match grade {
                "again" => {
                    let nm = steps.get(0).copied().unwrap_or(10);
                    let nr = now + nm * 60;
                    (i, 0, "relearning".to_string(), 0, lapse_count, nr, nm)
                }
                "hard" => {
                    let ns = i64::max(0, step);
                    let nm = steps.get(ns as usize).copied().unwrap_or(10);
                    let nr = now + nm * 60;
                    (i, 0, "relearning".to_string(), ns, lapse_count, nr, nm)
                }
                "good" => {
                    let next_step = step + 1;
                    if (next_step as usize) >= steps.len() {
                        let nm = i * 24 * 60;
                        let nr = now + i * 24 * 60 * 60;
                        (i, 1, "review".to_string(), 0, lapse_count, nr, nm)
                    } else {
                        let nm = steps.get(next_step as usize).copied().unwrap_or(10);
                        let nr = now + nm * 60;
                        (i, 0, "relearning".to_string(), next_step, lapse_count, nr, nm)
                    }
                }
                _ => {
                    let boosted = i64::max(1, (i as f64 * 1.3).round() as i64);
                    let nm = boosted * 24 * 60;
                    let nr = now + boosted * 24 * 60 * 60;
                    (boosted, 1, "review".to_string(), 0, lapse_count, nr, nm)
                }
            }
        }
        _ => {
            let nm = config.learning_steps.get(0).copied().unwrap_or(1);
            let nr = now + nm * 60;
            (1, 0, "learning".to_string(), 0, lapse_count, nr, nm)
        }
    };

    #[allow(non_snake_case)]
    SRSResult {
        EF: new_ef,
        I: new_i,
        R: new_r,
        state: new_state.clone(),
        step: new_step,
        lapse_count: new_lapse_count,
        next_review: next,
        next_minutes,
        lapsed,
        graduated: (state == "new" || state == "learning" || state == "relearning")
            && new_state == "review"
            && next > end_of_today(),
        fsrs_stability: None,
        fsrs_difficulty: None,
    }
}

fn graduate_with_fsrs(
    memory: Option<fsrs::MemoryState>,
    review_count: i64,
    lapse_count: i64,
    grade: &str,
    now: i64,
) -> Result<(SRSResult, Option<fsrs::MemoryState>), String> {
    let fsrs = get_fsrs();
    let next_states = fsrs.next_states(memory, DESIRED_RETENTION, 0)
        .map_err(|e| e.to_string())?;
    let chosen = match grade {
        "easy" => &next_states.easy,
        _ => &next_states.good,
    };
    let interval_days = chosen.interval;
    let approx_ef = fsrs_difficulty_to_ef(chosen.memory.difficulty);
    #[allow(non_snake_case)]
    Ok((SRSResult {
        EF: approx_ef,
        I: interval_days.round() as i64,
        R: review_count + 1,
        state: "review".to_string(),
        step: 0,
        lapse_count,
        next_review: now + (interval_days * 86400.0) as i64,
        next_minutes: (interval_days * 24.0 * 60.0) as i64,
        lapsed: false,
        graduated: true,
        fsrs_stability: Some(chosen.memory.stability),
        fsrs_difficulty: Some(chosen.memory.difficulty),
    }, Some(chosen.memory)))
}

fn re_graduate_with_fsrs(
    memory: Option<fsrs::MemoryState>,
    stored_interval: i64,
    review_count: i64,
    lapse_count: i64,
    now: i64,
) -> Result<(SRSResult, Option<fsrs::MemoryState>), String> {
    let interval_days = i64::max(1, stored_interval);
    let (approx_ef, stab, diff) = match memory {
        Some(m) => (fsrs_difficulty_to_ef(m.difficulty), Some(m.stability), Some(m.difficulty)),
        None => (2.5, None, None),
    };
    #[allow(non_snake_case)]
    Ok((SRSResult {
        EF: approx_ef,
        I: interval_days,
        R: review_count + 1,
        state: "review".to_string(),
        step: 0,
        lapse_count,
        next_review: now + interval_days * 86400,
        next_minutes: interval_days * 24 * 60,
        lapsed: false,
        graduated: true,
        fsrs_stability: stab,
        fsrs_difficulty: diff,
    }, memory))
}

fn schedule_fsrs(
    state: &str,
    step: i64,
    current_memory: Option<fsrs::MemoryState>,
    stored_interval: i64,
    review_count: i64,
    lapse_count: i64,
    next_review: Option<i64>,
    grade: &str,
    config: &SRSConfig,
) -> Result<(SRSResult, Option<fsrs::MemoryState>), String> {
    let now = chrono::Utc::now().timestamp();

    match state {
        "new" | "learning" => {
            let steps = &config.learning_steps;
            match grade {
                "again" => {
                    let nm = steps.get(0).copied().unwrap_or(1);
                    #[allow(non_snake_case)]
                    Ok((SRSResult {
                        EF: -1.0, I: 0, R: 0, state: "learning".to_string(),
                        step: 0, lapse_count, next_review: now + nm * 60,
                        next_minutes: nm, lapsed: false, graduated: false,
                        fsrs_stability: None, fsrs_difficulty: None,
                    }, None))
                }
                "hard" => {
                    let ns = i64::max(0, step);
                    let nm = steps.get(ns as usize).copied().unwrap_or(1);
                    #[allow(non_snake_case)]
                    Ok((SRSResult {
                        EF: -1.0, I: 0, R: 0, state: "learning".to_string(),
                        step: ns, lapse_count, next_review: now + nm * 60,
                        next_minutes: nm, lapsed: false, graduated: false,
                        fsrs_stability: None, fsrs_difficulty: None,
                    }, None))
                }
                "good" => {
                    let next_step = step + 1;
                    if (next_step as usize) >= steps.len() {
                        graduate_with_fsrs(None, review_count, lapse_count, "good", now)
                    } else {
                        let nm = steps.get(next_step as usize).copied().unwrap_or(1);
                        #[allow(non_snake_case)]
                        Ok((SRSResult {
                            EF: -1.0, I: 0, R: 0, state: "learning".to_string(),
                            step: next_step, lapse_count, next_review: now + nm * 60,
                            next_minutes: nm, lapsed: false, graduated: false,
                            fsrs_stability: None, fsrs_difficulty: None,
                        }, None))
                    }
                }
                _ => {
                    graduate_with_fsrs(None, review_count, lapse_count, "easy", now)
                }
            }
        }
        "review" => {
            let fsrs = get_fsrs();
            let days_elapsed = compute_days_elapsed(next_review);
            let next_states = fsrs.next_states(current_memory, DESIRED_RETENTION, days_elapsed)
                .unwrap_or_else(|e| {
                    eprintln!("[fsrs] next_states error: {}; treating as new card", e);
                    fsrs.next_states(None, DESIRED_RETENTION, 0).unwrap()
                });

            if grade == "again" {
                let post_lapse = next_states.again.memory;
                let relearning_steps = &config.relearning_steps;
                let nm = relearning_steps.get(0).copied().unwrap_or(10);
                #[allow(non_snake_case)]
                Ok((SRSResult {
                    EF: fsrs_difficulty_to_ef(post_lapse.difficulty),
                    I: next_states.again.interval.round() as i64,
                    R: 0,
                    state: "relearning".to_string(),
                    step: 0,
                    lapse_count: lapse_count + 1,
                    next_review: now + nm * 60,
                    next_minutes: nm,
                    lapsed: true,
                    graduated: false,
                    fsrs_stability: Some(post_lapse.stability),
                    fsrs_difficulty: Some(post_lapse.difficulty),
                }, Some(post_lapse)))
            } else {
                let chosen = match grade {
                    "hard" => &next_states.hard,
                    "easy" => &next_states.easy,
                    _ => &next_states.good,
                };
                let interval_days = chosen.interval;
                #[allow(non_snake_case)]
                Ok((SRSResult {
                    EF: fsrs_difficulty_to_ef(chosen.memory.difficulty),
                    I: interval_days.round() as i64,
                    R: review_count + 1,
                    state: "review".to_string(),
                    step: 0,
                    lapse_count,
                    next_review: now + (interval_days * 86400.0) as i64,
                    next_minutes: (interval_days * 24.0 * 60.0) as i64,
                    lapsed: false,
                    graduated: false,
                    fsrs_stability: Some(chosen.memory.stability),
                    fsrs_difficulty: Some(chosen.memory.difficulty),
                }, Some(chosen.memory)))
            }
        }
        "relearning" => {
            let steps = &config.relearning_steps;
            match grade {
                "again" => {
                    let nm = steps.get(0).copied().unwrap_or(10);
                    #[allow(non_snake_case)]
                    Ok((SRSResult {
                        EF: -1.0, I: stored_interval, R: 0, state: "relearning".to_string(),
                        step: 0, lapse_count, next_review: now + nm * 60,
                        next_minutes: nm, lapsed: false, graduated: false,
                        fsrs_stability: current_memory.map(|m| m.stability),
                        fsrs_difficulty: current_memory.map(|m| m.difficulty),
                    }, current_memory))
                }
                "hard" => {
                    let ns = i64::max(0, step);
                    let nm = steps.get(ns as usize).copied().unwrap_or(10);
                    #[allow(non_snake_case)]
                    Ok((SRSResult {
                        EF: -1.0, I: stored_interval, R: 0, state: "relearning".to_string(),
                        step: ns, lapse_count, next_review: now + nm * 60,
                        next_minutes: nm, lapsed: false, graduated: false,
                        fsrs_stability: current_memory.map(|m| m.stability),
                        fsrs_difficulty: current_memory.map(|m| m.difficulty),
                    }, current_memory))
                }
                "good" => {
                    let next_step = step + 1;
                    if (next_step as usize) >= steps.len() {
                        re_graduate_with_fsrs(current_memory, stored_interval, review_count, lapse_count, now)
                    } else {
                        let nm = steps.get(next_step as usize).copied().unwrap_or(10);
                        #[allow(non_snake_case)]
                        Ok((SRSResult {
                            EF: -1.0, I: stored_interval, R: 0, state: "relearning".to_string(),
                            step: next_step, lapse_count, next_review: now + nm * 60,
                            next_minutes: nm, lapsed: false, graduated: false,
                            fsrs_stability: current_memory.map(|m| m.stability),
                            fsrs_difficulty: current_memory.map(|m| m.difficulty),
                        }, current_memory))
                    }
                }
                _ => {
                    re_graduate_with_fsrs(current_memory, stored_interval, review_count, lapse_count, now)
                }
            }
        }
        _ => schedule_fsrs("new", step, current_memory, stored_interval, review_count, lapse_count, next_review, grade, config),
    }
}

#[tauri::command]
pub fn db_upsert_review_item(app: tauri::AppHandle, content: String, language: Option<String>) -> Result<(), String> {
    let conn = open_db(&app)?;
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO review_items(id, content, EF, I, R, state, step, lapse_count, next_review, language, fsrs_stability, fsrs_difficulty, created_at)
         VALUES (?1, ?2, -1.0, 0, 0, 'new', 0, 0, NULL, ?3, NULL, NULL, CAST(strftime('%s', 'now') * 1000 AS INTEGER))
         ON CONFLICT(content) DO UPDATE SET language=COALESCE(excluded.language, review_items.language)",
        params![id, content, language],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn db_get_review_items(app: tauri::AppHandle) -> Result<Vec<ReviewItem>, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare(&format!("SELECT {} FROM review_items", review_cols(""))).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], row_to_review_item).map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    for r in rows {
        items.push(r.map_err(|e| e.to_string())?);
    }
    Ok(items)
}

#[tauri::command]
pub fn db_get_review_items_by_words(app: tauri::AppHandle, words: Vec<String>) -> Result<Vec<ReviewItem>, String> {
    if words.is_empty() {
        return Ok(vec![]);
    }

    let conn = open_db(&app)?;
    let placeholders: Vec<String> = words.iter().map(|_| "?".to_string()).collect();
    let sql = format!(
        "SELECT {} FROM review_items WHERE content IN ({})",
        review_cols(""),
        placeholders.join(", ")
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let params: Vec<&dyn rusqlite::ToSql> = words.iter().map(|w| w as &dyn rusqlite::ToSql).collect();

    let rows = stmt.query_map(params.as_slice(), row_to_review_item).map_err(|e| e.to_string())?;

    let mut items = Vec::new();
    for r in rows {
        items.push(r.map_err(|e| e.to_string())?);
    }
    Ok(items)
}


#[derive(Serialize)]
pub struct GradePreview {
    pub grade: String,
    pub label: String,
    pub next_minutes: i64,
    pub next_review_formatted: String,
    pub state: String,
    pub lapsed: bool,
    pub graduated: bool,
}

#[tauri::command]
pub fn db_preview_srs_grades(app: tauri::AppHandle, id: String) -> Result<Vec<GradePreview>, String> {
    let conn = open_db(&app)?;
    let row: (f64, i64, i64, String, i64, i64, Option<i64>, Option<f32>, Option<f32>) = conn.query_row(
        "SELECT EF, I, R, state, step, lapse_count, next_review, fsrs_stability, fsrs_difficulty
         FROM review_items WHERE id=?1",
        params![id],
        |row| Ok((
            row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
            row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?,
        )),
    ).map_err(|e| e.to_string())?;

    let (ef, i, r, state, step, lapse_count, next_review, fsrs_stability, fsrs_difficulty) = row;

    let current_memory = if ef == -1.0 {
        memory_state_from_db(fsrs_stability, fsrs_difficulty)
    } else {
        let fsrs = get_fsrs();
        fsrs.memory_state_from_sm2(ef as f32, i as f32, DESIRED_RETENTION).ok()
    };

    let config = SRSConfig::default();
    let grades = ["again", "hard", "good", "easy"];
    let labels = ["Again", "Hard", "Good", "Easy"];

    let mut previews = Vec::new();
    for idx in 0..grades.len() {
        let (result, _) = schedule_fsrs(&state, step, current_memory, i, r, lapse_count, next_review, grades[idx], &config)?;
        previews.push(GradePreview {
            grade: grades[idx].to_string(),
            label: labels[idx].to_string(),
            next_minutes: result.next_minutes,
            next_review_formatted: format_next_review_minutes(result.next_minutes),
            state: result.state,
            lapsed: result.lapsed,
            graduated: result.graduated,
        });
    }
    Ok(previews)
}

fn compute_grade_previews(conn: &rusqlite::Connection, id: &str) -> Result<Vec<GradePreview>, String> {
    let row: (f64, i64, i64, String, i64, i64, Option<i64>, Option<f32>, Option<f32>) = conn.query_row(
        "SELECT EF, I, R, state, step, lapse_count, next_review, fsrs_stability, fsrs_difficulty
         FROM review_items WHERE id=?1",
        params![id],
        |row| Ok((
            row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
            row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?,
        )),
    ).map_err(|e| e.to_string())?;

    let (ef, i, r, state, step, lapse_count, next_review, fsrs_stability, fsrs_difficulty) = row;

    let current_memory = if ef == -1.0 {
        memory_state_from_db(fsrs_stability, fsrs_difficulty)
    } else {
        let fsrs = get_fsrs();
        fsrs.memory_state_from_sm2(ef as f32, i as f32, DESIRED_RETENTION).ok()
    };

    let config = SRSConfig::default();
    let grades = ["again", "hard", "good", "easy"];
    let labels = ["Again", "Hard", "Good", "Easy"];

    let mut previews = Vec::new();
    for idx in 0..grades.len() {
        let (result, _) = schedule_fsrs(&state, step, current_memory, i, r, lapse_count, next_review, grades[idx], &config)?;
        previews.push(GradePreview {
            grade: grades[idx].to_string(),
            label: labels[idx].to_string(),
            next_minutes: result.next_minutes,
            next_review_formatted: format_next_review_minutes(result.next_minutes),
            state: result.state,
            lapsed: result.lapsed,
            graduated: result.graduated,
        });
    }
    Ok(previews)
}

fn grade_card_in_db(conn: &rusqlite::Connection, id: &str, grade: &str) -> Result<SRSResult, String> {
    #[allow(clippy::type_complexity)]
    let row: (f64, i64, i64, String, i64, i64, Option<i64>, Option<f32>, Option<f32>) = conn.query_row(
        "SELECT EF, I, R, state, step, lapse_count, next_review, fsrs_stability, fsrs_difficulty
         FROM review_items WHERE id=?1",
        params![id],
        |row| Ok((
            row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
            row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?,
        )),
    ).map_err(|e| e.to_string())?;

    let (ef, i, r, state, step, lapse_count, next_review, fsrs_stability, fsrs_difficulty) = row;

    let current_memory = if ef == -1.0 {
        memory_state_from_db(fsrs_stability, fsrs_difficulty)
    } else {
        let fsrs = get_fsrs();
        match fsrs.memory_state_from_sm2(ef as f32, i as f32, DESIRED_RETENTION) {
            Ok(ms) => Some(ms),
            Err(e) => {
                eprintln!("[fsrs] On-the-fly migration failed for card {}: {}", id, e);
                let config = SRSConfig::default();
                let result = schedule_card(ef, i, r, &state, step, lapse_count, next_review, grade, &config);
                conn.execute(
                    "UPDATE review_items SET EF=?1, I=?2, R=?3, state=?4, step=?5, lapse_count=?6, next_review=?7 WHERE id=?8",
                    params![result.EF, result.I, result.R, result.state, result.step, result.lapse_count, result.next_review, id],
                ).map_err(|e| e.to_string())?;
                return Ok(result);
            }
        }
    };

    let config = SRSConfig::default();
    let (result, new_memory_opt) = schedule_fsrs(&state, step, current_memory, i, r, lapse_count, next_review, grade, &config)?;

    conn.execute(
        "UPDATE review_items SET EF=-1.0, I=?1, R=?2, state=?3, step=?4, lapse_count=?5, next_review=?6, fsrs_stability=?7, fsrs_difficulty=?8 WHERE id=?9",
        params![
            result.I, result.R, result.state, result.step, result.lapse_count,
            result.next_review,
            new_memory_opt.as_ref().map(|m| m.stability),
            new_memory_opt.as_ref().map(|m| m.difficulty),
            id,
        ],
    ).map_err(|e| e.to_string())?;

    if result.state == "review" && state != "review" {
        let _ = conn.execute(
            "UPDATE review_items SET mastered_at = CAST(strftime('%s', 'now') * 1000 AS INTEGER)
             WHERE id = ?1 AND mastered_at IS NULL",
            params![id],
        );
    }

    Ok(result)
}

#[derive(Serialize)]
pub struct SessionStartResult {
    pub total_cards: usize,
    pub next_card: Option<ReviewItem>,
    pub next_task: Option<QuizTask>,
    pub grade_options: Vec<GradePreview>,
    pub reviews_remaining: i64,
    pub new_remaining: i64,
    pub reviews_completed: i64,
    pub new_cards_seen: i64,
    /// Words in today's queue with no meaning yet, for the "Generate all" banner.
    pub missing_meaning: Vec<String>,
}

pub(crate) fn ensure_study(
    conn: &rusqlite::Connection,
    date: &str,
    language: &str,
    max_review: i64,
    max_new: i64,
) -> Result<i64, String> {
    conn.execute(
        "INSERT INTO study (date, language, max_review, max_new) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(date, language) DO UPDATE SET max_review = excluded.max_review, max_new = excluded.max_new",
        params![date, language, max_review, max_new],
    ).map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT study_id FROM study WHERE date = ?1 AND language = ?2",
        params![date, language],
        |row| row.get::<_, i64>(0),
    ).map_err(|e| e.to_string())
}

fn classify_kind(state: &str) -> &'static str {
    match state {
        "review" => "review",
        "learning" | "relearning" => "learning",
        _ => "new",
    }
}

fn top_up_queue(
    conn: &rusqlite::Connection,
    study_id: i64,
    language: &str,
    eod: i64,
    max_review: i64,
    max_new: i64,
) -> Result<(), String> {
    let (n_review, n_new): (i64, i64) = conn.query_row(
        "SELECT
            COALESCE(SUM(CASE WHEN kind='review' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN kind='new'    THEN 1 ELSE 0 END), 0)
         FROM study_card WHERE study_id = ?1",
        params![study_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap_or((0, 0));

    let mut review_slots = (max_review - n_review).max(0);
    let mut new_slots = (max_new - n_new).max(0);

    let mut stmt = conn.prepare(
        "SELECT id, state FROM review_items
         WHERE language = ?1
           AND (next_review IS NULL OR next_review <= ?2)
           AND id NOT IN (SELECT review_item_id FROM study_card WHERE study_id = ?3)
         ORDER BY
            CASE WHEN state IN ('learning','relearning') THEN 0
                 WHEN state = 'review' THEN 1
                 ELSE 2 END,
            next_review ASC NULLS LAST"
    ).map_err(|e| e.to_string())?;

    let candidates: Vec<(String, String)> = stmt
        .query_map(params![language, eod, study_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut insert_stmt = conn.prepare(
        "INSERT INTO study_card (study_id, review_item_id, kind, status, create_time, update_time)
         VALUES (?1, ?2, ?3, 'pending', ?4, ?4)"
    ).map_err(|e| e.to_string())?;

    for (id, state) in candidates {
        let kind = classify_kind(&state);
        match kind {
            "review" => {
                if review_slots <= 0 { continue; }
                review_slots -= 1;
            }
            "new" => {
                if new_slots <= 0 { continue; }
                new_slots -= 1;
            }
            _ => {}
        }
        insert_stmt
            .execute(params![study_id, id, kind, now_ms])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn fetch_next_pending(
    conn: &rusqlite::Connection,
    study_id: i64,
    now: i64,
) -> Result<Option<ReviewItem>, String> {
    let result = conn.query_row(
        &format!("SELECT {}
         FROM study_card sc
         JOIN review_items r ON r.id = sc.review_item_id
         WHERE sc.study_id = ?1 AND sc.status = 'pending'
         ORDER BY
            sc.update_time ASC,
            CASE
                WHEN r.state IN ('learning','relearning') AND r.next_review IS NOT NULL AND r.next_review <= ?2 THEN 0
                WHEN r.state = 'review' THEN 1
                WHEN r.state IN ('learning','relearning') THEN 2
                ELSE 3
            END
         LIMIT 1", review_cols("r.")),
        params![study_id, now],
        row_to_review_item,
    );
    match result {
        Ok(item) => Ok(Some(item)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn count_scalar(conn: &rusqlite::Connection, sql: &str, study_id: i64) -> Result<i64, String> {
    conn.query_row(sql, params![study_id], |row| row.get::<_, i64>(0))
        .map_err(|e| e.to_string())
}

fn count_pending(conn: &rusqlite::Connection, study_id: i64) -> Result<usize, String> {
    count_scalar(conn,
        "SELECT COUNT(*) FROM study_card WHERE study_id = ?1 AND status = 'pending'",
        study_id,
    ).map(|n| n as usize)
}

fn count_total(conn: &rusqlite::Connection, study_id: i64) -> Result<usize, String> {
    count_scalar(conn, "SELECT COUNT(*) FROM study_card WHERE study_id = ?1", study_id)
        .map(|n| n as usize)
}

fn count_reviewed(conn: &rusqlite::Connection, study_id: i64) -> Result<i64, String> {
    count_scalar(conn,
        "SELECT COUNT(*) FROM study_card WHERE study_id = ?1 AND kind = 'review' AND status = 'graded'",
        study_id,
    )
}

fn count_new_seen(conn: &rusqlite::Connection, study_id: i64) -> Result<i64, String> {
    count_scalar(conn,
        "SELECT COUNT(*) FROM study_card WHERE study_id = ?1 AND kind = 'new' AND status = 'graded'",
        study_id,
    )
}

pub(crate) fn set_card_status(
    conn: &rusqlite::Connection,
    study_id: i64,
    card_id: &str,
    status: &str,
) -> Result<(), String> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "UPDATE study_card SET status = ?1, update_time = ?2
         WHERE study_id = ?3 AND review_item_id = ?4",
        params![status, now_ms, study_id, card_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn fetch_review_item(conn: &rusqlite::Connection, id: &str) -> Result<Option<ReviewItem>, String> {
    let result = conn.query_row(
        &format!("SELECT {} FROM review_items WHERE id = ?1", review_cols("")),
        params![id],
        row_to_review_item,
    );
    match result {
        Ok(item) => Ok(Some(item)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Grades a card inside today's study session and re-queues it while it is still learning.
pub(crate) fn apply_session_grade(
    conn: &rusqlite::Connection,
    study_id: i64,
    card_id: &str,
    grade: &str,
) -> Result<SRSResult, String> {
    let result = grade_card_in_db(conn, card_id, grade)?;
    set_card_status(conn, study_id, card_id, "graded")?;
    if result.state == "learning" || result.state == "relearning" {
        set_card_status(conn, study_id, card_id, "pending")?;
    }
    Ok(result)
}

/// What the review screen needs after any session step: counters plus the next card and its question.
#[derive(Serialize)]
pub struct SessionProgress {
    pub reviews_completed: i64,
    pub new_cards_seen: i64,
    pub remaining_count: usize,
    pub session_done: bool,
    pub next_card: Option<ReviewItem>,
    pub next_task: Option<QuizTask>,
    pub grade_options: Vec<GradePreview>,
}

pub(crate) fn session_progress(conn: &rusqlite::Connection, study_id: i64) -> Result<SessionProgress, String> {
    let now = chrono::Utc::now().timestamp();
    let reviews_completed = count_reviewed(conn, study_id)?;
    let new_cards_seen = count_new_seen(conn, study_id)?;
    let remaining_count = count_pending(conn, study_id)?;
    let next_card = fetch_next_pending(conn, study_id, now)?;
    let (next_task, grade_options) = match &next_card {
        Some(card) => (
            Some(issue_task(conn, study_id, card)?),
            compute_grade_previews(conn, &card.id).unwrap_or_default(),
        ),
        None => (None, vec![]),
    };
    Ok(SessionProgress {
        reviews_completed,
        new_cards_seen,
        remaining_count,
        session_done: remaining_count == 0,
        next_card,
        next_task,
        grade_options,
    })
}

#[tauri::command]
pub fn db_session_start(
    app: tauri::AppHandle,
    language: String,
    max_reviews: i64,
    max_new_cards: i64,
) -> Result<SessionStartResult, String> {
    let conn = open_db(&app)?;
    let today = chrono::Local::now().date_naive().to_string();
    let eod = end_of_today();

    let study_id = ensure_study(&conn, &today, &language, max_reviews, max_new_cards)?;
    top_up_queue(&conn, study_id, &language, eod, max_reviews, max_new_cards)?;

    let total_cards = count_total(&conn, study_id)?;
    let progress = session_progress(&conn, study_id)?;
    let missing_meaning = cards_missing_meaning(&conn, study_id)?.into_iter().map(|(_, word)| word).collect();

    Ok(SessionStartResult {
        total_cards,
        next_card: progress.next_card,
        next_task: progress.next_task,
        grade_options: progress.grade_options,
        reviews_remaining: (max_reviews - progress.reviews_completed).max(0),
        new_remaining: (max_new_cards - progress.new_cards_seen).max(0),
        reviews_completed: progress.reviews_completed,
        new_cards_seen: progress.new_cards_seen,
        missing_meaning,
    })
}

#[derive(Serialize)]
pub struct SessionGradeResult {
    #[serde(flatten)]
    result: SRSResult,
    #[serde(flatten)]
    progress: SessionProgress,
}

/// Self-graded path, used only for cards that cannot be quizzed (no meaning to check against).
#[tauri::command]
pub fn db_session_grade(
    app: tauri::AppHandle,
    language: String,
    card_id: String,
    grade: String,
    max_reviews: i64,
    max_new_cards: i64,
) -> Result<SessionGradeResult, String> {
    let conn = open_db(&app)?;
    let today = chrono::Local::now().date_naive().to_string();

    let study_id = ensure_study(&conn, &today, &language, max_reviews, max_new_cards)?;

    let result = apply_session_grade(&conn, study_id, &card_id, &grade)?;
    clear_task(&conn, study_id, &card_id)?;
    let progress = session_progress(&conn, study_id)?;

    Ok(SessionGradeResult { result, progress })
}
