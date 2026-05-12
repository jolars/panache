//! Handler for textDocument/completion LSP requests.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;
use crate::syntax::{AstNode, ImageLink, Link, LinkDest, SyntaxNode};
use crate::utils::normalize_anchor_label;

use super::super::conversions::offset_to_position;
use super::super::helpers;
use crate::metadata::inline_reference_map;

/// Extensions accepted by Pandoc/Quarto image syntax `![](…)` across
/// LaTeX, Typst, and HTML output paths. Quarto also embeds short-form
/// video via `![](…)` for HTML output, so common video containers are
/// included alongside still-image formats.
const IMAGE_EXTENSIONS: &[&str] = &[
    // Raster
    "png", "jpg", "jpeg", "gif", "apng", "webp", "avif", "bmp", "tif", "tiff", "heic", "heif",
    "jxl", "ico", // Vector / print
    "svg", "pdf", "eps", "ps", // Video (Quarto/HTML)
    "mp4", "webm", "ogv", "mov", "m4v", "mkv", "avi", "flv", "mpeg", "mpg", "3gp",
];
const MAX_PATH_COMPLETIONS: usize = 250;

pub(crate) async fn completion(
    client: &tower_lsp_server::Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: CompletionParams,
) -> Result<Option<CompletionResponse>> {
    let uri = &params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;
    let config = helpers::get_config(client, &workspace_root, uri).await;

    // SyntaxNode is !Send, so derive everything we need from it inside a
    // sync block and let it drop before we hit any subsequent .await.
    let Some((text, offset, dest_ctx_opt)) = ({
        let Some((text, root)) =
            helpers::get_document_content_and_tree(&document_map, &salsa_db, uri).await
        else {
            return Ok(None);
        };
        let Some(offset) = super::super::conversions::position_to_offset(&text, position) else {
            return Ok(None);
        };
        let dest_ctx_opt = link_dest_context(&root, &text, offset);
        Some((text, offset, dest_ctx_opt))
    }) else {
        return Ok(None);
    };

    // Path-completion branch: cursor inside `[text](…)` or `![alt](…)` destination.
    if let Some(dest_ctx) = dest_ctx_opt {
        let doc_path = {
            let map = document_map.lock().await;
            map.get(&uri.to_string()).and_then(|s| s.path.clone())
        };
        let items = path_completion_items(&dest_ctx, doc_path.as_deref(), &text, offset).await;
        return match items {
            Some(items) if !items.is_empty() => Ok(Some(CompletionResponse::Array(items))),
            _ => Ok(None),
        };
    }

    let Some(query) = citation_query_prefix(&text, offset) else {
        return Ok(None);
    };

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

    let offset_in_frontmatter =
        helpers::is_offset_in_yaml_frontmatter(&parsed_yaml_regions, offset);
    if offset_in_frontmatter {
        return Ok(None);
    }

    let Some(doc_path) = doc_path else {
        return Ok(None);
    };
    let yaml_ok = helpers::is_yaml_frontmatter_valid(&parsed_yaml_regions);
    if !yaml_ok {
        return Ok(None);
    }

    let metadata = {
        let db = salsa_db.lock().await;
        crate::salsa::metadata(&*db, salsa_file, salsa_config, doc_path.clone()).clone()
    };
    let parse = metadata.bibliography_parse.as_ref();
    let symbol_index = {
        let db = salsa_db.lock().await;
        crate::salsa::symbol_usage_index(&*db, salsa_file, salsa_config, doc_path).clone()
    };

    let has_crossref_candidates = symbol_index
        .crossref_declaration_entries()
        .any(|(key, _)| is_supported_crossref_completion_key(key));
    if parse.is_none() && metadata.inline_references.is_empty() && !has_crossref_candidates {
        return Ok(None);
    }

    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    if let Some(parse) = parse {
        for entry in parse.index.entries() {
            if !seen.insert(entry.key.to_lowercase()) {
                continue;
            }
            if !matches_query(&entry.key, &query) {
                continue;
            }
            items.push(CompletionItem {
                label: entry.key.clone(),
                kind: Some(CompletionItemKind::REFERENCE),
                insert_text: Some(entry.key.clone()),
                insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                ..Default::default()
            });
        }
    }
    for (key, entries) in inline_reference_map(&metadata.inline_references) {
        if entries.is_empty() || !seen.insert(key.clone()) {
            continue;
        }
        let label = entries[0].id.clone();
        if !matches_query(&label, &query) {
            continue;
        }
        items.push(CompletionItem {
            label: label.clone(),
            kind: Some(CompletionItemKind::REFERENCE),
            insert_text: Some(label),
            insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
            ..Default::default()
        });
    }

    if config.extensions.quarto_crossrefs || config.extensions.bookdown_references {
        for (label, _) in symbol_index.crossref_declaration_entries() {
            if !is_supported_crossref_completion_key(label) {
                continue;
            }
            let display = normalize_anchor_label(label);
            if display.is_empty() || !seen.insert(display.to_lowercase()) {
                continue;
            }
            if !matches_query(&display, &query) {
                continue;
            }
            items.push(CompletionItem {
                label: display.clone(),
                kind: Some(CompletionItemKind::REFERENCE),
                insert_text: Some(display),
                insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                ..Default::default()
            });
        }
    }

    if items.is_empty() {
        return Ok(None);
    }

    Ok(Some(CompletionResponse::Array(items)))
}

