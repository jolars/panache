//! YAML parser groundwork for long-term Panache integration.
//!
//! This module is intentionally minimal and currently acts as a placeholder for a
//! future in-tree YAML parser that can produce Panache-compatible CST structures.
//! Initial goals:
//! - support plain YAML and hashpipe-prefixed YAML from shared parsing primitives,
//! - preserve lossless syntax/trivia needed for exact host document ranges,
//! - enable shadow-mode comparison against the existing YAML engine before rollout.
//! - prepare for first-class YAML formatting support once parser parity is proven.

#[path = "yaml/events.rs"]
mod events;
#[path = "yaml/model.rs"]
mod model;
#[path = "yaml/parser.rs"]
mod parser;
#[path = "yaml/parser_v2.rs"]
mod parser_v2;
#[path = "yaml/scanner.rs"]
mod scanner;
#[path = "yaml/validator.rs"]
mod validator;

pub use events::{project_events, project_events_from_tree};
pub use model::{
    ShadowYamlOptions, ShadowYamlOutcome, ShadowYamlReport, YamlDiagnostic, YamlInputKind,
    YamlParseReport, diagnostic_codes,
};
pub use parser::{parse_shadow, parse_yaml_report, parse_yaml_tree};
pub use parser_v2::{ShadowParserV2Report, parse_v2, shadow_parser_v2_check};
pub use scanner::{ShadowScannerReport, shadow_scanner_check};

