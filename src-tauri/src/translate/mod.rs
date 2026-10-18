pub mod http;
pub mod language;
pub mod openai_compat;
pub mod prompt;
pub mod provider;

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use provider::{TranslateError, TranslateProvider, TranslateResult};
use tokio::sync::RwLock;

use crate::config::ProviderConfig;

/// How long a result may be reused for an identical request.
///
/// The cache exists to absorb accidental repeats — a double-triggered click, the
/// same selection translated twice in a moment. It is not a translation memory.
const CACHE_TTL: Duration = Duration::from_secs(5);

/// Upper bound on cached results. They hold the selected text and its
/// translation, and the map used to grow without limit for the life of the app.
const CACHE_MAX_ENTRIES: usize = 64;

struct ProviderEntry {
    provider: Arc<dyn TranslateProvider>,
    /// Distinguishes cached results across settings changes.
    ///
    /// The cache key used to be `text:from:to` alone, so changing the model or
    /// the API key could serve the previous configuration's answer for up to the
    /// TTL. This is a hash of everything that shapes a request.
    cache_salt: String,
}

fn cache_salt(cfg: &ProviderConfig) -> String {
    let mut hasher = DefaultHasher::new();
    cfg.name.hash(&mut hasher);
    cfg.base_url.hash(&mut hasher);
    cfg.model.hash(&mut hasher);
    cfg.system_prompt.hash(&mut hasher);
    cfg.user_prompt.hash(&mut hasher);
    cfg.extra_params.to_string().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Build the provider for a config entry.
///
/// An unknown name is an error. It used to fall through to an
/// `OpenAICompatProvider::default()` pointing at `api.openai.com` with an empty
/// key, so a renamed or misspelled provider silently routed the user's text to a
/// different service than the one they configured.
///
/// Public so the "test this provider" command builds it exactly as the engine
/// does, rather than keeping a second copy of this match in sync.
pub fn build_provider(cfg: &ProviderConfig) -> Result<Arc<dyn TranslateProvider>, TranslateError> {
    match cfg.name.as_str() {
        // Ollama exposes an OpenAI-compatible endpoint at `/v1`, and its
        // `reasoning_effort` field can turn a thinking model's reasoning off, so
        // it needs no implementation of its own — only a preset in the default
        // config. Anything else speaking this protocol is a base_url away.
        "openai" | "ollama" => Ok(Arc::new(openai_compat::OpenAICompatProvider::new(
            &cfg.name,
            &cfg.base_url,
            &cfg.api_key,
            &cfg.model,
            &cfg.system_prompt,
            &cfg.user_prompt,
            cfg.extra_params.clone(),
        ))),
        other => Err(TranslateError::Config(format!(
            "Unknown provider '{other}'. Supported providers are: {}",
            SUPPORTED_PROVIDERS.join(", ")
        ))),
    }
}

/// The names [`build_provider`] accepts. Used for error messages and migration.
pub const SUPPORTED_PROVIDERS: &[&str] = &["openai", "ollama"];

pub struct TranslationEngine {
    providers: HashMap<String, ProviderEntry>,
    active_provider: String,
    dedup_cache: RwLock<HashMap<String, (TranslateResult, Instant)>>,
}

impl TranslationEngine {
    pub fn new(active_provider: String, provider_configs: &[ProviderConfig]) -> Self {
        let mut providers: HashMap<String, ProviderEntry> = HashMap::new();
        for cfg in provider_configs {
            match build_provider(cfg) {
                Ok(provider) => {
                    providers.insert(
                        cfg.name.clone(),
                        ProviderEntry {
                            provider,
                            cache_salt: cache_salt(cfg),
                        },
                    );
                }
                // One unusable entry must not stop the others from loading; the
                // error surfaces when the entry is actually selected.
                Err(error) => {
                    log::warn!("[providers] Skipping '{}': {}", cfg.name, error);
                }
            }
        }

        Self {
            providers,
            active_provider,
            dedup_cache: RwLock::new(HashMap::new()),
        }
    }

    pub async fn set_active_provider(&mut self, name: &str) {
        if self.active_provider != name {
            self.active_provider = name.to_string();
            self.dedup_cache.write().await.clear();
        }
    }

    pub async fn reload_provider(&mut self, cfg: &ProviderConfig) -> Result<(), TranslateError> {
        let provider = build_provider(cfg)?;
        let salt = cache_salt(cfg);

        // Settings that shape the request may have changed, so any cached answer
        // for this provider is stale even though the name is the same.
        self.dedup_cache.write().await.clear();
        self.providers.insert(
            cfg.name.clone(),
            ProviderEntry {
                provider,
                cache_salt: salt,
            },
        );
        Ok(())
    }

    /// Ask a model something that is not a translation.
    ///
    /// `provider` names the entry to use; `None` means the active one. A name
    /// that does not exist is an error rather than a quiet fall back to the
    /// active provider — answering from a service the user did not pick is how a
    /// carefully configured key ends up unused without anyone noticing.
    ///
    /// No system prompt is sent. A provider's own is a *translation*
    /// instruction, and prefixing one to "explain this text" asks the model to
    /// translate the explanation request.
    pub async fn complete(
        &self,
        provider: Option<&str>,
        prompt: &str,
    ) -> Result<String, TranslateError> {
        let name = provider.unwrap_or(&self.active_provider);
        let entry = self
            .providers
            .get(name)
            .ok_or_else(|| TranslateError::Config(format!("Provider '{name}' not found")))?;

        entry.provider.complete("", prompt).await
    }

    pub async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<TranslateResult, TranslateError> {
        let entry = self.providers.get(&self.active_provider).ok_or_else(|| {
            TranslateError::Config(format!("Provider '{}' not found", self.active_provider))
        })?;

        let cache_key = format!("{}:{}:{}:{}", entry.cache_salt, from, to, text);

        {
            let cache = self.dedup_cache.read().await;
            if let Some((result, cached_at)) = cache.get(&cache_key) {
                if cached_at.elapsed() < CACHE_TTL {
                    log::debug!("[translate] Reusing a result cached {:.1}s ago", cached_at.elapsed().as_secs_f32());
                    return Ok(result.clone());
                }
            }
        }

        let result = entry.provider.translate(text, from, to).await?;

        let mut cache = self.dedup_cache.write().await;
        // Drop anything already past its TTL before considering the size limit, so
        // a burst of distinct requests cannot evict the entries that still matter.
        cache.retain(|_, (_, cached_at)| cached_at.elapsed() < CACHE_TTL);
        if cache.len() >= CACHE_MAX_ENTRIES {
            if let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, (_, cached_at))| *cached_at)
                .map(|(key, _)| key.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(cache_key, (result.clone(), Instant::now()));

        Ok(result)
    }
}
