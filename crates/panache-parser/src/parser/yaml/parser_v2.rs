//! Step-11 parser scaffold — a CST builder that consumes the streaming
//! scanner. Wraps each contiguous run of body content in a
//! `YAML_DOCUMENT` node (with `---` / `...` markers consumed inside the
//! document they delimit), nests block-context content under
//! `YAML_BLOCK_MAP` / `YAML_BLOCK_SEQUENCE` containers driven by the
//! scanner's synthetic `BlockMappingStart` / `BlockSequenceStart` /
//! `BlockEnd` markers, wraps each key-value pair in
//! `YAML_BLOCK_MAP_ENTRY` / each `-` entry in
//! `YAML_BLOCK_SEQUENCE_ITEM`, splits each map entry into
//! `YAML_BLOCK_MAP_KEY` (everything up to and including the `:`) and
//! `YAML_BLOCK_MAP_VALUE` (everything after), and mirrors the same
//! shape for flow contexts: `YAML_FLOW_MAP` / `YAML_FLOW_MAP_ENTRY` /
//! `YAML_FLOW_MAP_KEY` / `YAML_FLOW_MAP_VALUE` and
//! `YAML_FLOW_SEQUENCE` / `YAML_FLOW_SEQUENCE_ITEM`. Source-backed
//! `[` / `]` / `{` / `}` / `,` are emitted at the container level
//! (matching v1's emission), with item/entry sub-wrappers closing on
//! `,` and the matching closer.
//!
//! Per-feature event-parity work (matching each fixture's `test.event`
//! exactly) lands incrementally on top of this shape.

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
    // Stack of currently-open block containers. Each frame tracks
    // whether its current `YAML_BLOCK_MAP_ENTRY` / `YAML_BLOCK_SEQUENCE_ITEM`
    // sub-wrapper is still open and waiting to be closed (by the next
    // `Key` / `BlockEntry` peer or by `BlockEnd`).
    let mut block_stack: Vec<BlockFrame> = Vec::new();
    while let Some(tok) = scanner.next_token() {
        match tok.kind {
            TokenKind::StreamStart | TokenKind::StreamEnd => continue,
            TokenKind::BlockMappingStart => {
                ensure_doc_open(&mut builder, &mut doc_open);
                ensure_flow_seq_item_open(&mut builder, &mut block_stack);
                builder.start_node(SyntaxKind::YAML_BLOCK_MAP.into());
                block_stack.push(BlockFrame::BlockMap {
                    entry_open: false,
                    in_value: false,
                });
                continue;
            }
            TokenKind::BlockSequenceStart => {
                ensure_doc_open(&mut builder, &mut doc_open);
                ensure_flow_seq_item_open(&mut builder, &mut block_stack);
                builder.start_node(SyntaxKind::YAML_BLOCK_SEQUENCE.into());
                block_stack.push(BlockFrame::BlockSequence { item_open: false });
                continue;
            }
            TokenKind::BlockEnd => {
                close_open_sub_wrapper(&mut builder, &mut block_stack);
                // Defensive: only close if the scanner gave us an open
                // container. A stray BlockEnd would otherwise pop the
                // YAML_DOCUMENT or YAML_STREAM frame.
                if block_stack.pop().is_some() {
                    builder.finish_node();
                }
                continue;
            }
            TokenKind::FlowSequenceStart => {
                ensure_doc_open(&mut builder, &mut doc_open);
                ensure_flow_seq_item_open(&mut builder, &mut block_stack);
                // If nested inside a Map's open KEY/VALUE wrapper, the
                // current open scope is the appropriate parent.
                builder.start_node(SyntaxKind::YAML_FLOW_SEQUENCE.into());
                block_stack.push(BlockFrame::FlowSequence { item_open: false });
                let text = &input[tok.start.index..tok.end.index];
                builder.token(SyntaxKind::YAML_SCALAR.into(), text);
                continue;
            }
            TokenKind::FlowSequenceEnd => {
                close_open_sub_wrapper(&mut builder, &mut block_stack);
                let text = &input[tok.start.index..tok.end.index];
                builder.token(SyntaxKind::YAML_SCALAR.into(), text);
                if matches!(
                    block_stack.last(),
                    Some(BlockFrame::FlowSequence { .. } | BlockFrame::FlowMap { .. })
                ) {
                    block_stack.pop();
                    builder.finish_node();
                }
                continue;
            }
            TokenKind::FlowMappingStart => {
                ensure_doc_open(&mut builder, &mut doc_open);
                ensure_flow_seq_item_open(&mut builder, &mut block_stack);
                builder.start_node(SyntaxKind::YAML_FLOW_MAP.into());
                block_stack.push(BlockFrame::FlowMap {
                    entry_open: false,
                    in_value: false,
                });
                let text = &input[tok.start.index..tok.end.index];
                builder.token(SyntaxKind::YAML_SCALAR.into(), text);
                continue;
            }
            TokenKind::FlowMappingEnd => {
                close_open_sub_wrapper(&mut builder, &mut block_stack);
                let text = &input[tok.start.index..tok.end.index];
                builder.token(SyntaxKind::YAML_SCALAR.into(), text);
                if matches!(
                    block_stack.last(),
                    Some(BlockFrame::FlowMap { .. } | BlockFrame::FlowSequence { .. })
                ) {
                    block_stack.pop();
                    builder.finish_node();
                }
                continue;
            }
            TokenKind::FlowEntry => {
                // `,` closes the current entry/item and lives at the
                // container level (between peer entries/items).
                close_open_sub_wrapper(&mut builder, &mut block_stack);
                let text = &input[tok.start.index..tok.end.index];
                builder.token(SyntaxKind::YAML_SCALAR.into(), text);
                continue;
            }
            TokenKind::Key => {
                // Both the synthetic 0-width splice and the source-backed
                // `?` indicator open a new map entry. Close the previous
                // entry first if still open. After this, the current
                // open scope is the new key wrapper.
                if matches!(
                    block_stack.last(),
                    Some(BlockFrame::BlockMap { .. } | BlockFrame::FlowMap { .. })
                ) {
                    open_map_entry_with_key(&mut builder, &mut block_stack);
                }
                if tok.start.index == tok.end.index {
                    // Synthetic Key splice carries no bytes.
                    continue;
                }
                // Source-backed `?`: ensure we have somewhere to put it.
                ensure_flow_seq_item_open(&mut builder, &mut block_stack);
                // Fall through to emit `?` inside the open KEY (or
                // current scope if not in a Map frame).
            }
            TokenKind::Value => {
                let map_state = match block_stack.last().copied() {
                    Some(BlockFrame::BlockMap {
                        entry_open,
                        in_value,
                    }) => Some((false, entry_open, in_value)),
                    Some(BlockFrame::FlowMap {
                        entry_open,
                        in_value,
                    }) => Some((true, entry_open, in_value)),
                    _ => None,
                };
                if let Some((is_flow, entry_open, in_value)) = map_state {
                    // Empty-key shorthand: `:` arriving without a prior
                    // Key opens an ENTRY+KEY before consuming the colon.
                    if !entry_open {
                        open_map_entry_with_key(&mut builder, &mut block_stack);
                    }
                    if !in_value {
                        // The colon is the last token of KEY. After it
                        // we close KEY and open VALUE.
                        let text = &input[tok.start.index..tok.end.index];
                        if !text.is_empty() {
                            builder.token(SyntaxKind::YAML_COLON.into(), text);
                        }
                        builder.finish_node(); // close KEY
                        let value_kind = if is_flow {
                            SyntaxKind::YAML_FLOW_MAP_VALUE
                        } else {
                            SyntaxKind::YAML_BLOCK_MAP_VALUE
                        };
                        builder.start_node(value_kind.into());
                        if let Some(
                            BlockFrame::BlockMap { in_value, .. }
                            | BlockFrame::FlowMap { in_value, .. },
                        ) = block_stack.last_mut()
                        {
                            *in_value = true;
                        }
                        continue;
                    }
                    // Already in_value: pathological double-colon. Fall
                    // through and emit at the current scope (inside
                    // VALUE) for losslessness.
                }
                // Not a Map frame: ensure flow-seq ITEM is open, then
                // fall through to emit `:` at current scope.
                ensure_flow_seq_item_open(&mut builder, &mut block_stack);
            }
            TokenKind::BlockEntry => {
                if matches!(block_stack.last(), Some(BlockFrame::BlockSequence { .. })) {
                    close_open_sub_wrapper(&mut builder, &mut block_stack);
                    builder.start_node(SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM.into());
                    if let Some(BlockFrame::BlockSequence { item_open }) = block_stack.last_mut() {
                        *item_open = true;
                    }
                }
                // Fall through to emit the `-` byte inside the new ITEM
                // (or at current scope if not in a Sequence frame).
            }
            TokenKind::Trivia(_) => {
                // Trivia bypasses item-opening: pre-content trivia in a
                // flow sequence stays at SEQUENCE level (matching v1's
                // emission shape).
            }
            _ => {
                // Any other source-backed content (Scalar, Anchor, Tag,
                // Alias, Directive, doc markers): if we're inside a
                // FlowSequence with no open ITEM, open one before
                // emitting. Doc markers are handled below.
                if !matches!(tok.kind, TokenKind::DocumentStart | TokenKind::DocumentEnd) {
                    ensure_flow_seq_item_open(&mut builder, &mut block_stack);
                }
            }
        }
        let text = &input[tok.start.index..tok.end.index];
        if text.is_empty() {
            // Defensive: never emit zero-width tokens (rowan rejects).
            continue;
        }
        let kind = map_token_to_syntax_kind(tok.kind);
        match tok.kind {
            TokenKind::DocumentStart => {
                // `---` always begins a fresh document. Close any
                // still-open block containers (the scanner unwinds the
                // indent stack at column 0, but a same-indent map at
                // indent==0 leaves them open) and the previous doc
                // before opening the new one.
                close_block_containers(&mut builder, &mut block_stack);
                if doc_open {
                    builder.finish_node();
                }
                builder.start_node(SyntaxKind::YAML_DOCUMENT.into());
                doc_open = true;
                builder.token(kind.into(), text);
            }
            TokenKind::DocumentEnd => {
                // `...` closes the current document. Close any open
                // block containers first so the marker is a child of
                // the document, not buried in a block container.
                close_block_containers(&mut builder, &mut block_stack);
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
                // trivia stays inside the YAML_DOCUMENT, the open
                // block container, or the open ENTRY/ITEM sub-wrapper.
                builder.token(kind.into(), text);
            }
            _ => {
                // Any non-trivia content opens an implicit document
                // when one isn't already in progress.
                ensure_doc_open(&mut builder, &mut doc_open);
                builder.token(kind.into(), text);
            }
        }
    }
    // Close any open block containers (and their open ENTRY/ITEM
    // sub-wrappers) and the open document. The scanner emits BlockEnd
    // on stream end via `unwind_indent(-1)`, so this is normally a
    // no-op for `block_stack`; kept for safety against truncated
    // inputs and future scanner quirks.
    close_block_containers(&mut builder, &mut block_stack);
    if doc_open {
        builder.finish_node();
    }
    builder.finish_node();
    SyntaxNode::new_root(builder.finish())
}

