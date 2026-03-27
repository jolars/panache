use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rowan::TextSize;
use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{Range, RenameFilesParams, TextEdit, Uri, WorkspaceEdit};

use crate::lsp::DocumentState;
use crate::lsp::conversions::offset_to_position;
use crate::syntax::{AstNode, ImageLink, Link, Shortcode, SyntaxKind};

use super::document_links::{extract_first_destination_token, resolve_link_target};

pub(crate) async fn will_rename_files(
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: RenameFilesParams,
) -> Result<Option<WorkspaceEdit>> {
    let root = workspace_root.lock().await.clone();
    let docs = candidate_documents_for_scan(&root, &document_map, &salsa_db).await;

    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    for rename in &params.files {
        let Some(old_uri) = parse_file_uri(&rename.old_uri) else {
            continue;
        };
        let Some(new_uri) = parse_file_uri(&rename.new_uri) else {
            continue;
        };
        for candidate in rename_candidates_for_pair(&docs, &old_uri, &new_uri) {
            changes
                .entry(candidate.uri)
                .or_default()
                .push(candidate.edit);
        }
    }

    for edits in changes.values_mut() {
        edits.sort_by(|a, b| {
            a.range
                .start
                .line
                .cmp(&b.range.start.line)
                .then(a.range.start.character.cmp(&b.range.start.character))
                .then(a.range.end.line.cmp(&b.range.end.line))
                .then(a.range.end.character.cmp(&b.range.end.character))
                .then(a.new_text.cmp(&b.new_text))
        });
        edits.dedup_by(|a, b| a.range == b.range && a.new_text == b.new_text);
    }

    log::debug!(
        "lsp.willRenameFiles: renames={} docs_scanned={} candidate_edits={}",
        params.files.len(),
        docs.len(),
        changes.values().map(|edits| edits.len()).sum::<usize>()
    );

    if changes.is_empty() {
        return Ok(None);
    }

    Ok(Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    }))
}

#[derive(Clone)]
struct DocInput {
    uri: Uri,
    path: PathBuf,
    text: String,
}

#[derive(Clone)]
struct CandidateEdit {
    uri: Uri,
    edit: TextEdit,
}

async fn candidate_documents_for_scan(
    workspace_root: &Option<PathBuf>,
    document_map: &Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: &Arc<Mutex<crate::salsa::SalsaDb>>,
) -> Vec<DocInput> {
    let mut by_path: HashMap<PathBuf, DocInput> = HashMap::new();
    if let Some(root) = workspace_root {
        let has_quarto = root.join("_quarto.yml").exists();
        let has_bookdown = root.join("_bookdown.yml").exists();

        let candidate_paths = if has_quarto || has_bookdown {
            let cfg = match crate::config::load(None, root, None) {
                Ok((cfg, _)) => cfg,
                Err(_) => crate::Config::default(),
            };
            crate::includes::find_project_documents(root, &cfg, has_bookdown)
        } else {
            discover_standalone_workspace_documents(root)
        };

        for path in candidate_paths {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some(uri) = Uri::from_file_path(&path) else {
                continue;
            };
            by_path.insert(path.clone(), DocInput { uri, path, text });
        }

        if has_quarto {
            let quarto_path = root.join("_quarto.yml");
            if let Ok(text) = std::fs::read_to_string(&quarto_path)
                && let Some(uri) = Uri::from_file_path(&quarto_path)
            {
                by_path.insert(
                    quarto_path.clone(),
                    DocInput {
                        uri,
                        path: quarto_path,
                        text,
                    },
                );
            }
        }
    }

    let states = {
        let docs = document_map.lock().await;
        docs.values().cloned().collect::<Vec<_>>()
    };
    let db = salsa_db.lock().await;
    for state in states {
        let Some(path) = state.path.clone() else {
            continue;
        };
        let text = state.salsa_file.text(&*db).clone();
        let Some(uri) = Uri::from_file_path(&path) else {
            continue;
        };
        by_path.insert(path.clone(), DocInput { uri, path, text });
    }

    let mut docs = by_path.into_values().collect::<Vec<_>>();
    docs.sort_by(|a, b| a.path.cmp(&b.path));
    docs
}

