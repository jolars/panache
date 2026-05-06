//! Step-11 parser scaffold — a CST builder that consumes the streaming
//! scanner. Currently wraps each contiguous run of body content in a
//! `YAML_DOCUMENT` node, with `---` / `...` markers consumed inside the
//! document they delimit. Inner containers (block maps, block sequences,
//! flow containers) remain flat — subsequent sub-commits introduce that
//! nesting so `project_events` over the resulting CST matches each
//! fixture's `test.event`.
//!
//! Until inner nesting is in place this module's output is byte-lossless
//! but structurally inadequate for event parity. The harness in
//! `tests/yaml.rs` (`shadow_parser_v2_text_losslessness_over_allowlist`)
//! gates the byte-completeness invariant on every commit.

#![allow(dead_code)]

use rowan::GreenNodeBuilder;

use crate::syntax::{SyntaxKind, SyntaxNode};

use super::scanner::{Scanner, TokenKind, TriviaKind};

/// Drive the scanner over `input` and build a CST. Always returns a
/// `SyntaxNode` — the scanner is permissive and the v2 builder
/// preserves bytes regardless of well-formedness.
pub(crate) fn parse_v2(input: &str) -> SyntaxNode {
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(SyntaxKind::YAML_STREAM.into());
    let mut scanner = Scanner::new(input);
    let mut doc_open = false;
    while let Some(tok) = scanner.next_token() {
        // Truly synthetic markers carry no source bytes; skip in the
        // flat shape. Sub-commits will use them to drive node
        // start/finish. Note that `FlowSequenceStart`/`FlowMappingEnd`
        // etc. ARE source-backed (the `[`/`}` chars) and stay.
        if matches!(
            tok.kind,
            TokenKind::StreamStart
                | TokenKind::StreamEnd
                | TokenKind::BlockSequenceStart
                | TokenKind::BlockMappingStart
                | TokenKind::BlockEnd
        ) {
            continue;
        }
        // `Key` tokens come in two flavours: source-backed (the `?`
        // explicit-key indicator, 1 byte) and synthetic (0-width
        // splice from `fetch_value`). Only the synthetic ones are
        // dropped — the explicit indicator's byte is real source.
        if tok.kind == TokenKind::Key && tok.start.index == tok.end.index {
            continue;
        }
        let text = &input[tok.start.index..tok.end.index];
        if text.is_empty() {
            // Defensive: never emit zero-width tokens (rowan rejects).
            continue;
        }
        let kind = map_token_to_syntax_kind(tok.kind);
        match tok.kind {
            TokenKind::DocumentStart => {
                // `---` always begins a fresh document. Close any open
                // doc first, then emit the marker inside the new one.
                if doc_open {
                    builder.finish_node();
                }
                builder.start_node(SyntaxKind::YAML_DOCUMENT.into());
                doc_open = true;
                builder.token(kind.into(), text);
            }
            TokenKind::DocumentEnd => {
                // `...` closes the current document. If none is open
                // (bare `...` at stream start), open a synthetic empty
                // document so the marker has somewhere lossless to live.
                if !doc_open {
                    builder.start_node(SyntaxKind::YAML_DOCUMENT.into());
                }
                builder.token(kind.into(), text);
                builder.finish_node();
                doc_open = false;
            }
            TokenKind::Trivia(_) => {
                // Trivia goes to whichever level is currently open;
                // pre-document trivia stays at YAML_STREAM, in-document
                // trivia stays inside the YAML_DOCUMENT.
                builder.token(kind.into(), text);
            }
            _ => {
                // Any non-trivia content opens an implicit document
                // when one isn't already in progress.
                if !doc_open {
                    builder.start_node(SyntaxKind::YAML_DOCUMENT.into());
                    doc_open = true;
                }
                builder.token(kind.into(), text);
            }
        }
    }
    if doc_open {
        builder.finish_node();
    }
    builder.finish_node();
    SyntaxNode::new_root(builder.finish())
}

