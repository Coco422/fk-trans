use super::provider::{TranslateError, TranslateProvider, TranslateResult};
use reqwest::Client;

pub struct OpenAICompatProvider {
    base_url: String,
    api_key: String,
    model: String,
    system_prompt: String,
    user_prompt: String,
    extra_params: serde_json::Value,
    client: Client,
}

impl Default for OpenAICompatProvider {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            model: "gpt-4.1-mini".to_string(),
            system_prompt: "You are a translator. Translate the following text from {from} to {to}. Output ONLY the translation, nothing else.".to_string(),
            user_prompt: "{text}".to_string(),
            extra_params: serde_json::json!({"enable_thinking": false}),
            client: Client::new(),
        }
    }
}

impl OpenAICompatProvider {
    pub fn new(
        base_url: &str,
        api_key: &str,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
        extra_params: serde_json::Value,
    ) -> Self {
        Self {
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            system_prompt: system_prompt.to_string(),
            user_prompt: user_prompt.to_string(),
            extra_params,
            client: Client::new(),
        }
    }

    fn format_prompt(template: &str, from: &str, to: &str, text: &str) -> String {
        let from_label = if from == "auto" { "the detected language" } else { from };
        template
            .replace("{from}", from_label)
            .replace("{to}", to)
            .replace("{text}", text)
    }
}

#[async_trait::async_trait]
impl TranslateProvider for OpenAICompatProvider {
    async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<TranslateResult, TranslateError> {
        if self.api_key.is_empty() {
            return Err(TranslateError::Config(
                "OpenAI API key not configured".into(),
            ));
        }

        let system_content = Self::format_prompt(&self.system_prompt, from, to, text);
        let user_content = Self::format_prompt(&self.user_prompt, from, to, text);

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system_content },
                { "role": "user", "content": user_content }
            ],
            "temperature": 0.3,
            "max_tokens": 2048
        });

        // Merge extra_params into the request body
        if let Some(obj) = self.extra_params.as_object() {
            if let Some(body_obj) = body.as_object_mut() {
                for (k, v) in obj {
                    body_obj.insert(k.clone(), v.clone());
                }
            }
        }

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
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

        let translated = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();

        Ok(TranslateResult {
            original: text.to_string(),
            translated,
            source_lang: from.to_string(),
            target_lang: to.to_string(),
            provider: "openai".into(),
            alternatives: vec![],
        })
    }
}
