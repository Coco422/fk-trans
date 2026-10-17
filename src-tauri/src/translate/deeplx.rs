use super::http::{require_translation, send, top_level_keys};
use super::language;
use super::provider::{TranslateError, TranslateProvider, TranslateResult};
use reqwest::Client;

/// A local DeepLX server.
///
/// Its own wire format, so not an OpenAI-compatible preset: the field names are
/// `source_lang`/`target_lang`, the codes are upper-case, and there is no
/// authentication.
pub struct DeepLXProvider {
    base_url: String,
    client: Client,
}

impl Default for DeepLXProvider {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:1188".to_string(),
            client: super::http::shared_client(),
        }
    }
}

impl DeepLXProvider {
    /// `base_url` comes from config. It used to be hardcoded here, which made the
    /// Base URL field in the settings UI decorative: editing it changed nothing,
    /// and pointing the app at a DeepLX on another host was impossible.
    pub fn new(base_url: &str) -> Self {
        let base_url = base_url.trim();
        Self {
            base_url: if base_url.is_empty() {
                "http://127.0.0.1:1188".to_string()
            } else {
                base_url.trim_end_matches('/').to_string()
            },
            client: super::http::shared_client(),
        }
    }

    fn extract_translation(json: &serde_json::Value) -> Option<String> {
        // DeepLX answers with `data`; some forks answer with a `translations`
        // array. Both are tried, but a body that holds neither is an error rather
        // than an empty success.
        json.get("data")
            .and_then(|value| value.as_str())
            .or_else(|| {
                json.get("translations")?
                    .get(0)?
                    .get("text")?
                    .as_str()
            })
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    }

    fn response_shape_summary(json: &serde_json::Value) -> String {
        let message = json
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or("none");
        format!(
            "top_level_keys={} message={message}",
            top_level_keys(json)
        )
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
        // DeepLX wants upper-case codes and spells auto-detection `AUTO`.
        let source = language::bcp47_of(from).to_uppercase();
        let target = language::bcp47_of(to).to_uppercase();

        let body = serde_json::json!({
            "text": text,
            "source_lang": source,
            "target_lang": target,
        });

        let response = send(
            self.client
                .post(format!("{}/translate", self.base_url))
                .json(&body),
        )
        .await?;

        let json = response.json()?;
        let translated = require_translation(
            Self::extract_translation(&json),
            "DeepLX",
            || Self::response_shape_summary(&json),
        )?;

        Ok(TranslateResult {
            original: text.to_string(),
            translated,
            source_lang: from.to_string(),
            target_lang: to.to_string(),
            provider: "deeplx".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_data_field() {
        let json = serde_json::json!({ "data": " 你好世界 " });
        assert_eq!(
            DeepLXProvider::extract_translation(&json).as_deref(),
            Some("你好世界")
        );
    }

    #[test]
    fn accepts_the_translations_array_shape_some_forks_return() {
        let json = serde_json::json!({ "translations": [{ "text": "你好" }] });
        assert_eq!(
            DeepLXProvider::extract_translation(&json).as_deref(),
            Some("你好")
        );
    }

    /// The old code returned `""` for any body it did not recognise, which the
    /// popup rendered as a blank translation.
    #[test]
    fn an_unrecognised_body_is_not_an_empty_success() {
        let json = serde_json::json!({ "code": 500, "message": "upstream failure" });

        assert_eq!(DeepLXProvider::extract_translation(&json), None);
        let error = require_translation(None, "DeepLX", || {
            DeepLXProvider::response_shape_summary(&json)
        })
        .expect_err("must not be treated as success");
        assert!(error.to_string().contains("upstream failure"));
    }

    #[test]
    fn an_empty_base_url_falls_back_to_the_default() {
        assert_eq!(
            DeepLXProvider::new("  ").base_url,
            "http://127.0.0.1:1188"
        );
    }

    #[test]
    fn a_configured_base_url_is_honoured_without_a_trailing_slash() {
        assert_eq!(
            DeepLXProvider::new("http://192.168.1.9:1188/").base_url,
            "http://192.168.1.9:1188"
        );
    }
}
