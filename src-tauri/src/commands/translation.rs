use crate::history::HistoryEntry;
use crate::translate::provider::TranslateResult;
use crate::translate::prompt::{format_prompt, PromptVars};
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
    let (prompt, provider) = {
        let config = state
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let action_config = config
            .actions
            .iter()
            .find(|candidate| candidate.id == action)
            .ok_or_else(|| format!("未知的动作：{action}"))?;

        // The same placeholders a provider's own prompts use, so `{to}` renders
        // as `Chinese (zh-Hans)` rather than as the bare code — the prompts used
        // to be formatted with `zh` interpolated into an English sentence.
        let vars = PromptVars {
            from: &source_lang,
            to: &target_lang,
            text: &text,
        };

        (
            format_prompt(&action_config.prompt, &vars),
            action_config.provider.clone(),
        )
    };

    let engine = state.translation_engine.read().await;
    engine
        .complete(provider.as_deref(), &prompt)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list the settings UI builds its language pickers from. It has to
    /// contain the code meaning "work it out yourself", because that is what the
    /// source-language picker offers as its first entry.
    #[test]
    fn the_language_list_offers_auto_and_nothing_empty() {
        let languages = get_languages();

        assert!(languages.iter().any(|language| language.code == "auto"));
        assert!(languages.iter().all(|language| !language.code.is_empty()));
        assert!(languages.iter().all(|language| !language.name.is_empty()));
    }
}

/// One language the settings UI offers.
///
/// Built from the same table the prompts use, so a language added there shows up
/// in the UI without anyone having to remember to add it in two places — which
/// is how the frontend and the backend came to disagree about them before.
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types.ts")]
pub struct LanguageOption {
    pub code: String,
    /// The English name. The UI shows its own label where it has one, and falls
    /// back to this.
    pub name: String,
}

#[tauri::command]
pub fn get_languages() -> Vec<LanguageOption> {
    crate::translate::language::LANGUAGES
        .iter()
        .map(|language| LanguageOption {
            code: language.code.to_string(),
            name: language.name.to_string(),
        })
        .collect()
}
