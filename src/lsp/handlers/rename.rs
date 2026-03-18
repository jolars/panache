use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;
use crate::metadata::{inline_bib_conflicts, inline_reference_map};
use crate::syntax::SyntaxNode;

use super::super::conversions::{offset_to_position, position_to_offset};
use super::super::helpers;
use crate::utils::normalize_label;

pub(crate) async fn rename(
    client: &tower_lsp_server::Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: RenameParams,
) -> Result<Option<WorkspaceEdit>> {
    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;
    let new_name = params.new_name;
    let config = helpers::get_config(client, &workspace_root, &uri).await;

    let (salsa_file, salsa_config, doc_path, content, green_tree, parsed_yaml_regions) = {
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
            state.parsed_yaml_regions.clone(),
        )
    };

    let Some(doc_path) = doc_path.clone() else {
        return Ok(None);
    };
    let Some(offset) = position_to_offset(&content, position) else {
        return Ok(None);
    };
    if helpers::is_offset_in_yaml_frontmatter(&parsed_yaml_regions, offset) {
        return Ok(None);
    }
    // First handle crossref/chunk-label rename without requiring bibliography metadata.
    let maybe_crossref_key = {
        let root = SyntaxNode::new_root(green_tree.clone());
        let Some(mut node) = helpers::find_node_at_offset(&root, offset) else {
            return Ok(None);
        };
        loop {
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
        }
    };
    if let Some(old_key) = maybe_crossref_key {
        let old_norm = normalize_label(&old_key);
        let search_keys =
            crate::utils::crossref_symbol_labels(&old_norm, config.extensions.bookdown_references);
        let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();

        let per_doc = {
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

            let mut out = Vec::new();
            for path in &doc_paths {
                let doc_uri = Uri::from_file_path(path).unwrap_or_else(|| uri.clone());
                let (file, text) = if doc_uri == uri {
                    (salsa_file, content.clone())
                } else {
                    let Some(file) = crate::salsa::Db::file_text(&*db, path.clone()) else {
                        continue;
                    };
                    (file, file.text(&*db).clone())
                };
                let symbol_index =
                    crate::salsa::symbol_usage_index(&*db, file, salsa_config, path.clone())
                        .clone();
                out.push((doc_uri, text, symbol_index));
            }
            out
        };

        for (doc_uri, text, symbol_index) in per_doc {
            let mut edits = Vec::new();
            for search_key in &search_keys {
                if let Some(ranges) = symbol_index.crossref_usages(search_key) {
                    edits.extend(text_edits_from_ranges(ranges, &text, &new_name));
                }
                if let Some(ranges) = symbol_index.chunk_label_value_ranges(search_key) {
                    edits.extend(text_edits_from_ranges(ranges, &text, &new_name));
                }
                if let Some(ranges) = symbol_index.crossref_declaration_value_ranges(search_key) {
                    edits.extend(text_edits_from_ranges(ranges, &text, &new_name));
                }
            }

            if config.extensions.bookdown_references {
                let root = crate::parse(&text, Some(config.clone()));
                let insert_ranges =
                    helpers::collect_implicit_heading_id_insert_ranges(&root, &old_norm);
                edits.extend(text_edits_from_ranges(
                    &insert_ranges,
                    &text,
                    &format!(" {{#{}}}", new_name),
                ));
            }

            if edits.is_empty() {
                continue;
            }
            changes.entry(doc_uri).or_default().extend(edits);
        }

        if changes.is_empty() {
            return Ok(None);
        }
        return Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }));
    }

    let maybe_heading_key = {
        let root = SyntaxNode::new_root(green_tree.clone());
        let Some(mut node) = helpers::find_node_at_offset(&root, offset) else {
            return Ok(None);
        };
        loop {
            if let Some(key) = helpers::extract_heading_link_target(&node) {
                break Some(key);
            }
            if let Some(key) = helpers::extract_heading_id_key(&node) {
                break Some(key);
            }
            match node.parent() {
                Some(parent) => node = parent,
                None => break None,
            }
        }
    };
    if let Some(old_key) = maybe_heading_key {
        let old_norm = normalize_label(&old_key);
        let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();

        let per_doc = {
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

            let mut out = Vec::new();
            for path in &doc_paths {
                let doc_uri = Uri::from_file_path(path).unwrap_or_else(|| uri.clone());
                let text = if doc_uri == uri {
                    content.clone()
                } else {
                    let Some(file) = crate::salsa::Db::file_text(&*db, path.clone()) else {
                        continue;
                    };
                    file.text(&*db).clone()
                };
                out.push((doc_uri, text));
            }
            out
        };

        for (doc_uri, text) in per_doc {
            let root = crate::parse(&text, Some(config.clone()));
            let ranges = helpers::collect_heading_rename_ranges(&root, &old_norm);
            let edits = text_edits_from_ranges(&ranges, &text, &new_name);
            if edits.is_empty() {
                continue;
            }
            changes.entry(doc_uri).or_default().extend(edits);
        }

        if changes.is_empty() {
            return Ok(None);
        }

        return Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }));
    }

    if !helpers::is_yaml_frontmatter_valid(&parsed_yaml_regions) {
        return Ok(None);
    }

    let metadata = {
        let db = salsa_db.lock().await;
        crate::salsa::metadata(&*db, salsa_file, salsa_config, doc_path.clone()).clone()
    };
    let (old_key, old_norm) = {
        let root = SyntaxNode::new_root(green_tree.clone());
        let Some(mut node) = helpers::find_node_at_offset(&root, offset) else {
            return Ok(None);
        };
        let old_key = loop {
            if let Some(key) = helpers::extract_citation_key(&node) {
                break key;
            }
            match node.parent() {
                Some(parent) => node = parent,
                None => return Ok(None),
            }
        };
        let old_norm = normalize_label(&old_key);
        (old_key, old_norm)
    };

    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    let mut doc_paths = Vec::new();
    let mut bib_paths = Vec::new();

    if let Some(parse) = metadata.bibliography_parse.as_ref() {
        let mut bib_entries: Vec<crate::bib::BibEntry> = Vec::new();
        if let Some(entry) = parse.index.get(&old_key) {
            bib_entries.push(entry.clone());
        } else {
            for conflict in inline_bib_conflicts(&metadata.inline_references, &parse.index) {
                if conflict.key.eq_ignore_ascii_case(&old_key) {
                    bib_entries.push(conflict.bib.clone());
                }
            }
        }
        bib_entries.sort_by(|a, b| a.source_file.cmp(&b.source_file));
        bib_entries
            .dedup_by(|a, b| a.source_file == b.source_file && a.key.eq_ignore_ascii_case(&b.key));
        for entry in bib_entries {
            let bib_path = entry.source_file.clone();
            let bib_text = {
                let db = salsa_db.lock().await;
                crate::salsa::Db::file_text(&*db, bib_path.clone())
                    .map(|file| file.text(&*db).clone())
                    .unwrap_or_default()
            };
            let bib_start = offset_to_position(&bib_text, entry.span.start);
            let bib_end = offset_to_position(&bib_text, entry.span.end);
            let bib_uri = Uri::from_file_path(&bib_path).unwrap_or_else(|| uri.clone());
            changes.entry(bib_uri).or_default().push(TextEdit {
                range: Range {
                    start: bib_start,
                    end: bib_end,
                },
                new_text: new_name.clone(),
            });
            bib_paths.push(bib_path);
        }
    }

    let graph = {
        let db = salsa_db.lock().await;
        crate::salsa::project_graph(&*db, salsa_file, salsa_config, doc_path.clone()).clone()
    };

    for bib_path in &bib_paths {
        doc_paths.extend(graph.dependents(bib_path, Some(crate::salsa::EdgeKind::Bibliography)));
    }

    let inline_refs = inline_reference_map(&metadata.inline_references);
    if inline_refs.contains_key(&old_norm) {
        let mut inline_doc_paths = Vec::new();
        let mut inline_edits: Vec<(Uri, TextEdit)> = Vec::new();
        for entry in metadata
            .inline_references
            .iter()
            .filter(|entry| entry.id.eq_ignore_ascii_case(&old_key))
        {
            let text = if entry.path == doc_path {
                content.clone()
            } else {
                let db = salsa_db.lock().await;
                crate::salsa::Db::file_text(&*db, entry.path.clone())
                    .map(|file| file.text(&*db).clone())
                    .unwrap_or_default()
            };
            let start = offset_to_position(&text, entry.range.start().into());
            let end = offset_to_position(&text, entry.range.end().into());
            let entry_uri = Uri::from_file_path(&entry.path).unwrap_or_else(|| uri.clone());
            inline_edits.push((
                entry_uri,
                TextEdit {
                    range: Range { start, end },
                    new_text: new_name.clone(),
                },
            ));
            inline_doc_paths.push(entry.path.clone());
        }
        for (entry_uri, edit) in inline_edits {
            changes.entry(entry_uri).or_default().push(edit);
        }
        for path in inline_doc_paths {
            doc_paths.push(path);
        }
    }

    if !doc_paths.contains(&doc_path) {
        doc_paths.push(doc_path.clone());
    }

    doc_paths.sort();
    doc_paths.dedup();

    let citation_usage_docs = {
        let db = salsa_db.lock().await;
        let mut out = Vec::new();
        for path in &doc_paths {
            let doc_uri = Uri::from_file_path(path).unwrap_or_else(|| uri.clone());
            let (file, text) = if doc_uri == uri {
                (salsa_file, content.clone())
            } else {
                let Some(file) = crate::salsa::Db::file_text(&*db, path.clone()) else {
                    continue;
                };
                (file, file.text(&*db).clone())
            };
            let symbol_index =
                crate::salsa::symbol_usage_index(&*db, file, salsa_config, path.clone()).clone();
            out.push((doc_uri, text, symbol_index));
        }
        out
    };
    for (doc_uri, text, symbol_index) in citation_usage_docs {
        let Some(ranges) = symbol_index.citation_usages(&old_norm) else {
            continue;
        };
        let edits = text_edits_from_ranges(ranges, &text, &new_name);
        if !edits.is_empty() {
            changes.entry(doc_uri).or_default().extend(edits);
        }
    }

    if changes.is_empty() {
        return Ok(None);
    }

    Ok(Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    }))
}

fn text_edits_from_ranges(
    ranges: &[rowan::TextRange],
    text: &str,
    new_text: &str,
) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    for range in ranges {
        let start = offset_to_position(text, range.start().into());
        let end = offset_to_position(text, range.end().into());
        edits.push(TextEdit {
            range: Range { start, end },
            new_text: new_text.to_string(),
        });
    }
    edits
}
