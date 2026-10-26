//! Reading files written by earlier builds.
//!
//! The wire and on-disk format is camelCase. Earlier builds wrote snake_case,
//! and `config.json` is the only copy of the user's API keys, so a rename that
//! quietly failed to read an existing file would be data loss.
//!
//! The translation lives here rather than in `#[serde(alias = "...")]` attributes
//! for two reasons: aliases make the struct itself accept both spellings, so a
//! stray snake_case key on the *IPC* path would also be silently accepted; and
//! ts-rs cannot parse `alias`, so every one of them produced a build warning
//! that would eventually hide a real one.
//!
//! Leniency is therefore confined to one call, at the single point where a file
//! from an older build is read.

use serde_json::Value;

/// `(old key, current key)` for every field that ever changed spelling.
const RENAMED_KEYS: &[(&str, &str)] = &[
    // AppConfig
    ("debug_logging", "debugLogging"),
    ("selection_trigger_enabled", "selectionTriggerEnabled"),
    ("source_lang", "sourceLang"),
    ("target_lang", "targetLang"),
    ("active_provider", "activeProvider"),
    ("mouse_trigger_button", "mouseTriggerButton"),
    // ProviderConfig
    ("base_url", "baseUrl"),
    ("api_key", "apiKey"),
    ("system_prompt", "systemPrompt"),
    ("user_prompt", "userPrompt"),
    ("extra_params", "extraParams"),
];

/// Rewrite legacy key spellings throughout `value`, in place.
///
/// A key already present under its current spelling wins, so a file that somehow
/// contains both is read as the current format.
pub fn rename_legacy_keys(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (legacy, current) in RENAMED_KEYS {
                if let Some(moved) = map.remove(*legacy) {
                    map.entry((*current).to_string()).or_insert(moved);
                }
            }
            for child in map.values_mut() {
                rename_legacy_keys(child);
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                rename_legacy_keys(item);
            }
        }
        _ => {}
    }
}

/// Parse JSON text, accepting keys written by an earlier build.
pub fn from_str_compat<T: serde::de::DeserializeOwned>(json: &str) -> serde_json::Result<T> {
    let mut value: Value = serde_json::from_str(json)?;
    rename_legacy_keys(&mut value);
    serde_json::from_value(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase")]
    struct Nested {
        base_url: String,
        api_key: String,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase")]
    struct Sample {
        source_lang: String,
        active_provider: String,
        items: Vec<Nested>,
    }

    #[test]
    fn legacy_keys_are_rewritten_at_every_depth() {
        let parsed: Sample = from_str_compat(
            r#"{
                "source_lang": "auto",
                "active_provider": "openai",
                "items": [
                    { "base_url": "https://a.test", "api_key": "sk-a" },
                    { "base_url": "https://b.test", "api_key": "sk-b" }
                ]
            }"#,
        )
        .expect("legacy keys must be accepted");

        assert_eq!(parsed.source_lang, "auto");
        assert_eq!(parsed.active_provider, "openai");
        assert_eq!(parsed.items[1].base_url, "https://b.test");
        assert_eq!(parsed.items[1].api_key, "sk-b");
    }

    #[test]
    fn current_keys_are_left_alone() {
        let parsed: Sample = from_str_compat(
            r#"{
                "sourceLang": "ja",
                "activeProvider": "ollama",
                "items": [{ "baseUrl": "https://c.test", "apiKey": "sk-c" }]
            }"#,
        )
        .expect("current keys must be accepted");

        assert_eq!(parsed.source_lang, "ja");
        assert_eq!(parsed.items[0].api_key, "sk-c");
    }

    /// A file holding both spellings is read as the current format, so a
    /// half-migrated file cannot silently win with its stale value.
    #[test]
    fn the_current_spelling_wins_when_both_are_present() {
        let parsed: Sample = from_str_compat(
            r#"{
                "source_lang": "stale",
                "sourceLang": "current",
                "activeProvider": "openai",
                "items": []
            }"#,
        )
        .expect("parses");

        assert_eq!(parsed.source_lang, "current");
    }

    #[test]
    fn syntactically_invalid_json_is_still_an_error() {
        assert!(from_str_compat::<Sample>("{ not json").is_err());
    }
}
