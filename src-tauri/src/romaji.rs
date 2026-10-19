//! Romaji for absolute beginners: Japanese shown in Latin letters, romaji typed into kana, and a
//! hiragana / katakana starter deck.

use crate::db::open_db;
use crate::furigana::FuriganaAnnotator;
use crate::locale::{explain_lang, ANY_LANG};
use rusqlite::params;
use std::sync::Mutex;

/// Hepburn romaji for each hiragana (two-character combinations first when converting).
const KANA_ROMAJI: &[(&str, &str)] = &[
    ("きゃ", "kya"), ("きゅ", "kyu"), ("きょ", "kyo"), ("しゃ", "sha"), ("しゅ", "shu"), ("しょ", "sho"),
    ("ちゃ", "cha"), ("ちゅ", "chu"), ("ちょ", "cho"), ("にゃ", "nya"), ("にゅ", "nyu"), ("にょ", "nyo"),
    ("ひゃ", "hya"), ("ひゅ", "hyu"), ("ひょ", "hyo"), ("みゃ", "mya"), ("みゅ", "myu"), ("みょ", "myo"),
    ("りゃ", "rya"), ("りゅ", "ryu"), ("りょ", "ryo"), ("ぎゃ", "gya"), ("ぎゅ", "gyu"), ("ぎょ", "gyo"),
    ("じゃ", "ja"), ("じゅ", "ju"), ("じょ", "jo"), ("びゃ", "bya"), ("びゅ", "byu"), ("びょ", "byo"),
    ("ぴゃ", "pya"), ("ぴゅ", "pyu"), ("ぴょ", "pyo"), ("ふぁ", "fa"), ("ふぃ", "fi"), ("ふぇ", "fe"),
    ("ふぉ", "fo"), ("てぃ", "ti"), ("でぃ", "di"), ("しぇ", "she"), ("じぇ", "je"), ("ちぇ", "che"),
    ("うぃ", "wi"), ("うぇ", "we"), ("ゔぁ", "va"),
    ("あ", "a"), ("い", "i"), ("う", "u"), ("え", "e"), ("お", "o"),
    ("か", "ka"), ("き", "ki"), ("く", "ku"), ("け", "ke"), ("こ", "ko"),
    ("さ", "sa"), ("し", "shi"), ("す", "su"), ("せ", "se"), ("そ", "so"),
    ("た", "ta"), ("ち", "chi"), ("つ", "tsu"), ("て", "te"), ("と", "to"),
    ("な", "na"), ("に", "ni"), ("ぬ", "nu"), ("ね", "ne"), ("の", "no"),
    ("は", "ha"), ("ひ", "hi"), ("ふ", "fu"), ("へ", "he"), ("ほ", "ho"),
    ("ま", "ma"), ("み", "mi"), ("む", "mu"), ("め", "me"), ("も", "mo"),
    ("や", "ya"), ("ゆ", "yu"), ("よ", "yo"),
    ("ら", "ra"), ("り", "ri"), ("る", "ru"), ("れ", "re"), ("ろ", "ro"),
    ("わ", "wa"), ("を", "o"), ("ん", "n"),
    ("が", "ga"), ("ぎ", "gi"), ("ぐ", "gu"), ("げ", "ge"), ("ご", "go"),
    ("ざ", "za"), ("じ", "ji"), ("ず", "zu"), ("ぜ", "ze"), ("ぞ", "zo"),
    ("だ", "da"), ("ぢ", "ji"), ("づ", "zu"), ("で", "de"), ("ど", "do"),
    ("ば", "ba"), ("び", "bi"), ("ぶ", "bu"), ("べ", "be"), ("ぼ", "bo"),
    ("ぱ", "pa"), ("ぴ", "pi"), ("ぷ", "pu"), ("ぺ", "pe"), ("ぽ", "po"),
    ("ぁ", "a"), ("ぃ", "i"), ("ぅ", "u"), ("ぇ", "e"), ("ぉ", "o"),
    ("ゃ", "ya"), ("ゅ", "yu"), ("ょ", "yo"), ("ゔ", "vu"),
];

