use crate::translate::claude::ClaudeProvider;
use crate::translate::custom_http::CustomHttpProvider;
use crate::translate::deeplx::DeepLXProvider;
use crate::translate::gemini::GeminiProvider;
use crate::translate::ollama::OllamaProvider;
use crate::translate::openai_compat::OpenAICompatProvider;
use crate::translate::provider::{TranslateError, TranslateProvider};
use crate::{
    config::{self, ProviderConfig},
    AppState,
};
use std::sync::Arc;
use tauri::State;

fn translate_error_kind(error: &TranslateError) -> &'static str {
    match error {
        TranslateError::Network(_) => "network",
        TranslateError::Api(_) => "api",
        TranslateError::RateLimited => "rate_limited",
        TranslateError::Config(_) => "config",
    }
}

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
    updates: serde_json::Value,
    state: State<'_, AppState>,
) -> Result<config::AppConfig, String> {
    let result = {
        let mut config = state
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(enabled) = updates.get("enabled").and_then(|v| v.as_bool()) {
            config.enabled = enabled;
        }
        if let Some(debug_logging) = updates.get("debug_logging").and_then(|v| v.as_bool()) {
            config.debug_logging = debug_logging;
        }
        if let Some(selection_trigger_enabled) = updates
            .get("selection_trigger_enabled")
            .and_then(|v| v.as_bool())
        {
            config.selection_trigger_enabled = selection_trigger_enabled;
        }
        if let Some(lang) = updates.get("source_lang").and_then(|v| v.as_str()) {
            config.source_lang = lang.to_string();
        }
        if let Some(lang) = updates.get("target_lang").and_then(|v| v.as_str()) {
            config.target_lang = lang.to_string();
        }
        if let Some(provider) = updates.get("active_provider").and_then(|v| v.as_str()) {
            config.active_provider = provider.to_string();
        }
        if let Some(button) = updates.get("mouse_trigger_button").and_then(|v| v.as_i64()) {
            config.mouse_trigger_button = button;
        }

        config::save_config(&config)?;
        config.clone()
    };

    if updates.get("active_provider").is_some() {
        let mut engine = state.translation_engine.write().await;
        engine.set_active_provider(&result.active_provider);
    }
    if updates.get("mouse_trigger_button").is_some() {
        crate::mouse::listener::set_trigger_button(
            &state.mouse_trigger_state,
            result.mouse_trigger_button,
        );
    }
    if updates.get("selection_trigger_enabled").is_some() {
        crate::mouse::listener::set_selection_trigger_enabled(
            &state.mouse_trigger_state,
            result.selection_trigger_enabled,
        );
    }
    if updates.get("debug_logging").is_some() {
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

#[tauri::command]
pub async fn update_provider(
    name: String,
    base_url: String,
    api_key: String,
    model: String,
    system_prompt: Option<String>,
    user_prompt: Option<String>,
    extra_params: Option<serde_json::Value>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let provider_cfg = {
        let mut config = state
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(p) = config.providers.iter_mut().find(|p| p.name == name) {
            p.base_url = base_url;
            p.api_key = api_key;
            p.model = model;
            if let Some(sp) = system_prompt {
                p.system_prompt = sp;
            }
            if let Some(up) = user_prompt {
                p.user_prompt = up;
            }
            if let Some(ep) = extra_params {
                p.extra_params = ep;
            }
        } else {
            config.providers.push(ProviderConfig {
                name: name.clone(),
                base_url,
                api_key,
                model,
                system_prompt: system_prompt.unwrap_or_else(config::default_system_prompt),
                user_prompt: user_prompt.unwrap_or_else(config::default_user_prompt),
                extra_params: extra_params.unwrap_or_default(),
            });
        }

        config::save_config(&config)?;
        config
            .providers
            .iter()
            .find(|p| p.name == name)
            .cloned()
            .ok_or_else(|| format!("Provider '{}' was not saved", name))?
    };

    log::info!(
        "[provider] Saved provider={} base_url_present={} api_key_present={} model_present={}",
        provider_cfg.name,
        !provider_cfg.base_url.trim().is_empty(),
        !provider_cfg.api_key.trim().is_empty(),
        !provider_cfg.model.trim().is_empty()
    );

    let mut engine = state.translation_engine.write().await;
    engine.reload_provider(&provider_cfg);

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

    let provider: Arc<dyn TranslateProvider> = match provider_name.as_str() {
        "deeplx" => Arc::new(DeepLXProvider::new()),
        "openai" => Arc::new(OpenAICompatProvider::new(
            &provider_config.base_url,
            &provider_config.api_key,
            &provider_config.model,
            &provider_config.system_prompt,
            &provider_config.user_prompt,
            provider_config.extra_params.clone(),
        )),
        "gemini" => Arc::new(GeminiProvider::new(
            &provider_config.base_url,
            &provider_config.api_key,
            &provider_config.model,
            &provider_config.system_prompt,
            &provider_config.user_prompt,
        )),
        "claude" => Arc::new(ClaudeProvider::new(
            &provider_config.base_url,
            &provider_config.api_key,
            &provider_config.model,
            &provider_config.system_prompt,
            &provider_config.user_prompt,
        )),
        "ollama" => Arc::new(OllamaProvider::new(
            &provider_config.base_url,
            &provider_config.model,
            &provider_config.system_prompt,
            &provider_config.user_prompt,
        )),
        "custom_http" => Arc::new(CustomHttpProvider::new(
            &provider_config.base_url,
            &provider_config.api_key,
            std::collections::HashMap::new(),
        )),
        _ => return Err(format!("Unknown provider: {}", provider_name)),
    };

    let result = provider
        .translate("Hello, world!", "en", "zh")
        .await
        .map_err(|e| {
            log::warn!(
                "[provider] Test failed provider={} error_kind={}",
                provider_name,
                translate_error_kind(&e)
            );
            e.to_string()
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
