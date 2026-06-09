use crate::config::Config;
use crate::syntax::{SyntaxKind, SyntaxNode};

#[derive(Clone, Copy)]
pub(super) enum SentenceLanguage {
    English,
    Czech,
    German,
    Spanish,
    French,
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

const CZECH_PROFILE: LanguageProfile = LanguageProfile {
    no_break_abbreviations: &[
        "např.", "tzv.", "tj.", "atd.", "apod.", "resp.", "mj.", "aj.",
    ],
    contextual_abbreviations: &[],
    meridiem_abbreviations: &[],
    sentence_starters: &[],
};

const GERMAN_PROFILE: LanguageProfile = LanguageProfile {
    no_break_abbreviations: &["bzw.", "usw.", "vgl.", "ggf."],
    contextual_abbreviations: &[],
    meridiem_abbreviations: &[],
    sentence_starters: &[],
};

// Conservative starter list; review/extend the contents as real usage surfaces
// false splits. Entries must be lowercase (candidates are lowercased before the
// comparison).
const SPANISH_PROFILE: LanguageProfile = LanguageProfile {
    no_break_abbreviations: &[
        "etc.", "p.ej.", "ej.", "vs.", "cf.", "núm.", "pág.", "págs.", "art.", "cap.", "fig.",
    ],
    contextual_abbreviations: &[],
    meridiem_abbreviations: &[],
    sentence_starters: &[],
};

// Conservative starter list; review/extend as above.
const FRENCH_PROFILE: LanguageProfile = LanguageProfile {
    no_break_abbreviations: &[
        "etc.", "cf.", "p.ex.", "ex.", "réf.", "fig.", "chap.", "éd.", "vol.",
    ],
    contextual_abbreviations: &[],
    meridiem_abbreviations: &[],
    sentence_starters: &[],
};

impl SentenceLanguage {
    fn profile(self) -> &'static LanguageProfile {
        match self {
            SentenceLanguage::English => &ENGLISH_PROFILE,
            SentenceLanguage::Czech => &CZECH_PROFILE,
            SentenceLanguage::German => &GERMAN_PROFILE,
            SentenceLanguage::Spanish => &SPANISH_PROFILE,
            SentenceLanguage::French => &FRENCH_PROFILE,
        }
    }
}

/// A built-in language profile plus any user-supplied no-break abbreviations
/// resolved for the current document. It holds two references, so it is `Copy`
/// and threads through the wrapper exactly like the former `SentenceLanguage`.
#[derive(Clone, Copy)]
pub(super) struct ResolvedProfile<'a> {
    builtin: &'static LanguageProfile,
    /// User additions, already candidate-normalized (see
    /// [`normalize_abbreviation_candidate`]).
    extra_no_break: &'a [String],
}

