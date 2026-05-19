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
pub struct AppConfig {
    pub enabled: bool,
    pub source_lang: String,
    pub target_lang: String,
    pub active_provider: String,
    pub providers: Vec<ProviderConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            source_lang: "auto".to_string(),
            target_lang: "zh".to_string(),
            active_provider: "openai".to_string(),
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
                    base_url: "http://172.16.99.204:3398/v1".into(),
                    api_key: "sk-6kVkKPLJYRYx9nZ2wJHofUO2wF9IEHu1afz8zGfXXbUb8YGg".into(),
                    model: "qwen3.6-27b".into(),
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

pub fn save_config(config: &AppConfig) {
    let path = config_path();
    if let Ok(json) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(path, json);
    }
}
