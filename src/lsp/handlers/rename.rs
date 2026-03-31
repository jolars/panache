use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;
use crate::lsp::symbols::{SymbolTarget, resolve_symbol_target_at_offset};
use crate::metadata::{inline_bib_conflicts, inline_reference_map};

use super::super::conversions::{offset_to_position, position_to_offset};
use super::super::helpers;
use crate::utils::{normalize_anchor_label, normalize_label};

struct RenameScanContext<'a> {
    salsa_db: &'a Arc<Mutex<crate::salsa::SalsaDb>>,
    salsa_file: crate::salsa::FileText,
    salsa_config: crate::salsa::FileConfig,
    doc_path: &'a Path,
    uri: &'a Uri,
    content: &'a str,
}

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

    let Some(ctx) =
        crate::lsp::context::get_open_document_context(&document_map, &salsa_db, &uri).await
    else {
        return Ok(None);
    };

    let salsa_file = ctx.salsa_file;
    let salsa_config = ctx.salsa_config;
    let doc_path = ctx.path.clone();
    let content = ctx.content.clone();
    let parsed_yaml_regions = ctx.parsed_yaml_regions.clone();

    let Some(doc_path) = doc_path.clone() else {
        return Ok(None);
    };
    let Some(offset) = position_to_offset(&content, position) else {
        log::debug!(
            "rename: position_to_offset failed uri={:?} line={} char={}",
            uri,
            position.line,
            position.character
        );
        return Ok(None);
    };
    if helpers::is_offset_in_yaml_frontmatter(&parsed_yaml_regions, offset) {
        return Ok(None);
    }
    let target = {
        let root = ctx.syntax_root();
        resolve_symbol_target_at_offset(&root, offset)
    };
    log::debug!(
        "rename: uri={:?} req=({}, {}) offset={} new_name={:?} target={:?}",
        uri,
        position.line,
        position.character,
        offset,
        new_name,
        target
    );

    // First handle crossref/chunk-label rename without requiring bibliography metadata.
    if let Some(SymbolTarget::Crossref(old_key)) = target.as_ref() {
        let changes = rename_crossref_symbol(
            &RenameScanContext {
                salsa_db: &salsa_db,
                salsa_file,
                salsa_config,
                doc_path: &doc_path,
                uri: &uri,
                content: &content,
            },
            old_key,
            &new_name,
            config.extensions.bookdown_references,
        )
        .await;
        if changes.is_empty() {
            return Ok(None);
        }
        return Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }));
    }

    if let Some(SymbolTarget::ChunkLabel(old_key)) = target.as_ref() {
        let changes = rename_chunk_label_symbol(
            &RenameScanContext {
                salsa_db: &salsa_db,
                salsa_file,
                salsa_config,
                doc_path: &doc_path,
                uri: &uri,
                content: &content,
            },
            old_key,
            &new_name,
            config.extensions.bookdown_references,
        )
        .await;
        if changes.is_empty() {
            return Ok(None);
        }
        return Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }));
    }

    if let Some(SymbolTarget::ExampleLabel(old_key)) = target.as_ref() {
        let changes = rename_example_label_symbol(
            &RenameScanContext {
                salsa_db: &salsa_db,
                salsa_file,
                salsa_config,
                doc_path: &doc_path,
                uri: &uri,
                content: &content,
            },
            old_key,
            &new_name,
            config.extensions.example_lists,
        )
        .await;
        if changes.is_empty() {
            return Ok(None);
        }
        return Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }));
    }

    if let Some(SymbolTarget::HeadingLink(old_key) | SymbolTarget::HeadingId(old_key)) =
        target.as_ref()
    {
        let old_norm = normalize_anchor_label(old_key);
        let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();

        let per_doc = crate::lsp::navigation::project_symbol_documents(
            &salsa_db,
            salsa_file,
            salsa_config,
            &doc_path,
            &uri,
            &content,
        )
        .await;

        for doc in per_doc {
            let doc_uri = doc.uri;
            let text = doc.text;
            let symbol_index = doc.symbol_index;
            let ranges = symbol_index.heading_rename_ranges(&old_norm);

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

    if let Some(SymbolTarget::Reference {
        label,
        is_footnote: true,
    }) = target.as_ref()
    {
        let symbol_index = {
            let db = salsa_db.lock().await;
            crate::salsa::symbol_usage_index(&*db, salsa_file, salsa_config, doc_path.clone())
                .clone()
        };
        let ranges = symbol_index.footnote_rename_ranges(label);
        let edits = text_edits_from_ranges(&ranges, &content, &new_name);
        if edits.is_empty() {
            return Ok(None);
        }
        let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
        changes.insert(uri.clone(), edits);
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
    let (old_key, old_norm) = match target {
        Some(SymbolTarget::Citation(key)) => {
            let norm = normalize_label(&key);
            (key, norm)
        }
        _ => return Ok(None),
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

    let citation_usage_inputs = crate::lsp::navigation::document_inputs_for_paths(
        &salsa_db, &doc_path, &content, doc_paths,
    )
    .await;
    let citation_usage_docs = crate::lsp::navigation::indexed_documents_from_inputs(
        &salsa_db,
        salsa_file,
        salsa_config,
        &doc_path,
        &uri,
        citation_usage_inputs,
    )
    .await;
    for doc in citation_usage_docs {
        let doc_uri = doc.uri;
        let text = doc.text;
        let symbol_index = doc.symbol_index;
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

async fn rename_crossref_symbol(
    ctx: &RenameScanContext<'_>,
    old_key: &str,
    new_name: &str,
    bookdown_references: bool,
) -> HashMap<Uri, Vec<TextEdit>> {
    let old_norm = normalize_anchor_label(old_key);
    let search_keys = crate::utils::crossref_symbol_labels(&old_norm, bookdown_references);
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();

    let per_doc = crate::lsp::navigation::project_symbol_documents(
        ctx.salsa_db,
        ctx.salsa_file,
        ctx.salsa_config,
        ctx.doc_path,
        ctx.uri,
        ctx.content,
    )
    .await;

    for doc in per_doc {
        let doc_uri = doc.uri;
        let text = doc.text;
        let symbol_index = doc.symbol_index;
        let mut edits = Vec::new();
        for search_key in &search_keys {
            if let Some(ranges) = symbol_index.crossref_usages(search_key) {
                edits.extend(text_edits_from_ranges(ranges, &text, new_name));
            }
            if let Some(ranges) = symbol_index.chunk_label_value_ranges(search_key) {
                edits.extend(text_edits_from_ranges(ranges, &text, new_name));
            }
            if let Some(ranges) = symbol_index.crossref_declaration_value_ranges(search_key) {
                edits.extend(text_edits_from_ranges(ranges, &text, new_name));
            }
        }

        if bookdown_references {
            let insert_ranges = symbol_index
                .implicit_heading_insert_ranges(&old_norm)
                .cloned()
                .unwrap_or_default();
            edits.extend(text_edits_from_ranges(
                &insert_ranges,
                &text,
                &format!(" {{#{}}}", new_name),
            ));
        }

        if edits.is_empty() {
            continue;
        }
        log::debug!(
            "rename[crossref]: uri={:?} edits={} keys={:?}",
            doc_uri,
            edits.len(),
            search_keys
        );
        changes.entry(doc_uri).or_default().extend(edits);
    }

    if changes.is_empty() {
        log::debug!(
            "rename[crossref]: no edits produced old_key={:?} old_norm={:?}",
            old_key,
            old_norm
        );
    }
    changes
}

async fn rename_chunk_label_symbol(
    ctx: &RenameScanContext<'_>,
    old_key: &str,
    new_name: &str,
    bookdown_references: bool,
) -> HashMap<Uri, Vec<TextEdit>> {
    let old_norm = normalize_anchor_label(old_key);
    let search_keys = crate::utils::crossref_symbol_labels(&old_norm, bookdown_references);
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();

    let per_doc = crate::lsp::navigation::project_symbol_documents(
        ctx.salsa_db,
        ctx.salsa_file,
        ctx.salsa_config,
        ctx.doc_path,
        ctx.uri,
        ctx.content,
    )
    .await;

    for doc in per_doc {
        let doc_uri = doc.uri;
        let text = doc.text;
        let symbol_index = doc.symbol_index;
        let mut edits = Vec::new();

        for search_key in &search_keys {
            if let Some(ranges) = symbol_index.chunk_label_value_ranges(search_key) {
                edits.extend(text_edits_from_ranges(ranges, &text, new_name));
            }
            if let Some(ranges) = symbol_index.crossref_usages(search_key) {
                edits.extend(text_edits_from_ranges(ranges, &text, new_name));
            }
        }

        if edits.is_empty() {
            continue;
        }
        changes.entry(doc_uri).or_default().extend(edits);
    }

    changes
}

async fn rename_example_label_symbol(
    ctx: &RenameScanContext<'_>,
    old_key: &str,
    new_name: &str,
    example_lists_enabled: bool,
) -> HashMap<Uri, Vec<TextEdit>> {
    if !example_lists_enabled {
        return HashMap::new();
    }

    let old_norm = normalize_label(old_key);
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();

    let per_doc = crate::lsp::navigation::project_symbol_documents(
        ctx.salsa_db,
        ctx.salsa_file,
        ctx.salsa_config,
        ctx.doc_path,
        ctx.uri,
        ctx.content,
    )
    .await;

    for doc in per_doc {
        let doc_uri = doc.uri;
        let text = doc.text;
        let symbol_index = doc.symbol_index;
        let mut edits = Vec::new();

        if let Some(ranges) = symbol_index.example_label_usages(&old_norm) {
            edits.extend(text_edits_from_ranges(ranges, &text, new_name));
        }
        if let Some(ranges) = symbol_index.example_label_definitions(&old_norm) {
            edits.extend(text_edits_from_ranges(ranges, &text, new_name));
        }
        if let Some(ranges) = symbol_index.citation_usages(&old_norm) {
            let parenthesized = ranges
                .iter()
                .copied()
                .filter(|range| is_parenthesized_at_label(&text, *range))
                .collect::<Vec<_>>();
            edits.extend(text_edits_from_ranges(&parenthesized, &text, new_name));
        }

        if edits.is_empty() {
            continue;
        }
        edits.sort_by(|a, b| {
            a.range
                .start
                .line
                .cmp(&b.range.start.line)
                .then(a.range.start.character.cmp(&b.range.start.character))
                .then(a.range.end.line.cmp(&b.range.end.line))
                .then(a.range.end.character.cmp(&b.range.end.character))
        });
        edits.dedup_by(|a, b| a.range == b.range && a.new_text == b.new_text);
        changes.entry(doc_uri).or_default().extend(edits);
    }

    changes
}

fn is_parenthesized_at_label(text: &str, range: rowan::TextRange) -> bool {
    let start: usize = range.start().into();
    let end: usize = range.end().into();
    if start < 2 || end >= text.len() {
        return false;
    }
    let bytes = text.as_bytes();
    bytes[start - 2] == b'(' && bytes[start - 1] == b'@' && bytes[end] == b')'
}