fn discover_standalone_workspace_documents(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if matches!(name, ".git" | "target" | "node_modules") || name.starts_with('.') {
                    continue;
                }
                dirs.push(path);
                continue;
            }
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if crate::all_document_extensions().contains(&ext) {
                out.push(path);
            }
        }
    }
    out
}

fn rename_candidates_for_pair(
    docs: &[DocInput],
    old_uri: &Uri,
    new_uri: &Uri,
) -> Vec<CandidateEdit> {
    let mut out = Vec::new();
    for doc in docs {
        if is_quarto_project_config(&doc.path) {
            out.extend(rename_candidates_for_quarto_config_yaml(
                doc, old_uri, new_uri,
            ));
        } else {
            let tree = crate::parse(&doc.text, None);
            out.extend(rename_candidates_for_links(doc, &tree, old_uri, new_uri));
        }
    }
    out.sort_by(|a, b| {
        a.uri
            .as_str()
            .cmp(b.uri.as_str())
            .then(a.edit.range.start.line.cmp(&b.edit.range.start.line))
            .then(
                a.edit
                    .range
                    .start
                    .character
                    .cmp(&b.edit.range.start.character),
            )
            .then(a.edit.range.end.line.cmp(&b.edit.range.end.line))
            .then(a.edit.range.end.character.cmp(&b.edit.range.end.character))
            .then(a.edit.new_text.cmp(&b.edit.new_text))
    });
    out.dedup_by(|a, b| a.uri == b.uri && a.edit == b.edit);
    out
}

fn is_quarto_project_config(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "_quarto.yml")
}

fn rename_candidates_for_links(
    doc: &DocInput,
    tree: &crate::syntax::SyntaxNode,
    old_uri: &Uri,
    new_uri: &Uri,
) -> Vec<CandidateEdit> {
    let mut out = Vec::new();
    for node in tree.descendants() {
        if let Some(link) = Link::cast(node.clone()) {
            if let Some(dest) = link.dest() {
                let raw = dest.url();
                if let Some(edit) = candidate_edit_for_destination(
                    &doc.path,
                    &doc.text,
                    old_uri,
                    new_uri,
                    dest.syntax().text_range(),
                    &raw,
                ) {
                    out.push(CandidateEdit {
                        uri: doc.uri.clone(),
                        edit,
                    });
                }
            }
            continue;
        }

        if let Some(image) = ImageLink::cast(node.clone())
            && let Some(dest) = image.dest()
        {
            let raw = dest.url();
            if let Some(edit) = candidate_edit_for_destination(
                &doc.path,
                &doc.text,
                old_uri,
                new_uri,
                dest.syntax().text_range(),
                &raw,
            ) {
                out.push(CandidateEdit {
                    uri: doc.uri.clone(),
                    edit,
                });
            }
        }

        if let Some(shortcode) = Shortcode::cast(node)
            && let Some(edit) = candidate_edit_for_shortcode_path(doc, &shortcode, old_uri, new_uri)
        {
            out.push(CandidateEdit {
                uri: doc.uri.clone(),
                edit,
            });
        }
    }
    out
}

fn candidate_edit_for_shortcode_path(
    doc: &DocInput,
    shortcode: &Shortcode,
    old_uri: &Uri,
    new_uri: &Uri,
) -> Option<TextEdit> {
    if shortcode.is_escaped() {
        return None;
    }
    if shortcode.name().as_deref() != Some("include") {
        return None;
    }

    let content_node = shortcode
        .syntax()
        .children()
        .find(|child| child.kind() == SyntaxKind::SHORTCODE_CONTENT)?;
    let content = content_node.text().to_string();
    let (value_start, value_end) = extract_shortcode_path_argument_span(&content)?;
    let raw_target = content.get(value_start..value_end)?;

    let absolute_start = content_node.text_range().start() + TextSize::from(value_start as u32);
    let absolute_end = content_node.text_range().start() + TextSize::from(value_end as u32);
    let range = rowan::TextRange::new(absolute_start, absolute_end);

    candidate_edit_for_destination(&doc.path, &doc.text, old_uri, new_uri, range, raw_target)
}

