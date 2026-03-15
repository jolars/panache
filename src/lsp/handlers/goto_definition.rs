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
use crate::syntax::SyntaxKind;

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

    let (salsa_file, salsa_config, doc_path) = {
        let map = document_map.lock().await;
        match map.get(&uri.to_string()) {
            Some(state) => (state.salsa_file, state.salsa_config, state.path.clone()),
            None => return Ok(None),
        }
    };

    let citation_def_index = if let Some(doc_path) = doc_path.clone() {
        let yaml_ok = {
            let db = salsa_db.lock().await;
            crate::salsa::yaml_metadata_parse_result(
                &*db,
                salsa_file,
                salsa_config,
                doc_path.clone(),
            )
            .is_ok()
        };
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

    enum PendingDefinition {
        Citation(String),
        Crossref(String),
        Reference { label: String, is_footnote: bool },
    }

    let (content, pending) = {
        let Some((content, root)) =
            helpers::get_document_content_and_tree(&document_map, &salsa_db, uri).await
        else {
            return Ok(None);
        };

        // Convert LSP position to byte offset
        let Some(offset) = conversions::position_to_offset(&content, position) else {
            return Ok(None);
        };

        // Find the node at this offset
        let Some(mut node) = helpers::find_node_at_offset(&root, offset) else {
            return Ok(None);
        };

        // Walk up the tree to find a citation, reference, or footnote
        let pending = loop {
            if let Some(key) = helpers::extract_citation_key(&node) {
                break Some(PendingDefinition::Citation(key));
            }

            // Quarto crossref: jump to attribute definition
            if let Some(label) = helpers::extract_crossref_key(&node)
                && let Some(definition) = helpers::find_crossref_definition_node(&root, &label)
            {
                let start_offset: usize = definition.text_range().start().into();
                let end_offset: usize = definition.text_range().end().into();

                let start_position = conversions::offset_to_position(&content, start_offset);
                let end_position = conversions::offset_to_position(&content, end_offset);

                let location = Location {
                    uri: uri.clone(),
                    range: Range {
                        start: start_position,
                        end: end_position,
                    },
                };

                return Ok(Some(GotoDefinitionResponse::Scalar(location)));
            }

            if config.extensions.implicit_header_references
                && config.extensions.auto_identifiers
                && let Some(label) = helpers::extract_crossref_key(&node)
                && let Some(definition) =
                    helpers::find_implicit_header_definition_node(&root, &label)
            {
                let start_offset: usize = definition.text_range().start().into();
                let end_offset: usize = definition.text_range().end().into();

                let start_position = conversions::offset_to_position(&content, start_offset);
                let end_position = conversions::offset_to_position(&content, end_offset);

                let location = Location {
                    uri: uri.clone(),
                    range: Range {
                        start: start_position,
                        end: end_position,
                    },
                };

                return Ok(Some(GotoDefinitionResponse::Scalar(location)));
            }

            if let Some(label) = helpers::extract_crossref_key(&node) {
                break Some(PendingDefinition::Crossref(label));
            }

            // Fallback: find reference/footnote definition at this node
            if let Some((label, is_footnote)) = helpers::extract_reference_label(&node)
                && let Some(definition) = helpers::find_definition_node(&root, &label, is_footnote)
            {
                let start_offset: usize = definition.text_range().start().into();
                let end_offset: usize = definition.text_range().end().into();

                let start_position = conversions::offset_to_position(&content, start_offset);
                let end_position = conversions::offset_to_position(&content, end_offset);

                let location = Location {
                    uri: uri.clone(),
                    range: Range {
                        start: start_position,
                        end: end_position,
                    },
                };

                return Ok(Some(GotoDefinitionResponse::Scalar(location)));
            }

            if let Some((label, is_footnote)) = helpers::extract_reference_label(&node) {
                break Some(PendingDefinition::Reference { label, is_footnote });
            }

            if node.kind() == SyntaxKind::LINK
                && let Some(link_ref) = node
                    .children()
                    .find(|child| child.kind() == SyntaxKind::LINK_REF)
                && let Some((label, is_footnote)) = helpers::extract_reference_label(&link_ref)
                && let Some(definition) = helpers::find_definition_node(&root, &label, is_footnote)
            {
                let start_offset: usize = definition.text_range().start().into();
                let end_offset: usize = definition.text_range().end().into();

                let start_position = conversions::offset_to_position(&content, start_offset);
                let end_position = conversions::offset_to_position(&content, end_offset);

                let location = Location {
                    uri: uri.clone(),
                    range: Range {
                        start: start_position,
                        end: end_position,
                    },
                };

                return Ok(Some(GotoDefinitionResponse::Scalar(location)));
            }

            if node.kind() == SyntaxKind::IMAGE_LINK
                && let Some(link_ref) = node
                    .children()
                    .find(|child| child.kind() == SyntaxKind::LINK_REF)
                && let Some((label, is_footnote)) = helpers::extract_reference_label(&link_ref)
                && let Some(definition) = helpers::find_definition_node(&root, &label, is_footnote)
            {
                let start_offset: usize = definition.text_range().start().into();
                let end_offset: usize = definition.text_range().end().into();

                let start_position = conversions::offset_to_position(&content, start_offset);
                let end_position = conversions::offset_to_position(&content, end_offset);

                let location = Location {
                    uri: uri.clone(),
                    range: Range {
                        start: start_position,
                        end: end_position,
                    },
                };

                return Ok(Some(GotoDefinitionResponse::Scalar(location)));
            }

            // Move up to parent, or return None if at root
            match node.parent() {
                Some(parent) => node = parent,
                None => break None,
            }
        };

        (content, pending)
    };

    let Some(pending) = pending else {
        return Ok(None);
    };

    // Cross-document lookup (done after CST traversal to avoid holding non-Send nodes across await).
    let definition_index =
        helpers::get_definition_index_with_includes(&document_map, &salsa_db, uri).await;

    let definition = match pending {
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
        PendingDefinition::Crossref(label) => definition_index.find_crossref(&label),
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
