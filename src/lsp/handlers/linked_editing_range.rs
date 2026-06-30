//! `textDocument/linkedEditingRange` handler.
//!
//! Returns the set of same-document spans that should be edited together when
//! the cursor sits on a renameable symbol: a reference label and its
//! definition, a citation key used several times, a heading id and the links
//! pointing at it, and so on. This is the live, type-to-rename sibling of
//! [`super::rename`] and [`super::references`], scoped to a single document (as
//! the protocol requires) and returning ranges instead of edits/locations.
//!
//! The LSP protocol requires every returned range to contain identical text
//! content. Our symbol matching is normalized (case-folding + whitespace
//! collapse), so two spans can share a normalized label yet differ in source
//! text (`[Foo]` vs `[foo]:`). We therefore gather candidate spans by
//! normalized label, then keep only those whose source text matches the span
//! under the cursor — the single filter that makes every symbol kind
//! protocol-correct.

use lsp_types::{LinkedEditingRangeParams, LinkedEditingRanges, Range};
use rowan::{TextRange, TextSize};

use crate::lsp::global_state::StateSnapshot;
use crate::lsp::symbols::{collect_symbol_ranges, resolve_symbol_target_at_offset};

use super::super::conversions::{offset_to_position, position_to_offset};
use super::super::helpers;

pub(crate) fn linked_editing_range(
    snap: &StateSnapshot,
    params: LinkedEditingRangeParams,
) -> Option<LinkedEditingRanges> {
    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;
    let config = snap.config(&uri);

    let ctx = crate::lsp::context::get_open_document_context(snap, &uri)?;
    let content = ctx.content.clone();
    let parsed_yaml_regions = snap.parsed_yaml_regions(&uri);

    let offset = position_to_offset(&content, position)?;
    if helpers::is_offset_in_yaml_frontmatter(parsed_yaml_regions, offset) {
        return None;
    }

    let root = ctx.syntax_root();
    let target = resolve_symbol_target_at_offset(&root, offset)?;

    let mut ranges = collect_symbol_ranges(snap, &ctx, &config, &root, &target);

    // The cursor's own span is the identical-text key. Anchoring on the
    // collected span that covers the cursor (rather than a separate
    // symbol-text lookup) keeps the handler self-consistent and covers
    // definition spans the generic symbol-text helper does not.
    let offset_ts = TextSize::from(offset as u32);
    let anchor = ranges
        .iter()
        .copied()
        .find(|r| r.contains_inclusive(offset_ts))?;
    let anchor_text = span_text(&content, anchor)?;

    ranges.retain(|r| span_text(&content, *r) == Some(anchor_text));
    ranges.sort_by_key(|r| r.start());
    ranges.dedup();

    // A single span has no linked partner — let the client fall back to plain
    // editing rather than advertising a degenerate linked-editing session.
    if ranges.len() < 2 {
        return None;
    }

    let lsp_ranges = ranges
        .into_iter()
        .map(|r| Range {
            start: offset_to_position(&content, r.start().into()),
            end: offset_to_position(&content, r.end().into()),
        })
        .collect();

    Some(LinkedEditingRanges {
        ranges: lsp_ranges,
        word_pattern: None,
    })
}

fn span_text(content: &str, range: TextRange) -> Option<&str> {
    content.get(usize::from(range.start())..usize::from(range.end()))
}