fn map_token_to_syntax_kind(kind: TokenKind) -> SyntaxKind {
    match kind {
        TokenKind::Trivia(TriviaKind::Whitespace) => SyntaxKind::WHITESPACE,
        TokenKind::Trivia(TriviaKind::Newline) => SyntaxKind::NEWLINE,
        TokenKind::Trivia(TriviaKind::Comment) => SyntaxKind::YAML_COMMENT,
        TokenKind::DocumentStart => SyntaxKind::YAML_DOCUMENT_START,
        TokenKind::DocumentEnd => SyntaxKind::YAML_DOCUMENT_END,
        TokenKind::Directive => SyntaxKind::YAML_SCALAR,
        TokenKind::BlockEntry => SyntaxKind::YAML_BLOCK_SEQ_ENTRY,
        TokenKind::FlowEntry => SyntaxKind::YAML_SCALAR,
        TokenKind::FlowSequenceStart | TokenKind::FlowSequenceEnd => SyntaxKind::YAML_SCALAR,
        TokenKind::FlowMappingStart | TokenKind::FlowMappingEnd => SyntaxKind::YAML_SCALAR,
        TokenKind::Value => SyntaxKind::YAML_COLON,
        TokenKind::Anchor | TokenKind::Alias | TokenKind::Tag => SyntaxKind::YAML_TAG,
        TokenKind::Scalar(_) => SyntaxKind::YAML_SCALAR,
        // Source-backed `Key` (the explicit `?` indicator) — there is
        // no dedicated SyntaxKind yet, route to YAML_KEY for now.
        TokenKind::Key => SyntaxKind::YAML_KEY,
        // Synthetic markers handled before this map; defensive
        // fallback.
        TokenKind::StreamStart
        | TokenKind::StreamEnd
        | TokenKind::BlockSequenceStart
        | TokenKind::BlockMappingStart
        | TokenKind::BlockEnd => SyntaxKind::YAML_SCALAR,
    }
}

/// Public byte-completeness report from running the v2 parser scaffold
/// over an input. The harness in `tests/yaml.rs` uses this to gate
/// each step-11 sub-commit on losslessness.
#[derive(Debug, Clone)]
pub struct ShadowParserV2Report {
    /// True if `tree.text() == input`.
    pub text_lossless: bool,
    /// Number of children directly under YAML_STREAM (a coarse proxy
    /// for "did we emit any nesting yet"); useful to track structural
    /// progression across sub-commits.
    pub stream_child_count: usize,
}