/// Tracks an open container in the v2 builder's stack. Block and
/// flow contexts share state shape, but their containers and
/// sub-wrappers use different `SyntaxKind` variants and they close on
/// different tokens (`BlockEnd` / dedent vs. `]` / `}` / `,`).
///
/// For maps, `entry_open` records whether the entry sub-wrapper is
/// still open, and `in_value` selects between the KEY and VALUE
/// sub-sub-wrapper. For sequences, `item_open` records whether the
/// item sub-wrapper is still open.
#[derive(Debug, Clone, Copy)]
enum BlockFrame {
    BlockMap { entry_open: bool, in_value: bool },
    BlockSequence { item_open: bool },
    FlowMap { entry_open: bool, in_value: bool },
    FlowSequence { item_open: bool },
}

fn ensure_doc_open(builder: &mut GreenNodeBuilder<'_>, doc_open: &mut bool) {
    if !*doc_open {
        builder.start_node(SyntaxKind::YAML_DOCUMENT.into());
        *doc_open = true;
    }
}

/// In a flow sequence, source-backed content opens a new
/// `YAML_FLOW_SEQUENCE_ITEM` lazily — there is no `-` token to drive
/// the boundary the way `BlockEntry` drives block sequences. Trivia
/// arriving before the first item stays at the container level.
fn ensure_flow_seq_item_open(builder: &mut GreenNodeBuilder<'_>, stack: &mut [BlockFrame]) {
    if let Some(BlockFrame::FlowSequence { item_open }) = stack.last_mut()
        && !*item_open
    {
        builder.start_node(SyntaxKind::YAML_FLOW_SEQUENCE_ITEM.into());
        *item_open = true;
    }
}

