//! Handler for textDocument/completion LSP requests.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;
use crate::syntax::{AstNode, ImageLink, Link, LinkDest, Shortcode, SyntaxKind, SyntaxNode};
use crate::utils::normalize_anchor_label;

use super::super::conversions::offset_to_position;
use super::super::helpers;
use super::shortcode_args::{shortcode_token_value_span, shortcode_tokens, token_is_named};
use crate::metadata::inline_reference_map;

/// Common still-image extensions accepted by Pandoc/Quarto image syntax
/// `![](…)` across LaTeX, Typst, and HTML output paths.
const STILL_IMAGE_EXTENSIONS: &[&str] = &[
    // Raster
    "png", "jpg", "jpeg", "gif", "apng", "webp", "avif", "bmp", "tif", "tiff", "heic", "heif",
    "jxl", "ico", // Vector / print
    "svg", "pdf", "eps", "ps",
];

/// Video container extensions that Quarto can embed directly via `![](…)`
/// or `{{< video >}}`.
const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "webm", "ogv", "mov", "m4v", "mkv", "avi", "flv", "mpeg", "mpg", "3gp",
];

/// Extensions accepted by `{{< include >}}` — markdown documents plus
/// script files that the user typically pulls into code chunks.
const INCLUDE_EXTENSIONS: &[&str] = &[
    "qmd",
    "md",
    "markdown",
    "rmd",
    "rmarkdown",
    "ipynb",
    "r",
    "py",
    "jl",
];

/// Extensions accepted by `{{< embed >}}` — Jupyter notebooks and Quarto
/// source documents.
const EMBED_EXTENSIONS: &[&str] = &["ipynb", "qmd"];

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
    let Some((text, offset, link_ctx_opt, shortcode_ctx_opt)) = ({
        let Some((text, root)) =
            helpers::get_document_content_and_tree(&document_map, &salsa_db, uri).await
        else {
            return Ok(None);
        };
        let Some(offset) = super::super::conversions::position_to_offset(&text, position) else {
            return Ok(None);
        };
        let link_ctx_opt = link_dest_context(&root, &text, offset);
        let shortcode_ctx_opt = if link_ctx_opt.is_none() && config.extensions.quarto_shortcodes {
            shortcode_arg_context(&root, &text, offset)
        } else {
            None
        };
        Some((text, offset, link_ctx_opt, shortcode_ctx_opt))
    }) else {
        return Ok(None);
    };

    // Path-completion branches: cursor inside `[text](…)` / `![alt](…)` destination,
    // or inside a path-bearing Quarto shortcode argument.
    if link_ctx_opt.is_some() || shortcode_ctx_opt.is_some() {
        let (doc_path, ws_root) = {
            let map = document_map.lock().await;
            let doc = map.get(&uri.to_string()).and_then(|s| s.path.clone());
            let ws = workspace_root.lock().await.clone();
            (doc, ws)
        };

        let request = if let Some(ctx) = link_ctx_opt {
            link_dest_path_request(&ctx, doc_path.as_deref())
        } else if let Some(ctx) = shortcode_ctx_opt {
            shortcode_path_request(&ctx, doc_path.as_deref(), ws_root.as_deref())
        } else {
            None
        };

        return match request {
            Some(req) => match path_completion_items(req, &text, offset).await {
                Some(items) if !items.is_empty() => Ok(Some(CompletionResponse::Array(items))),
                _ => Ok(None),
            },
            None => Ok(None),
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

/// Context for a cursor that sits inside the path argument of a
/// `{{< include | embed | video | placeholder >}}` shortcode.
struct ShortcodeArgContext {
    kind: ShortcodeKind,
    /// Value typed in the path argument so far (after `key=` and quote stripping).
    prefix: String,
}

#[derive(Clone, Copy)]
enum ShortcodeKind {
    Include,
    Embed,
    Video,
    Placeholder,
}

impl ShortcodeKind {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "include" => Some(Self::Include),
            "embed" => Some(Self::Embed),
            "video" => Some(Self::Video),
            "placeholder" => Some(Self::Placeholder),
            _ => None,
        }
    }

    fn accepts(self, file_name: &str) -> bool {
        let exts: &[&str] = match self {
            Self::Include => INCLUDE_EXTENSIONS,
            Self::Embed => EMBED_EXTENSIONS,
            Self::Video => VIDEO_EXTENSIONS,
            Self::Placeholder => STILL_IMAGE_EXTENSIONS,
        };
        has_extension(file_name, exts)
    }
}