/// Extra spellings accepted when typing romaji.
const TYPING_ALIASES: &[(&str, &str)] = &[
    ("si", "し"), ("ti", "ち"), ("tu", "つ"), ("hu", "ふ"), ("zi", "じ"), ("di", "ぢ"), ("du", "づ"),
    ("sya", "しゃ"), ("syu", "しゅ"), ("syo", "しょ"), ("tya", "ちゃ"), ("tyu", "ちゅ"), ("tyo", "ちょ"),
    ("cya", "ちゃ"), ("cyu", "ちゅ"), ("cyo", "ちょ"), ("zya", "じゃ"), ("zyu", "じゅ"), ("zyo", "じょ"),
    ("jya", "じゃ"), ("jyu", "じゅ"), ("jyo", "じょ"), ("wo", "を"), ("n'", "ん"),
    ("xtu", "っ"), ("ltu", "っ"), ("xya", "ゃ"), ("xyu", "ゅ"), ("xyo", "ょ"), ("-", "ー"),
    (",", "、"), (".", "。"), ("?", "？"), ("!", "！"),
];

pub fn katakana_to_hiragana(s: &str) -> String {
    s.chars()
        .map(|c| if ('\u{30A1}'..='\u{30F6}').contains(&c) { char::from_u32(c as u32 - 0x60).unwrap_or(c) } else { c })
        .collect()
}

fn hiragana_to_katakana(s: &str) -> String {
    s.chars()
        .map(|c| if ('\u{3041}'..='\u{3096}').contains(&c) { char::from_u32(c as u32 + 0x60).unwrap_or(c) } else { c })
        .collect()
}

/// Kana (hiragana or katakana) to Hepburn romaji. Other characters pass through.
pub fn kana_to_romaji(kana: &str) -> String {
    let hira: Vec<char> = katakana_to_hiragana(kana).chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut geminate = false;
    while i < hira.len() {
        let c = hira[i];
        if c == 'っ' {
            geminate = true;
            i += 1;
            continue;
        }
        if c == 'ー' {
            if let Some(last) = out.chars().last().filter(|l| "aiueo".contains(*l)) {
                out.push(last);
            }
            i += 1;
            continue;
        }
        let two: String = hira[i..(i + 2).min(hira.len())].iter().collect();
        let (romaji, used) = match KANA_ROMAJI.iter().find(|(k, _)| two.chars().count() == 2 && *k == two) {
            Some((_, r)) => (r.to_string(), 2),
            None => match KANA_ROMAJI.iter().find(|(k, _)| k.starts_with(c) && k.chars().count() == 1) {
                Some((_, r)) => (r.to_string(), 1),
                None => (c.to_string(), 1),
            },
        };
        if geminate {
            if let Some(first) = romaji.chars().next().filter(|f| f.is_ascii_alphabetic() && !"aiueon".contains(*f)) {
                out.push(if romaji.starts_with("ch") { 't' } else { first });
            }
            geminate = false;
        }
        // ん before a vowel or y is written n' so it doesn't merge with the next syllable.
        if c == 'ん' && used == 1 {
            if let Some(next) = hira.get(i + 1) {
                if "あいうえおやゆよ".contains(*next) {
                    out.push_str("n'");
                    i += 1;
                    continue;
                }
            }
        }
        out.push_str(&romaji);
        i += used;
    }
    out
}