fn rename_candidates_for_quarto_config_yaml(
    doc: &DocInput,
    old_uri: &Uri,
    new_uri: &Uri,
) -> Vec<CandidateEdit> {
    let mut out = Vec::new();
    let mut website_indent: Option<usize> = None;
    let mut navbar_indent: Option<usize> = None;
    let mut navbar_list_parent_indent: Option<usize> = None;
    let mut book_indent: Option<usize> = None;
    let mut book_list_parent_indent: Option<usize> = None;
    let mut bibliography_list_indent: Option<usize> = None;
    let mut offset = 0usize;

    for raw_line in doc.text.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let line_indent = line
            .chars()
            .take_while(|ch| *ch == ' ' || *ch == '\t')
            .count();
        let trimmed = line.trim_start();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            offset += raw_line.len();
            continue;
        }

        if let Some(level) = navbar_list_parent_indent
            && line_indent <= level
        {
            navbar_list_parent_indent = None;
        }
        if let Some(level) = book_list_parent_indent
            && line_indent <= level
        {
            book_list_parent_indent = None;
        }
        if let Some(level) = bibliography_list_indent
            && line_indent <= level
        {
            bibliography_list_indent = None;
        }
        if let Some(level) = navbar_indent
            && line_indent <= level
            && !trimmed.starts_with("navbar:")
        {
            navbar_indent = None;
            navbar_list_parent_indent = None;
        }
        if let Some(level) = book_indent
            && line_indent <= level
            && !trimmed.starts_with("book:")
        {
            book_indent = None;
            book_list_parent_indent = None;
        }
        if let Some(level) = website_indent
            && line_indent <= level
            && !trimmed.starts_with("website:")
        {
            website_indent = None;
            navbar_indent = None;
            navbar_list_parent_indent = None;
        }

        if let Some((key, value_start)) = yaml_key_value(trimmed) {
            match key {
                "website" => {
                    website_indent = Some(line_indent);
                    navbar_indent = None;
                    navbar_list_parent_indent = None;
                }
                "book" => {
                    book_indent = Some(line_indent);
                    book_list_parent_indent = None;
                }
                "navbar" if website_indent.is_some_and(|level| line_indent > level) => {
                    navbar_indent = Some(line_indent);
                    navbar_list_parent_indent = None;
                }
                "left" | "right" | "center"
                    if navbar_indent.is_some_and(|level| line_indent > level) =>
                {
                    navbar_list_parent_indent = Some(line_indent);
                }
                "chapters" | "appendices"
                    if book_indent.is_some_and(|level| line_indent > level) =>
                {
                    book_list_parent_indent = Some(line_indent);
                }
                "bibliography" => {
                    if let Some((line_start, line_end)) =
                        yaml_scalar_value_span(trimmed, value_start)
                    {
                        let absolute_start = offset + (line.len() - trimmed.len()) + line_start;
                        let absolute_end = offset + (line.len() - trimmed.len()) + line_end;
                        let range = rowan::TextRange::new(
                            TextSize::from(absolute_start as u32),
                            TextSize::from(absolute_end as u32),
                        );
                        let raw = &doc.text[absolute_start..absolute_end];
                        if let Some(edit) = candidate_edit_for_destination(
                            &doc.path, &doc.text, old_uri, new_uri, range, raw,
                        ) {
                            out.push(CandidateEdit {
                                uri: doc.uri.clone(),
                                edit,
                            });
                        }
                    } else {
                        bibliography_list_indent = Some(line_indent);
                    }
                }
                "href" if navbar_indent.is_some_and(|level| line_indent > level) => {
                    if let Some((line_start, line_end)) =
                        yaml_scalar_value_span(trimmed, value_start)
                    {
                        let absolute_start = offset + (line.len() - trimmed.len()) + line_start;
                        let absolute_end = offset + (line.len() - trimmed.len()) + line_end;
                        let range = rowan::TextRange::new(
                            TextSize::from(absolute_start as u32),
                            TextSize::from(absolute_end as u32),
                        );
                        let raw = &doc.text[absolute_start..absolute_end];
                        if let Some(edit) = candidate_edit_for_destination(
                            &doc.path, &doc.text, old_uri, new_uri, range, raw,
                        ) {
                            out.push(CandidateEdit {
                                uri: doc.uri.clone(),
                                edit,
                            });
                        }
                    }
                }
                "part" if book_indent.is_some_and(|level| line_indent > level) => {
                    if let Some((line_start, line_end)) =
                        yaml_scalar_value_span(trimmed, value_start)
                    {
                        let absolute_start = offset + (line.len() - trimmed.len()) + line_start;
                        let absolute_end = offset + (line.len() - trimmed.len()) + line_end;
                        let range = rowan::TextRange::new(
                            TextSize::from(absolute_start as u32),
                            TextSize::from(absolute_end as u32),
                        );
                        let raw = &doc.text[absolute_start..absolute_end];
                        if let Some(edit) = candidate_edit_for_destination(
                            &doc.path, &doc.text, old_uri, new_uri, range, raw,
                        ) {
                            out.push(CandidateEdit {
                                uri: doc.uri.clone(),
                                edit,
                            });
                        }
                    }
                }
                _ => {}
            }
        } else if navbar_list_parent_indent.is_some_and(|level| line_indent > level)
            && let Some(after_dash) = trimmed.strip_prefix("- ")
            && let Some((rel_start, rel_end)) = yaml_scalar_value_span(after_dash, 0)
        {
            let value = &after_dash[rel_start..rel_end];
            if value.contains(':')
                || matches!(value, "true" | "false" | "null" | "~")
                || value.ends_with(':')
            {
                offset += raw_line.len();
                continue;
            }
            let absolute_start = offset + (line.len() - trimmed.len()) + 2 + rel_start;
            let absolute_end = offset + (line.len() - trimmed.len()) + 2 + rel_end;
            let range = rowan::TextRange::new(
                TextSize::from(absolute_start as u32),
                TextSize::from(absolute_end as u32),
            );
            let raw = &doc.text[absolute_start..absolute_end];
            if let Some(edit) =
                candidate_edit_for_destination(&doc.path, &doc.text, old_uri, new_uri, range, raw)
            {
                out.push(CandidateEdit {
                    uri: doc.uri.clone(),
                    edit,
                });
            }
        } else if book_list_parent_indent.is_some_and(|level| line_indent > level)
            && let Some(after_dash) = trimmed.strip_prefix("- ")
            && let Some((rel_start, rel_end)) = yaml_scalar_value_span(after_dash, 0)
        {
            let value = &after_dash[rel_start..rel_end];
            if value.contains(':')
                || matches!(value, "true" | "false" | "null" | "~")
                || value.ends_with(':')
            {
                offset += raw_line.len();
                continue;
            }
            let absolute_start = offset + (line.len() - trimmed.len()) + 2 + rel_start;
            let absolute_end = offset + (line.len() - trimmed.len()) + 2 + rel_end;
            let range = rowan::TextRange::new(
                TextSize::from(absolute_start as u32),
                TextSize::from(absolute_end as u32),
            );
            let raw = &doc.text[absolute_start..absolute_end];
            if let Some(edit) =
                candidate_edit_for_destination(&doc.path, &doc.text, old_uri, new_uri, range, raw)
            {
                out.push(CandidateEdit {
                    uri: doc.uri.clone(),
                    edit,
                });
            }
        } else if bibliography_list_indent.is_some_and(|level| line_indent > level)
            && let Some(after_dash) = trimmed.strip_prefix("- ")
            && let Some((rel_start, rel_end)) = yaml_scalar_value_span(after_dash, 0)
        {
            let value = &after_dash[rel_start..rel_end];
            if value.contains(':')
                || matches!(value, "true" | "false" | "null" | "~")
                || value.ends_with(':')
            {
                offset += raw_line.len();
                continue;
            }
            let absolute_start = offset + (line.len() - trimmed.len()) + 2 + rel_start;
            let absolute_end = offset + (line.len() - trimmed.len()) + 2 + rel_end;
            let range = rowan::TextRange::new(
                TextSize::from(absolute_start as u32),
                TextSize::from(absolute_end as u32),
            );
            let raw = &doc.text[absolute_start..absolute_end];
            if let Some(edit) =
                candidate_edit_for_destination(&doc.path, &doc.text, old_uri, new_uri, range, raw)
            {
                out.push(CandidateEdit {
                    uri: doc.uri.clone(),
                    edit,
                });
            }
        }

        offset += raw_line.len();
    }

    out
}

