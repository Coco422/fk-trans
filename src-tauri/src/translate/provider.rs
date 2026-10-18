use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types.ts")]
pub struct TranslateResult {
    pub original: String,
    pub translated: String,
    pub source_lang: String,
    pub target_lang: String,
    pub provider: String,
}

#[derive(Debug, Error)]
pub enum TranslateError {
    #[error("Network error: {0}")]
    Network(String),
    #[error("API error: {0}")]
    Api(String),
    #[error("Rate limited: {0}")]
    RateLimited(String),
    #[error("Invalid configuration: {0}")]
    Config(String),
}

impl TranslateError {
    /// A stable label for diagnostics and reporting.
    ///
    /// This match used to be written out twice, verbatim, in `lib.rs` and
    /// `commands/settings.rs`.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Network(_) => "network",
            Self::Api(_) => "api",
            Self::RateLimited(_) => "rate_limited",
            Self::Config(_) => "config",
        }
    }
}

#[async_trait::async_trait]
pub trait TranslateProvider: Send + Sync {
    async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<TranslateResult, TranslateError>;

    /// Ask the model something that is not a translation.
    ///
    /// Separate from `translate` because a prompt is an instruction, not text to
    /// be translated. The popup's actions used to go through `translate`, which
    /// wrapped the instruction in the translation template — so the model was
    /// asked to translate its own instruction, and answered with a translation
    /// of it.
    ///
    /// `system` may be empty, which means the provider should send no system
    /// message at all rather than an empty one.
    async fn complete(&self, system: &str, user: &str) -> Result<String, TranslateError>;
}
