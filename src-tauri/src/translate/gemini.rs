use super::http::{require_translation, send, top_level_keys};
use super::prompt::{format_prompt, PromptVars};
use super::provider::{TranslateError, TranslateProvider, TranslateResult};
use reqwest::Client;

pub struct GeminiProvider {
    base_url: String,
    api_key: String,
    model: String,
    system_prompt: String,
    user_prompt: String,
    client: Client,
}

impl Default for GeminiProvider {
    fn default() -> Self {
        Self {
            base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
            api_key: String::new(),
            model: "gemini-2.0-flash".to_string(),
            system_prompt: crate::config::default_system_prompt(),
            user_prompt: crate::config::default_user_prompt(),
            client: super::http::shared_client(),
        }
    }
}

impl GeminiProvider {
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

    /// `candidates[0].content.parts[*].text`, joined.
    fn extract_translation(json: &serde_json::Value) -> Option<String> {
        let parts = json
            .get("candidates")?
            .get(0)?
            .get("content")?
            .get("parts")?
            .as_array()?;

        let text = parts
            .iter()
            .filter_map(|part| part.get("text").and_then(|value| value.as_str()))
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
        let finish_reason = json
            .get("candidates")
            .and_then(|value| value.get(0))
            .and_then(|value| value.get("finishReason"))
            .and_then(|value| value.as_str())
            .unwrap_or("missing");
        let block_reason = json
            .get("promptFeedback")
            .and_then(|value| value.get("blockReason"))
            .and_then(|value| value.as_str())
            .unwrap_or("none");

        format!(
            "top_level_keys={} finish_reason={finish_reason} block_reason={block_reason}",
            top_level_keys(json)
        )
    }
}

#[async_trait::async_trait]
impl TranslateProvider for GeminiProvider {
    async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<TranslateResult, TranslateError> {
        if self.api_key.trim().is_empty() {
            return Err(TranslateError::Config(
                "Gemini API key not configured".into(),
            ));
        }

        let vars = PromptVars { from, to, text };
        let body = serde_json::json!({
            "systemInstruction": {
                "parts": [{ "text": format_prompt(&self.system_prompt, &vars) }]
            },
            "contents": [{
                "parts": [{ "text": format_prompt(&self.user_prompt, &vars) }]
            }],
            "generationConfig": {
                "temperature": 0.3,
                "maxOutputTokens": 2048
            }
        });

        // The key goes in a header, not `?key=`: as a query parameter it ends up
        // in every proxy log and access log between here and Google.
        let response = send(
            self.client
                .post(format!(
                    "{}/models/{}:generateContent",
                    self.base_url, self.model
                ))
                .header("x-goog-api-key", &self.api_key)
                .json(&body),
        )
        .await?;

        let json = response.json()?;
        let translated = require_translation(
            Self::extract_translation(&json),
            "Gemini",
            || Self::response_shape_summary(&json),
        )?;

        Ok(TranslateResult {
            original: text.to_string(),
            translated,
            source_lang: from.to_string(),
            target_lang: to.to_string(),
            provider: "gemini".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_text_from_the_first_candidate() {
        let json = serde_json::json!({
            "candidates": [{ "content": { "parts": [{ "text": " 你好世界 " }] } }]
        });
        assert_eq!(
            GeminiProvider::extract_translation(&json).as_deref(),
            Some("你好世界")
        );
    }

    #[test]
    fn joins_multiple_parts() {
        let json = serde_json::json!({
            "candidates": [{
                "content": { "parts": [{ "text": "你好" }, { "text": "世界" }] }
            }]
        });
        assert_eq!(
            GeminiProvider::extract_translation(&json).as_deref(),
            Some("你好世界")
        );
    }

    /// A blocked prompt comes back with no candidates; that used to become an
    /// empty success. The summary has to say why.
    #[test]
    fn reports_a_blocked_prompt_rather_than_returning_nothing() {
        let json = serde_json::json!({
            "promptFeedback": { "blockReason": "SAFETY" }
        });

        assert_eq!(GeminiProvider::extract_translation(&json), None);
        let summary = GeminiProvider::response_shape_summary(&json);
        assert!(summary.contains("block_reason=SAFETY"), "got: {summary}");

        let error = require_translation(None, "Gemini", || summary);
        assert!(error.is_err());
    }

    #[test]
    fn reports_the_finish_reason_when_a_candidate_carries_no_text() {
        let json = serde_json::json!({
            "candidates": [{ "finishReason": "MAX_TOKENS", "content": { "parts": [] } }]
        });
        let summary = GeminiProvider::response_shape_summary(&json);
        assert!(summary.contains("finish_reason=MAX_TOKENS"), "got: {summary}");
    }
}
