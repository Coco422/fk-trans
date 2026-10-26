//! What a language is called, in the forms providers actually need.
//!
//! The app stores a short code per language (`zh`, `en`, `ja`) and that code is
//! what the settings UI shows and what lives in `config.json`. Prompts are a
//! different matter:
//!
//! * TranslateGemma's documented format wants the English name *and* a BCP-47
//!   code — `Chinese (zh-Hans)` — and Simplified Chinese is **not** `zh` in that
//!   table.
//! * DeepLX wants the code upper-cased.
//! * A plain "translate this" prompt reads better with the name than the code.
//!
//! Keeping one table here means a prompt template can ask for whichever form it
//! needs, instead of every provider inventing its own mapping (there was none
//! before: the raw `zh` was passed straight through to the model).

/// One selectable language, in every form a provider might need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Language {
    /// Stored in config and shown in the UI. Never changes once shipped.
    pub code: &'static str,
    /// English name, as translation models expect it in a prompt.
    pub name: &'static str,
    /// BCP-47 code. Not always the same as `code`.
    pub bcp47: &'static str,
}

/// The code meaning "work out the source language yourself".
pub const AUTO_CODE: &str = "auto";

/// English phrasing for an unspecified source language. Reads as a noun phrase
/// so it slots into "translate from {from} to {to}".
pub const AUTO_SOURCE_LABEL: &str = "the detected language";

/// Languages offered in the settings UI.
///
/// `code` values match what earlier builds wrote into `config.json`, so adding a
/// BCP-47 column here does not require migrating anyone's config.
pub const LANGUAGES: &[Language] = &[
    Language {
        code: "auto",
        name: AUTO_SOURCE_LABEL,
        bcp47: AUTO_CODE,
    },
    Language {
        code: "en",
        name: "English",
        bcp47: "en",
    },
    Language {
        code: "zh",
        name: "Chinese",
        // TranslateGemma's table lists Simplified Chinese as `zh-Hans`; plain
        // `zh` is ambiguous there.
        bcp47: "zh-Hans",
    },
    Language {
        code: "ja",
        name: "Japanese",
        bcp47: "ja",
    },
    Language {
        code: "ko",
        name: "Korean",
        bcp47: "ko",
    },
    Language {
        code: "fr",
        name: "French",
        bcp47: "fr",
    },
    Language {
        code: "de",
        name: "German",
        bcp47: "de",
    },
    Language {
        code: "es",
        name: "Spanish",
        bcp47: "es",
    },
    Language {
        code: "ru",
        name: "Russian",
        bcp47: "ru",
    },
    Language {
        code: "pt",
        name: "Portuguese",
        bcp47: "pt",
    },
    Language {
        code: "ar",
        name: "Arabic",
        bcp47: "ar",
    },
];

pub fn lookup(code: &str) -> Option<&'static Language> {
    LANGUAGES.iter().find(|language| language.code == code)
}

pub fn is_auto(code: &str) -> bool {
    code == AUTO_CODE
}

/// The English name for `code`, falling back to the code itself.
///
/// A code the table does not know is still passed through rather than dropped:
/// the user may be targeting a language we have no row for, and a model can
/// often cope with the raw code.
pub fn name_of(code: &str) -> &str {
    lookup(code).map_or(code, |language| language.name)
}

/// The BCP-47 code for `code`, falling back to the code itself.
pub fn bcp47_of(code: &str) -> &str {
    lookup(code).map_or(code, |language| language.bcp47)
}

/// `name (bcp47)` — the form TranslateGemma's prompt asks for.
///
/// For an unknown code the parenthetical would just repeat it, so the code is
/// returned alone. For `auto` this returns the noun phrase, because
/// "the detected language (auto)" reads badly and names nothing.
pub fn labelled(code: &str) -> String {
    if is_auto(code) {
        return AUTO_SOURCE_LABEL.to_string();
    }
    match lookup(code) {
        Some(language) => format!("{} ({})", language.name, language.bcp47),
        None => code.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_code_is_unique() {
        let mut codes: Vec<&str> = LANGUAGES.iter().map(|l| l.code).collect();
        let total = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), total, "duplicate code in LANGUAGES");
    }

    /// The BCP-47 code is the whole reason this table exists: TranslateGemma's
    /// table does not have plain `zh` for Simplified Chinese.
    #[test]
    fn simplified_chinese_maps_to_zh_hans() {
        assert_eq!(bcp47_of("zh"), "zh-Hans");
        assert_eq!(name_of("zh"), "Chinese");
        assert_eq!(labelled("zh"), "Chinese (zh-Hans)");
    }

    #[test]
    fn a_known_language_renders_as_name_and_code() {
        assert_eq!(labelled("en"), "English (en)");
        assert_eq!(labelled("ja"), "Japanese (ja)");
    }

    /// Not knowing a language must not lose it: the raw code goes through.
    #[test]
    fn an_unknown_code_falls_back_to_itself() {
        assert_eq!(labelled("xx"), "xx");
        assert_eq!(name_of("xx"), "xx");
        assert_eq!(bcp47_of("xx"), "xx");
        assert!(lookup("xx").is_none());
    }

    #[test]
    fn auto_renders_as_a_phrase_not_a_code_pair() {
        assert!(is_auto("auto"));
        assert_eq!(name_of("auto"), AUTO_SOURCE_LABEL);
        assert_eq!(labelled("auto"), AUTO_SOURCE_LABEL);
        assert!(!labelled("auto").contains('('));
    }
}
