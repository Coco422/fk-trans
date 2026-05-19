use crate::history::HistoryEntry;
use crate::translate::provider::TranslateResult;
use crate::AppState;
use tauri::State;

#[tauri::command]
pub async fn translate_text(
    text: String,
    from: String,
    to: String,
    state: State<'_, AppState>,
) -> Result<TranslateResult, String> {
    let engine = state.translation_engine.read().await;
    engine
        .translate(&text, &from, &to)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_history(state: State<'_, AppState>) -> Result<Vec<HistoryEntry>, String> {
    Ok(state.history.get_all())
}

#[tauri::command]
pub async fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    state.history.clear();
    Ok(())
}

#[tauri::command]
pub async fn ai_action(
    text: String,
    action: String,
    source_lang: String,
    target_lang: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let prompt = match action.as_str() {
        "explain" => format!(
            "Explain the meaning and usage of this text in detail, in {}: {}",
            target_lang, text
        ),
        "dict" => format!(
            "Provide dictionary-style information for this word/phrase: pronunciation, part of speech, definitions, example sentences, in {}. Text: {}",
            target_lang, text
        ),
        "summary" => format!(
            "Summarize this text concisely in {}: {}",
            target_lang, text
        ),
        _ => return Err(format!("Unknown action: {}", action)),
    };

    let engine = state.translation_engine.read().await;
    let result = engine
        .translate(&prompt, &source_lang, &target_lang)
        .await
        .map_err(|e| e.to_string())?;

    Ok(result.translated)
}