fn citation_query_prefix(text: &str, offset: usize) -> Option<String> {
    let start = offset.saturating_sub(8);
    let snippet = &text[start..offset];
    let at = snippet.rfind('@')?;
    let query = &snippet[at + 1..];
    if query
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':' | '.'))
    {
        Some(query.to_string())
    } else {
        None
    }
}

fn matches_query(candidate: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    candidate
        .to_ascii_lowercase()
        .starts_with(&query.to_ascii_lowercase())
}

fn is_supported_crossref_completion_key(key: &str) -> bool {
    panache_parser::parser::inlines::citations::is_quarto_crossref_key(key)
        || panache_parser::parser::inlines::citations::has_bookdown_prefix(key)
}

/// Context for a cursor that sits inside an inline link or image destination.
struct LinkDestContext {
    is_image: bool,
    /// Text typed inside the destination, from the start of the URL up to the cursor.
    prefix: String,
}

fn link_dest_context(root: &SyntaxNode, text: &str, offset: usize) -> Option<LinkDestContext> {
    let starting = helpers::find_node_at_offset(root, offset)?;
    for ancestor in starting.ancestors() {
        let (is_image, dest): (bool, LinkDest) = if let Some(link) = Link::cast(ancestor.clone()) {
            (false, link.dest()?)
        } else if let Some(image) = ImageLink::cast(ancestor.clone()) {
            (true, image.dest()?)
        } else {
            continue;
        };
        let range = dest.syntax().text_range();
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        if offset < start || offset > end {
            return None;
        }
        let prefix = text.get(start..offset)?.to_string();
        // Bail if the prefix isn't plausibly part of an inline URL (e.g. cursor
        // has crossed into the title portion `[text](url "title")`).
        if prefix.chars().any(|c| c.is_whitespace() || c == '"') {
            return None;
        }
        return Some(LinkDestContext { is_image, prefix });
    }
    None
}

async fn path_completion_items(
    ctx: &LinkDestContext,
    doc_path: Option<&Path>,
    text: &str,
    offset: usize,
) -> Option<Vec<CompletionItem>> {
    let doc_path = doc_path?;
    let doc_dir = doc_path.parent()?.to_path_buf();

    // Absolute paths, fragment-only anchors, and URLs are out of scope.
    let first = ctx.prefix.chars().next();
    if matches!(first, Some('/') | Some('#') | Some('<')) || ctx.prefix.contains("://") {
        return None;
    }

    let (subdir, name_prefix) = match ctx.prefix.rsplit_once('/') {
        Some((before, after)) => (before.to_string(), after.to_string()),
        None => (String::new(), ctx.prefix.clone()),
    };
    let target_dir = doc_dir.join(&subdir);
    let is_image = ctx.is_image;
    let name_prefix_owned = name_prefix.clone();

    let entries = tokio::task::spawn_blocking(move || {
        read_dir_entries(&target_dir, &name_prefix_owned, is_image)
    })
    .await
    .ok()?;

    let name_start = offset.saturating_sub(name_prefix.len());
    let range = Range::new(
        offset_to_position(text, name_start),
        offset_to_position(text, offset),
    );

    let mut items: Vec<CompletionItem> = entries
        .into_iter()
        .map(|entry| {
            let new_text = if entry.is_dir {
                format!("{}/", entry.name)
            } else {
                entry.name.clone()
            };
            CompletionItem {
                label: new_text.clone(),
                kind: Some(if entry.is_dir {
                    CompletionItemKind::FOLDER
                } else {
                    CompletionItemKind::FILE
                }),
                filter_text: Some(new_text.clone()),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit { range, new_text })),
                insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                ..Default::default()
            }
        })
        .collect();

    items.truncate(MAX_PATH_COMPLETIONS);
    Some(items)
}

struct PathEntry {
    name: String,
    is_dir: bool,
}

fn read_dir_entries(dir: &Path, name_prefix: &str, is_image_context: bool) -> Vec<PathEntry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let prefix_lower = name_prefix.to_ascii_lowercase();
    let include_hidden = name_prefix.starts_with('.');

    let mut dirs: Vec<PathEntry> = Vec::new();
    let mut files: Vec<PathEntry> = Vec::new();

    for entry in read.flatten() {
        let name_os = entry.file_name();
        let Some(name) = name_os.to_str() else {
            continue;
        };
        if !include_hidden && name.starts_with('.') {
            continue;
        }
        if !name.to_ascii_lowercase().starts_with(&prefix_lower) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            dirs.push(PathEntry {
                name: name.to_string(),
                is_dir: true,
            });
        } else if metadata.is_file() {
            if is_image_context && !has_image_extension(name) {
                continue;
            }
            files.push(PathEntry {
                name: name.to_string(),
                is_dir: false,
            });
        }
    }

    let cmp = |a: &PathEntry, b: &PathEntry| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    };
    dirs.sort_by(cmp);
    files.sort_by(cmp);
    dirs.into_iter().chain(files).collect()
}

fn has_image_extension(name: &str) -> bool {
    let Some(dot) = name.rfind('.') else {
        return false;
    };
    let ext = name[dot + 1..].to_ascii_lowercase();
    IMAGE_EXTENSIONS.contains(&ext.as_str())
}
