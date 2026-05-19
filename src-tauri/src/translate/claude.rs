use super::provider::{TranslateError, TranslateProvider, TranslateResult};
use reqwest::Client;

pub struct ClaudeProvider {
    base_url: String,
    api_key: String,
    model: String,
    client: Client,
}

impl Default for ClaudeProvider {
    fn default() -> Self {
        Self {
            base_url: "https://api.anthropic.com".to_string(),
            api_key: String::new(),
            model: "claude-haiku-4-5-20251001".to_string(),
            client: Client::new(),
        }
    }
}

impl ClaudeProvider {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            client: Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl TranslateProvider for ClaudeProvider {
    async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<TranslateResult, TranslateError> {
        if self.api_key.is_empty() {
            return Err(TranslateError::Config(
                "Claude API key not configured".into(),
            ));
        }

        let prompt = format!(
            "Translate the following text from {} to {}. Output ONLY the translation.\n\n{}",
            if from == "auto" { "the detected language" } else { from },
            to,
            text
        );

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 2048,
            "messages": [
                { "role": "user", "content": prompt }
            ]
        });

        let resp = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| TranslateError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            return Err(TranslateError::Api(format!("HTTP {}: {}", status, body_text)));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| TranslateError::Api(e.to_string()))?;

        let translated = json["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();

        Ok(TranslateResult {
            original: text.to_string(),
            translated,
            source_lang: from.to_string(),
            target_lang: to.to_string(),
            provider: "claude".into(),
            alternatives: vec![],
        })
    }
}
