//! `textDocument/documentHighlight` handler.
//!
//! Highlights every occurrence of the symbol under the cursor within the
//! current document: a reference label and its definition, a citation key used
//! several times, a footnote and its definition, a heading id and the links
//! pointing at it, and so on. This is the read-only, visual sibling of
//! [`super::linked_editing_range`] and [`super::references`] — it reuses the
//! same symbol resolution and occurrence collection, but returns the full set
//! of spans as highlights rather than synchronized edit ranges or cross-file
//! locations.
//!
//! Unlike linked editing, document highlight does not require the spans to share
//! identical source text (highlighting `[Foo]` together with `[foo]:` is
//! desirable), and a lone occurrence is still a valid highlight. Every span is
//! reported as [`DocumentHighlightKind::TEXT`]: the collected ranges do not
//! cleanly separate definitions from usages, so the neutral kind is the
//! conventional choice.

use lsp_types::{DocumentHighlight, DocumentHighlightKind, DocumentHighlightParams, Range};

use crate::lsp::global_state::StateSnapshot;
use crate::lsp::symbols::{collect_symbol_ranges, resolve_symbol_target_at_offset};

use super::super::conversions::{offset_to_position, position_to_offset};
use super::super::helpers;

pub(crate) fn document_highlight(
    snap: &StateSnapshot,
    params: DocumentHighlightParams,
) -> Option<Vec<DocumentHighlight>> {
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
    ranges.sort_by_key(|r| r.start());
    ranges.dedup();

    if ranges.is_empty() {
        return None;
    }

    let highlights = ranges
        .into_iter()
        .map(|r| DocumentHighlight {
            range: Range {
                start: offset_to_position(&content, r.start().into()),
                end: offset_to_position(&content, r.end().into()),
            },
            kind: Some(DocumentHighlightKind::TEXT),
        })
        .collect();

    Some(highlights)
}
