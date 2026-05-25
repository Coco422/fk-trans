use crate::history::HistoryEntry;
use crate::translate::provider::TranslateResult;
use crate::AppState;
use tauri::State;

fn ai_action_prompt(
    text: &str,
    action: &str,
    source_lang: &str,
    target_lang: &str,
) -> Result<String, String> {
    match action {
        "explain" => Ok(format!(
            "Explain the meaning and usage of this text in detail, in {}: {}",
            target_lang, text
        )),
        "dict" => Ok(format!(
            "Provide dictionary-style information for this word/phrase: pronunciation, part of speech, definitions, example sentences, in {}. Text: {}",
            target_lang, text
        )),
        "summary" | "summarize" => Ok(format!(
            "Summarize this text concisely in {}: {}",
            target_lang, text
        )),
        "polish" => Ok(format!(
            "Rewrite the following text to be clearer, smoother, and more natural while preserving its original meaning. Keep the tone appropriate to the source language ({}), and answer in {} unless the polished text itself should remain in the original language. Text: {}",
            source_lang, target_lang, text
        )),
        _ => Err(format!("Unknown action: {}", action)),
    }
}

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
    let prompt = ai_action_prompt(&text, &action, &source_lang, &target_lang)?;

    let engine = state.translation_engine.read().await;
    let result = engine
        .translate(&prompt, &source_lang, &target_lang)
        .await
        .map_err(|e| e.to_string())?;

    Ok(result.translated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_action_prompt_supports_polish_and_summarize_alias() {
        let polish = ai_action_prompt("rough sentence", "polish", "en", "zh").unwrap();
        assert!(polish.contains("clearer"));
        assert!(polish.contains("rough sentence"));

        let summary = ai_action_prompt("long text", "summarize", "en", "zh").unwrap();
        assert!(summary.contains("Summarize"));
    }

    #[test]
    fn ai_action_prompt_rejects_unknown_actions() {
        let error = ai_action_prompt("text", "unknown", "en", "zh").unwrap_err();

        assert_eq!(error, "Unknown action: unknown");
    }
}
