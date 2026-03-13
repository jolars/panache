use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;
use crate::parser::utils::attributes::try_parse_trailing_attributes;
use crate::syntax::{AstNode, ChunkOption, Crossref, SyntaxKind, SyntaxNode};
use crate::utils::normalize_label;

use super::super::conversions::{offset_to_position, position_to_offset};
use super::super::helpers;

pub(crate) async fn references(
    _client: &tower_lsp_server::Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
    _workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: ReferenceParams,
) -> Result<Option<Vec<Location>>> {
    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;
    let include_declaration = params.context.include_declaration;

    let (salsa_file, salsa_config, doc_path, content, green_tree) = {
        let map = document_map.lock().await;
        let Some(state) = map.get(&uri.to_string()) else {
            return Ok(None);
        };
        let db = salsa_db.lock().await;
        (
            state.salsa_file,
            state.salsa_config,
            state.path.clone(),
            state.salsa_file.text(&*db).clone(),
            state.tree.clone(),
        )
    };

    let Some(doc_path) = doc_path.clone() else {
        return Ok(None);
    };

    let target_norm = {
        let root = SyntaxNode::new_root(green_tree.clone());
        let Some(offset) = position_to_offset(&content, position) else {
            return Ok(None);
        };
        let Some(mut node) = helpers::find_node_at_offset(&root, offset) else {
            return Ok(None);
        };

        let target = loop {
            if let Some(key) = helpers::extract_crossref_key(&node) {
                break Some(key);
            }
            if let Some(key) = helpers::extract_chunk_label_key(&node) {
                break Some(key);
            }
            match node.parent() {
                Some(parent) => node = parent,
                None => break None,
            }
        };

        let Some(target) = target else {
            return Ok(None);
        };
        normalize_label(&target)
    };

    let mut doc_paths = {
        let db = salsa_db.lock().await;
        crate::salsa::project_graph(&*db, salsa_file, salsa_config, doc_path.clone())
            .documents()
            .iter()
            .cloned()
            .collect::<Vec<_>>()
    };
    if !doc_paths.contains(&doc_path) {
        doc_paths.push(doc_path.clone());
    }
    doc_paths.sort();
    doc_paths.dedup();

    let mut locations = Vec::new();
    for path in doc_paths {
        let doc_uri = Uri::from_file_path(&path).unwrap_or_else(|| uri.clone());
        let (text, tree) = if path == doc_path {
            (content.clone(), SyntaxNode::new_root(green_tree.clone()))
        } else {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let tree = crate::parse(&text, None);
            (text, tree)
        };

        locations.extend(crossref_usage_locations(
            &tree,
            &text,
            &target_norm,
            &doc_uri,
        ));
        if include_declaration {
            locations.extend(crossref_definition_locations(
                &tree,
                &text,
                &target_norm,
                &doc_uri,
            ));
        }
    }

    if locations.is_empty() {
        return Ok(None);
    }

    Ok(Some(locations))
}

fn crossref_usage_locations(
    root: &SyntaxNode,
    text: &str,
    target_norm: &str,
    uri: &Uri,
) -> Vec<Location> {
    let mut out = Vec::new();
    for node in root
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::CROSSREF)
    {
        let Some(crossref) = Crossref::cast(node) else {
            continue;
        };
        for key in crossref.keys() {
            if normalize_label(&key.text()) != target_norm {
                continue;
            }
            let range = key.text_range();
            out.push(Location {
                uri: uri.clone(),
                range: Range {
                    start: offset_to_position(text, range.start().into()),
                    end: offset_to_position(text, range.end().into()),
                },
            });
        }
    }
    out
}

fn crossref_definition_locations(
    root: &SyntaxNode,
    text: &str,
    target_norm: &str,
    uri: &Uri,
) -> Vec<Location> {
    let mut out = Vec::new();

    for node in root.descendants() {
        if node.kind() != SyntaxKind::ATTRIBUTE {
            continue;
        }
        let text_value = node.text().to_string();
        if let Some(attrs) = try_parse_trailing_attributes(&text_value).map(|(attrs, _)| attrs)
            && let Some(id) = attrs.identifier
            && normalize_label(&id) == target_norm
        {
            let range = node.text_range();
            out.push(Location {
                uri: uri.clone(),
                range: Range {
                    start: offset_to_position(text, range.start().into()),
                    end: offset_to_position(text, range.end().into()),
                },
            });
        }
    }

    for option in root.descendants().filter_map(ChunkOption::cast) {
        let Some(key) = option.key() else {
            continue;
        };
        if !key.eq_ignore_ascii_case("label") {
            continue;
        }
        let Some(value) = option.value() else {
            continue;
        };
        if normalize_label(&value) != target_norm {
            continue;
        }
        let range = option.syntax().text_range();
        out.push(Location {
            uri: uri.clone(),
            range: Range {
                start: offset_to_position(text, range.start().into()),
                end: offset_to_position(text, range.end().into()),
            },
        });
    }

    out
}