/// Run the v2 parser and return a losslessness report. Exposed so the
/// integration harness can run over allowlisted fixtures without
/// depending on private types.
pub fn shadow_parser_v2_check(input: &str) -> ShadowParserV2Report {
    let tree = parse_v2(input);
    let text = tree.text().to_string();
    ShadowParserV2Report {
        text_lossless: text == input,
        stream_child_count: tree.children().count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_returns_byte_lossless_cst_for_empty_input() {
        let report = shadow_parser_v2_check("");
        assert!(report.text_lossless);
    }

    #[test]
    fn v2_returns_byte_lossless_cst_for_simple_mapping() {
        let report = shadow_parser_v2_check("key: value\n");
        assert!(report.text_lossless);
    }

    #[test]
    fn v2_returns_byte_lossless_cst_for_block_sequence() {
        let report = shadow_parser_v2_check("- a\n- b\n");
        assert!(report.text_lossless);
    }

    #[test]
    fn v2_returns_byte_lossless_cst_for_flow_mapping() {
        let report = shadow_parser_v2_check("{a: b, c: d}\n");
        assert!(report.text_lossless);
    }

    #[test]
    fn v2_returns_byte_lossless_cst_for_block_scalar() {
        let report = shadow_parser_v2_check("key: |\n  hello\n  world\n");
        assert!(report.text_lossless);
    }

    #[test]
    fn v2_returns_byte_lossless_cst_for_quoted_scalar() {
        let report = shadow_parser_v2_check("\"key\": \"value\"\n");
        assert!(report.text_lossless);
    }

    #[test]
    fn v2_returns_byte_lossless_cst_for_multi_line_plain_scalar() {
        let report = shadow_parser_v2_check("key: hello\n  world\n");
        assert!(report.text_lossless);
    }

    #[test]
    fn v2_preserves_explicit_key_indicator_byte_in_flow_context() {
        // The `?` explicit-key indicator carries a 1-byte source span
        // even in flow context, so the v2 builder must NOT drop it
        // (only zero-width `Key` splices from `fetch_value` should be
        // dropped). Regression: an earlier draft filtered every Key.
        let input = "{ ?foo: bar }\n";
        let report = shadow_parser_v2_check(input);
        assert!(report.text_lossless, "input {input:?} not preserved");
    }

    #[test]
    fn v2_does_not_absorb_terminator_line_break_into_flow_scalar() {
        // Regression: in flow context the multi-line plain
        // continuation must abort if the next non-blank char is a
        // flow terminator (`}`/`]`/`,`). Otherwise the trailing
        // newline got swallowed into the scalar (`42\n` instead of
        // `42`) and the closer's byte position drifted.
        let input = "{a: 42\n}\n";
        let report = shadow_parser_v2_check(input);
        assert!(report.text_lossless, "input {input:?} not preserved");
    }

    fn document_count(tree: &SyntaxNode) -> usize {
        tree.children()
            .filter(|n| n.kind() == SyntaxKind::YAML_DOCUMENT)
            .count()
    }

    #[test]
    fn implicit_document_wraps_body_with_no_markers() {
        // No explicit `---` or `...` — the body still belongs to one
        // YAML_DOCUMENT so projection has a node to walk.
        let input = "key: value\n";
        let tree = parse_v2(input);
        assert_eq!(document_count(&tree), 1);
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn explicit_doc_start_opens_document_marker_lives_inside() {
        let input = "---\nkey: value\n";
        let tree = parse_v2(input);
        assert_eq!(document_count(&tree), 1);
        let doc = tree
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_DOCUMENT)
            .expect("document node");
        assert!(
            doc.children_with_tokens().any(|el| el
                .as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_DOCUMENT_START)),
            "`---` token should live inside YAML_DOCUMENT"
        );
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn explicit_doc_end_closes_document_marker_lives_inside() {
        let input = "key: value\n...\n";
        let tree = parse_v2(input);
        assert_eq!(document_count(&tree), 1);
        let doc = tree
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_DOCUMENT)
            .expect("document node");
        assert!(
            doc.children_with_tokens().any(|el| el
                .as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_DOCUMENT_END)),
            "`...` token should live inside YAML_DOCUMENT"
        );
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn consecutive_doc_starts_emit_two_documents() {
        let input = "---\na\n---\nb\n";
        let tree = parse_v2(input);
        assert_eq!(document_count(&tree), 2);
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn pre_document_trivia_stays_at_stream_level() {
        // A leading newline before the first document content should
        // sit under YAML_STREAM, not inside a YAML_DOCUMENT — there is
        // no document yet at that point.
        let input = "\n---\nkey: value\n";
        let tree = parse_v2(input);
        let stream_token_kinds: Vec<SyntaxKind> = tree
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .map(|t| t.kind())
            .collect();
        assert!(
            stream_token_kinds.contains(&SyntaxKind::NEWLINE),
            "leading newline should be a direct child of YAML_STREAM, got {stream_token_kinds:?}"
        );
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn bare_doc_end_at_stream_start_opens_synthetic_empty_document() {
        // Pathological but lossless: a stream that begins with `...`
        // wraps the marker in an empty YAML_DOCUMENT so no source
        // bytes leak out at YAML_STREAM level uncoupled from a doc.
        let input = "...\n";
        let tree = parse_v2(input);
        assert_eq!(document_count(&tree), 1);
        assert_eq!(tree.text().to_string(), input);
    }
}
