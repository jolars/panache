use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;
use crate::metadata::{inline_bib_conflicts, inline_reference_map};
use crate::syntax::{AstNode, Citation, SyntaxKind, SyntaxNode};

use super::super::conversions::{offset_to_position, position_to_offset};
use super::super::helpers;
use crate::utils::normalize_label;

pub(crate) async fn rename(
    _client: &tower_lsp_server::Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    _workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: RenameParams,
) -> Result<Option<WorkspaceEdit>> {
    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;
    let new_name = params.new_name;

    let (metadata, graph, content, root) = {
        let map = document_map.lock().await;
        let Some(state) = map.get(&uri.to_string()) else {
            return Ok(None);
        };
        (
            state.metadata.clone(),
            state.graph.clone(),
            state.text.clone(),
            SyntaxNode::new_root(state.tree.clone()),
        )
    };
    let (old_key, old_norm) = {
        let Some(offset) = position_to_offset(&content, position) else {
            return Ok(None);
        };
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

    let Some(metadata) = metadata else {
        return Ok(None);
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
            let bib_text = std::fs::read_to_string(&bib_path).unwrap_or_default();
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

    let doc_path = uri.to_file_path().map(|p| p.into_owned());
    for bib_path in &bib_paths {
        doc_paths.extend(graph.dependents(bib_path, Some(crate::includes::EdgeKind::Bibliography)));
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
            let text = if Some(entry.path.clone()) == uri.to_file_path().map(|p| p.into_owned()) {
                content.clone()
            } else {
                std::fs::read_to_string(&entry.path).unwrap_or_default()
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

    if let Some(path) = doc_path
        && !doc_paths.contains(&path)
    {
        doc_paths.push(path);
    }

    doc_paths.sort();
    doc_paths.dedup();

    for path in doc_paths {
        let doc_uri = Uri::from_file_path(&path).unwrap_or_else(|| uri.clone());
        let (text, tree) = if doc_uri == uri {
            (content.clone(), root.clone())
        } else {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let tree = crate::parse(&text, None);
            (text, tree)
        };

        let edits = citation_key_edits(&tree, &text, &old_norm, &new_name);
        if edits.is_empty() {
            continue;
        }
        changes.entry(doc_uri).or_default().extend(edits);
    }

    if changes.is_empty() {
        return Ok(None);
    }

    Ok(Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    }))
}

fn citation_key_edits(
    root: &SyntaxNode,
    text: &str,
    old_norm: &str,
    new_key: &str,
) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    for node in root
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::CITATION)
    {
        let Some(citation) = Citation::cast(node) else {
            continue;
        };
        for key in citation.keys() {
            if normalize_label(&key.text()) != old_norm {
                continue;
            }
            let range = key.text_range();
            let start = offset_to_position(text, range.start().into());
            let end = offset_to_position(text, range.end().into());
            edits.push(TextEdit {
                range: Range { start, end },
                new_text: new_key.to_string(),
            });
        }
    }
    edits
}
