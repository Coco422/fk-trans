use super::provider::{TranslateError, TranslateProvider, TranslateResult};
use reqwest::Client;

pub struct DeepLXProvider {
    base_url: String,
    client: Client,
}

impl DeepLXProvider {
    pub fn new() -> Self {
        Self {
            base_url: "http://127.0.0.1:1188".to_string(),
            client: Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl TranslateProvider for DeepLXProvider {
    async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<TranslateResult, TranslateError> {
        let from = if from == "auto" { "auto" } else { from };
        let to = to.to_uppercase();

        let body = serde_json::json!({
            "text": text,
            "source_lang": from.to_uppercase(),
            "target_lang": to,
        });

        let resp = self
            .client
            .post(format!("{}/translate", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| TranslateError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(TranslateError::Api(format!(
                "HTTP {}",
                resp.status()
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| TranslateError::Api(e.to_string()))?;

        let translated = json["data"]
            .as_str()
            .or_else(|| json["translations"][0]["text"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(TranslateResult {
            original: text.to_string(),
            translated,
            source_lang: from.to_string(),
            target_lang: to.to_lowercase(),
            provider: "deeplx".into(),
            alternatives: vec![],
        })
    }
}
