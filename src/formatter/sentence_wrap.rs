use crate::syntax::{SyntaxKind, SyntaxNode};

#[derive(Clone, Copy)]
pub(super) enum SentenceLanguage {
    English,
}

struct LanguageProfile {
    no_break_abbreviations: &'static [&'static str],
}

const ENGLISH_PROFILE: LanguageProfile = LanguageProfile {
    no_break_abbreviations: &[
        "e.g.", "i.e.", "etc.", "mr.", "mrs.", "ms.", "dr.", "prof.", "vs.", "cf.", "fig.",
        "figs.", "eq.", "no.", "dept.", "st.",
    ],
};

impl SentenceLanguage {
    fn profile(self) -> &'static LanguageProfile {
        match self {
            SentenceLanguage::English => &ENGLISH_PROFILE,
        }
    }
}

fn trim_sentence_closing_punctuation(word: &str) -> &str {
    word.trim_end_matches(['"', '\'', ')', ']', '}', '`'])
}

fn normalize_abbreviation_candidate(word: &str) -> String {
    let trimmed = trim_sentence_closing_punctuation(word)
        .trim_start_matches(['"', '\'', '(', '[', '{', '`'])
        .trim_end_matches([',', ';', ':']);
    trimmed.to_ascii_lowercase()
}

fn is_no_break_abbreviation(word: &str, profile: &LanguageProfile) -> bool {
    let candidate = normalize_abbreviation_candidate(word);
    if profile.no_break_abbreviations.contains(&candidate.as_str()) {
        return true;
    }
    candidate.ends_with('.') && candidate.matches('.').count() >= 2 && {
        let without_periods = candidate.replace('.', "");
        !without_periods.is_empty() && without_periods.chars().all(|c| c.is_ascii_lowercase())
    }
}

pub(super) fn is_sentence_boundary_text(
    word: &str,
    has_whitespace_after: bool,
    is_last: bool,
    language: SentenceLanguage,
) -> bool {
    let profile = language.profile();
    let trimmed = trim_sentence_closing_punctuation(word);
    if trimmed.ends_with("...") || trimmed.ends_with("…") {
        return false;
    }
    let Some(last_char) = trimmed.chars().last() else {
        return false;
    };
    if last_char == '.' && is_no_break_abbreviation(trimmed, profile) {
        return false;
    }
    matches!(last_char, '.' | '!' | '?') && (has_whitespace_after || is_last)
}

fn extract_lang_from_yaml_text(yaml_text: &str) -> Option<String> {
    let mut lines = yaml_text.lines().peekable();
    if lines
        .peek()
        .is_some_and(|line| line.trim() == "---" || line.trim() == "...")
    {
        lines.next();
    }
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" || trimmed == "..." {
            break;
        }
        if line.starts_with(' ') || line.starts_with('\t') || trimmed.starts_with('#') {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("lang:") else {
            continue;
        };
        let value = rest.trim().trim_matches(['"', '\'']).to_string();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

pub(super) fn resolve_sentence_language(node: &SyntaxNode) -> SentenceLanguage {
    let lang = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::DOCUMENT)
        .and_then(|document| {
            document
                .children()
                .find(|child| child.kind() == SyntaxKind::YAML_METADATA)
        })
        .and_then(|yaml| extract_lang_from_yaml_text(&yaml.text().to_string()))
        .map(|lang| lang.to_ascii_lowercase());

    if let Some(lang) = lang
        && (lang == "en" || lang.starts_with("en-"))
    {
        return SentenceLanguage::English;
    }
    SentenceLanguage::English
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn abbreviation_periods_are_not_sentence_boundaries() {
        assert!(!is_sentence_boundary_text(
            "(e.g.)",
            true,
            false,
            SentenceLanguage::English
        ));
        assert!(!is_sentence_boundary_text(
            "i.e.",
            true,
            false,
            SentenceLanguage::English
        ));
        assert!(!is_sentence_boundary_text(
            "`etc.`",
            true,
            false,
            SentenceLanguage::English
        ));
        assert!(is_sentence_boundary_text(
            "complete.",
            true,
            false,
            SentenceLanguage::English
        ));
    }

    #[test]
    fn extracts_lang_from_yaml_frontmatter() {
        let yaml = "---\nlang: en-GB\ntitle: Test\n---";
        assert_eq!(extract_lang_from_yaml_text(yaml).as_deref(), Some("en-GB"));
    }

    #[test]
    fn resolves_sentence_language_from_document_metadata() {
        let input = "---\nlang: sv\ntitle: Test\n---\n\nA sentence.";
        let tree = parse(input, None);
        let paragraph = tree
            .descendants()
            .find(|node| node.kind() == SyntaxKind::PARAGRAPH)
            .expect("paragraph node");

        let lang = resolve_sentence_language(&paragraph);
        assert!(matches!(lang, SentenceLanguage::English));
    }
}
