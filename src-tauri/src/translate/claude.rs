use super::http::{require_translation, send, top_level_keys};
use super::prompt::{format_prompt, PromptVars};
use super::provider::{TranslateError, TranslateProvider, TranslateResult};
use reqwest::Client;

/// Anthropic's Messages API. Not expressible as an OpenAI-compatible request:
/// the system prompt is a top-level field rather than a message, `max_tokens` is
/// required, and authentication is a header of its own.
pub struct ClaudeProvider {
    base_url: String,
    api_key: String,
    model: String,
    system_prompt: String,
    user_prompt: String,
    client: Client,
}

impl Default for ClaudeProvider {
    fn default() -> Self {
        Self {
            base_url: "https://api.anthropic.com".to_string(),
            api_key: String::new(),
            model: "claude-haiku-4-5-20251001".to_string(),
            system_prompt: crate::config::default_system_prompt(),
            user_prompt: crate::config::default_user_prompt(),
            client: super::http::shared_client(),
        }
    }
}

impl ClaudeProvider {
    pub fn new(
        base_url: &str,
        api_key: &str,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            system_prompt: system_prompt.to_string(),
            user_prompt: user_prompt.to_string(),
            client: super::http::shared_client(),
        }
    }

    fn extract_translation(json: &serde_json::Value) -> Option<String> {
        let blocks = json.get("content")?.as_array()?;

        let text = blocks
            .iter()
            .filter(|block| block.get("type").and_then(|value| value.as_str()) == Some("text"))
            .filter_map(|block| block.get("text").and_then(|value| value.as_str()))
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
        let stop_reason = json
            .get("stop_reason")
            .and_then(|value| value.as_str())
            .unwrap_or("missing");
        let block_types = json
            .get("content")
            .and_then(|value| value.as_array())
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|block| block.get("type").and_then(|value| value.as_str()))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_else(|| "missing".to_string());

        format!(
            "top_level_keys={} stop_reason={stop_reason} content_blocks={block_types}",
            top_level_keys(json)
        )
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
        if self.api_key.trim().is_empty() {
            return Err(TranslateError::Config(
                "Claude API key not configured".into(),
            ));
        }

        let vars = PromptVars { from, to, text };
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 2048,
            "system": format_prompt(&self.system_prompt, &vars),
            "messages": [
                { "role": "user", "content": format_prompt(&self.user_prompt, &vars) }
            ]
        });

        let response = send(
            self.client
                .post(format!("{}/v1/messages", self.base_url))
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&body),
        )
        .await?;

        let json = response.json()?;
        let translated = require_translation(
            Self::extract_translation(&json),
            "Claude",
            || Self::response_shape_summary(&json),
        )?;

        Ok(TranslateResult {
            original: text.to_string(),
            translated,
            source_lang: from.to_string(),
            target_lang: to.to_string(),
            provider: "claude".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_text_block() {
        let json = serde_json::json!({
            "content": [{ "type": "text", "text": "你好世界" }],
            "stop_reason": "end_turn"
        });
        assert_eq!(
            ClaudeProvider::extract_translation(&json).as_deref(),
            Some("你好世界")
        );
    }

    /// Extended thinking and tool use put non-text blocks first; only text
    /// blocks are the translation.
    #[test]
    fn ignores_non_text_blocks() {
        let json = serde_json::json!({
            "content": [
                { "type": "thinking", "thinking": "let me consider..." },
                { "type": "text", "text": "  你好  " }
            ]
        });
        assert_eq!(
            ClaudeProvider::extract_translation(&json).as_deref(),
            Some("你好")
        );
    }

    #[test]
    fn a_max_tokens_stop_reports_why_there_is_no_translation() {
        let json = serde_json::json!({
            "content": [{ "type": "text", "text": "" }],
            "stop_reason": "max_tokens"
        });

        assert_eq!(ClaudeProvider::extract_translation(&json), None);
        let summary = ClaudeProvider::response_shape_summary(&json);
        assert!(summary.contains("stop_reason=max_tokens"), "got: {summary}");
        assert!(summary.contains("content_blocks=text"), "got: {summary}");
    }
}
