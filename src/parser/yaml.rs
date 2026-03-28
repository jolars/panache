//! YAML parser groundwork for long-term Panache integration.
//!
//! This module is intentionally minimal and currently acts as a placeholder for a
//! future in-tree YAML parser that can produce Panache-compatible CST structures.
//! Initial goals:
//! - support plain YAML and hashpipe-prefixed YAML from shared parsing primitives,
//! - preserve lossless syntax/trivia needed for exact host document ranges,
//! - enable shadow-mode comparison against the existing YAML engine before rollout.
//! - prepare for first-class YAML formatting support once parser parity is proven.

#[path = "yaml/core.rs"]
mod core;
#[path = "yaml/model.rs"]
mod model;

pub use core::{
    lex_basic_mapping_tokens, parse_basic_entry, parse_basic_entry_tree, parse_basic_mapping_tree,
    parse_shadow,
};
pub use model::{
    BasicYamlEntry, ShadowYamlOptions, ShadowYamlOutcome, ShadowYamlReport, YamlInputKind,
    YamlShadowToken, YamlShadowTokenKind,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::SyntaxKind;

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
    fn parses_single_line_with_multiple_colons() {
        let parsed = parse_basic_entry("a: b: c: d");
        assert_eq!(
            parsed,
            Some(BasicYamlEntry {
                key: "a",
                value: "b: c: d"
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

        let mapping = content
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
            .expect("yaml block map");
        let entry = mapping
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
            .expect("yaml block map entry");
        let key = entry
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_KEY)
            .expect("yaml block map key");
        let value = entry
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE)
            .expect("yaml block map value");

        let key_token_kinds: Vec<_> = key
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .map(|tok| tok.kind())
            .collect();
        assert_eq!(
            key_token_kinds,
            vec![SyntaxKind::YAML_KEY, SyntaxKind::YAML_COLON,]
        );

        let value_token_kinds: Vec<_> = value
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .map(|tok| tok.kind())
            .collect();
        assert_eq!(
            value_token_kinds,
            vec![SyntaxKind::WHITESPACE, SyntaxKind::YAML_SCALAR,]
        );
    }

    #[test]
    fn builds_basic_rowan_tree_for_multiline_mapping() {
        let tree = parse_basic_mapping_tree("title: My Title\nauthor: Me\n").expect("tree");
        assert_eq!(tree.kind(), SyntaxKind::DOCUMENT);
        assert_eq!(tree.text().to_string(), "title: My Title\nauthor: Me\n");

        let content = tree
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_METADATA_CONTENT)
            .expect("yaml metadata content");
        let mapping = content
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
            .expect("yaml block map");
        let entries: Vec<_> = mapping
            .children()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
            .collect();
        assert_eq!(entries.len(), 2);

        let token_kinds: Vec<_> = mapping
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .map(|tok| tok.kind())
            .collect();
        assert_eq!(
            token_kinds,
            vec![
                SyntaxKind::YAML_KEY,
                SyntaxKind::YAML_COLON,
                SyntaxKind::WHITESPACE,
                SyntaxKind::YAML_SCALAR,
                SyntaxKind::NEWLINE,
                SyntaxKind::YAML_KEY,
                SyntaxKind::YAML_COLON,
                SyntaxKind::WHITESPACE,
                SyntaxKind::YAML_SCALAR,
                SyntaxKind::NEWLINE,
            ]
        );
    }

    #[test]
    fn mapping_nodes_preserve_entry_text_boundaries() {
        let tree = parse_basic_mapping_tree("title: A\nauthor: B\n").expect("tree");
        let content = tree
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_METADATA_CONTENT)
            .expect("yaml metadata content");
        let mapping = content
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
            .expect("yaml block map");

        let entry_texts: Vec<_> = mapping
            .children()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
            .map(|n| n.text().to_string())
            .collect();
        assert_eq!(
            entry_texts,
            vec!["title: A\n".to_string(), "author: B\n".to_string(),]
        );
    }

    #[test]
    fn splits_mapping_on_colon_outside_quoted_key() {
        let input = "\"foo:bar\": 23\n'x:y': 24\n";
        let tree = parse_basic_mapping_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);

        let keys: Vec<String> = tree
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::YAML_KEY)
            .map(|tok| tok.text().to_string())
            .collect();
        assert_eq!(keys, vec!["\"foo:bar\"".to_string(), "'x:y'".to_string()]);
    }

    #[test]
    fn preserves_explicit_tag_tokens_in_key_and_value() {
        let input = "!!str a: !!int 42\n";
        let tree = parse_basic_mapping_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);

        let tag_tokens: Vec<_> = tree
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::YAML_TAG)
            .map(|tok| tok.text().to_string())
            .collect();
        assert_eq!(tag_tokens, vec!["!!str".to_string(), "!!int".to_string()]);
    }

    #[test]
    fn lexer_emits_tokens_for_quoted_keys_and_inline_comments() {
        let input = "\"foo:bar\": 23 # note\n'x:y': 'z' # ok\n";
        let tokens = lex_basic_mapping_tokens(input).expect("tokens");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                YamlShadowTokenKind::Key,
                YamlShadowTokenKind::Colon,
                YamlShadowTokenKind::Whitespace,
                YamlShadowTokenKind::Scalar,
                YamlShadowTokenKind::Whitespace,
                YamlShadowTokenKind::Comment,
                YamlShadowTokenKind::Newline,
                YamlShadowTokenKind::Key,
                YamlShadowTokenKind::Colon,
                YamlShadowTokenKind::Whitespace,
                YamlShadowTokenKind::Scalar,
                YamlShadowTokenKind::Whitespace,
                YamlShadowTokenKind::Comment,
                YamlShadowTokenKind::Newline,
            ]
        );
        let comments: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == YamlShadowTokenKind::Comment)
            .map(|t| t.text)
            .collect();
        assert_eq!(comments, vec!["# note", "# ok"]);
    }

    #[test]
    fn lexer_emits_indent_and_dedent_for_indented_entries() {
        let input = "root: 1\n  child: 2\n";
        let tokens = lex_basic_mapping_tokens(input).expect("tokens");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&YamlShadowTokenKind::Indent));
        assert!(kinds.contains(&YamlShadowTokenKind::Dedent));
    }

    #[test]
    fn parser_builds_nested_block_map_from_indent_tokens() {
        let input = "root: 1\n  child: 2\n";
        let tree = parse_basic_mapping_tree(input).expect("tree");

        let outer_map = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
            .expect("outer map");
        let outer_entry = outer_map
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
            .expect("outer entry");
        let outer_value = outer_entry
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE)
            .expect("outer value");

        let nested_map = outer_value
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
            .expect("nested map");
        let nested_entry_count = nested_map
            .children()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
            .count();
        assert_eq!(nested_entry_count, 1);
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
        assert_eq!(report.shadow_reason, "prototype-basic-mapping-parsed");
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
        assert_eq!(report.shadow_reason, "prototype-basic-mapping-rejected");
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
        assert_eq!(report.shadow_reason, "prototype-basic-mapping-parsed");
        assert_eq!(report.normalized_input.as_deref(), Some("title: My Title"));
    }
}
