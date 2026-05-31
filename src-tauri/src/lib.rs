use std::sync::Mutex;
use tauri::{Emitter, Manager};

mod google_auth;
mod google_drive;
mod background_sync;
mod furigana;
mod db;
mod chat;
mod sessions;
mod srs;
mod words;
mod stats;
mod grammar;
mod feedback;
mod reading;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn add_furigana(
    state: tauri::State<'_, Mutex<Option<furigana::FuriganaAnnotator>>>,
    text: String,
    language: String,
) -> String {
    if language != "ja" {
        return text;
    }
    match state.lock() {
        Ok(mut guard) => match guard.as_mut() {
            Some(annotator) => annotator.annotate(&text),
            None => text,
        },
        Err(_) => text,
    }
}

#[derive(serde::Serialize)]
struct TokenWithBase {
    surface: String,
    base_form: String,
}

#[tauri::command]
fn tokenize_japanese(
    state: tauri::State<'_, Mutex<Option<furigana::FuriganaAnnotator>>>,
    text: String,
) -> Result<Vec<TokenWithBase>, String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let annotator = guard.as_mut().ok_or("Tokenizer not initialized")?;
    let tokens = annotator.tokenize(&text);
    Ok(tokens
        .into_iter()
        .map(|(surface, base_form)| TokenWithBase { surface, base_form })
        .collect())
}

#[tauri::command]
async fn init_lindera_dict(
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<Option<furigana::FuriganaAnnotator>>>,
) -> Result<String, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let dict_dir = data_dir.join("ipadic");

    if !furigana::is_dict_ready(&dict_dir) {
        let dict_dir_clone = dict_dir.clone();
        let app_clone = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            furigana::ensure_dict(&dict_dir_clone, |stage, message, percent| {
                let _ = app_clone.emit(
                    "lindera-progress",
                    serde_json::json!({
                        "stage": stage,
                        "message": message,
                        "percent": percent
                    }),
                );
            })
        })
        .await
        .map_err(|e| e.to_string())??;
    }

    let annotator = furigana::FuriganaAnnotator::from_path(&dict_dir)?;
    {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        *guard = Some(annotator);
    }
    let _ = app.emit(
        "lindera-progress",
        serde_json::json!({"stage": "ready", "message": "Japanese dictionary ready", "percent": 100}),
    );
    eprintln!("[furigana] Lindera tokenizer initialized from {}", dict_dir.display());
    Ok("initialized".to_string())
}

#[tauri::command]
fn get_lindera_dict_status(app: tauri::AppHandle) -> bool {
    app.path()
        .app_data_dir()
        .map(|d| furigana::is_dict_ready(&d.join("ipadic")))
        .unwrap_or(false)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[allow(unused_mut)]
    let mut app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_background_service::init_with_service(
            || background_sync::AutoSyncService::new(),
        ));

    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        app = app.plugin(
            tauri::plugin::Builder::new("google-auth")
                .setup(|app, api: tauri::plugin::PluginApi<tauri::Wry, serde_json::Value>| {
                    let handle = google_auth::mobile::init(&api)
                        .expect("Failed to initialize Google Auth plugin");
                    app.manage(handle);
                    Ok(())
                })
                .build(),
        );
    }

    app
        .setup(|app| {
            let initial = app
                .path()
                .app_data_dir()
                .ok()
                .and_then(|d| {
                    let dict_dir = d.join("ipadic");
                    furigana::FuriganaAnnotator::from_path(&dict_dir).ok()
                });
            if initial.is_some() {
                eprintln!("[furigana] Lindera tokenizer loaded from cache");
            } else {
                eprintln!("[furigana] Dict not cached — call init_lindera_dict from frontend");
            }
            app.manage(Mutex::new(initial));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            add_furigana,
            tokenize_japanese,
            init_lindera_dict,
            get_lindera_dict_status,
            // Chat / LLM
            chat::test_provider,
            chat::list_models,
            chat::llm_chat,
            // Sessions / Messages
            sessions::db_add_message,
            sessions::db_new_session,
            sessions::db_list_sessions,
            sessions::db_add_message_in_session,
            sessions::db_list_session_messages,
            sessions::db_update_message_content,
            sessions::db_deactivate_message_branch,
            sessions::db_get_message_siblings,
            sessions::db_get_bot_response_for_user,
            sessions::db_rename_session,
            sessions::db_delete_session,
            sessions::db_set_session_topic,
            sessions::db_get_session_topic,
            // SRS / Review
            srs::db_upsert_review_item,
            srs::db_get_review_items,
            srs::db_get_review_items_by_words,
            srs::db_preview_srs_grades,
            srs::db_session_start,
            srs::db_session_grade,
            // Words
            words::db_list_known_words,
            words::db_add_known_word,
            words::db_generate_mnemonic,
            words::db_update_mnemonic,
            words::db_update_review_detail,
            words::db_get_review_items_by_language,
            words::db_search_review_items,
            words::db_get_due_review_items,
            words::db_get_difficult_and_known_words,
            words::db_delete_review_item,
            words::db_update_review_item,
            words::db_import_review_items,
            words::db_generate_detail,
            // Stats / Telemetry
            stats::db_increment_session_today,
            stats::db_get_telemetry_days,
            stats::db_record_activity,
            stats::db_get_user_activity,
            stats::db_record_active_time,
            stats::db_get_active_time,
            stats::db_get_mastery_history,
            stats::db_get_cards_added_history,
            // Grammar
            grammar::db_list_grammar_items,
            grammar::db_add_grammar_item,
            grammar::db_update_grammar_item,
            grammar::db_delete_grammar_item,
            grammar::db_seed_grammar_items,
            grammar::db_get_grammar_levels,
            grammar::db_import_grammar_csv,
            grammar::db_save_selected_grammar,
            grammar::db_get_selected_grammar,
            grammar::db_get_grammar_analysis,
            grammar::db_analyze_grammar,
            // Feedback
            feedback::db_get_user_feedback,
            feedback::db_list_all_feedback,
            feedback::db_evaluate_user_message,
            // Reading
            reading::fetch_reading_text,
            reading::generate_reading_questions,
            reading::evaluate_reading_answer,
            reading::db_save_reading_session,
            reading::db_list_reading_sessions,
            reading::db_get_reading_session,
            reading::db_get_active_reading_session,
            reading::db_update_reading_session,
            // Google Auth
            google_auth::google_sign_in,
            google_auth::google_sign_out,
            google_auth::google_get_auth_status,
            // Google Drive
            google_drive::google_backup_db,
            google_drive::google_restore_db,
            google_drive::google_get_backup_status,
            google_drive::google_set_auto_sync,
            google_drive::google_get_auto_sync,
            google_drive::google_trigger_sync,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