/// Open `<MAP>_ENTRY` > `<MAP>_KEY` for the next entry, closing any
/// previously-open entry on the same Map frame. Caller must have
/// verified the top frame is a Map (Block or Flow).
fn open_map_entry_with_key(builder: &mut GreenNodeBuilder<'_>, stack: &mut [BlockFrame]) {
    close_open_sub_wrapper(builder, stack);
    let (entry_kind, key_kind) = match stack.last() {
        Some(BlockFrame::BlockMap { .. }) => (
            SyntaxKind::YAML_BLOCK_MAP_ENTRY,
            SyntaxKind::YAML_BLOCK_MAP_KEY,
        ),
        Some(BlockFrame::FlowMap { .. }) => (
            SyntaxKind::YAML_FLOW_MAP_ENTRY,
            SyntaxKind::YAML_FLOW_MAP_KEY,
        ),
        _ => return,
    };
    builder.start_node(entry_kind.into());
    builder.start_node(key_kind.into());
    if let Some(
        BlockFrame::BlockMap {
            entry_open,
            in_value,
        }
        | BlockFrame::FlowMap {
            entry_open,
            in_value,
        },
    ) = stack.last_mut()
    {
        *entry_open = true;
        *in_value = false;
    }
}

/// Close the top-of-stack frame's entry/item sub-wrapper if still open
/// and clear the flag. For maps, this closes the inner KEY/VALUE
/// node and the surrounding ENTRY (two `finish_node` calls). For
/// sequences it closes the ITEM. Caller decides whether to also pop
/// the frame itself.
fn close_open_sub_wrapper(builder: &mut GreenNodeBuilder<'_>, stack: &mut [BlockFrame]) {
    let Some(frame) = stack.last_mut() else {
        return;
    };
    match frame {
        BlockFrame::BlockMap {
            entry_open: true, ..
        } => {
            builder.finish_node(); // close KEY or VALUE
            builder.finish_node(); // close ENTRY
            *frame = BlockFrame::BlockMap {
                entry_open: false,
                in_value: false,
            };
        }
        BlockFrame::FlowMap {
            entry_open: true, ..
        } => {
            builder.finish_node();
            builder.finish_node();
            *frame = BlockFrame::FlowMap {
                entry_open: false,
                in_value: false,
            };
        }
        BlockFrame::BlockSequence { item_open: true } => {
            builder.finish_node();
            *frame = BlockFrame::BlockSequence { item_open: false };
        }
        BlockFrame::FlowSequence { item_open: true } => {
            builder.finish_node();
            *frame = BlockFrame::FlowSequence { item_open: false };
        }
        _ => {}
    }
}

