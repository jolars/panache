use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;
use crate::metadata::DocumentMetadata;
use crate::parser::utils::attributes::try_parse_trailing_attributes;
use crate::syntax::{AstNode, ChunkOption, Citation, Crossref, SyntaxKind, SyntaxNode};
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

    enum Target {
        Crossref(String),
        Citation { key: String, norm: String },
    }

    let target = {
        let root = SyntaxNode::new_root(green_tree.clone());
        let Some(offset) = position_to_offset(&content, position) else {
            return Ok(None);
        };
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

    let metadata = if include_declaration {
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
            Some(crate::salsa::metadata(&*db, salsa_file, salsa_config, doc_path.clone()).clone())
        } else {
            None
        }
    } else {
        None
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

        match &target {
            Target::Crossref(target_norm) => {
                locations.extend(crossref_usage_locations(
                    &tree,
                    &text,
                    target_norm,
                    &doc_uri,
                ));
                if include_declaration {
                    locations.extend(crossref_definition_locations(
                        &tree,
                        &text,
                        target_norm,
                        &doc_uri,
                    ));
                }
            }
            Target::Citation { norm, .. } => {
                locations.extend(citation_usage_locations(&tree, &text, norm, &doc_uri));
            }
        }
    }

    if include_declaration
        && let (Target::Citation { key, norm }, Some(metadata)) = (&target, metadata.as_ref())
    {
        locations.extend(citation_definition_locations(
            metadata, key, norm, &uri, &content,
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

fn citation_usage_locations(
    root: &SyntaxNode,
    text: &str,
    target_norm: &str,
    uri: &Uri,
) -> Vec<Location> {
    let mut out = Vec::new();
    for node in root
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::CITATION)
    {
        let Some(citation) = Citation::cast(node) else {
            continue;
        };
        for key in citation.keys() {
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

fn citation_definition_locations(
    metadata: &DocumentMetadata,
    key: &str,
    norm: &str,
    default_uri: &Uri,
    default_content: &str,
) -> Vec<Location> {
    let mut out = Vec::new();

    if let Some(parse) = metadata.bibliography_parse.as_ref() {
        for entry in parse.index.entries.values().filter(|entry| {
            entry.key.eq_ignore_ascii_case(key) || normalize_label(&entry.key) == norm
        }) {
            let entry_uri =
                Uri::from_file_path(&entry.source_file).unwrap_or_else(|| default_uri.clone());
            let bib_text = std::fs::read_to_string(&entry.source_file).unwrap_or_default();
            out.push(Location {
                uri: entry_uri,
                range: Range {
                    start: offset_to_position(&bib_text, entry.span.start),
                    end: offset_to_position(&bib_text, entry.span.end),
                },
            });
        }
    }

    for inline in metadata
        .inline_references
        .iter()
        .filter(|entry| entry.id.eq_ignore_ascii_case(key) || normalize_label(&entry.id) == norm)
    {
        let entry_uri = Uri::from_file_path(&inline.path).unwrap_or_else(|| default_uri.clone());
        let inline_text = if entry_uri == *default_uri {
            default_content.to_string()
        } else {
            std::fs::read_to_string(&inline.path).unwrap_or_default()
        };
        out.push(Location {
            uri: entry_uri,
            range: Range {
                start: offset_to_position(&inline_text, inline.range.start().into()),
                end: offset_to_position(&inline_text, inline.range.end().into()),
            },
        });
    }

    out
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
