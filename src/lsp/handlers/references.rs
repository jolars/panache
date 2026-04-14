use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;
use crate::lsp::symbols::{SymbolTarget, resolve_symbol_target_at_offset};
use crate::utils::{normalize_anchor_label, normalize_label};

use super::super::conversions::{offset_to_position, position_to_offset};
use super::super::helpers;

pub(crate) async fn references(
    client: &tower_lsp_server::Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: ReferenceParams,
) -> Result<Option<Vec<Location>>> {
    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;
    let include_declaration = params.context.include_declaration;
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
        return Ok(None);
    };
    if helpers::is_offset_in_yaml_frontmatter(&parsed_yaml_regions, offset) {
        return Ok(None);
    }

    let target = {
        let root = ctx.syntax_root();
        resolve_symbol_target_at_offset(&root, offset)
    };
    let Some(target) = target else {
        return Ok(None);
    };

    let mut locations = Vec::new();
    let citation_def_index = {
        let docs = crate::lsp::navigation::project_symbol_documents(
            &salsa_db,
            salsa_file,
            salsa_config,
            &doc_path,
            &uri,
            &content,
        )
        .await;

        for doc in docs {
            let doc_uri = doc.uri;
            let text = doc.text;
            let symbol_index = doc.symbol_index;

            match &target {
                SymbolTarget::Crossref(label) => {
                    let candidates =
                        crossref_candidates(label, config.extensions.bookdown_references);
                    for candidate in candidates {
                        if let Some(ranges) = symbol_index.crossref_usages(&candidate) {
                            add_locations(&mut locations, &doc_uri, &text, ranges);
                        }
                        if include_declaration
                            && let Some(ranges) =
                                symbol_index.crossref_declaration_value_ranges(&candidate)
                        {
                            add_locations(&mut locations, &doc_uri, &text, ranges);
                        }
                    }
                }
                SymbolTarget::ChunkLabel(label) => {
                    let candidates =
                        crossref_candidates(label, config.extensions.bookdown_references);
                    for candidate in candidates {
                        if let Some(ranges) = symbol_index.crossref_usages(&candidate) {
                            add_locations(&mut locations, &doc_uri, &text, ranges);
                        }
                        if include_declaration
                            && let Some(ranges) = symbol_index.chunk_label_value_ranges(&candidate)
                        {
                            add_locations(&mut locations, &doc_uri, &text, ranges);
                        }
                    }
                }
                SymbolTarget::ExampleLabel(label) => {
                    if let Some(ranges) = symbol_index.example_label_definitions(label)
                        && include_declaration
                    {
                        add_locations(&mut locations, &doc_uri, &text, ranges);
                    }
                }
                SymbolTarget::HeadingLink(label) | SymbolTarget::HeadingId(label) => {
                    let ranges = symbol_index.heading_reference_ranges(label, include_declaration);
                    add_locations(&mut locations, &doc_uri, &text, &ranges);
                }
                SymbolTarget::Citation(key) => {
                    let norm = normalize_label(key);
                    if let Some(ranges) = symbol_index.citation_usages(&norm) {
                        add_locations(&mut locations, &doc_uri, &text, ranges);
                    }
                }
                SymbolTarget::Reference { label, is_footnote } => {
                    let norm = normalize_label(label);
                    if *is_footnote {
                        let mut ranges = symbol_index.footnote_rename_ranges(&norm);
                        if !include_declaration
                            && let Some(definition_ranges) =
                                symbol_index.footnote_definitions(&norm)
                        {
                            ranges.retain(|range| {
                                !definition_ranges.iter().any(|def| {
                                    def.start() <= range.start() && range.end() <= def.end()
                                })
                            });
                        }
                        if !ranges.is_empty() {
                            add_locations(&mut locations, &doc_uri, &text, &ranges);
                        }
                    } else if let Some(ranges) = symbol_index
                        .reference_definition_entries()
                        .find_map(|(id, ranges)| (id == &norm).then_some(ranges))
                    {
                        add_locations(&mut locations, &doc_uri, &text, ranges);
                    }
                }
            }
        }

        if include_declaration {
            let yaml_ok = helpers::is_yaml_frontmatter_valid(&parsed_yaml_regions);
            if yaml_ok {
                let db = salsa_db.lock().await;
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
        && let (SymbolTarget::Citation(key), Some(index)) = (&target, citation_def_index.as_ref())
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

fn crossref_candidates(label: &str, bookdown_references: bool) -> Vec<String> {
    crate::utils::crossref_symbol_labels(&normalize_anchor_label(label), bookdown_references)
}
