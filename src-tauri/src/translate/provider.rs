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
    #[error("Rate limited")]
    RateLimited,
    #[error("Invalid configuration: {0}")]
    Config(String),
}

#[async_trait::async_trait]
pub trait TranslateProvider: Send + Sync {
    async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<TranslateResult, TranslateError>;
}
