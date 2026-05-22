use super::provider::{TranslateError, TranslateProvider, TranslateResult};
use reqwest::Client;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

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

    fn response_hash(body: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        body.hash(&mut hasher);
        hasher.finish()
    }

    fn top_level_keys(json: &serde_json::Value) -> String {
        json.as_object()
            .map(|object| object.keys().cloned().collect::<Vec<_>>().join(","))
            .unwrap_or_else(|| "non_object".to_string())
    }

    fn extract_api_error(json: &serde_json::Value) -> Option<String> {
        let error = json.get("error")?;
        if let Some(message) = error.as_str() {
            return Some(message.to_string());
        }

        let message = error
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or("OpenAI-compatible API error");
        let code = error.get("code").and_then(|value| value.as_str());
        let error_type = error.get("type").and_then(|value| value.as_str());

        let mut parts = vec![message.to_string()];
        if let Some(code) = code {
            parts.push(format!("code={}", code));
        }
        if let Some(error_type) = error_type {
            parts.push(format!("type={}", error_type));
        }

        Some(parts.join(" "))
    }

    fn content_value_to_text(value: &serde_json::Value) -> Option<String> {
        if let Some(text) = value
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            return Some(text.to_string());
        }

        let parts = value.as_array()?;
        let text = parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .or_else(|| part.get("content"))
                    .and_then(|value| value.as_str())
            })
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("");

        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    fn response_shape_summary(json: &serde_json::Value) -> String {
        let top_level_keys = Self::top_level_keys(json);
        let choices_len = json
            .get("choices")
            .and_then(|value| value.as_array())
            .map(|choices| choices.len());
        let first_choice_keys = json
            .get("choices")
            .and_then(|value| value.get(0))
            .and_then(|value| value.as_object())
            .map(|object| object.keys().cloned().collect::<Vec<_>>().join(","));
        let message_keys = json
            .get("choices")
            .and_then(|value| value.get(0))
            .and_then(|value| value.get("message"))
            .and_then(|value| value.as_object())
            .map(|object| object.keys().cloned().collect::<Vec<_>>().join(","));
        let content_type = json
            .get("choices")
            .and_then(|value| value.get(0))
            .and_then(|value| value.get("message"))
            .and_then(|value| value.get("content"))
            .map(|value| match value {
                serde_json::Value::Null => "null",
                serde_json::Value::Bool(_) => "bool",
                serde_json::Value::Number(_) => "number",
                serde_json::Value::String(value) if value.trim().is_empty() => "empty_string",
                serde_json::Value::String(_) => "string",
                serde_json::Value::Array(_) => "array",
                serde_json::Value::Object(_) => "object",
            });

        format!(
            "top_level_keys={} choices_len={} first_choice_keys={} message_keys={} content_type={}",
            top_level_keys,
            choices_len
                .map(|value| value.to_string())
                .unwrap_or_else(|| "missing".to_string()),
            first_choice_keys.unwrap_or_else(|| "missing".to_string()),
            message_keys.unwrap_or_else(|| "missing".to_string()),
            content_type.unwrap_or("missing")
        )
    }

    fn http_error_message(status: reqwest::StatusCode, body_text: &str) -> String {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(body_text) {
            if let Some(message) = Self::extract_api_error(&json) {
                return format!("HTTP {}: {}", status, message);
            }
        }

        format!(
            "HTTP {}: non-JSON response bytes={} hash={:016x}",
            status,
            body_text.len(),
            Self::response_hash(body_text)
        )
    }

    fn extract_translated(json: &serde_json::Value) -> Result<String, TranslateError> {
        if let Some(message) = Self::extract_api_error(json) {
            return Err(TranslateError::Api(message));
        }

        if let Some(translated) =
            Self::content_value_to_text(&json["choices"][0]["message"]["content"])
        {
            return Ok(translated);
        }

        if let Some(translated) = json["choices"][0]["text"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Ok(translated.to_string());
        }

        if let Some(translated) = json["output_text"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Ok(translated.to_string());
        }

        Err(TranslateError::Api(format!(
            "OpenAI-compatible response missing translated text ({})",
            Self::response_shape_summary(json)
        )))
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

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        log::debug!(
            "[openai_compat] response status={} bytes={} hash={:016x}",
            status,
            body_text.len(),
            Self::response_hash(&body_text)
        );

        if !status.is_success() {
            return Err(TranslateError::Api(Self::http_error_message(
                status, &body_text,
            )));
        }

        let json: serde_json::Value = serde_json::from_str(&body_text)
            .map_err(|e| TranslateError::Api(format!("Response JSON parse failed: {}", e)))?;
        log::debug!(
            "[openai_compat] response shape {}",
            Self::response_shape_summary(&json)
        );

        let translated = Self::extract_translated(&json)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_openai_compatible_translation() {
        let json = serde_json::json!({
            "choices": [
                { "message": { "content": " 你好 " } }
            ]
        });

        assert_eq!(
            OpenAICompatProvider::extract_translated(&json).unwrap(),
            "你好"
        );
    }

    #[test]
    fn rejects_missing_openai_compatible_content() {
        let json = serde_json::json!({ "message": "pong" });

        let error = OpenAICompatProvider::extract_translated(&json)
            .expect_err("missing content should be an API error")
            .to_string();

        assert!(error.contains("missing translated text"));
        assert!(error.contains("top_level_keys=message"));
    }

    #[test]
    fn extracts_openai_compatible_content_array() {
        let json = serde_json::json!({
            "choices": [
                {
                    "message": {
                        "content": [
                            { "type": "text", "text": " 你" },
                            { "type": "text", "text": "好 " }
                        ]
                    }
                }
            ]
        });

        assert_eq!(
            OpenAICompatProvider::extract_translated(&json).unwrap(),
            "你好"
        );
    }

    #[test]
    fn extracts_legacy_choice_text() {
        let json = serde_json::json!({
            "choices": [
                { "text": " 你好 " }
            ]
        });

        assert_eq!(
            OpenAICompatProvider::extract_translated(&json).unwrap(),
            "你好"
        );
    }

    #[test]
    fn extracts_response_output_text() {
        let json = serde_json::json!({
            "output_text": " 你好 "
        });

        assert_eq!(
            OpenAICompatProvider::extract_translated(&json).unwrap(),
            "你好"
        );
    }

    #[test]
    fn reports_openai_compatible_error_payload() {
        let json = serde_json::json!({
            "error": {
                "message": "No available channel for model qwen3.6-27b",
                "code": "model_not_found",
                "type": "new_api_error"
            }
        });

        let error = OpenAICompatProvider::extract_translated(&json)
            .expect_err("error payload should be returned")
            .to_string();

        assert!(error.contains("No available channel"));
        assert!(error.contains("model_not_found"));
    }

    #[test]
    fn http_error_message_does_not_echo_non_json_body() {
        let message = OpenAICompatProvider::http_error_message(
            reqwest::StatusCode::BAD_GATEWAY,
            "upstream echoed sensitive selected text",
        );

        assert!(message.contains("HTTP 502"));
        assert!(message.contains("bytes="));
        assert!(message.contains("hash="));
        assert!(!message.contains("sensitive selected text"));
    }
}
