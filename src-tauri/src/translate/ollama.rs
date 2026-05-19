use super::provider::{TranslateError, TranslateProvider, TranslateResult};
use reqwest::Client;

pub struct OllamaProvider {
    base_url: String,
    model: String,
    client: Client,
}

impl Default for OllamaProvider {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:11434".to_string(),
            model: "llama3".to_string(),
            client: Client::new(),
        }
    }
}

impl OllamaProvider {
    pub fn new(base_url: &str, model: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            model: model.to_string(),
            client: Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl TranslateProvider for OllamaProvider {
    async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<TranslateResult, TranslateError> {
        let prompt = format!(
            "Translate the following text from {} to {}. Output ONLY the translation.\n\n{}",
            if from == "auto" { "the detected language" } else { from },
            to,
            text
        );

        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "user", "content": prompt }
            ],
            "stream": false
        });

        let resp = self
            .client
            .post(format!("{}/api/chat", self.base_url))
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

        let translated = json["message"]["content"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();

        Ok(TranslateResult {
            original: text.to_string(),
            translated,
            source_lang: from.to_string(),
            target_lang: to.to_string(),
            provider: "ollama".into(),
            alternatives: vec![],
        })
    }
}
