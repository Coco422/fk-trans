pub mod provider;
pub mod deeplx;
pub mod openai_compat;
pub mod gemini;
pub mod claude;
pub mod ollama;
pub mod custom_http;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use provider::{TranslateError, TranslateProvider, TranslateResult};
use tokio::sync::RwLock;

pub struct TranslationEngine {
    providers: HashMap<String, Arc<dyn TranslateProvider>>,
    active_provider: String,
    dedup_cache: RwLock<HashMap<String, (TranslateResult, Instant)>>,
}

impl TranslationEngine {
    pub fn new(active_provider: String) -> Self {
        let mut providers: HashMap<String, Arc<dyn TranslateProvider>> = HashMap::new();
        providers.insert("deeplx".into(), Arc::new(deeplx::DeepLXProvider::new()));
        providers.insert(
            "openai".into(),
            Arc::new(openai_compat::OpenAICompatProvider::default()),
        );
        providers.insert(
            "gemini".into(),
            Arc::new(gemini::GeminiProvider::default()),
        );
        providers.insert(
            "claude".into(),
            Arc::new(claude::ClaudeProvider::default()),
        );
        providers.insert(
            "ollama".into(),
            Arc::new(ollama::OllamaProvider::default()),
        );
        providers.insert(
            "custom_http".into(),
            Arc::new(custom_http::CustomHttpProvider::default()),
        );

        Self {
            providers,
            active_provider,
            dedup_cache: RwLock::new(HashMap::new()),
        }
    }

    pub fn set_active_provider(&mut self, name: &str) {
        self.active_provider = name.to_string();
    }

    pub fn update_provider_config(
        &mut self,
        name: &str,
        provider: Arc<dyn TranslateProvider>,
    ) {
        self.providers.insert(name.to_string(), provider);
    }

    pub async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<TranslateResult, TranslateError> {
        let cache_key = format!("{}:{}:{}", text, from, to);

        {
            let cache = self.dedup_cache.read().await;
            if let Some((result, ts)) = cache.get(&cache_key) {
                if ts.elapsed() < Duration::from_secs(5) {
                    return Ok(result.clone());
                }
            }
        }

        let provider = self
            .providers
            .get(&self.active_provider)
            .ok_or_else(|| {
                TranslateError::Config(format!(
                    "Provider '{}' not found",
                    self.active_provider
                ))
            })?;

        let result = provider.translate(text, from, to).await?;

        self.dedup_cache
            .write()
            .await
            .insert(cache_key, (result.clone(), Instant::now()));

        Ok(result)
    }

    pub async fn translate_with_provider(
        &self,
        provider_name: &str,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<TranslateResult, TranslateError> {
        let provider = self
            .providers
            .get(provider_name)
            .ok_or_else(|| {
                TranslateError::Config(format!(
                    "Provider '{}' not found",
                    provider_name
                ))
            })?;

        provider.translate(text, from, to).await
    }
}
