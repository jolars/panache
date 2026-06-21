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

use crate::lsp::context::OpenDocumentContext;
use crate::lsp::global_state::StateSnapshot;
use crate::lsp::symbols::{SymbolTarget, resolve_symbol_target_at_offset};
use crate::syntax::{AstNode, ImageLink, Link, ReferenceDefinition, SyntaxNode};
use crate::utils::{normalize_anchor_label, normalize_label};

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

    let mut ranges = collect_ranges(snap, &ctx, &config, &root, &target);

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

/// Gather candidate value-spans for `target` from the current document only.
///
/// Everything except link references routes through the per-document salsa
/// [`SymbolUsageIndex`](crate::salsa::SymbolUsageIndex) accessors that
/// `rename`/`references` already use; link references are walked from the CST
/// because the index tracks only their definitions (as full-node ranges) and
/// none of their usages.
fn collect_ranges(
    snap: &StateSnapshot,
    ctx: &OpenDocumentContext,
    config: &crate::Config,
    root: &SyntaxNode,
    target: &SymbolTarget,
) -> Vec<TextRange> {
    let index = {
        let db = snap.db();
        crate::salsa::symbol_usage_index(db, ctx.salsa_file, ctx.salsa_config).clone()
    };

    let mut ranges: Vec<TextRange> = Vec::new();
    match target {
        SymbolTarget::Citation(key) => {
            if let Some(rs) = index.citation_usages(key) {
                ranges.extend(rs.iter().copied());
            }
        }
        SymbolTarget::Crossref(label) | SymbolTarget::ChunkLabel(label) => {
            let candidates = crate::utils::crossref_symbol_labels(
                &normalize_anchor_label(label),
                config.extensions.bookdown_references,
            );
            for candidate in &candidates {
                if let Some(rs) = index.crossref_usages(candidate) {
                    ranges.extend(rs.iter().copied());
                }
                if let Some(rs) = index.crossref_declaration_value_ranges(candidate) {
                    ranges.extend(rs.iter().copied());
                }
                if let Some(rs) = index.chunk_label_value_ranges(candidate) {
                    ranges.extend(rs.iter().copied());
                }
            }
        }
        SymbolTarget::ExampleLabel(label) => {
            if let Some(rs) = index.example_label_usages(label) {
                ranges.extend(rs.iter().copied());
            }
            if let Some(rs) = index.example_label_definitions(label) {
                ranges.extend(rs.iter().copied());
            }
        }
        SymbolTarget::HeadingId(label) | SymbolTarget::HeadingLink(label) => {
            ranges.extend(index.heading_rename_ranges(label));
        }
        SymbolTarget::Reference {
            label,
            is_footnote: true,
        } => {
            ranges.extend(index.footnote_rename_ranges(label));
        }
        SymbolTarget::Reference {
            label,
            is_footnote: false,
        } => {
            ranges.extend(collect_reference_link_ranges(root, label));
        }
    }
    ranges
}

/// Collect the label value-spans for a link reference: the definition
/// (`[label]: url`) plus full-form usages (`[text][label]`, `![alt][label]`).
/// Shortcut (`[label]`) and collapsed (`[label][]`) forms are classified as
/// implicit heading links by [`resolve_symbol_target_at_offset`] and handled
/// through the heading branch instead.
fn collect_reference_link_ranges(root: &SyntaxNode, label: &str) -> Vec<TextRange> {
    let norm = normalize_label(label);
    if norm.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for node in root.descendants() {
        if let Some(def) = ReferenceDefinition::cast(node.clone()) {
            if normalize_label(&def.label()) == norm
                && let Some(range) = def.label_value_range()
            {
                out.push(range);
            }
        } else if let Some(link) = Link::cast(node.clone()) {
            if let Some(reference) = link.reference()
                && normalize_label(&reference.label()) == norm
                && let Some(range) = reference.label_value_range()
            {
                out.push(range);
            }
        } else if let Some(image) = ImageLink::cast(node.clone())
            && let Some(reference) = image.reference()
            && normalize_label(&reference.label()) == norm
            && let Some(range) = reference.label_value_range()
        {
            out.push(range);
        }
    }
    out
}

fn span_text(content: &str, range: TextRange) -> Option<&str> {
    content.get(usize::from(range.start())..usize::from(range.end()))
}
