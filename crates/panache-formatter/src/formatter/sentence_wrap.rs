use crate::syntax::{SyntaxKind, SyntaxNode};

#[derive(Clone, Copy)]
pub(super) enum SentenceLanguage {
    English,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BoundaryDecision {
    Break,
    NoBreak,
    Undecided,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SentenceBoundaryClass {
    Normal,
    NonBoundary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SentenceSegment {
    pub text: String,
    pub has_whitespace_after: bool,
    pub boundary_class: SentenceBoundaryClass,
}

#[derive(Clone, Copy)]
struct BoundaryContext<'a> {
    current_word: &'a str,
    next_word: Option<&'a str>,
    has_whitespace_after: bool,
    is_last: bool,
}

struct LanguageProfile {
    no_break_abbreviations: &'static [&'static str],
    contextual_abbreviations: &'static [&'static str],
    meridiem_abbreviations: &'static [&'static str],
    sentence_starters: &'static [&'static str],
}

const ENGLISH_PROFILE: LanguageProfile = LanguageProfile {
    no_break_abbreviations: &[
        "e.g.", "i.e.", "etc.", "mr.", "mrs.", "ms.", "dr.", "prof.", "vs.", "cf.", "fig.",
        "figs.", "eq.", "dept.", "st.",
    ],
    contextual_abbreviations: &["co.", "inc.", "ltd.", "corp.", "u.s.", "u.k."],
    meridiem_abbreviations: &["a.m.", "p.m."],
    sentence_starters: &[
        "a", "an", "and", "but", "for", "he", "how", "however", "i", "in", "it", "my", "she", "so",
        "that", "the", "there", "they", "this", "we", "what", "when", "where", "who", "why", "you",
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

fn normalize_next_token_candidate(word: &str) -> String {
    word.trim_start_matches(['"', '\'', '(', '[', '{', '`'])
        .trim_end_matches(['"', '\'', ')', ']', '}', ',', ';', ':', '`'])
        .to_ascii_lowercase()
}

fn starts_with_uppercase_after_opening_punct(word: &str) -> bool {
    word.trim_start_matches(['"', '\'', '(', '[', '{', '`'])
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
}

fn is_contextual_sentence_boundary(
    current_word: &str,
    next_word: Option<&str>,
    profile: &LanguageProfile,
) -> bool {
    let current = normalize_abbreviation_candidate(current_word);
    if !profile.contextual_abbreviations.contains(&current.as_str())
        && !profile.meridiem_abbreviations.contains(&current.as_str())
    {
        return false;
    }
    let Some(next) = next_word else {
        return false;
    };
    if !starts_with_uppercase_after_opening_punct(next) {
        return false;
    }
    if profile.meridiem_abbreviations.contains(&current.as_str()) {
        return true;
    }
    let next_norm = normalize_next_token_candidate(next);
    profile.sentence_starters.contains(&next_norm.as_str())
}

fn rule_ellipsis_no_break(ctx: &BoundaryContext<'_>) -> BoundaryDecision {
    let trimmed = trim_sentence_closing_punctuation(ctx.current_word);
    if trimmed.ends_with("...") || trimmed.ends_with("…") {
        return BoundaryDecision::NoBreak;
    }
    BoundaryDecision::Undecided
}

fn rule_contextual_abbreviation_break(
    ctx: &BoundaryContext<'_>,
    profile: &LanguageProfile,
) -> BoundaryDecision {
    let trimmed = trim_sentence_closing_punctuation(ctx.current_word);
    let Some(last_char) = trimmed.chars().last() else {
        return BoundaryDecision::NoBreak;
    };
    if last_char == '.' && is_contextual_sentence_boundary(trimmed, ctx.next_word, profile) {
        return BoundaryDecision::Break;
    }
    BoundaryDecision::Undecided
}

fn rule_abbreviation_no_break(
    ctx: &BoundaryContext<'_>,
    profile: &LanguageProfile,
) -> BoundaryDecision {
    let trimmed = trim_sentence_closing_punctuation(ctx.current_word);
    let Some(last_char) = trimmed.chars().last() else {
        return BoundaryDecision::NoBreak;
    };
    if last_char == '.' && is_no_break_abbreviation(trimmed, profile) {
        return BoundaryDecision::NoBreak;
    }
    BoundaryDecision::Undecided
}

fn rule_terminal_punctuation_break(ctx: &BoundaryContext<'_>) -> BoundaryDecision {
    let trimmed = trim_sentence_closing_punctuation(ctx.current_word);
    let Some(last_char) = trimmed.chars().last() else {
        return BoundaryDecision::NoBreak;
    };
    if matches!(last_char, '.' | '!' | '?') && (ctx.has_whitespace_after || ctx.is_last) {
        return BoundaryDecision::Break;
    }
    BoundaryDecision::NoBreak
}

pub(super) fn decide_sentence_boundary(
    word: &str,
    next_word: Option<&str>,
    has_whitespace_after: bool,
    is_last: bool,
    language: SentenceLanguage,
) -> BoundaryDecision {
    let profile = language.profile();
    let ctx = BoundaryContext {
        current_word: word,
        next_word,
        has_whitespace_after,
        is_last,
    };

    let rules: [BoundaryDecision; 4] = [
        rule_ellipsis_no_break(&ctx),
        rule_contextual_abbreviation_break(&ctx, profile),
        rule_abbreviation_no_break(&ctx, profile),
        rule_terminal_punctuation_break(&ctx),
    ];

    for decision in rules {
        if decision != BoundaryDecision::Undecided {
            return decision;
        }
    }
    BoundaryDecision::NoBreak
}

pub(super) fn is_sentence_boundary_text(
    word: &str,
    next_word: Option<&str>,
    has_whitespace_after: bool,
    is_last: bool,
    language: SentenceLanguage,
) -> bool {
    matches!(
        decide_sentence_boundary(word, next_word, has_whitespace_after, is_last, language),
        BoundaryDecision::Break
    )
}

pub(super) fn is_sentence_boundary_segment(
    segment: &SentenceSegment,
    next_segment: Option<&SentenceSegment>,
    is_last: bool,
    language: SentenceLanguage,
) -> bool {
    if segment.boundary_class == SentenceBoundaryClass::NonBoundary {
        return false;
    }
    is_sentence_boundary_text(
        &segment.text,
        next_segment.map(|next| next.text.as_str()),
        segment.has_whitespace_after,
        is_last,
        language,
    )
}

pub(super) fn split_sentence_segments(
    segments: &[SentenceSegment],
    language: SentenceLanguage,
) -> Vec<String> {
    if segments.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let mut current = String::new();

    for (idx, segment) in segments.iter().enumerate() {
        if !current.is_empty() && segment.has_whitespace_after {
            // spacing is handled when appending previous segment
        }
        if !current.is_empty()
            && idx > 0
            && segments
                .get(idx.wrapping_sub(1))
                .is_some_and(|prev| prev.has_whitespace_after)
        {
            current.push(' ');
        }
        current.push_str(&segment.text);

        let is_last = idx + 1 == segments.len();
        let next = segments.get(idx + 1);
        if is_sentence_boundary_segment(segment, next, is_last, language) {
            lines.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

pub(super) fn split_sentence_text(text: &str, language: SentenceLanguage) -> Vec<String> {
    let words: Vec<&str> = text.split_ascii_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let segments: Vec<SentenceSegment> = words
        .iter()
        .enumerate()
        .map(|(idx, word)| SentenceSegment {
            text: (*word).to_string(),
            has_whitespace_after: idx + 1 < words.len(),
            boundary_class: SentenceBoundaryClass::Normal,
        })
        .collect();
    split_sentence_segments(&segments, language)
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
            Some("Next"),
            true,
            false,
            SentenceLanguage::English
        ));
        assert!(!is_sentence_boundary_text(
            "i.e.",
            Some("Next"),
            true,
            false,
            SentenceLanguage::English
        ));
        assert!(!is_sentence_boundary_text(
            "`etc.`",
            Some("Next"),
            true,
            false,
            SentenceLanguage::English
        ));
        assert!(is_sentence_boundary_text(
            "complete.",
            Some("Next"),
            true,
            false,
            SentenceLanguage::English
        ));
        assert_eq!(
            decide_sentence_boundary(
                "complete.",
                Some("Next"),
                true,
                false,
                SentenceLanguage::English
            ),
            BoundaryDecision::Break
        );
    }

    #[test]
    fn boundary_decision_reports_no_break_for_abbreviation() {
        assert_eq!(
            decide_sentence_boundary("e.g.", Some("Next"), true, false, SentenceLanguage::English),
            BoundaryDecision::NoBreak
        );
    }

    #[test]
    fn contextual_abbreviations_use_next_token_signal() {
        assert!(!is_sentence_boundary_text(
            "co.",
            Some("at"),
            false,
            false,
            SentenceLanguage::English
        ));
        assert!(is_sentence_boundary_text(
            "co.",
            Some("They"),
            true,
            false,
            SentenceLanguage::English
        ));
        assert!(!is_sentence_boundary_text(
            "U.S.",
            Some("Government"),
            false,
            false,
            SentenceLanguage::English
        ));
        assert!(is_sentence_boundary_text(
            "U.S.",
            Some("How"),
            true,
            false,
            SentenceLanguage::English
        ));
        assert!(!is_sentence_boundary_text(
            "p.m.",
            Some("traveler"),
            false,
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

    #[test]
    fn non_boundary_segment_never_breaks() {
        let segments = vec![
            SentenceSegment {
                text: "`???`".to_string(),
                has_whitespace_after: true,
                boundary_class: SentenceBoundaryClass::NonBoundary,
            },
            SentenceSegment {
                text: "also".to_string(),
                has_whitespace_after: true,
                boundary_class: SentenceBoundaryClass::Normal,
            },
        ];
        let lines = split_sentence_segments(&segments, SentenceLanguage::English);
        assert_eq!(lines, vec!["`???` also"]);
    }

    #[test]
    fn split_sentence_text_uses_normal_segment_defaults() {
        let lines = split_sentence_text("Alpha. Beta.", SentenceLanguage::English);
        assert_eq!(lines, vec!["Alpha.", "Beta."]);
    }
}
