//! YAML parser groundwork for long-term Panache integration.
//!
//! This module is intentionally minimal and currently acts as a placeholder for a
//! future in-tree YAML parser that can produce Panache-compatible CST structures.
//! Initial goals:
//! - support plain YAML and hashpipe-prefixed YAML from shared parsing primitives,
//! - preserve lossless syntax/trivia needed for exact host document ranges,
//! - enable shadow-mode comparison against the existing YAML engine before rollout.
//! - prepare for first-class YAML formatting support once parser parity is proven.

use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum YamlInputKind {
    #[default]
    Plain,
    Hashpipe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShadowYamlOptions {
    pub enabled: bool,
    pub input_kind: YamlInputKind,
}

impl Default for ShadowYamlOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            input_kind: YamlInputKind::Plain,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowYamlOutcome {
    SkippedDisabled,
    PrototypeParsed,
    PrototypeRejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowYamlReport {
    pub outcome: ShadowYamlOutcome,
    pub shadow_reason: &'static str,
    pub input_kind: YamlInputKind,
    pub input_len_bytes: usize,
    pub line_count: usize,
    pub normalized_input: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicYamlEntry<'a> {
    pub key: &'a str,
    pub value: &'a str,
}

/// Parse YAML in shadow mode using prototype groundwork only.
///
/// This API is intentionally read-only and does not replace production YAML
/// parsing. By default it is disabled and reports `SkippedDisabled`.
pub fn parse_shadow(input: &str, options: ShadowYamlOptions) -> ShadowYamlReport {
    let line_count = input.lines().count().max(1);

    if !options.enabled {
        return ShadowYamlReport {
            outcome: ShadowYamlOutcome::SkippedDisabled,
            shadow_reason: "shadow-disabled",
            input_kind: options.input_kind,
            input_len_bytes: input.len(),
            line_count,
            normalized_input: None,
        };
    }

    let normalized = match options.input_kind {
        YamlInputKind::Plain => input.to_owned(),
        YamlInputKind::Hashpipe => normalize_hashpipe_input(input),
    };

    let normalized_for_entry = normalized.trim_end_matches(['\n', '\r']);
    let parsed = parse_basic_entry(normalized_for_entry).is_some();

    ShadowYamlReport {
        outcome: if parsed {
            ShadowYamlOutcome::PrototypeParsed
        } else {
            ShadowYamlOutcome::PrototypeRejected
        },
        shadow_reason: if parsed {
            "prototype-basic-entry-parsed"
        } else {
            "prototype-basic-entry-rejected"
        },
        input_kind: options.input_kind,
        input_len_bytes: input.len(),
        line_count,
        normalized_input: Some(normalized),
    }
}

fn normalize_hashpipe_input(input: &str) -> String {
    input
        .lines()
        .map(strip_hashpipe_prefix)
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_hashpipe_prefix(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix("#|") {
        return rest.strip_prefix(' ').unwrap_or(rest);
    }
    line
}

/// Parse a single-line YAML mapping entry like `title: My Title`.
///
/// This is intentionally minimal groundwork and currently supports exactly one
/// `key: value` line.
pub fn parse_basic_entry(input: &str) -> Option<BasicYamlEntry<'_>> {
    if input.contains('\n') {
        return None;
    }

    let (raw_key, raw_value) = input.split_once(':')?;
    let key = raw_key.trim();
    let value = raw_value.trim();

    if key.is_empty() || value.is_empty() {
        return None;
    }

    Some(BasicYamlEntry { key, value })
}

/// Parse a single-line YAML mapping entry and emit a tiny Rowan CST.
///
/// The current prototype emits:
/// DOCUMENT
///   YAML_METADATA_CONTENT
///     TEXT(key)
///     TEXT(":")
///     [WHITESPACE(" ")] // when present in the original input
///     TEXT(value)
pub fn parse_basic_entry_tree(input: &str) -> Option<SyntaxNode> {
    let entry = parse_basic_entry(input)?;
    let (_, raw_value) = input.split_once(':')?;

    let mut builder = GreenNodeBuilder::new();
    builder.start_node(SyntaxKind::DOCUMENT.into());
    builder.start_node(SyntaxKind::YAML_METADATA_CONTENT.into());
    builder.token(SyntaxKind::TEXT.into(), entry.key);
    builder.token(SyntaxKind::TEXT.into(), ":");

    let leading_spaces = raw_value.len() - raw_value.trim_start_matches(' ').len();
    if leading_spaces > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &raw_value[..leading_spaces]);
    }
    builder.token(SyntaxKind::TEXT.into(), entry.value);
    builder.finish_node(); // YAML_METADATA_CONTENT
    builder.finish_node(); // DOCUMENT

    Some(SyntaxNode::new_root(builder.finish()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_title_entry() {
        let parsed = parse_basic_entry("title: My Title");
        assert_eq!(
            parsed,
            Some(BasicYamlEntry {
                key: "title",
                value: "My Title"
            })
        );
    }

    #[test]
    fn rejects_missing_value() {
        assert_eq!(parse_basic_entry("title:"), None);
    }

    #[test]
    fn rejects_multiline_input() {
        assert_eq!(parse_basic_entry("title: My Title\nauthor: Me"), None);
    }

    #[test]
    fn accepts_single_line_with_crlf_terminator() {
        let parsed = parse_basic_entry("title: My Title\r");
        assert_eq!(
            parsed,
            Some(BasicYamlEntry {
                key: "title",
                value: "My Title"
            })
        );
    }

    #[test]
    fn builds_basic_rowan_tree() {
        let tree = parse_basic_entry_tree("title: My Title").expect("tree");
        assert_eq!(tree.kind(), SyntaxKind::DOCUMENT);
        assert_eq!(tree.text().to_string(), "title: My Title");

        let content = tree
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_METADATA_CONTENT)
            .expect("yaml metadata content");
        assert_eq!(content.text().to_string(), "title: My Title");
    }

    #[test]
    fn rejects_tree_for_invalid_input() {
        assert!(parse_basic_entry_tree("title:").is_none());
    }

    #[test]
    fn shadow_parse_is_disabled_by_default() {
        let report = parse_shadow("title: My Title", ShadowYamlOptions::default());
        assert_eq!(report.outcome, ShadowYamlOutcome::SkippedDisabled);
        assert_eq!(report.shadow_reason, "shadow-disabled");
        assert_eq!(report.normalized_input, None);
    }

    #[test]
    fn shadow_parse_skips_when_disabled_even_for_valid_input() {
        let report = parse_shadow(
            "title: My Title",
            ShadowYamlOptions {
                enabled: false,
                input_kind: YamlInputKind::Plain,
            },
        );
        assert_eq!(report.outcome, ShadowYamlOutcome::SkippedDisabled);
        assert_eq!(report.shadow_reason, "shadow-disabled");
    }

    #[test]
    fn shadow_parse_reports_prototype_parsed_when_enabled() {
        let report = parse_shadow(
            "title: My Title",
            ShadowYamlOptions {
                enabled: true,
                input_kind: YamlInputKind::Plain,
            },
        );
        assert_eq!(report.outcome, ShadowYamlOutcome::PrototypeParsed);
        assert_eq!(report.shadow_reason, "prototype-basic-entry-parsed");
        assert_eq!(report.normalized_input.as_deref(), Some("title: My Title"));
    }

    #[test]
    fn shadow_parse_reports_prototype_rejected_when_enabled() {
        let report = parse_shadow(
            "title:",
            ShadowYamlOptions {
                enabled: true,
                input_kind: YamlInputKind::Plain,
            },
        );
        assert_eq!(report.outcome, ShadowYamlOutcome::PrototypeRejected);
        assert_eq!(report.shadow_reason, "prototype-basic-entry-rejected");
    }

    #[test]
    fn shadow_parse_accepts_hashpipe_mode_but_remains_prototype_scoped() {
        let report = parse_shadow(
            "#| title: My Title",
            ShadowYamlOptions {
                enabled: true,
                input_kind: YamlInputKind::Hashpipe,
            },
        );
        assert_eq!(report.outcome, ShadowYamlOutcome::PrototypeParsed);
        assert_eq!(report.shadow_reason, "prototype-basic-entry-parsed");
        assert_eq!(report.normalized_input.as_deref(), Some("title: My Title"));
    }
}