fn yaml_key_value(trimmed_line: &str) -> Option<(&str, usize)> {
    let (key_part, value_part) = trimmed_line.split_once(':')?;
    let key = key_part.trim().trim_start_matches("- ").trim();
    let value_start = trimmed_line.len() - value_part.len();
    Some((key, value_start))
}

fn yaml_scalar_value_span(line: &str, value_start: usize) -> Option<(usize, usize)> {
    let value = line.get(value_start..)?;
    let leading_trimmed = value.trim_start();
    if leading_trimmed.is_empty() {
        return None;
    }
    let leading_ws = value.len() - leading_trimmed.len();
    let start = value_start + leading_ws;

    if leading_trimmed.starts_with('"') || leading_trimmed.starts_with('\'') {
        let quote = leading_trimmed.chars().next()?;
        let mut i = quote.len_utf8();
        while i < leading_trimmed.len() {
            let ch = leading_trimmed[i..].chars().next()?;
            if ch == quote {
                return Some((start + quote.len_utf8(), start + i));
            }
            i += ch.len_utf8();
        }
        return None;
    }

    let raw_end = leading_trimmed.find(" #").unwrap_or(leading_trimmed.len());
    let end = start + leading_trimmed[..raw_end].trim_end().len();
    (end > start).then_some((start, end))
}