#[doc(hidden)]
pub fn validate_yaml_for_test(input: &str) -> Option<YamlDiagnostic> {
    validator::validate_yaml(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::SyntaxKind;

    #[test]
    fn builds_basic_rowan_tree_for_multiline_mapping() {
        let tree = parse_yaml_tree("title: My Title\nauthor: Me\n").expect("tree");
        assert_eq!(tree.kind(), SyntaxKind::DOCUMENT);
        assert_eq!(tree.text().to_string(), "title: My Title\nauthor: Me\n");

        let mapping = tree
            .descendants()
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
                SyntaxKind::YAML_SCALAR,
                SyntaxKind::YAML_COLON,
                SyntaxKind::WHITESPACE,
                SyntaxKind::YAML_SCALAR,
                SyntaxKind::NEWLINE,
                SyntaxKind::YAML_SCALAR,
                SyntaxKind::YAML_COLON,
                SyntaxKind::WHITESPACE,
                SyntaxKind::YAML_SCALAR,
                SyntaxKind::NEWLINE,
            ]
        );
    }

    fn block_map_key_texts(tree: &crate::syntax::SyntaxNode) -> Vec<String> {
        tree.descendants()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_KEY)
            .map(|key| {
                key.children_with_tokens()
                    .filter_map(|el| el.into_token())
                    .filter(|tok| tok.kind() == SyntaxKind::YAML_SCALAR)
                    .map(|tok| tok.text().to_string())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .filter(|s| !s.is_empty())
            .collect()
    }

    #[test]
    fn mapping_nodes_preserve_entry_text_boundaries() {
        let tree = parse_yaml_tree("title: A\nauthor: B\n").expect("tree");
        let mapping = tree
            .descendants()
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
        let tree = parse_yaml_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);
        assert_eq!(
            block_map_key_texts(&tree),
            vec!["\"foo:bar\"".to_string(), "'x:y'".to_string()]
        );
    }

    #[test]
    fn keeps_colon_inside_escaped_double_quoted_key() {
        let input = "\"foo\\\":bar\": 23\n";
        let tree = parse_yaml_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);
        assert_eq!(
            block_map_key_texts(&tree),
            vec!["\"foo\\\":bar\"".to_string()]
        );
    }

    #[test]
    fn keeps_hash_in_double_quoted_scalar_value() {
        let input = "foo: \"a#b\"\n";
        let tree = parse_yaml_tree(input).expect("tree");

        let comment_count = tree
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::YAML_COMMENT)
            .count();
        assert_eq!(comment_count, 0);

        let value_scalars: Vec<String> = tree
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE)
            .flat_map(|value| {
                value
                    .children_with_tokens()
                    .filter_map(|el| el.into_token())
                    .filter(|tok| tok.kind() == SyntaxKind::YAML_SCALAR)
                    .map(|tok| tok.text().to_string())
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(value_scalars, vec!["\"a#b\"".to_string()]);
    }

    #[test]
    fn keeps_colon_inside_single_quoted_key_with_escaped_quote() {
        let input = "'foo'':bar': 23\n";
        let tree = parse_yaml_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);
        assert_eq!(block_map_key_texts(&tree), vec!["'foo'':bar'".to_string()]);
    }

    #[test]
    fn parser_preserves_document_markers_and_directives() {
        let input = "%YAML 1.2\n---\nfoo: bar\n...\n";
        let tree = parse_yaml_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);

        let scalar_tokens: Vec<String> = tree
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::YAML_SCALAR)
            .map(|tok| tok.text().to_string())
            .collect();

        assert!(scalar_tokens.contains(&"%YAML 1.2".to_string()));
        assert!(scalar_tokens.contains(&"bar".to_string()));

        let has_doc_start = tree
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .any(|tok| tok.kind() == SyntaxKind::YAML_DOCUMENT_START && tok.text() == "---");
        assert!(has_doc_start, "--- should be a YAML_DOCUMENT_START token");

        let has_doc_end = tree
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .any(|tok| tok.kind() == SyntaxKind::YAML_DOCUMENT_END && tok.text() == "...");
        assert!(has_doc_end, "... should be a YAML_DOCUMENT_END token");
    }

    #[test]
    fn parser_preserves_standalone_flow_mapping_lines() {
        let input = "{foo: bar}\n";
        let tree = parse_yaml_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);

        let flow_entry_count = tree
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP_ENTRY)
            .count();
        assert_eq!(flow_entry_count, 1);

        let flow_values: Vec<String> = tree
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP_VALUE)
            .map(|n| n.text().to_string())
            .collect();
        assert_eq!(flow_values, vec![" bar".to_string()]);
    }

    #[test]
    fn parser_preserves_top_level_quoted_scalar_document() {
        let input = "\"foo: bar\\\": baz\"\n";
        let tree = parse_yaml_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn parse_yaml_report_emits_error_code_for_invalid_yaml() {
        // `this` at the top of a block-map context is a stray scalar with no
        // following colon — flagged at the leading scalar rather than at the
        // later indent that surfaced as a side-effect.
        let report = parse_yaml_report("this\n is\n  invalid: x\n");
        assert!(report.tree.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            diagnostic_codes::PARSE_INVALID_KEY_TOKEN
        );
    }

    #[test]
    fn parse_yaml_report_detects_trailing_content_after_document_end() {
        let report = parse_yaml_report("---\nkey: value\n... invalid\n");
        assert!(report.tree.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            diagnostic_codes::LEX_TRAILING_CONTENT_AFTER_DOCUMENT_END
        );
    }

    #[test]
    fn parse_yaml_report_detects_unexpected_flow_closer() {
        let report = parse_yaml_report("---\n[ a, b, c ] ]\n");
        assert!(report.tree.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            diagnostic_codes::PARSE_TRAILING_CONTENT_AFTER_FLOW_END
        );
    }

    #[test]
    fn parse_yaml_report_detects_unterminated_nested_flow_sequence() {
        let report = parse_yaml_report("---\n[ [ a, b, c ]\n");
        assert!(report.tree.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            diagnostic_codes::PARSE_UNTERMINATED_FLOW_SEQUENCE
        );
    }

    #[test]
    fn parse_yaml_report_detects_invalid_leading_flow_sequence_comma() {
        let report = parse_yaml_report("---\n[ , a, b, c ]\n");
        assert!(report.tree.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            diagnostic_codes::PARSE_INVALID_FLOW_SEQUENCE_COMMA
        );
    }

    #[test]
    fn parse_yaml_report_detects_trailing_content_after_flow_end() {
        let report = parse_yaml_report("---\n[ a, b, c, ]#invalid\n");
        assert!(report.tree.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            diagnostic_codes::PARSE_TRAILING_CONTENT_AFTER_FLOW_END
        );
    }

    #[test]
    fn parse_yaml_report_detects_invalid_double_quoted_escape() {
        let report = parse_yaml_report("---\n\"\\.\"\n");
        assert!(report.tree.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            diagnostic_codes::LEX_INVALID_DOUBLE_QUOTED_ESCAPE
        );
    }

    #[test]
    fn parse_yaml_report_detects_trailing_content_after_document_start() {
        let report = parse_yaml_report("--- key1: value1\n    key2: value2\n");
        assert!(report.tree.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            diagnostic_codes::LEX_TRAILING_CONTENT_AFTER_DOCUMENT_START
        );
    }

    #[test]
    fn parse_yaml_report_detects_directive_without_document_start() {
        let report = parse_yaml_report("%YAML 1.2\n");
        assert!(report.tree.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            diagnostic_codes::PARSE_DIRECTIVE_WITHOUT_DOCUMENT_START
        );
    }

    #[test]
    fn parse_yaml_report_detects_directive_after_content() {
        // EB22-shape: a comment terminates the plain scalar, leaving
        // `%YAML 1.2` at column 0 in directive position after content.
        let report = parse_yaml_report("---\nscalar1 # comment\n%YAML 1.2\n---\nscalar2\n");
        assert!(report.tree.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            diagnostic_codes::PARSE_DIRECTIVE_AFTER_CONTENT
        );
    }

    #[test]
    fn parse_yaml_report_detects_wrong_indented_flow_continuation() {
        let report = parse_yaml_report("---\nflow: [a,\nb,\nc]\n");
        assert!(report.tree.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            diagnostic_codes::LEX_WRONG_INDENTED_FLOW
        );
    }

    #[test]
    fn parser_builds_flow_sequence_nodes_in_mapping_value() {
        let input = "a: [b, c]\n";
        let tree = parse_yaml_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);

        let seq = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
            .expect("flow sequence node");
        let item_count = seq
            .children()
            .filter(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE_ITEM)
            .count();
        assert_eq!(item_count, 2);
    }

    #[test]
    fn parser_absorbs_literal_block_scalar_into_map_value() {
        let input = "a: |\n  line1\n  line2\n";
        let tree = parse_yaml_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);

        let map = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
            .expect("block map");
        let entry = map
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
            .expect("entry");
        let value = entry
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE)
            .expect("value");
        let value_text = value.text().to_string();
        assert!(
            value_text.starts_with('|') || value_text.starts_with(" |"),
            "value should contain the `|` header, got {value_text:?}"
        );
        assert!(
            value_text.contains("line1") && value_text.contains("line2"),
            "value should absorb block scalar content, got {value_text:?}"
        );
    }

    #[test]
    fn parser_builds_nested_block_sequence_on_same_line() {
        let input = "- - a\n  - b\n- c\n";
        let tree = parse_yaml_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);

        let outer = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE)
            .expect("outer block sequence");
        let outer_items: Vec<_> = outer
            .children()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM)
            .collect();
        assert_eq!(outer_items.len(), 2);

        let nested = outer_items[0]
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE)
            .expect("nested block sequence inside first item");
        let nested_items = nested
            .children()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM)
            .count();
        assert_eq!(nested_items, 2);
    }

    #[test]
    fn parser_builds_multiline_flow_map_inside_block_sequence_item() {
        let input = "- { multi\n  line, a: b}\n";
        let tree = parse_yaml_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);

        let seq = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE)
            .expect("block sequence");
        let item = seq
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM)
            .expect("sequence item");
        item.children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP)
            .expect("flow map inside sequence item");
    }

    #[test]
    fn parser_builds_flow_sequence_inside_block_sequence_item() {
        let input = "- [a, b]\n- [c, d]\n";
        let tree = parse_yaml_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);

        let seq = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE)
            .expect("block sequence");
        let items: Vec<_> = seq
            .children()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM)
            .collect();
        assert_eq!(items.len(), 2);

        for item in &items {
            let flow = item
                .children()
                .find(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
                .expect("flow sequence inside item");
            let flow_items = flow
                .children()
                .filter(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE_ITEM)
                .count();
            assert_eq!(flow_items, 2);
        }
    }

    #[test]
    fn parser_emits_scalar_document_for_tag_without_colon() {
        let input = "! a\n";
        let tree = parse_yaml_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);

        let has_block_map = tree
            .descendants()
            .any(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP);
        assert!(
            !has_block_map,
            "scalar document should not be wrapped in YAML_BLOCK_MAP"
        );

        // v2 keeps the leading `!` inside the scalar's source bytes; the
        // projection layer resolves tags from the scalar text.
        let has_tagged_scalar = tree
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .any(|tok| tok.kind() == SyntaxKind::YAML_SCALAR && tok.text().starts_with('!'));
        assert!(has_tagged_scalar, "tree should contain tag bytes in scalar");
    }

    #[test]
    fn parser_builds_nested_block_map_inside_block_sequence() {
        let input = "-\n  name: Mark\n  hr: 65\n";
        let tree = parse_yaml_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);

        let seq = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE)
            .expect("block sequence");
        let items: Vec<_> = seq
            .children()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM)
            .collect();
        assert_eq!(items.len(), 1);

        let nested_map = items[0]
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
            .expect("nested block map inside sequence item");
        let entry_count = nested_map
            .children()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
            .count();
        assert_eq!(entry_count, 2);
    }

    #[test]
    fn parser_builds_nested_block_map_from_indent_tokens() {
        let input = "root:\n  child: 2\n";
        let tree = parse_yaml_tree(input).expect("tree");

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
        // An unterminated flow sequence is rejected by the v2-aware
        // structural validator, which is the rejection signal exercised
        // by the shadow parse plumbing.
        let report = parse_shadow(
            "[ a, b",
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
