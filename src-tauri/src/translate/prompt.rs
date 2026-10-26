//! Prompt templates, in one place.
//!
//! `format_prompt` used to exist as four byte-identical copies (in the OpenAI,
//! Gemini, Claude, and Ollama providers), each substituting the same three
//! placeholders. There is now one implementation, and it understands the
//! language forms a provider might need.

use super::language;

/// The values a template is rendered with.
#[derive(Debug, Clone, Copy)]
pub struct PromptVars<'a> {
    /// Source language code as stored in config, e.g. `auto` or `en`.
    pub from: &'a str,
    /// Target language code, e.g. `zh`.
    pub to: &'a str,
    pub text: &'a str,
}

/// Placeholders, longest first.
///
/// Order matters twice over. `{from}` is a prefix of `{from_code}`, so the
/// shorter key has to be matched after the longer ones — and `{text}` is last,
/// so that text the user selected containing `{to}` is not then substituted.
const PLACEHOLDERS: &[&str] = &[
    "{from_code}",
    "{from_name}",
    "{to_code}",
    "{to_name}",
    "{from}",
    "{to}",
    "{text}",
];

fn value_for(key: &str, vars: &PromptVars) -> String {
    match key {
        // `{from}` and `{to}` render as `English (en)`, which is the form
        // TranslateGemma documents and reads correctly in a plain prompt.
        "{from}" => language::labelled(vars.from),
        "{to}" => language::labelled(vars.to),
        "{from_name}" => language::name_of(vars.from).to_string(),
        "{from_code}" => language::bcp47_of(vars.from).to_string(),
        "{to_name}" => language::name_of(vars.to).to_string(),
        "{to_code}" => language::bcp47_of(vars.to).to_string(),
        "{text}" => vars.text.to_string(),
        other => {
            debug_assert!(false, "unhandled placeholder {other}");
            other.to_string()
        }
    }
}

pub fn format_prompt(template: &str, vars: &PromptVars) -> String {
    let mut rendered = template.to_string();
    for key in PLACEHOLDERS {
        if rendered.contains(key) {
            rendered = rendered.replace(key, &value_for(key, vars));
        }
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars<'a>(from: &'a str, to: &'a str, text: &'a str) -> PromptVars<'a> {
        PromptVars { from, to, text }
    }

    #[test]
    fn substitutes_all_three_original_placeholders() {
        let rendered = format_prompt(
            "Translate from {from} to {to}: {text}",
            &vars("en", "zh", "hello"),
        );
        assert_eq!(rendered, "Translate from English (en) to Chinese (zh-Hans): hello");
    }

    /// The template the presets ship, in the shape TranslateGemma documents.
    #[test]
    fn renders_the_translategemma_prompt_format() {
        let rendered = format_prompt(
            "You are a professional {from} to {to} translator.\nProduce only the {to_name} translation:\n\n{text}",
            &vars("en", "zh", "Hello, how are you?"),
        );

        assert!(rendered.contains(
            "You are a professional English (en) to Chinese (zh-Hans) translator."
        ));
        assert!(rendered.ends_with("\n\nHello, how are you?"));
    }

    /// `{from}` is a prefix of `{from_code}`; the longer key must win.
    #[test]
    fn code_and_name_placeholders_are_not_confused_with_from_and_to() {
        let rendered = format_prompt(
            "src={from} code={from_code} name={from_name} dst={to} code={to_code} name={to_name}",
            &vars("en", "zh", "x"),
        );
        assert_eq!(
            rendered,
            "src=English (en) code=en name=English dst=Chinese (zh-Hans) code=zh-Hans name=Chinese"
        );
    }

    /// Selected text is the last thing substituted, so a document that happens
    /// to contain a placeholder cannot have it expanded.
    #[test]
    fn placeholders_inside_the_selected_text_are_left_alone() {
        let rendered = format_prompt("{text}", &vars("en", "zh", "literal {to} and {from_code}"));
        assert_eq!(rendered, "literal {to} and {from_code}");
    }

    #[test]
    fn an_auto_source_renders_as_a_phrase_in_every_form() {
        let rendered = format_prompt(
            "{from} / {from_name} / {from_code}",
            &vars("auto", "zh", "x"),
        );
        assert_eq!(rendered, "the detected language / the detected language / auto");
    }

    #[test]
    fn a_template_without_placeholders_is_returned_unchanged() {
        assert_eq!(format_prompt("Translate this.", &vars("en", "zh", "x")), "Translate this.");
    }

    #[test]
    fn an_unknown_language_code_is_passed_through() {
        let rendered = format_prompt("{to}", &vars("en", "xx", "x"));
        assert_eq!(rendered, "xx");
    }
}