fn extract_shortcode_path_argument_span(content: &str) -> Option<(usize, usize)> {
    let tokens = shortcode_tokens(content);
    let first = tokens.first()?;
    let name = content.get(first.0..first.1)?.trim();
    if !name.eq_ignore_ascii_case("include") {
        return None;
    }

    // Preferred: `{{< include path.qmd >}}` (second positional argument).
    if let Some(token) = tokens
        .iter()
        .skip(1)
        .find(|(start, end)| !content[*start..*end].contains('='))
        && let Some(span) = shortcode_token_value_span(content, *token)
    {
        return Some(span);
    }

    // Also support `{{< include file=path.qmd >}}` or `path=...`.
    for token in tokens.iter().skip(1) {
        let raw = &content[token.0..token.1];
        let Some(eq_idx) = raw.find('=') else {
            continue;
        };
        let key = raw[..eq_idx].trim();
        if !matches!(key, "file" | "path") {
            continue;
        }
        if let Some(span) = shortcode_token_value_span(content, *token) {
            return Some(span);
        }
    }

    None
}

fn shortcode_tokens(content: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    let mut in_quotes = false;
    let mut quote_char = '\0';

    for (idx, ch) in content.char_indices() {
        if in_quotes {
            if ch == quote_char {
                in_quotes = false;
            }
            continue;
        }

        if ch == '"' || ch == '\'' {
            if start.is_none() {
                start = Some(idx);
            }
            in_quotes = true;
            quote_char = ch;
            continue;
        }

        if ch.is_whitespace() {
            if let Some(s) = start.take() {
                out.push((s, idx));
            }
            continue;
        }

        if start.is_none() {
            start = Some(idx);
        }
    }

    if let Some(s) = start {
        out.push((s, content.len()));
    }
    out
}

fn shortcode_token_value_span(content: &str, token: (usize, usize)) -> Option<(usize, usize)> {
    let raw = content.get(token.0..token.1)?;
    let value = if let Some(eq_idx) = raw.find('=') {
        let after_eq = token.0 + eq_idx + 1;
        (after_eq, token.1)
    } else {
        (token.0, token.1)
    };

    let start_char = content.get(value.0..value.0 + 1)?;
    let end_char = content.get(value.1.saturating_sub(1)..value.1)?;
    if (start_char == "\"" && end_char == "\"") || (start_char == "'" && end_char == "'") {
        if value.1 <= value.0 + 1 {
            return None;
        }
        Some((value.0 + 1, value.1 - 1))
    } else {
        Some(value)
    }
}

