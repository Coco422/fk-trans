use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    #[serde(default = "default_user_prompt")]
    pub user_prompt: String,
    #[serde(default)]
    pub extra_params: serde_json::Value,
}

pub fn default_system_prompt() -> String {
    "You are a translator. Translate the following text from {from} to {to}. Output ONLY the translation, nothing else.".to_string()
}

pub fn default_user_prompt() -> String {
    "{text}".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionPromptConfig {
    #[serde(default = "default_explain_prompt")]
    pub explain: String,
    #[serde(default = "default_summary_prompt")]
    pub summary: String,
    #[serde(default = "default_polish_prompt")]
    pub polish: String,
    #[serde(default = "default_dict_prompt")]
    pub dict: String,
}

impl Default for ActionPromptConfig {
    fn default() -> Self {
        Self {
            explain: default_explain_prompt(),
            summary: default_summary_prompt(),
            polish: default_polish_prompt(),
            dict: default_dict_prompt(),
        }
    }
}

pub fn default_ai_action_system_prompt() -> String {
    "You are a precise language assistant. Follow the user's requested action exactly. Answer in {to} unless the action explicitly asks for another language.".to_string()
}

pub fn default_explain_prompt() -> String {
    "Explain the meaning, context, nuance, and usage of this text in {to}. Keep the answer useful and concrete.\n\nText:\n{text}".to_string()
}

pub fn default_summary_prompt() -> String {
    "Summarize this text concisely in {to}. Preserve the key points and omit filler.\n\nText:\n{text}".to_string()
}

pub fn default_polish_prompt() -> String {
    "Rewrite this text to be clearer, smoother, and more natural while preserving the original meaning. Keep the appropriate language and tone; if the source is not {to}, provide the polished version in {to}.\n\nText:\n{text}".to_string()
}

pub fn default_dict_prompt() -> String {
    "Create a dictionary-style entry for this word or phrase in {to}. Include pronunciation if inferable, part of speech, concise definitions, usage notes, and 2 short examples.\n\nText:\n{text}".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub enabled: bool,
    #[serde(default)]
    pub debug_logging: bool,
    #[serde(default = "default_ocr_enabled")]
    pub ocr_enabled: bool,
    #[serde(default = "default_selection_trigger_enabled")]
    pub selection_trigger_enabled: bool,
    pub source_lang: String,
    pub target_lang: String,
    pub active_provider: String,
    #[serde(default = "default_mouse_trigger_button")]
    pub mouse_trigger_button: i64,
    #[serde(default)]
    pub action_prompts: ActionPromptConfig,
    pub providers: Vec<ProviderConfig>,
}

pub fn default_mouse_trigger_button() -> i64 {
    2
}

pub fn default_ocr_enabled() -> bool {
    true
}

pub fn default_selection_trigger_enabled() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            debug_logging: false,
            ocr_enabled: default_ocr_enabled(),
            selection_trigger_enabled: default_selection_trigger_enabled(),
            source_lang: "auto".to_string(),
            target_lang: "zh".to_string(),
            active_provider: "openai".to_string(),
            mouse_trigger_button: default_mouse_trigger_button(),
            action_prompts: ActionPromptConfig::default(),
            providers: vec![
                ProviderConfig {
                    name: "deeplx".into(),
                    base_url: "http://127.0.0.1:1188".into(),
                    api_key: String::new(),
                    model: String::new(),
                    system_prompt: String::new(),
                    user_prompt: String::new(),
                    extra_params: serde_json::json!({}),
                },
                ProviderConfig {
                    name: "openai".into(),
                    base_url: "https://api.openai.com/v1".into(),
                    api_key: String::new(),
                    model: "gpt-4o-mini".into(),
                    system_prompt: default_system_prompt(),
                    user_prompt: default_user_prompt(),
                    extra_params: serde_json::json!({
                        "chat_template_kwargs": { "enable_thinking": false }
                    }),
                },
                ProviderConfig {
                    name: "gemini".into(),
                    base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
                    api_key: String::new(),
                    model: "gemini-2.0-flash".into(),
                    system_prompt: default_system_prompt(),
                    user_prompt: default_user_prompt(),
                    extra_params: serde_json::json!({}),
                },
                ProviderConfig {
                    name: "claude".into(),
                    base_url: "https://api.anthropic.com".into(),
                    api_key: String::new(),
                    model: "claude-haiku-4-5-20251001".into(),
                    system_prompt: default_system_prompt(),
                    user_prompt: default_user_prompt(),
                    extra_params: serde_json::json!({}),
                },
                ProviderConfig {
                    name: "ollama".into(),
                    base_url: "http://127.0.0.1:11434".into(),
                    api_key: String::new(),
                    model: "llama3".into(),
                    system_prompt: default_system_prompt(),
                    user_prompt: default_user_prompt(),
                    extra_params: serde_json::json!({}),
                },
                ProviderConfig {
                    name: "custom_http".into(),
                    base_url: String::new(),
                    api_key: String::new(),
                    model: String::new(),
                    system_prompt: String::new(),
                    user_prompt: String::new(),
                    extra_params: serde_json::json!({}),
                },
            ],
        }
    }
}

