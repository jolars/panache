use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;
use crate::syntax::SyntaxNode;
use crate::utils::normalize_label;

use super::super::conversions::{offset_to_position, position_to_offset};
use super::super::helpers;

enum Target {
    Crossref(String),
    Citation { key: String, norm: String },
}

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
    let Some(offset) = position_to_offset(&content, position) else {
        return Ok(None);
    };
    if helpers::is_offset_in_yaml_frontmatter(&SyntaxNode::new_root(green_tree.clone()), offset) {
        return Ok(None);
    }

    let target = {
        let root = SyntaxNode::new_root(green_tree.clone());
        let Some(mut node) = helpers::find_node_at_offset(&root, offset) else {
            return Ok(None);
        };

        loop {
            if let Some(key) = helpers::extract_citation_key(&node) {
                break Some(Target::Citation {
                    norm: normalize_label(&key),
                    key,
                });
            }
            if let Some(key) = helpers::extract_crossref_key(&node) {
                break Some(Target::Crossref(normalize_label(&key)));
            }
            if let Some(key) = helpers::extract_chunk_label_key(&node) {
                break Some(Target::Crossref(normalize_label(&key)));
            }
            match node.parent() {
                Some(parent) => node = parent,
                None => break None,
            }
        }
    };
    let Some(target) = target else {
        return Ok(None);
    };

    let mut locations = Vec::new();
    let citation_def_index = {
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

        for path in doc_paths {
            let (file, text) = if path == doc_path {
                (salsa_file, content.clone())
            } else {
                let Some(file) = crate::salsa::Db::file_text(&*db, path.clone()) else {
                    continue;
                };
                (file, file.text(&*db).clone())
            };
            let symbol_index =
                crate::salsa::symbol_usage_index(&*db, file, salsa_config, path.clone()).clone();
            let doc_uri = Uri::from_file_path(&path).unwrap_or_else(|| uri.clone());

            match &target {
                Target::Crossref(label) => {
                    if let Some(ranges) = symbol_index.crossref_usages(label) {
                        add_locations(&mut locations, &doc_uri, &text, ranges);
                    }
                    if include_declaration
                        && let Some(ranges) = symbol_index.crossref_declarations(label)
                    {
                        add_locations(&mut locations, &doc_uri, &text, ranges);
                    }
                }
                Target::Citation { norm, .. } => {
                    if let Some(ranges) = symbol_index.citation_usages(norm) {
                        add_locations(&mut locations, &doc_uri, &text, ranges);
                    }
                }
            }
        }

        if include_declaration {
            let yaml_ok =
                helpers::is_yaml_frontmatter_valid(&SyntaxNode::new_root(green_tree.clone()));
            if yaml_ok {
                Some(
                    crate::salsa::citation_definition_index(
                        &*db,
                        salsa_file,
                        salsa_config,
                        doc_path.clone(),
                    )
                    .clone(),
                )
            } else {
                None
            }
        } else {
            None
        }
    };

    if include_declaration
        && let (Target::Citation { key, .. }, Some(index)) = (&target, citation_def_index.as_ref())
    {
        let db = salsa_db.lock().await;
        locations.extend(helpers::citation_definition_locations(
            index, key, &uri, &content, &*db,
        ));
    }

    locations.sort_by(|a, b| {
        a.uri
            .as_str()
            .cmp(b.uri.as_str())
            .then(a.range.start.line.cmp(&b.range.start.line))
            .then(a.range.start.character.cmp(&b.range.start.character))
            .then(a.range.end.line.cmp(&b.range.end.line))
            .then(a.range.end.character.cmp(&b.range.end.character))
    });
    locations.dedup_by(|a, b| a.uri == b.uri && a.range == b.range);

    if locations.is_empty() {
        return Ok(None);
    }
    Ok(Some(locations))
}

fn add_locations(out: &mut Vec<Location>, uri: &Uri, text: &str, ranges: &[rowan::TextRange]) {
    for range in ranges {
        out.push(Location {
            uri: uri.clone(),
            range: Range {
                start: offset_to_position(text, range.start().into()),
                end: offset_to_position(text, range.end().into()),
            },
        });
    }
}