fn candidate_edit_for_destination(
    doc_path: &Path,
    doc_text: &str,
    old_uri: &Uri,
    new_uri: &Uri,
    range: rowan::TextRange,
    raw_destination: &str,
) -> Option<TextEdit> {
    let raw_target = extract_first_destination_token(raw_destination);
    if raw_target.is_empty() {
        return None;
    }
    let resolved = resolve_link_target(raw_target, Some(doc_path), None)?;
    let resolved_path = resolved.to_file_path()?;
    let old_path = old_uri.to_file_path()?;
    if resolved_path.as_ref() != old_path.as_ref() {
        return None;
    }

    let new_path = new_uri.to_file_path()?;
    let replacement = rewrite_destination_target(doc_path, raw_target, &old_path, &new_path)?;
    if replacement == raw_target {
        return None;
    }

    let start = offset_to_position(doc_text, range.start().into());
    let end = offset_to_position(doc_text, range.end().into());

    let replaced_full = raw_destination.replacen(raw_target, &replacement, 1);
    Some(TextEdit {
        range: Range { start, end },
        new_text: replaced_full,
    })
}

fn rewrite_destination_target(
    doc_path: &Path,
    raw_target: &str,
    old_path: &Path,
    new_path: &Path,
) -> Option<String> {
    if is_external_target(raw_target) || raw_target.starts_with('#') {
        return None;
    }
    let (path_part, fragment) = split_fragment(raw_target);
    let rebuilt = if Path::new(path_part).is_absolute() {
        new_path.to_string_lossy().to_string()
    } else {
        let base = doc_path.parent().unwrap_or_else(|| Path::new("."));
        relative_path_from(base, new_path)?
    };
    let with_fragment = if let Some(fragment) = fragment {
        format!("{rebuilt}#{fragment}")
    } else {
        rebuilt
    };
    if old_path == new_path {
        return None;
    }
    Some(with_fragment)
}

fn split_fragment(target: &str) -> (&str, Option<&str>) {
    if let Some((path, fragment)) = target.split_once('#') {
        (path, Some(fragment))
    } else {
        (target, None)
    }
}

