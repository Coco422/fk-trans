use crate::config::{self, ProviderConfig};
use crate::AppState;
use std::sync::Arc;
use tauri::State;
use crate::translate::openai_compat::OpenAICompatProvider;
use crate::translate::deeplx::DeepLXProvider;
use crate::translate::gemini::GeminiProvider;
use crate::translate::claude::ClaudeProvider;
use crate::translate::ollama::OllamaProvider;
use crate::translate::custom_http::CustomHttpProvider;
use crate::translate::provider::TranslateProvider;

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<config::AppConfig, String> {
    let config = state.config.lock().unwrap();
    Ok(config.clone())
}

#[tauri::command]
pub async fn update_config(
    updates: serde_json::Value,
    state: State<'_, AppState>,
) -> Result<config::AppConfig, String> {
    let result = {
        let mut config = state.config.lock().unwrap();

        if let Some(enabled) = updates.get("enabled").and_then(|v| v.as_bool()) {
            config.enabled = enabled;
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

        config::save_config(&config);
        config.clone()
    };

    // Rebuild engine if active provider changed
    if updates.get("active_provider").is_some() {
        let mut engine = state.translation_engine.write().await;
        engine.set_active_provider(&result.active_provider);
    }

    Ok(result)
}

#[tauri::command]
pub async fn update_provider(
    name: String,
    base_url: String,
    api_key: String,
    model: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let provider_cfg = {
        let mut config = state.config.lock().unwrap();

        if let Some(p) = config.providers.iter_mut().find(|p| p.name == name) {
            p.base_url = base_url;
            p.api_key = api_key;
            p.model = model;
        } else {
            config.providers.push(ProviderConfig {
                name: name.clone(),
                base_url,
                api_key,
                model,
            });
        }

        config::save_config(&config);
        config.providers.iter().find(|p| p.name == name).unwrap().clone()
    };

    // Rebuild this provider in the engine
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
        let config = state.config.lock().unwrap();
        config
            .providers
            .iter()
            .find(|p| p.name == provider_name)
            .ok_or_else(|| format!("Provider '{}' not found", provider_name))?
            .clone()
    };

    let provider: Arc<dyn TranslateProvider> = match provider_name.as_str() {
        "deeplx" => Arc::new(DeepLXProvider::new()),
        "openai" => Arc::new(OpenAICompatProvider::new(
            &provider_config.base_url,
            &provider_config.api_key,
            &provider_config.model,
        )),
        "gemini" => Arc::new(GeminiProvider::new(
            &provider_config.base_url,
            &provider_config.api_key,
            &provider_config.model,
        )),
        "claude" => Arc::new(ClaudeProvider::new(
            &provider_config.base_url,
            &provider_config.api_key,
            &provider_config.model,
        )),
        "ollama" => Arc::new(OllamaProvider::new(
            &provider_config.base_url,
            &provider_config.model,
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
        .map_err(|e| e.to_string())?;

    Ok(format!(
        "Test successful!\nTranslated: {}",
        result.translated
    ))
}