impl ResolvedProfile<'static> {
    /// Built-in profile only, no user additions. Used by tests and by callers
    /// that never enter sentence mode (where the profile is never consulted).
    pub(super) fn builtin_only(language: SentenceLanguage) -> Self {
        Self {
            builtin: language.profile(),
            extra_no_break: &[],
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
    trimmed.to_lowercase()
}

fn is_no_break_abbreviation(word: &str, profile: ResolvedProfile<'_>) -> bool {
    let candidate = normalize_abbreviation_candidate(word);
    if profile
        .builtin
        .no_break_abbreviations
        .contains(&candidate.as_str())
    {
        return true;
    }
    if profile
        .extra_no_break
        .iter()
        .any(|entry| entry == &candidate)
    {
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
    profile: ResolvedProfile<'_>,
) -> BoundaryDecision {
    let trimmed = trim_sentence_closing_punctuation(ctx.current_word);
    let Some(last_char) = trimmed.chars().last() else {
        return BoundaryDecision::NoBreak;
    };
    if last_char == '.' && is_contextual_sentence_boundary(trimmed, ctx.next_word, profile.builtin)
    {
        return BoundaryDecision::Break;
    }
    BoundaryDecision::Undecided
}

fn rule_abbreviation_no_break(
    ctx: &BoundaryContext<'_>,
    profile: ResolvedProfile<'_>,
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
    profile: ResolvedProfile<'_>,
) -> BoundaryDecision {
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
    profile: ResolvedProfile<'_>,
) -> bool {
    matches!(
        decide_sentence_boundary(word, next_word, has_whitespace_after, is_last, profile),
        BoundaryDecision::Break
    )
}

pub(super) fn is_sentence_boundary_segment(
    segment: &SentenceSegment,
    next_segment: Option<&SentenceSegment>,
    is_last: bool,
    profile: ResolvedProfile<'_>,
) -> bool {
    if segment.boundary_class == SentenceBoundaryClass::NonBoundary {
        return false;
    }
    is_sentence_boundary_text(
        &segment.text,
        next_segment.map(|next| next.text.as_str()),
        segment.has_whitespace_after,
        is_last,
        profile,
    )
}

pub(super) fn split_sentence_segments(
    segments: &[SentenceSegment],
    profile: ResolvedProfile<'_>,
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
        if is_sentence_boundary_segment(segment, next, is_last, profile) {
            lines.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

pub(super) fn split_sentence_text(text: &str, profile: ResolvedProfile<'_>) -> Vec<String> {
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
    split_sentence_segments(&segments, profile)
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

fn extract_document_lang(node: &SyntaxNode) -> Option<String> {
    node.ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::DOCUMENT)
        .and_then(|document| {
            document
                .children()
                .find(|child| child.kind() == SyntaxKind::YAML_METADATA)
        })
        .and_then(|yaml| extract_lang_from_yaml_text(&yaml.text().to_string()))
}

/// Resolve the active document language as a normalized (lowercased) string.
/// Precedence: the document's YAML `lang:` over the `config_lang` fallback. The
/// region subtag is preserved; callers fold it with [`primary_subtag`].
pub(super) fn resolve_lang_string(node: &SyntaxNode, config_lang: Option<&str>) -> Option<String> {
    extract_document_lang(node)
        .or_else(|| config_lang.map(str::to_string))
        .map(|lang| lang.to_lowercase())
}

/// Primary language subtag, e.g. `en-gb` -> `en`, `pt_br` -> `pt`.
fn primary_subtag(lang: &str) -> &str {
    lang.split(['-', '_']).next().unwrap_or(lang)
}

fn sentence_language_for(lang: Option<&str>) -> SentenceLanguage {
    match lang.map(primary_subtag) {
        Some("cs") => SentenceLanguage::Czech,
        Some("de") => SentenceLanguage::German,
        Some("es") => SentenceLanguage::Spanish,
        Some("fr") => SentenceLanguage::French,
        // "en", unknown languages, and absent metadata fall back to English.
        _ => SentenceLanguage::English,
    }
}

/// Merge the user-configured no-break abbreviations that apply to `lang`:
/// the `default` bucket plus the bucket for the language's primary subtag,
/// each normalized to a comparison candidate. Shared by [`resolve_profile`]
/// (markdown path) and the YAML formatter bridge so both resolve the same
/// set.
pub(super) fn merge_no_break_list(config: &Config, lang: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(entries) = config.no_break_abbreviations.get("default") {
        out.extend(
            entries
                .iter()
                .map(|entry| normalize_abbreviation_candidate(entry)),
        );
    }
    if let Some(code) = lang.map(primary_subtag)
        && let Some(entries) = config.no_break_abbreviations.get(code)
    {
        out.extend(
            entries
                .iter()
                .map(|entry| normalize_abbreviation_candidate(entry)),
        );
    }
    out
}

/// Build a [`ResolvedProfile`] directly from a language code and an
/// already-merged, already-normalized list of user no-break abbreviations.
/// Used by the YAML formatter, which carries these as plain data on its
/// options rather than holding a `Config`/`SyntaxNode`.
pub(super) fn profile_from<'a>(
    lang: Option<&str>,
    extra_no_break: &'a [String],
) -> ResolvedProfile<'a> {
    ResolvedProfile {
        builtin: sentence_language_for(lang).profile(),
        extra_no_break,
    }
}

/// Resolve the built-in profile plus any user-configured no-break abbreviations
/// for `node`'s document language. `scratch` owns the normalized user entries
/// for the lifetime of the returned profile. Built once per node-wrap; this
/// could be hoisted to once-per-document if profiling ever warrants it.
pub(super) fn resolve_profile<'a>(
    node: &SyntaxNode,
    config: &Config,
    scratch: &'a mut Vec<String>,
) -> ResolvedProfile<'a> {
    let lang = resolve_lang_string(node, config.lang.as_deref());
    let language = sentence_language_for(lang.as_deref());

    scratch.clear();
    scratch.extend(merge_no_break_list(config, lang.as_deref()));

    ResolvedProfile {
        builtin: language.profile(),
        extra_no_break: scratch.as_slice(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn english() -> ResolvedProfile<'static> {
        ResolvedProfile::builtin_only(SentenceLanguage::English)
    }

    #[test]
    fn abbreviation_periods_are_not_sentence_boundaries() {
        assert!(!is_sentence_boundary_text(
            "(e.g.)",
            Some("Next"),
            true,
            false,
            english()
        ));
        assert!(!is_sentence_boundary_text(
            "i.e.",
            Some("Next"),
            true,
            false,
            english()
        ));
        assert!(!is_sentence_boundary_text(
            "`etc.`",
            Some("Next"),
            true,
            false,
            english()
        ));
        assert!(is_sentence_boundary_text(
            "complete.",
            Some("Next"),
            true,
            false,
            english()
        ));
        assert_eq!(
            decide_sentence_boundary("complete.", Some("Next"), true, false, english()),
            BoundaryDecision::Break
        );
    }

    #[test]
    fn boundary_decision_reports_no_break_for_abbreviation() {
        assert_eq!(
            decide_sentence_boundary("e.g.", Some("Next"), true, false, english()),
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
            english()
        ));
        assert!(is_sentence_boundary_text(
            "co.",
            Some("They"),
            true,
            false,
            english()
        ));
        assert!(!is_sentence_boundary_text(
            "U.S.",
            Some("Government"),
            false,
            false,
            english()
        ));
        assert!(is_sentence_boundary_text(
            "U.S.",
            Some("How"),
            true,
            false,
            english()
        ));
        assert!(!is_sentence_boundary_text(
            "p.m.",
            Some("traveler"),
            false,
            false,
            english()
        ));
    }

    #[test]
    fn german_builtin_abbreviation_no_break() {
        let de = ResolvedProfile::builtin_only(SentenceLanguage::German);
        assert!(!is_sentence_boundary_text(
            "bzw.",
            Some("Next"),
            true,
            false,
            de
        ));
        // The English profile doesn't know `bzw.`, so there it ends a sentence.
        assert!(is_sentence_boundary_text(
            "bzw.",
            Some("Next"),
            true,
            false,
            english()
        ));
    }

    #[test]
    fn czech_builtin_abbreviation_no_break_is_case_insensitive() {
        let cs = ResolvedProfile::builtin_only(SentenceLanguage::Czech);
        assert!(!is_sentence_boundary_text(
            "např.",
            Some("Next"),
            true,
            false,
            cs
        ));
        // Mixed case exercises the `to_lowercase()` normalization path.
        assert!(!is_sentence_boundary_text(
            "Např.",
            Some("Next"),
            true,
            false,
            cs
        ));
        assert!(!is_sentence_boundary_text(
            "atd.",
            Some("Next"),
            true,
            false,
            cs
        ));
    }

    #[test]
    fn spanish_non_ascii_abbreviation_matches_via_list() {
        let es = ResolvedProfile::builtin_only(SentenceLanguage::Spanish);
        // `núm.` is single-period and non-ASCII, so the multi-period heuristic
        // does not apply; it matches only because it is in the Spanish list.
        assert!(!is_sentence_boundary_text(
            "núm.",
            Some("Next"),
            true,
            false,
            es
        ));
        assert!(is_sentence_boundary_text(
            "núm.",
            Some("Next"),
            true,
            false,
            english()
        ));
        // A bogus non-list, non-ASCII multi-period token still breaks: the
        // heuristic stays ASCII-only.
        assert!(is_sentence_boundary_text(
            "ñ.ñ.",
            Some("Next"),
            true,
            false,
            english()
        ));
    }

    #[test]
    fn user_extra_abbreviations_merge_with_builtin() {
        let extras = vec!["zzz.".to_string()];
        let profile = ResolvedProfile {
            builtin: SentenceLanguage::English.profile(),
            extra_no_break: &extras,
        };
        // The user-supplied entry suppresses the break...
        assert!(!is_sentence_boundary_text(
            "zzz.",
            Some("Next"),
            true,
            false,
            profile
        ));
        // ...the built-in English entry still suppresses...
        assert!(!is_sentence_boundary_text(
            "e.g.",
            Some("Next"),
            true,
            false,
            profile
        ));
        // ...and an ordinary word still ends the sentence.
        assert!(is_sentence_boundary_text(
            "done.",
            Some("Next"),
            true,
            false,
            profile
        ));
    }

    #[test]
    fn extracts_lang_from_yaml_frontmatter() {
        let yaml = "---\nlang: en-GB\ntitle: Test\n---";
        assert_eq!(extract_lang_from_yaml_text(yaml).as_deref(), Some("en-GB"));
    }

    #[test]
    fn region_subtag_selects_primary_language_profile() {
        assert!(matches!(
            sentence_language_for(Some("de-at")),
            SentenceLanguage::German
        ));
        assert!(matches!(
            sentence_language_for(Some("en-gb")),
            SentenceLanguage::English
        ));
    }

    #[test]
    fn resolves_lang_string_from_document_metadata() {
        let input = "---\nlang: sv\ntitle: Test\n---\n\nA sentence.";
        let tree = parse(input, None);
        let paragraph = tree
            .descendants()
            .find(|node| node.kind() == SyntaxKind::PARAGRAPH)
            .expect("paragraph node");

        assert_eq!(resolve_lang_string(&paragraph, None).as_deref(), Some("sv"));
        // Swedish has no built-in profile yet, so it falls back to English.
        assert!(matches!(
            sentence_language_for(Some("sv")),
            SentenceLanguage::English
        ));
    }

    #[test]
    fn config_lang_used_as_fallback_when_no_frontmatter() {
        let tree = parse("A sentence.", None);
        let paragraph = tree
            .descendants()
            .find(|node| node.kind() == SyntaxKind::PARAGRAPH)
            .expect("paragraph node");
        assert_eq!(
            resolve_lang_string(&paragraph, Some("de")).as_deref(),
            Some("de")
        );

        // Frontmatter wins over the config fallback.
        let tree = parse("---\nlang: cs\n---\n\nText.", None);
        let paragraph = tree
            .descendants()
            .find(|node| node.kind() == SyntaxKind::PARAGRAPH)
            .expect("paragraph node");
        assert_eq!(
            resolve_lang_string(&paragraph, Some("de")).as_deref(),
            Some("cs")
        );
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
        let lines = split_sentence_segments(&segments, english());
        assert_eq!(lines, vec!["`???` also"]);
    }

    #[test]
    fn split_sentence_text_uses_normal_segment_defaults() {
        let lines = split_sentence_text("Alpha. Beta.", english());
        assert_eq!(lines, vec!["Alpha.", "Beta."]);
    }
}