fn is_external_target(target: &str) -> bool {
    let t = target.trim();
    if t.contains('@') && !t.contains(':') {
        return true;
    }
    let Some(idx) = t.find(':') else {
        return false;
    };
    if idx == 1 {
        let bytes = t.as_bytes();
        if bytes.get(2).is_some_and(|b| *b == b'/' || *b == b'\\') {
            return false;
        }
    }
    let scheme = &t[..idx];
    if scheme.is_empty() {
        return false;
    }
    let mut chars = scheme.chars();
    if !chars.next().is_some_and(|ch| ch.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.')
}

fn relative_path_from(base: &Path, target: &Path) -> Option<String> {
    let base_components = base.components().collect::<Vec<_>>();
    let target_components = target.components().collect::<Vec<_>>();

    let mut common = 0usize;
    while common < base_components.len()
        && common < target_components.len()
        && base_components[common] == target_components[common]
    {
        common += 1;
    }

    let mut out = PathBuf::new();
    for _ in common..base_components.len() {
        out.push("..");
    }
    for comp in target_components.iter().skip(common) {
        out.push(comp.as_os_str());
    }
    if out.as_os_str().is_empty() {
        Some(".".to_string())
    } else {
        Some(out.to_string_lossy().replace('\\', "/"))
    }
}

fn parse_file_uri(value: &str) -> Option<Uri> {
    let parsed = value.parse::<Uri>().ok()?;
    parsed.to_file_path().is_some().then_some(parsed)
}

#[cfg(test)]
mod tests {
    use super::{
        discover_standalone_workspace_documents, extract_shortcode_path_argument_span,
        is_external_target, is_quarto_project_config, relative_path_from,
        rewrite_destination_target, yaml_key_value, yaml_scalar_value_span,
    };
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn rewrites_same_directory_file_rename() {
        let doc = Path::new("/repo/docs/index.qmd");
        let old = Path::new("/repo/docs/tables.qmd");
        let new = Path::new("/repo/docs/tabular.qmd");
        let rewritten = rewrite_destination_target(doc, "tables.qmd", old, new).expect("rewrite");
        assert_eq!(rewritten, "tabular.qmd");
    }

    #[test]
    fn rewrites_cross_directory_relative_path() {
        let doc = Path::new("/repo/docs/ch1/index.qmd");
        let old = Path::new("/repo/assets/img/plot.png");
        let new = Path::new("/repo/assets/fig/plot.png");
        let rewritten = rewrite_destination_target(doc, "../../assets/img/plot.png", old, new)
            .expect("rewrite");
        assert_eq!(rewritten, "../../assets/fig/plot.png");
    }

    #[test]
    fn rewrites_and_preserves_fragment() {
        let doc = Path::new("/repo/docs/index.qmd");
        let old = Path::new("/repo/docs/tables.qmd");
        let new = Path::new("/repo/docs/tabular.qmd");
        let rewritten =
            rewrite_destination_target(doc, "tables.qmd#sec-1", old, new).expect("rewrite");
        assert_eq!(rewritten, "tabular.qmd#sec-1");
    }

    #[test]
    fn skips_external_targets() {
        assert!(is_external_target("https://example.com/x"));
        assert!(is_external_target("mailto:user@example.com"));
        assert!(!is_external_target("../docs/a.qmd"));
    }

    #[test]
    fn computes_relative_path_with_parent_segments() {
        let base = Path::new("/repo/docs/ch1");
        let target = Path::new("/repo/assets/fig/plot.png");
        let rel = relative_path_from(base, target).expect("relative path");
        assert_eq!(rel, "../../assets/fig/plot.png");
    }

    #[test]
    fn discovers_standalone_workspace_markdown_docs() {
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path();
        std::fs::create_dir_all(root.join("nested")).expect("mkdir");
        std::fs::create_dir_all(root.join(".git")).expect("mkdir");
        std::fs::write(root.join("doc.md"), "doc").expect("write");
        std::fs::write(root.join("nested").join("chapter.qmd"), "qmd").expect("write");
        std::fs::write(root.join(".git").join("ignored.md"), "ignored").expect("write");
        std::fs::write(root.join("note.txt"), "txt").expect("write");

        let mut docs = discover_standalone_workspace_documents(root);
        docs.sort();
        assert!(docs.contains(&root.join("doc.md")));
        assert!(docs.contains(&root.join("nested").join("chapter.qmd")));
        assert!(!docs.contains(&root.join(".git").join("ignored.md")));
        assert!(!docs.contains(&root.join("note.txt")));
    }

    #[test]
    fn extracts_include_shortcode_positional_path() {
        let span = extract_shortcode_path_argument_span(r#" include "chapters/part 1.qmd" "#)
            .expect("path span");
        assert_eq!(
            &r#" include "chapters/part 1.qmd" "#[span.0..span.1],
            "chapters/part 1.qmd"
        );
    }

    #[test]
    fn extracts_include_shortcode_file_key_path() {
        let span = extract_shortcode_path_argument_span(r#" include file="chapters/part 1.qmd" "#)
            .expect("path span");
        assert_eq!(
            &r#" include file="chapters/part 1.qmd" "#[span.0..span.1],
            "chapters/part 1.qmd"
        );
    }

    #[test]
    fn detects_quarto_project_config_path() {
        assert!(is_quarto_project_config(Path::new("/repo/_quarto.yml")));
        assert!(!is_quarto_project_config(Path::new("/repo/doc.qmd")));
    }

    #[test]
    fn extracts_yaml_quoted_scalar_span() {
        let (key, value_start) = yaml_key_value(r#"href: "index.qmd""#).expect("key value");
        assert_eq!(key, "href");
        let span = yaml_scalar_value_span(r#"href: "index.qmd""#, value_start).expect("span");
        assert_eq!(&r#"href: "index.qmd""#[span.0..span.1], "index.qmd");
    }
}