/// Romaji typed by the learner to hiragana, as far as it can be converted; the unfinished tail
/// (e.g. a single "k") is left as typed. UPPER CASE input gives katakana. `finish` also turns a
/// final "n" into ん (while typing it may still become な, に, …).
pub fn romaji_to_kana(input: &str, finish: bool) -> String {
    let mut table: Vec<(String, String)> = KANA_ROMAJI
        .iter()
        .filter(|(k, r)| !matches!(*k, "ぁ" | "ぃ" | "ぅ" | "ぇ" | "ぉ" | "ゃ" | "ゅ" | "ょ" | "を" | "ぢ" | "づ") && *r != "n")
        .map(|(k, r)| (r.to_string(), k.to_string()))
        .collect();
    table.extend(TYPING_ALIASES.iter().map(|(r, k)| (r.to_string(), k.to_string())));
    table.sort_by_key(|(r, _)| std::cmp::Reverse(r.len()));

    let chars: Vec<char> = input.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if !c.is_ascii() {
            out.push(c);
            i += 1;
            continue;
        }
        let upper = c.is_ascii_uppercase();
        let rest: String = chars[i..].iter().collect::<String>().to_lowercase();
        let next = rest.chars().nth(1);
        // Doubled consonant → small っ.
        if let Some(n) = next {
            let first = rest.chars().next().unwrap_or(' ');
            if first == n && first.is_ascii_alphabetic() && !"aiueon".contains(first) {
                out.push_str(&case_kana("っ", upper));
                i += 1;
                continue;
            }
        }
        // "nn": ん, keeping the second n when a vowel follows (konnichiha → こんにちは).
        if rest.starts_with("nn") {
            let third = rest.chars().nth(2);
            let keep_second = third.is_some_and(|t| "aiueoy".contains(t));
            out.push_str(&case_kana("ん", upper));
            i += if keep_second { 1 } else { 2 };
            continue;
        }
        // n followed by a consonant (not y), a non-letter, or the end when finishing → ん.
        if rest.starts_with('n') {
            let to_n = match next {
                Some(n) => (!n.is_ascii_alphabetic() && n != '\'') || (n.is_ascii_alphabetic() && !"aiueoy".contains(n)),
                None => finish,
            };
            if to_n {
                out.push_str(&case_kana("ん", upper));
                i += 1;
                continue;
            }
        }
        match table.iter().find(|(r, _)| rest.starts_with(r.as_str())) {
            Some((r, k)) => {
                out.push_str(&case_kana(k, upper));
                i += r.chars().count();
            }
            None => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn case_kana(kana: &str, upper: bool) -> String {
    if upper { hiragana_to_katakana(kana) } else { kana.to_string() }
}

/// Romaji for a Japanese sentence, word by word, using the tokenizer's readings.
#[tauri::command]
pub fn to_romaji(state: tauri::State<'_, Mutex<Option<FuriganaAnnotator>>>, text: String) -> Result<String, String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let Some(annotator) = guard.as_mut() else {
        return Ok(kana_to_romaji(&text));
    };
    let words: Vec<String> = annotator
        .readings(&text)
        .into_iter()
        .map(|(surface, reading, particle)| match (particle, surface.as_str()) {
            (true, "は") => "wa".to_string(),
            (true, "へ") => "e".to_string(),
            (true, "を") => "o".to_string(),
            _ => kana_to_romaji(reading.as_deref().unwrap_or(&surface)),
        })
        .filter(|w| !w.trim().is_empty())
        .collect();
    Ok(join_romaji(&words))
}

/// Joins words with spaces, without a space before punctuation.
fn join_romaji(words: &[String]) -> String {
    let mut out = String::new();
    for w in words {
        let punct = w.chars().all(|c| "。、！？!?.,「」『』（）()…・".contains(c));
        if !out.is_empty() && !punct {
            out.push(' ');
        }
        let mapped: String = w
            .chars()
            .map(|c| match c {
                '。' => '.',
                '、' => ',',
                '！' => '!',
                '？' => '?',
                other => other,
            })
            .collect();
        out.push_str(&mapped);
    }
    out
}

#[tauri::command]
pub fn romaji_to_kana_input(text: String, finish: Option<bool>) -> String {
    romaji_to_kana(&text, finish.unwrap_or(false))
}

/// Hints for kana whose romaji an Indonesian speaker would read differently.
fn kana_hint(kana: &str, romaji: &str, explain: &str) -> Option<String> {
    let id = explain == "id";
    let hint = match (kana, romaji) {
        ("を", _) => if id { "dipakai sebagai partikel, dibaca \"o\"" } else { "used as the particle, read \"o\"" },
        ("ん", _) => if id { "berdiri sendiri, dibaca \"n\"" } else { "stands alone, read \"n\"" },
        (_, romaji) => match romaji {
        "shi" => if id { "dibaca \"syi\", seperti \"sy\" dalam \"syukur\"" } else { "like \"she\"" },
        "chi" => if id { "dibaca \"ci\", seperti dalam \"cinta\"" } else { "like \"cheese\"" },
        "tsu" => if id { "\"ts\" cepat, seperti akhir kata \"cats\" lalu \"u\"" } else { "like the end of \"cats\" + u" },
        "fu" => if id { "di antara \"fu\" dan \"hu\", bibir tidak menyentuh gigi" } else { "between \"fu\" and \"hu\"" },
        "ji" => if id { "dibaca \"ji\", seperti dalam \"jika\"" } else { "like \"jeep\"" },
        "ra" | "ri" | "ru" | "re" | "ro" => if id { "\"r\" ringan, di antara \"r\" dan \"l\", tidak digetarkan" } else { "a light tap between r and l" },
        _ => return None,
        },
    };
    Some(hint.to_string())
}

/// Adds the 46 basic hiragana or katakana as new cards, meaning = romaji (language-neutral, so it
/// survives a language switch), with pronunciation hints in the learner's language as the mnemonic.
/// Returns how many were added.
#[tauri::command]
pub fn db_seed_kana_deck(app: tauri::AppHandle, script: String) -> Result<usize, String> {
    let conn = open_db(&app)?;
    let explain = explain_lang(&conn);
    let basic = KANA_ROMAJI
        .iter()
        .filter(|(k, _)| k.chars().count() == 1)
        .take_while(|(k, _)| *k != "が");
    let mut added = 0;
    for (kana, romaji) in basic {
        let content = if script == "katakana" { hiragana_to_katakana(kana) } else { kana.to_string() };
        let detail = serde_json::json!({
            "reading": kana,
            "meaning": romaji,
            "part_of_speech": "other",
            "example": "",
            "example_meaning": "",
        })
        .to_string();
        let n = conn
            .execute(
                "INSERT INTO review_items(id, content, EF, I, R, state, step, lapse_count, next_review, language, detail, meaning_lang, mnemonic, tags, created_at)
                 VALUES (?1, ?2, -1.0, 0, 0, 'new', 0, 0, NULL, 'ja', ?3, ?4, ?5, ?6, CAST(strftime('%s', 'now') * 1000 AS INTEGER))
                 ON CONFLICT(content) DO NOTHING",
                params![uuid::Uuid::new_v4().to_string(), content, detail, ANY_LANG, kana_hint(kana, romaji, &explain), script],
            )
            .map_err(|e| e.to_string())?;
        added += n;
    }
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kana_to_hepburn() {
        assert_eq!(kana_to_romaji("ありがとう"), "arigatou");
        assert_eq!(kana_to_romaji("きって"), "kitte");
        assert_eq!(kana_to_romaji("まっちゃ"), "matcha");
        assert_eq!(kana_to_romaji("トーキョー"), "tookyoo");
        assert_eq!(kana_to_romaji("きんえん"), "kin'en");
        assert_eq!(kana_to_romaji("しんぶん"), "shinbun");
    }

    #[test]
    fn romaji_typing_to_kana() {
        assert_eq!(romaji_to_kana("arigatou", false), "ありがとう");
        assert_eq!(romaji_to_kana("kitte", false), "きって");
        assert_eq!(romaji_to_kana("shinbun", true), "しんぶん");
        assert_eq!(romaji_to_kana("konnichiha", false), "こんにちは");
        assert_eq!(romaji_to_kana("PAN", true), "パン");
        assert_eq!(romaji_to_kana("pan", false), "ぱn");
        assert_eq!(romaji_to_kana("k", false), "k");
        assert_eq!(romaji_to_kana("watashi ha", false), "わたし は");
        assert_eq!(romaji_to_kana("hon desu", false), "ほん です");
    }

    #[test]
    fn joins_words_without_space_before_punctuation() {
        let w: Vec<String> = ["watashi", "wa", "gakusei", "desu", "。"].iter().map(|s| s.to_string()).collect();
        assert_eq!(join_romaji(&w), "watashi wa gakusei desu.");
    }
}
