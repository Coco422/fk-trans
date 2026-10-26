use crate::{
    config::{self, ProviderConfig},
    AppState,
};
use tauri::State;

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<config::AppConfig, String> {
    let config = state
        .config
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(config.clone())
}

#[tauri::command]
pub async fn update_config(
    patch: config::ConfigPatch,
    state: State<'_, AppState>,
) -> Result<config::AppConfig, String> {
    let result = {
        let mut config = state
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        patch.apply_to(&mut config)?;
        config::save_config(&config)?;
        config.clone()
    };

    // Side effects are keyed off which fields the patch actually carried, which
    // is now a field access rather than a string lookup.
    if patch.active_provider.is_some() {
        let mut engine = state.translation_engine.write().await;
        engine.set_active_provider(&result.active_provider).await;
    }
    if patch.mouse_trigger_button.is_some() {
        crate::mouse::listener::set_trigger_button(
            &state.mouse_trigger_state,
            result.mouse_trigger_button,
        );
    }
    if patch.selection_trigger_enabled.is_some() {
        crate::mouse::listener::set_selection_trigger_enabled(
            &state.mouse_trigger_state,
            result.selection_trigger_enabled,
        );
    }
    if patch.debug_logging.is_some() {
        crate::apply_log_level(result.debug_logging);
        log::info!(
            "[settings] Debug logging {}",
            if result.debug_logging {
                "enabled"
            } else {
                "disabled"
            }
        );
    }

    Ok(result)
}

/// The fields the settings UI may write for one provider.
///
/// A struct rather than seven positional parameters: the frontend type is then
/// generated from this, and `deny_unknown_fields` turns a stray or misspelled
/// key into an error instead of a field that silently never arrives.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, export_to = "types.ts")]
pub struct ProviderUpdate {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    #[ts(optional)]
    pub system_prompt: Option<String>,
    #[ts(optional)]
    pub user_prompt: Option<String>,
    #[ts(optional)]
    #[ts(type = "Record<string, unknown> | null")]
    pub extra_params: Option<serde_json::Value>,
}

#[tauri::command]
pub async fn update_provider(
    update: ProviderUpdate,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let provider_cfg = {
        let mut config = state
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(existing) = config
            .providers
            .iter_mut()
            .find(|candidate| candidate.name == update.name)
        {
            existing.base_url = update.base_url;
            existing.api_key = update.api_key;
            existing.model = update.model;
            if let Some(system_prompt) = update.system_prompt {
                existing.system_prompt = system_prompt;
            }
            if let Some(user_prompt) = update.user_prompt {
                existing.user_prompt = user_prompt;
            }
            if let Some(extra_params) = update.extra_params {
                existing.extra_params = extra_params;
            }
        } else {
            config.providers.push(ProviderConfig {
                name: update.name.clone(),
                base_url: update.base_url,
                api_key: update.api_key,
                model: update.model,
                system_prompt: update
                    .system_prompt
                    .unwrap_or_else(config::default_system_prompt),
                user_prompt: update.user_prompt.unwrap_or_else(config::default_user_prompt),
                // Not `serde_json::Value::default()`: that is `Null`, which the
                // request merge step skips, so a new provider would silently lose
                // the seeded defaults it is supposed to start with.
                extra_params: update
                    .extra_params
                    .unwrap_or_else(config::default_extra_params),
            });
        }

        config::save_config(&config)?;
        config
            .providers
            .iter()
            .find(|candidate| candidate.name == update.name)
            .cloned()
            .ok_or_else(|| format!("Provider '{}' was not saved", update.name))?
    };

    log::info!(
        "[provider] Saved provider={} base_url_present={} api_key_present={} model_present={}",
        provider_cfg.name,
        !provider_cfg.base_url.trim().is_empty(),
        !provider_cfg.api_key.trim().is_empty(),
        !provider_cfg.model.trim().is_empty()
    );

    let mut engine = state.translation_engine.write().await;
    // The config is saved either way, but a provider the engine cannot build is
    // reported rather than silently left running the previous settings.
    engine
        .reload_provider(&provider_cfg)
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn test_provider(
    provider_name: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let provider_config = {
        let config = state
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        config
            .providers
            .iter()
            .find(|p| p.name == provider_name)
            .ok_or_else(|| format!("Provider '{}' not found", provider_name))?
            .clone()
    };

    config::validate_provider(&provider_config)?;
    log::info!(
        "[provider] Testing provider={} base_url_present={} api_key_present={} model_present={}",
        provider_config.name,
        !provider_config.base_url.trim().is_empty(),
        !provider_config.api_key.trim().is_empty(),
        !provider_config.model.trim().is_empty()
    );

    // Build the provider the same way the engine does. This used to be a second
    // copy of the dispatch match, so adding a provider meant editing two places
    // and testing one of them could exercise a different code path than the app.
    let provider = crate::translate::build_provider(&provider_config)
        .map_err(|error| error.to_string())?;

    let result = provider
        .translate("Hello, world!", "en", "zh")
        .await
        .map_err(|error| {
            log::warn!(
                "[provider] Test failed provider={} error_kind={}",
                provider_name,
                error.kind()
            );
            error.to_string()
        })?;

    log::info!(
        "[provider] Test succeeded provider={} translated_chars={}",
        provider_name,
        result.translated.chars().count()
    );
    Ok(format!(
        "Test successful!\nTranslated: {}",
        result.translated
    ))
}