fn shortcode_arg_context(
    root: &SyntaxNode,
    text: &str,
    offset: usize,
) -> Option<ShortcodeArgContext> {
    let starting = helpers::find_node_at_offset(root, offset)?;
    let shortcode = starting
        .ancestors()
        .find_map(|ancestor| Shortcode::cast(ancestor.clone()))?;

    let content_node = shortcode
        .syntax()
        .children()
        .find(|child| child.kind() == SyntaxKind::SHORTCODE_CONTENT)?;
    let content_range = content_node.text_range();
    let content_start: usize = content_range.start().into();
    let content_end: usize = content_range.end().into();
    if offset < content_start || offset > content_end {
        return None;
    }
    let content_text = text.get(content_start..content_end)?;
    let rel = offset - content_start;

    let tokens = shortcode_tokens(content_text);
    if tokens.is_empty() {
        return None;
    }

    // First positional token is the shortcode name. Determine the active
    // token, treating whitespace/end-of-content as an empty next arg.
    let name_token = tokens.first().copied()?;
    let name_value_span = shortcode_token_value_span(content_text, name_token)?;
    let name = content_text
        .get(name_value_span.0..name_value_span.1)?
        .to_string();
    let kind = ShortcodeKind::from_name(&name)?;

    // Locate the token whose range contains `rel`, if any.
    let active_idx = tokens.iter().position(|&(s, e)| rel >= s && rel <= e);
    let (active_token, active_idx) = match active_idx {
        Some(0) => {
            // Cursor sits on the shortcode name itself; don't complete the name in v1.
            return None;
        }
        Some(i) => (tokens[i], i),
        None => {
            // Cursor is in whitespace or past the last token: start a new arg.
            (
                (rel.min(content_text.len()), rel.min(content_text.len())),
                tokens.len(),
            )
        }
    };

    // Skip named args (`key=value`) — v1 handles positional args only.
    if active_idx > 0 && token_is_named(content_text, active_token) {
        return None;
    }

    // Count positional tokens (no `=`) up to and including the active one.
    let positional_index = tokens
        .iter()
        .take(active_idx)
        .filter(|&&t| !token_is_named(content_text, t))
        .count();
    // Positional arg #1 (after the name at #0) is the path arg for all
    // four supported shortcodes; bail if the cursor is on a later
    // positional arg.
    if positional_index != 1 {
        return None;
    }

    // Reduce the active token to its value span (strips `key=` and surrounding quotes).
    let value_span = if active_token.0 == active_token.1 {
        active_token
    } else {
        shortcode_token_value_span(content_text, active_token)?
    };
    if rel < value_span.0 || rel > value_span.1 {
        return None;
    }

    let prefix = content_text.get(value_span.0..rel)?.to_string();

    // Out-of-scope prefixes: URLs and (for embed) the cell-id portion after `#`.
    if prefix.contains("://") {
        return None;
    }
    if matches!(kind, ShortcodeKind::Embed) && prefix.contains('#') {
        return None;
    }

    Some(ShortcodeArgContext { kind, prefix })
}

/// A normalized request to enumerate filesystem entries.
struct PathRequest {
    /// Directory whose entries to enumerate (already absolute).
    base_dir: PathBuf,
    /// Path prefix relative to `base_dir` (may contain `/` separators).
    effective_prefix: String,
    /// Decides whether a file entry should appear in the completion list.
    /// Directories always appear regardless.
    accept_file: Box<dyn Fn(&str) -> bool + Send + 'static>,
}

fn link_dest_path_request(ctx: &LinkDestContext, doc_path: Option<&Path>) -> Option<PathRequest> {
    let doc_dir = doc_path?.parent()?.to_path_buf();
    let first = ctx.prefix.chars().next();
    // Absolute paths, fragment-only anchors, and URLs are out of scope.
    if matches!(first, Some('/') | Some('#') | Some('<')) || ctx.prefix.contains("://") {
        return None;
    }
    let is_image = ctx.is_image;
    Some(PathRequest {
        base_dir: doc_dir,
        effective_prefix: ctx.prefix.clone(),
        accept_file: Box::new(move |name| {
            if is_image {
                has_extension(name, STILL_IMAGE_EXTENSIONS) || has_extension(name, VIDEO_EXTENSIONS)
            } else {
                true
            }
        }),
    })
}

fn shortcode_path_request(
    ctx: &ShortcodeArgContext,
    doc_path: Option<&Path>,
    workspace_root: Option<&Path>,
) -> Option<PathRequest> {
    let kind = ctx.kind;
    let (base_dir, effective_prefix) = if let Some(stripped) = ctx.prefix.strip_prefix('/') {
        (workspace_root?.to_path_buf(), stripped.to_string())
    } else {
        (doc_path?.parent()?.to_path_buf(), ctx.prefix.clone())
    };
    Some(PathRequest {
        base_dir,
        effective_prefix,
        accept_file: Box::new(move |name| kind.accepts(name)),
    })
}

async fn path_completion_items(
    req: PathRequest,
    text: &str,
    offset: usize,
) -> Option<Vec<CompletionItem>> {
    let (subdir, name_prefix) = match req.effective_prefix.rsplit_once('/') {
        Some((before, after)) => (before.to_string(), after.to_string()),
        None => (String::new(), req.effective_prefix.clone()),
    };
    let target_dir = req.base_dir.join(&subdir);
    let name_prefix_owned = name_prefix.clone();
    let accept = req.accept_file;

    let entries = tokio::task::spawn_blocking(move || {
        read_dir_entries(&target_dir, &name_prefix_owned, accept.as_ref())
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

fn read_dir_entries(
    dir: &Path,
    name_prefix: &str,
    accept_file: &(dyn Fn(&str) -> bool + Send + 'static),
) -> Vec<PathEntry> {
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
            if !accept_file(name) {
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

fn has_extension(name: &str, allowed: &[&str]) -> bool {
    let Some(dot) = name.rfind('.') else {
        return false;
    };
    let ext = name[dot + 1..].to_ascii_lowercase();
    allowed.contains(&ext.as_str())
}
