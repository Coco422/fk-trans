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
            extra_params: serde_json::json!({
                "chat_template_kwargs": { "enable_thinking": false }
            }),
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
        let from_label = if from == "auto" {
            "the detected language"
        } else {
            from
        };
        template
            .replace("{from}", from_label)
            .replace("{to}", to)
            .replace("{text}", text)
    }

    fn merge_extra_params(body: &mut serde_json::Value, extra_params: &serde_json::Value) {
        let Some(body_obj) = body.as_object_mut() else {
            return;
        };
        let Some(extra_obj) = extra_params.as_object() else {
            return;
        };

        let mut legacy_enable_thinking = None;
        let mut nested_enable_thinking_set = false;

        for (key, value) in extra_obj {
            match key.as_str() {
                // Older configs used the top-level Qwen flag. vLLM expects this under
                // chat_template_kwargs, so do not forward the legacy key directly.
                "enable_thinking" => {
                    legacy_enable_thinking = Some(value.clone());
                }
                "chat_template_kwargs" => {
                    if let Some(incoming_obj) = value.as_object() {
                        nested_enable_thinking_set = incoming_obj.contains_key("enable_thinking");
                        let target = body_obj
                            .entry("chat_template_kwargs".to_string())
                            .or_insert_with(|| serde_json::json!({}));

                        if let Some(target_obj) = target.as_object_mut() {
                            for (nested_key, nested_value) in incoming_obj {
                                target_obj.insert(nested_key.clone(), nested_value.clone());
                            }
                        } else {
                            body_obj.insert(key.clone(), value.clone());
                        }
                    } else {
                        body_obj.insert(key.clone(), value.clone());
                    }
                }
                _ => {
                    body_obj.insert(key.clone(), value.clone());
                }
            }
        }

        if let Some(value) = legacy_enable_thinking {
            if !nested_enable_thinking_set {
                let target = body_obj
                    .entry("chat_template_kwargs".to_string())
                    .or_insert_with(|| serde_json::json!({}));
                if let Some(target_obj) = target.as_object_mut() {
                    target_obj.insert("enable_thinking".to_string(), value);
                }
            }
        }
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
            "max_tokens": 2048,
            "chat_template_kwargs": {
                "enable_thinking": false
            }
        });

        // Merge extra_params into the request body
        Self::merge_extra_params(&mut body, &self.extra_params);

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
            return Err(TranslateError::Api(format!(
                "HTTP {}: {}",
                status, body_text
            )));
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
