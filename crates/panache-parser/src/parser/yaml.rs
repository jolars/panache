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
#[path = "yaml/lexer.rs"]
mod lexer;
#[path = "yaml/model.rs"]
mod model;
#[path = "yaml/parser.rs"]
mod parser;
#[path = "yaml/scanner.rs"]
mod scanner;

pub use events::project_events;
pub use lexer::lex_mapping_tokens;
pub use model::{
    ShadowYamlOptions, ShadowYamlOutcome, ShadowYamlReport, YamlDiagnostic, YamlInputKind,
    YamlParseReport, YamlToken, YamlTokenSpan, diagnostic_codes,
};
pub use parser::{parse_shadow, parse_yaml_report, parse_yaml_tree};
pub use scanner::{ShadowScannerReport, shadow_scanner_check};

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

        let keys: Vec<String> = tree
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::YAML_KEY)
            .map(|tok| tok.text().to_string())
            .collect();
        assert_eq!(keys, vec!["\"foo:bar\"".to_string(), "'x:y'".to_string()]);
    }

    #[test]
    fn splits_mapping_on_colon_outside_flow_key() {
        let input = "{a: b}: 23\n";
        let tree = parse_yaml_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);

        let keys: Vec<String> = tree
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::YAML_KEY)
            .map(|tok| tok.text().to_string())
            .collect();
        assert_eq!(keys, vec!["{a: b}".to_string()]);
    }

    #[test]
    fn keeps_colon_inside_escaped_double_quoted_key() {
        let input = "\"foo\\\":bar\": 23\n";
        let tree = parse_yaml_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);

        let keys: Vec<String> = tree
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::YAML_KEY)
            .map(|tok| tok.text().to_string())
            .collect();
        assert_eq!(keys, vec!["\"foo\\\":bar\"".to_string()]);
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

        let scalar_values: Vec<String> = tree
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::YAML_SCALAR)
            .map(|tok| tok.text().to_string())
            .collect();
        assert_eq!(scalar_values, vec!["\"a#b\"".to_string()]);
    }

    #[test]
    fn keeps_colon_inside_single_quoted_key_with_escaped_quote() {
        let input = "'foo'':bar': 23\n";
        let tree = parse_yaml_tree(input).expect("tree");
        assert_eq!(tree.text().to_string(), input);

        let keys: Vec<String> = tree
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::YAML_KEY)
            .map(|tok| tok.text().to_string())
            .collect();
        assert_eq!(keys, vec!["'foo'':bar'".to_string()]);
    }

    #[test]
    fn preserves_explicit_tag_tokens_in_key_and_value() {
        let input = "!!str a: !!int 42\n";
        let tree = parse_yaml_tree(input).expect("tree");
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
        let tokens = lex_mapping_tokens(input).expect("tokens");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                YamlToken::Key,
                YamlToken::Colon,
                YamlToken::Whitespace,
                YamlToken::Scalar,
                YamlToken::Whitespace,
                YamlToken::Comment,
                YamlToken::Newline,
                YamlToken::Key,
                YamlToken::Colon,
                YamlToken::Whitespace,
                YamlToken::Scalar,
                YamlToken::Whitespace,
                YamlToken::Comment,
                YamlToken::Newline,
            ]
        );
        let comments: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == YamlToken::Comment)
            .map(|t| t.text)
            .collect();
        assert_eq!(comments, vec!["# note", "# ok"]);
    }

    #[test]
    fn lexer_emits_indent_and_dedent_for_indented_entries() {
        let input = "root: 1\n  child: 2\n";
        let tokens = lex_mapping_tokens(input).expect("tokens");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&YamlToken::Indent));
        assert!(kinds.contains(&YamlToken::Dedent));
    }

    #[test]
    fn lexer_emits_document_start_marker_token() {
        let input = "---\n";
        let tokens = lex_mapping_tokens(input).expect("tokens");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![YamlToken::DocumentStart, YamlToken::Newline,]);
    }

    #[test]
    fn lexer_emits_flow_tokens_for_standalone_flow_mapping() {
        let input = "{foo: bar}\n";
        let tokens = lex_mapping_tokens(input).expect("tokens");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                YamlToken::FlowMapStart,
                YamlToken::Scalar,
                YamlToken::FlowMapEnd,
                YamlToken::Newline,
            ]
        );
    }

    #[test]
    fn lexer_emits_flow_sequence_tokens_in_mapping_value() {
        let input = "a: [b, c]\n";
        let tokens = lex_mapping_tokens(input).expect("tokens");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                YamlToken::Key,
                YamlToken::Colon,
                YamlToken::Whitespace,
                YamlToken::FlowSeqStart,
                YamlToken::Scalar,
                YamlToken::Comma,
                YamlToken::Scalar,
                YamlToken::FlowSeqEnd,
                YamlToken::Newline,
            ]
        );
    }

    #[test]
    fn lexer_tokens_round_trip_input_bytes_for_supported_cases() {
        let cases = [
            "foo: bar\n",
            "a: [b, c]\n",
            "---\nfoo: bar\n...\n",
            "%YAML 1.2\nfoo: \"a#b\"\n",
        ];

        for input in cases {
            let tokens = lex_mapping_tokens(input).expect("tokens");
            let rebuilt = tokens.iter().map(|t| t.text).collect::<String>();
            assert_eq!(rebuilt, input);
        }
    }

    #[test]
    fn lexer_emits_monotonic_byte_ranges() {
        let input = "root: 1\n  child: 2\n";
        let tokens = lex_mapping_tokens(input).expect("tokens");

        let mut offset = 0usize;
        for token in tokens {
            if token.text.is_empty() {
                assert_eq!(token.byte_start, offset);
                assert_eq!(token.byte_end, offset);
                continue;
            }

            assert_eq!(token.byte_start, offset);
            assert_eq!(&input[token.byte_start..token.byte_end], token.text);
            offset = token.byte_end;
        }

        assert_eq!(offset, input.len());
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
        let report = parse_yaml_report("!foo \"bar\"\n%TAG ! tag:example.com,2000:app/\n---\n");
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
    fn lexer_emits_literal_block_scalar_header_and_content() {
        let input = "a: |\n  line1\n  line2\n";
        let tokens = lex_mapping_tokens(input).expect("tokens");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                YamlToken::Key,
                YamlToken::Colon,
                YamlToken::Whitespace,
                YamlToken::BlockScalarHeader,
                YamlToken::Newline,
                YamlToken::BlockScalarContent,
                YamlToken::Newline,
                YamlToken::BlockScalarContent,
                YamlToken::Newline,
            ]
        );
        let texts: Vec<_> = tokens.iter().map(|t| t.text).collect();
        assert_eq!(
            texts,
            vec!["a", ":", " ", "|", "\n", "  line1", "\n", "  line2", "\n"]
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
        let flow_map = item
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP)
            .expect("flow map inside sequence item");
        let entry_count = flow_map
            .children()
            .filter(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP_ENTRY)
            .count();
        assert_eq!(entry_count, 2);
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
    fn lexer_recognizes_single_bang_tag_in_top_level_scalar() {
        let tokens = lex_mapping_tokens("! a\n").expect("tokens");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                YamlToken::Tag,
                YamlToken::Whitespace,
                YamlToken::Scalar,
                YamlToken::Newline,
            ]
        );
        let texts: Vec<_> = tokens.iter().map(|t| t.text).collect();
        assert_eq!(texts, vec!["!", " ", "a", "\n"]);
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

        let has_tag = tree
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .any(|tok| tok.kind() == SyntaxKind::YAML_TAG && tok.text() == "!");
        assert!(has_tag, "tree should contain YAML_TAG '!'");
    }

    #[test]
    fn lexer_extracts_explicit_tag_before_block_sequence_scalar() {
        let tokens = lex_mapping_tokens("- !!int 1\n").expect("tokens");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                YamlToken::BlockSeqEntry,
                YamlToken::Whitespace,
                YamlToken::Tag,
                YamlToken::Whitespace,
                YamlToken::Scalar,
                YamlToken::Newline,
            ]
        );
        let texts: Vec<_> = tokens.iter().map(|t| t.text).collect();
        assert_eq!(texts, vec!["-", " ", "!!int", " ", "1", "\n"]);
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
        let input = "root: 1\n  child: 2\n";
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
        // Tab indentation is prohibited by YAML spec for block structures
        let report = parse_shadow(
            "\ttitle: value",
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
