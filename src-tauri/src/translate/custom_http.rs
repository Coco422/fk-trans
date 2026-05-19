use super::provider::{TranslateError, TranslateProvider, TranslateResult};
use reqwest::Client;
use std::collections::HashMap;

pub struct CustomHttpProvider {
    base_url: String,
    api_key: String,
    headers: HashMap<String, String>,
    client: Client,
}

impl Default for CustomHttpProvider {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            api_key: String::new(),
            headers: HashMap::new(),
            client: Client::new(),
        }
    }
}

impl CustomHttpProvider {
    pub fn new(base_url: &str, api_key: &str, headers: HashMap<String, String>) -> Self {
        Self {
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            headers,
            client: Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl TranslateProvider for CustomHttpProvider {
    async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<TranslateResult, TranslateError> {
        if self.base_url.is_empty() {
            return Err(TranslateError::Config(
                "Custom HTTP endpoint not configured".into(),
            ));
        }

        let url = self
            .base_url
            .replace("{text}", &urlencoding::encode(text))
            .replace("{from}", from)
            .replace("{to}", to);

        let body = serde_json::json!({
            "text": text,
            "from": from,
            "to": to
        });

        let mut req = self.client.post(&url).json(&body);

        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        for (k, v) in &self.headers {
            req = req.header(k, v);
        }

        let resp = req
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

        let translated = json["translated"]
            .as_str()
            .or_else(|| json["result"].as_str())
            .or_else(|| json["data"].as_str())
            .or_else(|| json["text"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(TranslateResult {
            original: text.to_string(),
            translated,
            source_lang: from.to_string(),
            target_lang: to.to_string(),
            provider: "custom_http".into(),
            alternatives: vec![],
        })
    }
}
