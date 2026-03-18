//! Handler for textDocument/definition LSP requests.
//!
//! Provides "go to definition" functionality for:
//! - Reference links: `[text][ref]` → `[ref]: url`
//! - Reference images: `![alt][ref]` → `[ref]: url`
//! - Footnote references: `[^id]` → `[^id]: content`

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;
use crate::lsp::symbols::{SymbolTarget, resolve_symbol_target_at_offset};

use super::super::{conversions, helpers};

/// Handle textDocument/definition request
pub(crate) async fn goto_definition(
    client: &tower_lsp_server::Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>> {
    let uri = &params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;
    let config = helpers::get_config(client, &workspace_root, uri).await;

    let (salsa_file, salsa_config, doc_path, parsed_yaml_regions) = {
        let map = document_map.lock().await;
        match map.get(&uri.to_string()) {
            Some(state) => (
                state.salsa_file,
                state.salsa_config,
                state.path.clone(),
                state.parsed_yaml_regions.clone(),
            ),
            None => return Ok(None),
        }
    };

    let citation_def_index = if let Some(doc_path) = doc_path.clone() {
        let yaml_ok = helpers::is_yaml_frontmatter_valid(&parsed_yaml_regions);
        if yaml_ok {
            let db = salsa_db.lock().await;
            Some(
                crate::salsa::citation_definition_index(&*db, salsa_file, salsa_config, doc_path)
                    .clone(),
            )
        } else {
            None
        }
    } else {
        None
    };

    let this_path = {
        let map = document_map.lock().await;
        map.get(&uri.to_string())
            .and_then(|state| state.path.clone())
    };
    let content_for_offset = {
        let db = salsa_db.lock().await;
        salsa_file.text(&*db).clone()
    };
    let Some(offset) = conversions::position_to_offset(&content_for_offset, position) else {
        return Ok(None);
    };
    if helpers::is_offset_in_yaml_frontmatter(&parsed_yaml_regions, offset) {
        return Ok(None);
    }

    enum PendingDefinition {
        Citation(String),
        Crossref(String),
        HeadingLink(String),
        Reference { label: String, is_footnote: bool },
    }

    let (content, pending) = {
        let Some((content, root)) =
            helpers::get_document_content_and_tree(&document_map, &salsa_db, uri).await
        else {
            return Ok(None);
        };

        let pending = match resolve_symbol_target_at_offset(&root, offset) {
            Some(SymbolTarget::Citation(key)) => Some(PendingDefinition::Citation(key)),
            Some(SymbolTarget::Crossref(label)) => Some(PendingDefinition::Crossref(label)),
            Some(SymbolTarget::HeadingId(label)) | Some(SymbolTarget::HeadingLink(label)) => {
                Some(PendingDefinition::HeadingLink(label))
            }
            Some(SymbolTarget::Reference { label, is_footnote }) => {
                Some(PendingDefinition::Reference { label, is_footnote })
            }
            None => None,
        };

        (content, pending)
    };

    let Some(pending) = pending else {
        return Ok(None);
    };

    // Cross-document lookup (done after CST traversal to avoid holding non-Send nodes across await).
    let doc_indices = {
        let Some(doc_path) = doc_path.clone() else {
            return Ok(None);
        };
        let db = salsa_db.lock().await;
        let mut doc_paths =
            crate::salsa::project_graph(&*db, salsa_file, salsa_config, doc_path.clone())
                .documents()
                .iter()
                .cloned()
                .collect::<Vec<_>>();
        if !doc_paths.contains(&doc_path) {
            doc_paths.push(doc_path.clone());
        }
        doc_paths.sort();
        doc_paths.dedup();

        let mut per_doc = Vec::new();
        for path in &doc_paths {
            let doc_uri = Uri::from_file_path(path).unwrap_or_else(|| uri.clone());
            let (file, text) = if doc_uri == *uri {
                (salsa_file, content.clone())
            } else {
                let Some(file) = crate::salsa::Db::file_text(&*db, path.clone()) else {
                    continue;
                };
                (file, file.text(&*db).clone())
            };
            let symbol_index =
                crate::salsa::symbol_usage_index(&*db, file, salsa_config, path.clone()).clone();
            per_doc.push((doc_uri, text, symbol_index));
        }
        per_doc
    };
    let definition_index =
        helpers::get_definition_index_with_includes(&document_map, &salsa_db, uri).await;

    if let PendingDefinition::HeadingLink(label) = &pending {
        for (doc_uri, text, symbol_index) in &doc_indices {
            if let Some(ranges) = symbol_index.heading_explicit_definition_ranges(label)
                && let Some(range) = ranges.first()
            {
                let start = conversions::offset_to_position(text, range.start().into());
                let end = conversions::offset_to_position(text, range.end().into());
                let location = Location {
                    uri: doc_uri.clone(),
                    range: Range { start, end },
                };
                return Ok(Some(GotoDefinitionResponse::Scalar(location)));
            }
        }

        if config.extensions.implicit_header_references && config.extensions.auto_identifiers {
            for (doc_uri, text, symbol_index) in &doc_indices {
                if let Some(ranges) = symbol_index.heading_implicit_definition_ranges(label)
                    && let Some(range) = ranges.first()
                {
                    let start = conversions::offset_to_position(text, range.start().into());
                    let end = conversions::offset_to_position(text, range.end().into());
                    let location = Location {
                        uri: doc_uri.clone(),
                        range: Range { start, end },
                    };
                    return Ok(Some(GotoDefinitionResponse::Scalar(location)));
                }
            }
        }
    }

    if let PendingDefinition::Crossref(label) = &pending {
        for (doc_uri, text, symbol_index) in &doc_indices {
            for candidate in
                crate::utils::crossref_symbol_labels(label, config.extensions.bookdown_references)
            {
                if let Some(ranges) = symbol_index.crossref_declarations(&candidate)
                    && let Some(range) = ranges.first()
                {
                    let start = conversions::offset_to_position(text, range.start().into());
                    let end = conversions::offset_to_position(text, range.end().into());
                    let location = Location {
                        uri: doc_uri.clone(),
                        range: Range { start, end },
                    };
                    return Ok(Some(GotoDefinitionResponse::Scalar(location)));
                }

                if config.extensions.implicit_header_references
                    && config.extensions.auto_identifiers
                    && let Some(ranges) =
                        symbol_index.heading_implicit_definition_ranges(&candidate)
                    && let Some(range) = ranges.first()
                {
                    let start = conversions::offset_to_position(text, range.start().into());
                    let end = conversions::offset_to_position(text, range.end().into());
                    let location = Location {
                        uri: doc_uri.clone(),
                        range: Range { start, end },
                    };
                    return Ok(Some(GotoDefinitionResponse::Scalar(location)));
                }
            }
        }
    }

    if let PendingDefinition::Reference { label, is_footnote } = &pending {
        for (doc_uri, text, symbol_index) in &doc_indices {
            let ranges = if *is_footnote {
                symbol_index.footnote_definitions(label)
            } else {
                symbol_index.reference_definitions(label)
            };
            if let Some(ranges) = ranges
                && let Some(range) = ranges.first()
            {
                let start = conversions::offset_to_position(text, range.start().into());
                let end = conversions::offset_to_position(text, range.end().into());
                let location = Location {
                    uri: doc_uri.clone(),
                    range: Range { start, end },
                };
                return Ok(Some(GotoDefinitionResponse::Scalar(location)));
            }
        }
    }

    let definition =
        match pending {
            PendingDefinition::Citation(key) => {
                let Some(index) = citation_def_index.as_ref() else {
                    return Ok(None);
                };
                let db = salsa_db.lock().await;
                let mut locations =
                    helpers::citation_definition_locations(index, &key, uri, &content, &*db);
                if locations.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(GotoDefinitionResponse::Scalar(locations.remove(0))));
            }
            PendingDefinition::Crossref(label) => definition_index
                .find_crossref_resolved(&label, config.extensions.bookdown_references),
            PendingDefinition::HeadingLink(label) => definition_index
                .find_crossref_resolved(&label, config.extensions.bookdown_references),
            PendingDefinition::Reference { label, is_footnote } => {
                if is_footnote {
                    definition_index.find_footnote(&label)
                } else {
                    definition_index.find_reference(&label)
                }
            }
        };

    let Some(definition) = definition else {
        return Ok(None);
    };

    let target_uri = Uri::from_file_path(definition.path()).unwrap_or_else(|| uri.clone());
    let target_text = if Some(definition.path().to_path_buf()) == this_path {
        content
    } else {
        let db = salsa_db.lock().await;
        crate::salsa::Db::file_text(&*db, definition.path().to_path_buf())
            .map(|file| file.text(&*db).clone())
            .unwrap_or_default()
    };
    let start = conversions::offset_to_position(&target_text, definition.range().start().into());
    let end = conversions::offset_to_position(&target_text, definition.range().end().into());
    let location = Location {
        uri: target_uri,
        range: Range { start, end },
    };
    Ok(Some(GotoDefinitionResponse::Scalar(location)))
}