fn is_blank(value: &str) -> bool {
    value.trim().is_empty()
}

pub fn validate_provider(provider: &ProviderConfig) -> Result<(), String> {
    match provider.name.as_str() {
        "deeplx" => {
            if is_blank(&provider.base_url) {
                Err("DeepLX base URL is not configured".to_string())
            } else {
                Ok(())
            }
        }
        "ollama" => {
            if is_blank(&provider.base_url) {
                Err("Ollama base URL is not configured".to_string())
            } else if is_blank(&provider.model) {
                Err("Ollama model is not configured".to_string())
            } else {
                Ok(())
            }
        }
        "custom_http" => {
            if is_blank(&provider.base_url) {
                Err("Custom HTTP endpoint is not configured".to_string())
            } else {
                Ok(())
            }
        }
        "openai" | "gemini" | "claude" => {
            if is_blank(&provider.base_url) {
                Err(format!("{} base URL is not configured", provider.name))
            } else if is_blank(&provider.api_key) {
                Err(format!("{} API key is not configured", provider.name))
            } else if is_blank(&provider.model) {
                Err(format!("{} model is not configured", provider.name))
            } else {
                Ok(())
            }
        }
        _ => Err(format!("Unknown provider: {}", provider.name)),
    }
}

pub fn validate_active_provider(config: &AppConfig) -> Result<(), String> {
    let provider = config
        .providers
        .iter()
        .find(|provider| provider.name == config.active_provider)
        .ok_or_else(|| format!("Provider '{}' not found", config.active_provider))?;

    validate_provider(provider)
}

fn config_path() -> PathBuf {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("fk-trans");
    std::fs::create_dir_all(&dir).ok();
    dir.join("config.json")
}

pub fn load_config() -> AppConfig {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => AppConfig::default(),
    }
}

pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let path = config_path();
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    std::fs::write(path, json).map_err(|e| format!("Failed to write config: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(name: &str, base_url: &str, api_key: &str, model: &str) -> ProviderConfig {
        ProviderConfig {
            name: name.to_string(),
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            system_prompt: default_system_prompt(),
            user_prompt: default_user_prompt(),
            extra_params: serde_json::json!({}),
        }
    }

    #[test]
    fn mouse_trigger_button_defaults_to_middle_button() {
        let config = AppConfig::default();

        assert_eq!(config.mouse_trigger_button, 2);
    }

    #[test]
    fn debug_logging_defaults_to_off() {
        let config = AppConfig::default();

        assert!(!config.debug_logging);
        assert!(config.selection_trigger_enabled);
    }

    #[test]
    fn legacy_config_without_mouse_trigger_button_uses_default() {
        let json = r#"{
            "enabled": true,
            "source_lang": "auto",
            "target_lang": "zh",
            "active_provider": "openai",
            "providers": []
        }"#;

        let config: AppConfig = serde_json::from_str(json).unwrap();

        assert_eq!(config.mouse_trigger_button, 2);
        assert!(!config.debug_logging);
        assert!(config.ocr_enabled);
        assert!(config.selection_trigger_enabled);
    }

    #[test]
    fn debug_logging_mouse_trigger_button_and_ocr_round_trip_through_json() {
        let config = AppConfig {
            debug_logging: true,
            ocr_enabled: false,
            selection_trigger_enabled: false,
            mouse_trigger_button: 4,
            action_prompts: ActionPromptConfig {
                summary: "Summarize {text} in {to}".to_string(),
                ..ActionPromptConfig::default()
            },
            ..AppConfig::default()
        };

        let json = serde_json::to_string(&config).unwrap();
        let loaded: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.mouse_trigger_button, 4);
        assert!(loaded.debug_logging);
        assert!(!loaded.ocr_enabled);
        assert!(!loaded.selection_trigger_enabled);
        assert_eq!(loaded.action_prompts.summary, "Summarize {text} in {to}");
        assert!(loaded.action_prompts.explain.contains("{text}"));
    }

    #[test]
    fn legacy_config_without_action_prompts_uses_defaults() {
        let json = r#"{
            "enabled": true,
            "source_lang": "auto",
            "target_lang": "zh",
            "active_provider": "openai",
            "providers": []
        }"#;

        let config: AppConfig = serde_json::from_str(json).unwrap();

        assert!(config.action_prompts.explain.contains("{text}"));
        assert!(config.action_prompts.dict.contains("dictionary-style"));
    }

    #[test]
    fn default_config_is_not_ready_without_openai_key() {
        let config = AppConfig::default();

        assert!(validate_active_provider(&config).is_err());
    }

    #[test]
    fn missing_active_provider_is_not_ready() {
        let config = AppConfig {
            active_provider: "missing".to_string(),
            ..AppConfig::default()
        };

        assert!(validate_active_provider(&config).is_err());
    }

    #[test]
    fn openai_compatible_config_is_ready_with_key_model_and_base_url() {
        let config = AppConfig {
            active_provider: "openai".to_string(),
            providers: vec![provider(
                "openai",
                "https://api.example.com/v1",
                "sk-test",
                "model",
            )],
            ..AppConfig::default()
        };

        assert!(validate_active_provider(&config).is_ok());
    }

    #[test]
    fn local_providers_are_ready_with_local_endpoint_requirements() {
        let deeplx = AppConfig {
            active_provider: "deeplx".to_string(),
            providers: vec![provider("deeplx", "http://127.0.0.1:1188", "", "")],
            ..AppConfig::default()
        };
        let ollama = AppConfig {
            active_provider: "ollama".to_string(),
            providers: vec![provider("ollama", "http://127.0.0.1:11434", "", "llama3")],
            ..AppConfig::default()
        };
        let custom_http = AppConfig {
            active_provider: "custom_http".to_string(),
            providers: vec![provider(
                "custom_http",
                "http://127.0.0.1:8080/translate",
                "",
                "",
            )],
            ..AppConfig::default()
        };

        assert!(validate_active_provider(&deeplx).is_ok());
        assert!(validate_active_provider(&ollama).is_ok());
        assert!(validate_active_provider(&custom_http).is_ok());
    }

    #[test]
    fn local_providers_reject_missing_required_fields() {
        let ollama = AppConfig {
            active_provider: "ollama".to_string(),
            providers: vec![provider("ollama", "http://127.0.0.1:11434", "", "")],
            ..AppConfig::default()
        };
        let custom_http = AppConfig {
            active_provider: "custom_http".to_string(),
            providers: vec![provider("custom_http", "", "", "")],
            ..AppConfig::default()
        };

        assert!(validate_active_provider(&ollama).is_err());
        assert!(validate_active_provider(&custom_http).is_err());
    }
}
