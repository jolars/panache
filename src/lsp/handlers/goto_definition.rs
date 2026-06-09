//! Handler for textDocument/definition LSP requests.
//!
//! Provides "go to definition" functionality for:
//! - Reference links: `[text][ref]` → `[ref]: url`
//! - Reference images: `![alt][ref]` → `[ref]: url`
//! - Footnote references: `[^id]` → `[^id]: content`

use crate::lsp::uri_ext::UriExt;
use lsp_types::*;

use crate::lsp::global_state::StateSnapshot;
use crate::lsp::symbols::{SymbolTarget, resolve_symbol_target_at_offset};
use crate::syntax::{AstNode, Link};

use super::super::{conversions, helpers};

/// Handle textDocument/definition request
pub(crate) fn goto_definition(
    snap: &StateSnapshot,
    params: GotoDefinitionParams,
) -> Option<GotoDefinitionResponse> {
    let uri = &params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;
    let config = snap.config(uri);

    let ctx = crate::lsp::context::get_open_document_context(snap, uri)?;
    let salsa_file = ctx.salsa_file;
    let salsa_config = ctx.salsa_config;
    let doc_path = ctx.path.clone();
    let parsed_yaml_regions = ctx.parsed_yaml_regions.clone();

    let citation_def_index = if let Some(doc_path) = doc_path.clone() {
        let yaml_ok = helpers::is_yaml_frontmatter_valid(&parsed_yaml_regions);
        if yaml_ok {
            Some(
                crate::salsa::citation_definition_index(
                    snap.db(),
                    salsa_file,
                    salsa_config,
                    doc_path,
                )
                .clone(),
            )
        } else {
            None
        }
    } else {
        None
    };

    let this_path = ctx.path.clone();
    let content_for_offset = ctx.content.clone();
    let offset = conversions::position_to_offset(&content_for_offset, position)?;
    if helpers::is_offset_in_yaml_frontmatter(&parsed_yaml_regions, offset) {
        return None;
    }

    enum PendingDefinition {
        Citation(String),
        Crossref(String),
        ChunkLabel(String),
        ExampleLabel(String),
        HeadingId(String),
        HeadingLink(String),
        Reference { label: String, is_footnote: bool },
    }

    let (content, pending, heading_link_is_explicit_anchor) = {
        let content = ctx.content.clone();
        let root = ctx.syntax_root();

        let pending = match resolve_symbol_target_at_offset(&root, offset) {
            Some(SymbolTarget::Citation(key)) => Some(PendingDefinition::Citation(key)),
            Some(SymbolTarget::Crossref(label)) => Some(PendingDefinition::Crossref(label)),
            Some(SymbolTarget::ChunkLabel(label)) => Some(PendingDefinition::ChunkLabel(label)),
            Some(SymbolTarget::ExampleLabel(label)) => config
                .extensions
                .example_lists
                .then_some(PendingDefinition::ExampleLabel(label)),
            Some(SymbolTarget::HeadingId(label)) => Some(PendingDefinition::HeadingId(label)),
            Some(SymbolTarget::HeadingLink(label)) => Some(PendingDefinition::HeadingLink(label)),
            Some(SymbolTarget::Reference { label, is_footnote }) => {
                Some(PendingDefinition::Reference { label, is_footnote })
            }
            None => None,
        };

        let heading_link_is_explicit_anchor =
            matches!(pending, Some(PendingDefinition::HeadingLink(_)))
                && is_explicit_heading_anchor_at_offset(&root, offset);

        (content, pending, heading_link_is_explicit_anchor)
    };

    let pending = pending?;

    // Cross-document lookup.
    let doc_indices = {
        let doc_path = doc_path.clone()?;
        crate::lsp::navigation::project_symbol_documents(
            snap.db(),
            salsa_file,
            salsa_config,
            &doc_path,
            uri,
            &content,
        )
    };
    let definition_index = snap.definition_index_with_includes(uri);

    if let PendingDefinition::HeadingId(label) = &pending {
        for doc in &doc_indices {
            if let Some(ranges) = doc.symbol_index.heading_explicit_definition_ranges(label)
                && let Some(range) = ranges.first()
            {
                let location =
                    crate::lsp::navigation::location_from_range(&doc.uri, &doc.text, *range);
                return Some(GotoDefinitionResponse::Scalar(location));
            }
        }
    }

    if let PendingDefinition::HeadingLink(label) = &pending {
        if heading_link_is_explicit_anchor {
            for doc in &doc_indices {
                if let Some(ranges) = doc.symbol_index.heading_explicit_definition_ranges(label)
                    && let Some(range) = ranges.first()
                {
                    let location =
                        crate::lsp::navigation::location_from_range(&doc.uri, &doc.text, *range);
                    return Some(GotoDefinitionResponse::Scalar(location));
                }
            }
        }

        if config.extensions.implicit_header_references && config.extensions.auto_identifiers {
            if heading_link_is_explicit_anchor {
                for doc in &doc_indices {
                    if let Some(ranges) = doc.symbol_index.heading_implicit_definition_ranges(label)
                        && let Some(range) = ranges.first()
                    {
                        let location = crate::lsp::navigation::location_from_range(
                            &doc.uri, &doc.text, *range,
                        );
                        return Some(GotoDefinitionResponse::Scalar(location));
                    }
                }
            }

            for doc in &doc_indices {
                if let Some(ranges) = doc.symbol_index.heading_label_ranges(label)
                    && let Some(range) = ranges.first()
                {
                    let location =
                        crate::lsp::navigation::location_from_range(&doc.uri, &doc.text, *range);
                    return Some(GotoDefinitionResponse::Scalar(location));
                }
            }
        }
    }

    if let PendingDefinition::Crossref(label) = &pending {
        for doc in &doc_indices {
            for candidate in
                crate::utils::crossref_symbol_labels(label, config.extensions.bookdown_references)
            {
                if let Some(ranges) = doc.symbol_index.crossref_declarations(&candidate)
                    && let Some(range) = ranges.first()
                {
                    let location =
                        crate::lsp::navigation::location_from_range(&doc.uri, &doc.text, *range);
                    return Some(GotoDefinitionResponse::Scalar(location));
                }

                if config.extensions.implicit_header_references
                    && config.extensions.auto_identifiers
                    && let Some(ranges) = doc
                        .symbol_index
                        .heading_implicit_definition_ranges(&candidate)
                    && let Some(range) = ranges.first()
                {
                    let location =
                        crate::lsp::navigation::location_from_range(&doc.uri, &doc.text, *range);
                    return Some(GotoDefinitionResponse::Scalar(location));
                }
            }
        }
    }

    if let PendingDefinition::ChunkLabel(label) = &pending {
        for doc in &doc_indices {
            for candidate in
                crate::utils::crossref_symbol_labels(label, config.extensions.bookdown_references)
            {
                if let Some(ranges) = doc.symbol_index.chunk_label_value_ranges(&candidate)
                    && let Some(range) = ranges.first()
                {
                    let location =
                        crate::lsp::navigation::location_from_range(&doc.uri, &doc.text, *range);
                    return Some(GotoDefinitionResponse::Scalar(location));
                }
            }
        }
    }

    if let PendingDefinition::ExampleLabel(label) = &pending {
        for doc in &doc_indices {
            if let Some(ranges) = doc.symbol_index.example_label_definitions(label)
                && let Some(range) = ranges.first()
            {
                let location =
                    crate::lsp::navigation::location_from_range(&doc.uri, &doc.text, *range);
                return Some(GotoDefinitionResponse::Scalar(location));
            }
        }
    }

    if let PendingDefinition::Reference { label, is_footnote } = &pending {
        for doc in &doc_indices {
            let ranges = if *is_footnote {
                doc.symbol_index.footnote_definitions(label)
            } else {
                doc.symbol_index.reference_definitions(label)
            };
            if let Some(ranges) = ranges
                && let Some(range) = ranges.first()
            {
                let location =
                    crate::lsp::navigation::location_from_range(&doc.uri, &doc.text, *range);
                return Some(GotoDefinitionResponse::Scalar(location));
            }
        }
    }

    let definition =
        match pending {
            PendingDefinition::Citation(key) => {
                let index = citation_def_index.as_ref()?;
                let mut locations =
                    helpers::citation_definition_locations(index, &key, uri, &content, snap.db());
                if locations.is_empty() {
                    return None;
                }
                return Some(GotoDefinitionResponse::Scalar(locations.remove(0)));
            }
            PendingDefinition::Crossref(label) => definition_index
                .find_crossref_resolved(&label, config.extensions.bookdown_references),
            PendingDefinition::ChunkLabel(label) => definition_index
                .find_crossref_resolved(&label, config.extensions.bookdown_references),
            PendingDefinition::ExampleLabel(label) => definition_index.find_example_label(&label),
            PendingDefinition::HeadingId(label) => definition_index
                .find_crossref_resolved(&label, config.extensions.bookdown_references),
            PendingDefinition::HeadingLink(label) => {
                if heading_link_is_explicit_anchor {
                    definition_index
                        .find_crossref_resolved(&label, config.extensions.bookdown_references)
                } else {
                    None
                }
            }
            PendingDefinition::Reference { label, is_footnote } => {
                if is_footnote {
                    definition_index.find_footnote(&label)
                } else {
                    definition_index.find_reference(&label)
                }
            }
        };

    let definition = definition?;

    let target_uri = Uri::from_file_path(definition.path()).unwrap_or_else(|| uri.clone());
    let target_text = if Some(definition.path().to_path_buf()) == this_path {
        content
    } else {
        crate::salsa::Db::file_text(snap.db(), definition.path().to_path_buf())
            .map(|file| file.text(snap.db()).clone())
            .unwrap_or_default()
    };
    let start = conversions::offset_to_position(&target_text, definition.range().start().into());
    let end = conversions::offset_to_position(&target_text, definition.range().end().into());
    let location = Location {
        uri: target_uri,
        range: Range { start, end },
    };
    Some(GotoDefinitionResponse::Scalar(location))
}

fn is_explicit_heading_anchor_at_offset(root: &crate::syntax::SyntaxNode, offset: usize) -> bool {
    let Some(mut node) = helpers::find_node_at_offset(root, offset) else {
        return false;
    };

    loop {
        if let Some(link) = Link::cast(node.clone()) {
            return link
                .dest()
                .and_then(|dest| dest.hash_anchor_id_range())
                .is_some();
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        node = parent;
    }
}