fn close_block_containers(builder: &mut GreenNodeBuilder<'_>, stack: &mut Vec<BlockFrame>) {
    while let Some(frame) = stack.pop() {
        match frame {
            BlockFrame::BlockMap {
                entry_open: true, ..
            }
            | BlockFrame::FlowMap {
                entry_open: true, ..
            } => {
                // Close inner KEY/VALUE then ENTRY before the container.
                builder.finish_node();
                builder.finish_node();
            }
            BlockFrame::BlockSequence { item_open: true }
            | BlockFrame::FlowSequence { item_open: true } => {
                builder.finish_node();
            }
            _ => {}
        }
        builder.finish_node();
    }
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

    fn first_document(tree: &SyntaxNode) -> SyntaxNode {
        tree.children()
            .find(|n| n.kind() == SyntaxKind::YAML_DOCUMENT)
            .expect("at least one document")
    }

    fn block_map_under(parent: &SyntaxNode) -> Option<SyntaxNode> {
        parent
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
    }

    fn block_seq_under(parent: &SyntaxNode) -> Option<SyntaxNode> {
        parent
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE)
    }

    fn block_map_entries(map: &SyntaxNode) -> Vec<SyntaxNode> {
        map.children()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_ENTRY)
            .collect()
    }

    fn block_seq_items(seq: &SyntaxNode) -> Vec<SyntaxNode> {
        seq.children()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM)
            .collect()
    }

    fn entry_key(entry: &SyntaxNode) -> SyntaxNode {
        entry
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_KEY)
            .expect("entry should have a YAML_BLOCK_MAP_KEY child")
    }

    fn entry_value(entry: &SyntaxNode) -> SyntaxNode {
        entry
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE)
            .expect("entry should have a YAML_BLOCK_MAP_VALUE child")
    }

    #[test]
    fn block_mapping_wraps_key_value_with_key_and_value_sub_wrappers() {
        let input = "key: value\n";
        let tree = parse_v2(input);
        let doc = first_document(&tree);
        let map = block_map_under(&doc).expect("YAML_BLOCK_MAP child");
        let entries = block_map_entries(&map);
        assert_eq!(entries.len(), 1, "expected one ENTRY for `key: value`");
        let key = entry_key(&entries[0]);
        let value = entry_value(&entries[0]);
        // Colon ends the KEY (last token); VALUE has the scalar.
        assert!(
            key.children_with_tokens().any(|el| el
                .as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_COLON)),
            "colon should be the trailing token of YAML_BLOCK_MAP_KEY",
        );
        assert!(
            value.children_with_tokens().any(|el| el
                .as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_SCALAR)),
            "scalar `value` should live inside YAML_BLOCK_MAP_VALUE",
        );
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn block_sequence_wraps_entries_in_yaml_block_sequence() {
        let input = "- a\n- b\n";
        let tree = parse_v2(input);
        let doc = first_document(&tree);
        let seq = block_seq_under(&doc).expect("YAML_BLOCK_SEQUENCE child");
        let items = block_seq_items(&seq);
        assert_eq!(items.len(), 2, "expected 2 YAML_BLOCK_SEQUENCE_ITEM");
        // Each item must own its own `-` entry token.
        for item in &items {
            let dash_count = item
                .children_with_tokens()
                .filter(|el| {
                    el.as_token()
                        .is_some_and(|t| t.kind() == SyntaxKind::YAML_BLOCK_SEQ_ENTRY)
                })
                .count();
            assert_eq!(dash_count, 1, "each item owns exactly one `-` token");
        }
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn nested_block_mapping_nests_inner_block_map_inside_outer_value() {
        let input = "outer:\n  inner: x\n";
        let tree = parse_v2(input);
        let doc = first_document(&tree);
        let outer = block_map_under(&doc).expect("outer YAML_BLOCK_MAP");
        let outer_entries = block_map_entries(&outer);
        assert_eq!(outer_entries.len(), 1);
        let outer_value = entry_value(&outer_entries[0]);
        let inner = outer_value
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
            .expect("inner YAML_BLOCK_MAP nested under outer VALUE");
        let inner_entries = block_map_entries(&inner);
        assert_eq!(inner_entries.len(), 1);
        let inner_key = entry_key(&inner_entries[0]);
        assert!(
            inner_key.children_with_tokens().any(|el| el
                .as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_COLON)),
            "inner key should own its colon",
        );
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn block_sequence_inside_mapping_nests_under_outer_map_value() {
        let input = "items:\n  - a\n  - b\n";
        let tree = parse_v2(input);
        let doc = first_document(&tree);
        let map = block_map_under(&doc).expect("YAML_BLOCK_MAP child");
        let entries = block_map_entries(&map);
        assert_eq!(entries.len(), 1, "one entry: `items: <seq>`");
        let value = entry_value(&entries[0]);
        let seq = value
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE)
            .expect("YAML_BLOCK_SEQUENCE nested under map VALUE");
        let items = block_seq_items(&seq);
        assert_eq!(items.len(), 2);
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn dedent_closes_inner_block_map_before_next_outer_key() {
        // outer:
        //   inner: x
        // sibling: y
        // The dedent before `sibling` must close the inner map and
        // its outer ENTRY so `sibling: y` lands as a sibling ENTRY
        // under the outer map.
        let input = "outer:\n  inner: x\nsibling: y\n";
        let tree = parse_v2(input);
        let doc = first_document(&tree);
        let outer = block_map_under(&doc).expect("outer YAML_BLOCK_MAP");
        let entries = block_map_entries(&outer);
        assert_eq!(
            entries.len(),
            2,
            "outer map should have two entries (`outer:` and `sibling:`)",
        );
        // Only the first entry's VALUE has a nested map; the second is flat.
        let first_value = entry_value(&entries[0]);
        let nested_in_first = first_value
            .children()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
            .count();
        assert_eq!(nested_in_first, 1);
        let second_value = entry_value(&entries[1]);
        let nested_in_second = second_value
            .children()
            .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP)
            .count();
        assert_eq!(nested_in_second, 0);
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn block_map_with_two_top_level_entries_emits_two_entry_wrappers() {
        let input = "a: 1\nb: 2\n";
        let tree = parse_v2(input);
        let doc = first_document(&tree);
        let map = block_map_under(&doc).expect("YAML_BLOCK_MAP child");
        assert_eq!(block_map_entries(&map).len(), 2);
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn explicit_key_indicator_question_mark_lives_inside_key() {
        // `? a\n: b\n` — the `?` is a source-backed Key token. It
        // opens the ENTRY and lives inside the resulting KEY node
        // (alongside the scalar `a` and the trailing `:`).
        let input = "? a\n: b\n";
        let tree = parse_v2(input);
        let doc = first_document(&tree);
        let map = block_map_under(&doc).expect("YAML_BLOCK_MAP child");
        let entries = block_map_entries(&map);
        assert_eq!(entries.len(), 1);
        let key = entry_key(&entries[0]);
        let has_question = key.children_with_tokens().any(|el| {
            el.as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_KEY)
        });
        assert!(has_question, "`?` should live inside YAML_BLOCK_MAP_KEY");
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn empty_key_shorthand_opens_entry_with_empty_key() {
        // `: value\n` — bare `:` at column 0 is the empty-implicit-key
        // shorthand. The v2 builder must open ENTRY+KEY before the
        // colon arrives so the colon ends up as the only KEY child.
        let input = ": value\n";
        let tree = parse_v2(input);
        let doc = first_document(&tree);
        let map = block_map_under(&doc).expect("YAML_BLOCK_MAP child");
        let entries = block_map_entries(&map);
        assert_eq!(entries.len(), 1);
        let key = entry_key(&entries[0]);
        // KEY has no scalar; only the colon.
        assert!(
            !key.children_with_tokens().any(|el| el
                .as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_SCALAR)),
            "empty-key shorthand has no scalar in KEY",
        );
        assert!(
            key.children_with_tokens().any(|el| el
                .as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_COLON)),
            "empty-key KEY still owns the `:` token",
        );
        let value = entry_value(&entries[0]);
        assert!(
            value.children_with_tokens().any(|el| el
                .as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_SCALAR)),
            "VALUE owns the `value` scalar",
        );
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn document_end_marker_lives_at_document_level_not_inside_block_map() {
        // `...` must not be buried inside the block map; it is a
        // document-level marker. The v2 builder closes any open block
        // containers before consuming `DocumentEnd`.
        let input = "key: value\n...\n";
        let tree = parse_v2(input);
        let doc = first_document(&tree);
        let has_doc_end = doc.children_with_tokens().any(|el| {
            el.as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::YAML_DOCUMENT_END)
        });
        assert!(
            has_doc_end,
            "DOCUMENT_END should be a direct child of YAML_DOCUMENT"
        );
        assert_eq!(tree.text().to_string(), input);
    }

    fn flow_map_under(parent: &SyntaxNode) -> Option<SyntaxNode> {
        parent
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP)
    }

    fn flow_seq_under(parent: &SyntaxNode) -> Option<SyntaxNode> {
        parent
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE)
    }

    fn flow_map_entries(map: &SyntaxNode) -> Vec<SyntaxNode> {
        map.children()
            .filter(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP_ENTRY)
            .collect()
    }

    fn flow_seq_items(seq: &SyntaxNode) -> Vec<SyntaxNode> {
        seq.children()
            .filter(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE_ITEM)
            .collect()
    }

    #[test]
    fn flow_sequence_wraps_each_item_in_flow_sequence_item() {
        let input = "[a, b, c]\n";
        let tree = parse_v2(input);
        let doc = first_document(&tree);
        let seq = flow_seq_under(&doc).expect("YAML_FLOW_SEQUENCE child");
        let items = flow_seq_items(&seq);
        assert_eq!(items.len(), 3);
        // The opening `[` and closing `]` live at SEQUENCE level
        // (siblings of items), matching v1's emission.
        let bracket_count = seq
            .children_with_tokens()
            .filter(|el| {
                el.as_token().map(|t| t.text()) == Some("[")
                    || el.as_token().map(|t| t.text()) == Some("]")
            })
            .count();
        assert_eq!(bracket_count, 2, "`[` and `]` at SEQUENCE level");
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn flow_mapping_wraps_each_entry_with_key_and_value() {
        let input = "{a: 1, b: 2}\n";
        let tree = parse_v2(input);
        let doc = first_document(&tree);
        let map = flow_map_under(&doc).expect("YAML_FLOW_MAP child");
        let entries = flow_map_entries(&map);
        assert_eq!(entries.len(), 2);
        for entry in &entries {
            let key = entry
                .children()
                .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP_KEY)
                .expect("entry has YAML_FLOW_MAP_KEY");
            assert!(
                key.children_with_tokens().any(|el| el
                    .as_token()
                    .is_some_and(|t| t.kind() == SyntaxKind::YAML_COLON)),
                "flow KEY owns trailing `:`",
            );
            let value = entry
                .children()
                .find(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP_VALUE)
                .expect("entry has YAML_FLOW_MAP_VALUE");
            assert!(
                value.children_with_tokens().any(|el| el
                    .as_token()
                    .is_some_and(|t| t.kind() == SyntaxKind::YAML_SCALAR)),
                "flow VALUE owns its scalar",
            );
        }
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn flow_sequence_inside_flow_sequence_nests_under_outer_item() {
        let input = "[[1, 2], [3, 4]]\n";
        let tree = parse_v2(input);
        let doc = first_document(&tree);
        let outer = flow_seq_under(&doc).expect("outer YAML_FLOW_SEQUENCE");
        let outer_items = flow_seq_items(&outer);
        assert_eq!(outer_items.len(), 2);
        for item in &outer_items {
            assert!(
                item.children()
                    .any(|n| n.kind() == SyntaxKind::YAML_FLOW_SEQUENCE),
                "outer item should contain a nested YAML_FLOW_SEQUENCE",
            );
        }
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn flow_mapping_inside_flow_sequence_nests_under_item() {
        let input = "[{a: 1}, {b: 2}]\n";
        let tree = parse_v2(input);
        let doc = first_document(&tree);
        let seq = flow_seq_under(&doc).expect("YAML_FLOW_SEQUENCE child");
        let items = flow_seq_items(&seq);
        assert_eq!(items.len(), 2);
        for item in &items {
            assert!(
                item.children()
                    .any(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP),
                "each item should contain a nested YAML_FLOW_MAP",
            );
        }
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn flow_mapping_at_block_map_value_nests_under_block_map_value() {
        let input = "key: {a: 1, b: 2}\n";
        let tree = parse_v2(input);
        let doc = first_document(&tree);
        let block_map = block_map_under(&doc).expect("YAML_BLOCK_MAP child");
        let entries = block_map_entries(&block_map);
        assert_eq!(entries.len(), 1);
        let value = entry_value(&entries[0]);
        assert!(
            value
                .children()
                .any(|n| n.kind() == SyntaxKind::YAML_FLOW_MAP),
            "flow map should be nested under outer block map's VALUE",
        );
        assert_eq!(tree.text().to_string(), input);
    }
}
