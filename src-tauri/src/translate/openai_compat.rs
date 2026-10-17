use super::http::{require_translation, send, top_level_keys};
use super::prompt::{format_prompt, PromptVars};
use super::provider::{TranslateError, TranslateProvider, TranslateResult};
use reqwest::Client;

/// Any service that speaks the OpenAI chat-completions API.
///
/// This is the provider most setups end up using: OpenAI itself, but also vLLM,
/// LM Studio, one-api, and Ollama (whose `/v1/chat/completions` is compatible).
/// Those are shipped as presets in the default config rather than as separate
/// implementations — the six providers that used to exist here each carried their
/// own copy of the same request-and-parse plumbing.
pub struct OpenAICompatProvider {
    /// Shown in the popup. Comes from the config entry, so a provider renamed in
    /// settings reports its own name instead of always claiming to be OpenAI.
    label: String,
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
            label: "openai".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            model: "gpt-4.1-mini".to_string(),
            system_prompt: crate::config::default_system_prompt(),
            user_prompt: crate::config::default_user_prompt(),
            extra_params: crate::config::default_extra_params(),
            client: super::http::shared_client(),
        }
    }
}

impl OpenAICompatProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        label: &str,
        base_url: &str,
        api_key: &str,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
        extra_params: serde_json::Value,
    ) -> Self {
        Self {
            label: label.to_string(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            system_prompt: system_prompt.to_string(),
            user_prompt: user_prompt.to_string(),
            extra_params,
            client: super::http::shared_client(),
        }
    }

    /// Fold `extra_params` into the request body.
    ///
    /// `extra_params` is how a preset expresses what its endpoint needs, so this
    /// is the only place provider-specific request fields come from. Previously
    /// `chat_template_kwargs.enable_thinking = false` — a vLLM flag — was baked
    /// into the base body of every request, including ones to OpenAI, which has
    /// no idea what it means.
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

    /// A `message.content` that is either a string or an array of parts.
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

    /// Enough of the response's shape to diagnose a missing translation, without
    /// quoting it.
    fn response_shape_summary(json: &serde_json::Value) -> String {
        let choices_len = json
            .get("choices")
            .and_then(|value| value.as_array())
            .map(|choices| choices.len().to_string())
            .unwrap_or_else(|| "missing".to_string());
        let message_keys = json
            .get("choices")
            .and_then(|value| value.get(0))
            .and_then(|value| value.get("message"))
            .and_then(|value| value.as_object())
            .map(|object| object.keys().cloned().collect::<Vec<_>>().join(","))
            .unwrap_or_else(|| "missing".to_string());
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
            "top_level_keys={} choices_len={choices_len} message_keys={message_keys} content_type={} reasoning_chars={}",
            top_level_keys(json),
            content_type.unwrap_or("missing"),
            json["choices"][0]["message"]["reasoning"]
                .as_str()
                .map(|reasoning| reasoning.chars().count().to_string())
                .unwrap_or_else(|| "none".to_string())
        )
    }

    /// Pull the translation out of a successful response.
    ///
    /// A 200 response can still carry an application-level error, and several
    /// shapes are accepted because "OpenAI-compatible" is a loose description:
    /// `choices[0].message.content` (string or parts), the legacy
    /// `choices[0].text`, and the responses-style top-level `output_text`.
    fn extract_translated(json: &serde_json::Value) -> Result<String, TranslateError> {
        if json.get("error").is_some() {
            return Err(TranslateError::Api(
                super::http::error_message(reqwest::StatusCode::OK, &json.to_string()),
            ));
        }

        let extracted = Self::content_value_to_text(&json["choices"][0]["message"]["content"])
            .or_else(|| {
                json["choices"][0]["text"]
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .or_else(|| {
                json["output_text"]
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            });

        // A thinking model puts its reasoning in `reasoning` and leaves `content`
        // empty until it finishes — and with a small `max_tokens` budget it can
        // spend the whole budget there and never produce content at all. Verified
        // against Ollama: without `reasoning_effort: "none"`, gemma4 answered a
        // one-word request with 124 characters of reasoning and an empty
        // translation. Say what to do about it rather than showing a shape dump.
        if extracted.is_none() {
            if let Some(reasoning) = json["choices"][0]["message"]["reasoning"]
                .as_str()
                .filter(|reasoning| !reasoning.trim().is_empty())
            {
                return Err(TranslateError::Api(format!(
                    "The model replied with reasoning ({} chars) but no translation: it is in \
                     thinking mode. Turn it off in this provider's Extra Parameters — for Ollama, \
                     {{\"reasoning_effort\": \"none\"}}.",
                    reasoning.chars().count()
                )));
            }
        }

        require_translation(extracted, "OpenAI-compatible", || {
            Self::response_shape_summary(json)
        })
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
        if self.model.trim().is_empty() {
            return Err(TranslateError::Config(
                "No model configured for this provider".into(),
            ));
        }

        let vars = PromptVars { from, to, text };
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": format_prompt(&self.system_prompt, &vars) },
                { "role": "user", "content": format_prompt(&self.user_prompt, &vars) }
            ],
            "temperature": 0.3,
            "max_tokens": 2048
        });

        Self::merge_extra_params(&mut body, &self.extra_params);

        let mut request = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .json(&body);

        // Authentication is optional. Local endpoints — Ollama, LM Studio, a
        // self-hosted vLLM — take no key, and requiring one here is what stopped
        // Ollama from working. An endpoint that does need a key says so itself,
        // in a message we now pass through faithfully.
        let api_key = self.api_key.trim();
        if !api_key.is_empty() {
            request = request.header("Authorization", format!("Bearer {api_key}"));
        }

        let response = send(request).await?;
        log::debug!(
            "[provider] {} status={} bytes={}",
            self.label,
            response.status,
            response.body().len()
        );

        let json = response.json()?;
        let translated = Self::extract_translated(&json)?;

        Ok(TranslateResult {
            original: text.to_string(),
            translated,
            source_lang: from.to_string(),
            target_lang: to.to_string(),
            provider: self.label.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(extra_params: serde_json::Value) -> OpenAICompatProvider {
        OpenAICompatProvider::new(
            "openai",
            "https://example.test/v1/",
            "sk-test",
            "gpt-4o-mini",
            "sys",
            "{text}",
            extra_params,
        )
    }

    #[test]
    fn extracts_openai_compatible_translation() {
        let json = serde_json::json!({
            "choices": [{ "message": { "content": " 你好 " } }]
        });
        assert_eq!(
            OpenAICompatProvider::extract_translated(&json).unwrap(),
            "你好"
        );
    }

    #[test]
    fn extracts_openai_compatible_content_array() {
        let json = serde_json::json!({
            "choices": [{
                "message": { "content": [{ "type": "text", "text": "你好" }, { "type": "text", "text": "世界" }] }
            }]
        });
        assert_eq!(
            OpenAICompatProvider::extract_translated(&json).unwrap(),
            "你好世界"
        );
    }

    #[test]
    fn extracts_legacy_choice_text() {
        let json = serde_json::json!({ "choices": [{ "text": " legacy " }] });
        assert_eq!(
            OpenAICompatProvider::extract_translated(&json).unwrap(),
            "legacy"
        );
    }

    #[test]
    fn extracts_response_output_text() {
        let json = serde_json::json!({ "output_text": "hi" });
        assert_eq!(OpenAICompatProvider::extract_translated(&json).unwrap(), "hi");
    }

    #[test]
    fn rejects_missing_openai_compatible_content() {
        let json = serde_json::json!({ "message": "oops" });
        let error = OpenAICompatProvider::extract_translated(&json)
            .expect_err("must not be an empty success")
            .to_string();
        assert!(error.contains("no translation"), "got: {error}");
        assert!(error.contains("top_level_keys=message"), "got: {error}");
    }

    #[test]
    fn reports_openai_compatible_error_payload() {
        let json = serde_json::json!({
            "error": { "message": "quota exceeded", "code": "insufficient_quota" }
        });
        let error = OpenAICompatProvider::extract_translated(&json)
            .expect_err("an error payload is not a translation")
            .to_string();
        assert!(error.contains("quota exceeded"), "got: {error}");
        assert!(error.contains("code=insufficient_quota"), "got: {error}");
    }

    /// The shape below is the one Ollama actually returned for a thinking model
    /// with no `reasoning_effort`: `content` empty, the answer in `reasoning`.
    /// The message has to name the fix, not just report a shape.
    #[test]
    fn a_reasoning_only_response_says_how_to_turn_thinking_off() {
        let json = serde_json::json!({
            "choices": [{
                "finish_reason": "stop",
                "message": { "role": "assistant", "content": "", "reasoning": "The user wants me to reply with OK, so I should..." }
            }]
        });

        let error = OpenAICompatProvider::extract_translated(&json)
            .expect_err("reasoning is not a translation")
            .to_string();

        assert!(error.contains("thinking mode"), "got: {error}");
        assert!(error.contains("reasoning_effort"), "got: {error}");
    }

    /// A response with neither content nor reasoning is still just missing.
    #[test]
    fn an_empty_message_without_reasoning_reports_the_shape() {
        let json = serde_json::json!({
            "choices": [{ "message": { "role": "assistant", "content": "" } }]
        });

        let error = OpenAICompatProvider::extract_translated(&json)
            .expect_err("must not be a success")
            .to_string();

        assert!(error.contains("no translation"), "got: {error}");
        assert!(error.contains("reasoning_chars=none"), "got: {error}");
    }

    #[test]
    fn the_shape_summary_counts_reasoning_characters() {
        let json = serde_json::json!({
            "choices": [{ "message": { "content": "ok", "reasoning": "abcd" } }]
        });
        let summary = OpenAICompatProvider::response_shape_summary(&json);
        assert!(summary.contains("reasoning_chars=4"), "got: {summary}");
    }

    #[test]
    fn a_trailing_slash_on_the_base_url_does_not_double_up() {
        assert_eq!(provider(serde_json::json!({})).base_url, "https://example.test/v1");
    }

    #[test]
    fn extra_params_land_in_the_request_body() {
        let mut body = serde_json::json!({ "model": "m" });
        OpenAICompatProvider::merge_extra_params(
            &mut body,
            &serde_json::json!({ "reasoning_effort": "none" }),
        );
        assert_eq!(body["reasoning_effort"], "none");
    }

    /// The base body must not carry provider-specific flags: it is sent to OpenAI
    /// too, which has no notion of `chat_template_kwargs`.
    #[test]
    fn the_base_body_carries_no_provider_specific_flags() {
        let mut body = serde_json::json!({ "model": "m" });
        OpenAICompatProvider::merge_extra_params(&mut body, &serde_json::json!({}));
        assert!(body.get("chat_template_kwargs").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }

    /// vLLM needs the flag under `chat_template_kwargs`; a config that still
    /// carries the legacy top-level spelling must keep working.
    #[test]
    fn a_legacy_top_level_enable_thinking_is_moved_under_chat_template_kwargs() {
        let mut body = serde_json::json!({ "model": "m" });
        OpenAICompatProvider::merge_extra_params(
            &mut body,
            &serde_json::json!({ "enable_thinking": false }),
        );

        assert!(body.get("enable_thinking").is_none(), "legacy key must not be forwarded");
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
    }

    #[test]
    fn extra_params_override_the_defaults() {
        let mut body = serde_json::json!({ "model": "m", "temperature": 0.3 });
        OpenAICompatProvider::merge_extra_params(
            &mut body,
            &serde_json::json!({ "temperature": 0.0, "max_tokens": 64 }),
        );
        assert_eq!(body["temperature"], 0.0);
        assert_eq!(body["max_tokens"], 64);
    }
}
