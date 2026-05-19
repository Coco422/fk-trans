use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
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
                },
                ProviderConfig {
                    name: "openai".into(),
                    base_url: "http://172.16.99.204:3398/v1".into(),
                    api_key: "sk-6kVkKPLJYRYx9nZ2wJHofUO2wF9IEHu1afz8zGfXXbUb8YGg".into(),
                    model: "qwen3.6-27b".into(),
                },
                ProviderConfig {
                    name: "gemini".into(),
                    base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
                    api_key: String::new(),
                    model: "gemini-2.0-flash".into(),
                },
                ProviderConfig {
                    name: "claude".into(),
                    base_url: "https://api.anthropic.com".into(),
                    api_key: String::new(),
                    model: "claude-haiku-4-5-20251001".into(),
                },
                ProviderConfig {
                    name: "ollama".into(),
                    base_url: "http://127.0.0.1:11434".into(),
                    api_key: String::new(),
                    model: "llama3".into(),
                },
                ProviderConfig {
                    name: "custom_http".into(),
                    base_url: String::new(),
                    api_key: String::new(),
                    model: String::new(),
                },
            ],
        }
    }
}
