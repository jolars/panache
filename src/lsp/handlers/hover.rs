//! Handler for textDocument/hover LSP requests.
//!
//! Provides hover information for:
//! - Footnote references: `[^id]` → shows footnote content from `[^id]: content`

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;
use crate::lsp::symbols::{SymbolTarget, resolve_symbol_target_at_offset};
use crate::metadata::inline_reference_contains;
use crate::syntax::{
    AstNode, DisplayMath, Document, FootnoteDefinition, Heading, Link, ReferenceDefinition,
};
use crate::utils::{crossref_resolution_labels, normalize_label};

use super::super::{conversions, helpers};

/// Handle textDocument/hover request
pub(crate) async fn hover(
    _client: &tower_lsp_server::Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
    _workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: HoverParams,
) -> Result<Option<Hover>> {
    let uri = &params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    let Some(ctx) =
        crate::lsp::context::get_open_document_context(&document_map, &salsa_db, uri).await
    else {
        return Ok(None);
    };
    let salsa_file = ctx.salsa_file;
    let salsa_config = ctx.salsa_config;
    let doc_path = ctx.path.clone();
    let parsed_yaml_regions = ctx.parsed_yaml_regions.clone();

    let Some(doc_path) = doc_path else {
        return Ok(None);
    };
    let content_for_offset = ctx.content.clone();
    let Some(offset) = conversions::position_to_offset(&content_for_offset, position) else {
        return Ok(None);
    };
    let in_frontmatter_region =
        helpers::is_offset_in_yaml_frontmatter(&parsed_yaml_regions, offset);
    if in_frontmatter_region {
        return Ok(None);
    }
    let yaml_ok = helpers::is_yaml_frontmatter_valid(&parsed_yaml_regions);
    if !yaml_ok {
        return Ok(None);
    }

    let metadata = {
        let db = salsa_db.lock().await;
        crate::salsa::metadata(&*db, salsa_file, salsa_config, doc_path.clone()).clone()
    };

    let target = {
        let root = ctx.syntax_root();
        resolve_symbol_target_at_offset(&root, offset)
    };

    if let Some(SymbolTarget::HeadingLink(label)) = target.as_ref() {
        let doc_indices = crate::lsp::navigation::project_symbol_documents(
            &salsa_db,
            salsa_file,
            salsa_config,
            &doc_path,
            uri,
            &content_for_offset,
        )
        .await;

        for doc in &doc_indices {
            if let Some(markdown) = section_hover_markdown(doc, label) {
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: None,
                }));
            }
        }
    }
    if let Some(SymbolTarget::Reference {
        label,
        is_footnote: false,
    }) = target.as_ref()
    {
        let doc_indices = crate::lsp::navigation::project_symbol_documents(
            &salsa_db,
            salsa_file,
            salsa_config,
            &doc_path,
            uri,
            &content_for_offset,
        )
        .await;

        for doc in &doc_indices {
            let Some(heading_label) = reference_definition_heading_target(doc, label) else {
                continue;
            };
            for candidate_doc in &doc_indices {
                if let Some(markdown) = section_hover_markdown(candidate_doc, &heading_label) {
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: markdown,
                        }),
                        range: None,
                    }));
                }
            }
        }
    }
    if let Some(SymbolTarget::Crossref(label)) = target.as_ref() {
        let doc_indices = crate::lsp::navigation::project_symbol_documents(
            &salsa_db,
            salsa_file,
            salsa_config,
            &doc_path,
            uri,
            &content_for_offset,
        )
        .await;

        for doc in &doc_indices {
            if let Some(markdown) = equation_hover_markdown(doc, label) {
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: None,
                }));
            }
            if let Some(markdown) = section_hover_markdown(doc, label) {
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: None,
                }));
            }
        }
    }
    let link_target = {
        let root = ctx.syntax_root();
        hovered_link_target(&root, offset)
    };
    if let Some(markdown) = linked_document_hover_markdown(
        link_target.as_deref(),
        &salsa_db,
        salsa_file,
        salsa_config,
        &doc_path,
        &content_for_offset,
        uri,
    )
    .await
    {
        return Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: markdown,
            }),
            range: None,
        }));
    }

    let pending_footnote = {
        let root = ctx.syntax_root();
        let Some(mut node) = helpers::find_node_at_offset(&root, offset) else {
            return Ok(None);
        };

        // Walk up the tree to find a footnote reference or citation
        loop {
            if let Some((label, is_footnote)) = helpers::extract_reference_label(&node) {
                // Only handle footnotes (not regular references)
                if is_footnote {
                    break Some(label);
                }
            }

            if let Some(key) = helpers::extract_citation_key(&node) {
                if let Some(ref parse) = metadata.bibliography_parse
                    && let Some(entry) = parse.index.get(&key)
                {
                    let summary = format_bibliography_entry(entry);
                    if !summary.is_empty() {
                        return Ok(Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: summary,
                            }),
                            range: None,
                        }));
                    }
                }

                if inline_reference_contains(&metadata.inline_references, &key) {
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: "Inline YAML reference".to_string(),
                        }),
                        range: None,
                    }));
                }
            }

            // Move up to parent, or return None if at root
            match node.parent() {
                Some(parent) => node = parent,
                None => break None,
            }
        }
    };

    let Some(label) = pending_footnote else {
        return Ok(None);
    };

    // Cross-document footnote lookup via symbol usage index.
    let doc_indices = crate::lsp::navigation::project_symbol_documents(
        &salsa_db,
        salsa_file,
        salsa_config,
        &doc_path,
        uri,
        &content_for_offset,
    )
    .await;

    for doc in doc_indices {
        let Some(ranges) = doc.symbol_index.footnote_definitions(&label) else {
            continue;
        };
        let Some(range) = ranges.first() else {
            continue;
        };

        let tree = crate::parse(&doc.text, None);
        let Some(footnote_def) = tree
            .descendants()
            .filter_map(FootnoteDefinition::cast)
            .find(|def| def.syntax().text_range() == *range)
        else {
            continue;
        };

        let trimmed = footnote_def.content().trim().to_string();
        return Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: trimmed,
            }),
            range: None,
        }));
    }

    Ok(None)
}

const HOVER_PREVIEW_MAX_CHARS: usize = 180;
const HOVER_EQUATION_PREVIEW_MAX_LINES: usize = 6;

fn equation_hover_markdown(
    doc: &crate::lsp::navigation::IndexedDocument,
    label: &str,
) -> Option<String> {
    let candidates = crossref_resolution_labels(label, true);
    let mut declaration_ranges = Vec::new();
    for candidate in candidates {
        if let Some(ranges) = doc.symbol_index.crossref_declarations(&candidate) {
            declaration_ranges.extend(ranges.iter().copied());
        }
    }
    declaration_ranges.sort_by_key(|range| range.start());
    declaration_ranges.dedup();
    if declaration_ranges.is_empty() {
        return None;
    }

    let tree = crate::parse(&doc.text, None);
    for declaration in declaration_ranges {
        let Some(math) = display_math_for_declaration(&tree, declaration) else {
            continue;
        };
        let content = math.content();
        let trimmed = content.trim_matches('\n').trim_end();
        if trimmed.is_empty() {
            continue;
        }
        let snippet = crop_preview_lines(trimmed, HOVER_EQUATION_PREVIEW_MAX_LINES);
        return Some(format!("**Equation:** `{label}`\n\n```tex\n{snippet}\n```"));
    }
    None
}

fn section_hover_markdown(
    doc: &crate::lsp::navigation::IndexedDocument,
    label: &str,
) -> Option<String> {
    let heading_range = first_heading_definition_range(&doc.symbol_index, label)?;
    let tree = crate::parse(&doc.text, None);
    let document = Document::cast(tree)?;

    let blocks: Vec<_> = document.blocks().collect();
    let heading_idx = blocks.iter().position(|node| {
        node.kind() == crate::syntax::SyntaxKind::HEADING && node.text_range() == heading_range
    })?;
    let heading_node = &blocks[heading_idx];
    let heading = Heading::cast(heading_node.clone())?;
    let title = heading.title_or("(empty)");
    let section_end = section_end_offset(&doc.symbol_index, heading_range, doc.text.len());

    let mut preview = None;
    for block in blocks.iter().skip(heading_idx + 1) {
        let start: usize = block.text_range().start().into();
        if start >= section_end {
            break;
        }
        let normalized = normalize_preview_text(block.text().to_string().trim());
        if !normalized.is_empty() {
            preview = Some(crop_preview(&normalized, HOVER_PREVIEW_MAX_CHARS));
            break;
        }
    }

    let markdown = match preview {
        Some(snippet) => format!("**Section:** {}\n\n{}", title, snippet),
        None => format!("**Section:** {}", title),
    };
    Some(markdown)
}

fn reference_definition_heading_target(
    doc: &crate::lsp::navigation::IndexedDocument,
    label: &str,
) -> Option<String> {
    let tree = crate::parse(&doc.text, None);
    let normalized = normalize_label(label);
    let def = tree
        .descendants()
        .filter_map(ReferenceDefinition::cast)
        .find(|def| normalize_label(&def.label()) == normalized)?;
    let destination = def.destination()?;
    heading_label_from_destination(&destination)
}

fn heading_label_from_destination(destination: &str) -> Option<String> {
    let mut target = destination.trim();
    if let Some(rest) = target.strip_prefix('<')
        && let Some(end) = rest.find('>')
    {
        target = &rest[..end];
    } else if let Some((head, _)) = target.split_once(char::is_whitespace) {
        target = head;
    }
    let anchor = target.strip_prefix('#')?;
    let normalized = normalize_label(anchor);
    (!normalized.is_empty()).then_some(normalized)
}

fn first_heading_definition_range(
    index: &crate::salsa::SymbolUsageIndex,
    label: &str,
) -> Option<rowan::TextRange> {
    let mut all = Vec::new();
    if let Some(ranges) = index.heading_explicit_definition_ranges(label) {
        all.extend(ranges.iter().copied());
    }
    if let Some(ranges) = index.heading_implicit_definition_ranges(label) {
        all.extend(ranges.iter().copied());
    }
    all.into_iter().min_by_key(|range| range.start())
}

fn section_end_offset(
    index: &crate::salsa::SymbolUsageIndex,
    heading_range: rowan::TextRange,
    text_len: usize,
) -> usize {
    let Some((at, level)) = index
        .heading_sequence()
        .iter()
        .enumerate()
        .find_map(|(idx, (range, lvl))| (*range == heading_range).then_some((idx, *lvl)))
    else {
        return text_len;
    };

    index
        .heading_sequence()
        .iter()
        .skip(at + 1)
        .find_map(|(next_range, next_level)| (*next_level <= level).then_some(next_range.start()))
        .map(Into::<usize>::into)
        .unwrap_or(text_len)
}

fn normalize_preview_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn crop_preview(text: &str, max_chars: usize) -> String {
    let mut iter = text.chars();
    let mut out = String::new();
    for _ in 0..max_chars {
        if let Some(ch) = iter.next() {
            out.push(ch);
        } else {
            return out;
        }
    }
    if iter.next().is_some() {
        out.push_str("...");
    }
    out
}

fn display_math_for_declaration(
    tree: &crate::syntax::SyntaxNode,
    declaration: rowan::TextRange,
) -> Option<DisplayMath> {
    let math_nodes: Vec<_> = tree.descendants().filter_map(DisplayMath::cast).collect();

    if let Some(math) = math_nodes.iter().find(|math| {
        let range = math.syntax().text_range();
        range.start() <= declaration.start() && declaration.end() <= range.end()
    }) {
        return DisplayMath::cast(math.syntax().clone());
    }

    let declaration_start: usize = declaration.start().into();
    math_nodes
        .into_iter()
        .filter_map(|math| {
            let range = math.syntax().text_range();
            let math_end: usize = range.end().into();
            (math_end <= declaration_start).then_some((declaration_start - math_end, math))
        })
        .min_by_key(|(distance, _)| *distance)
        .and_then(|(distance, math)| (distance <= 16).then_some(math))
}

fn crop_preview_lines(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        return text.to_string();
    }
    let mut out = lines[..max_lines].join("\n");
    out.push_str("\n...");
    out
}

async fn linked_document_hover_markdown(
    raw_link_target: Option<&str>,
    salsa_db: &Arc<Mutex<crate::salsa::SalsaDb>>,
    salsa_file: crate::salsa::FileText,
    salsa_config: crate::salsa::FileConfig,
    doc_path: &Path,
    content: &str,
    uri: &Uri,
) -> Option<String> {
    let link_target = raw_link_target?;
    let target_uri = resolve_local_markdown_target(
        salsa_db,
        salsa_file,
        salsa_config,
        doc_path,
        content,
        uri,
        link_target,
    )
    .await?;
    let target_path = target_uri.to_file_path()?;
    if target_path == doc_path {
        return None;
    }
    let target_text = std::fs::read_to_string(&target_path).ok()?;
    linked_doc_preview_markdown(&target_text, &target_path)
}

fn hovered_link_target(root: &crate::syntax::SyntaxNode, offset: usize) -> Option<String> {
    let mut node = helpers::find_node_at_offset(root, offset)?;
    loop {
        if let Some(link) = Link::cast(node.clone()) {
            if let Some(dest) = link.dest() {
                let dest_url = dest.url();
                let raw = crate::lsp::handlers::document_links::extract_first_destination_token(
                    &dest_url,
                );
                return (!raw.is_empty()).then_some(raw.to_string());
            }
            if let Some(link_ref) = link.reference() {
                let label = normalize_label(&link_ref.label());
                return (!label.is_empty()).then_some(format!("[ref]:{label}"));
            }
            if let Some(text) = link.text() {
                let label = normalize_label(&text.text_content());
                return (!label.is_empty()).then_some(format!("[ref]:{label}"));
            }
        }
        node = node.parent()?;
    }
}

async fn resolve_local_markdown_target(
    salsa_db: &Arc<Mutex<crate::salsa::SalsaDb>>,
    salsa_file: crate::salsa::FileText,
    salsa_config: crate::salsa::FileConfig,
    doc_path: &Path,
    content: &str,
    uri: &Uri,
    raw_target: &str,
) -> Option<Uri> {
    let resolved = if let Some(label) = raw_target.strip_prefix("[ref]:") {
        let ref_targets = crate::lsp::handlers::document_links::build_reference_targets(
            salsa_db,
            salsa_file,
            salsa_config,
            doc_path,
            content,
            uri,
        )
        .await;
        let target = ref_targets.get(label)?;
        crate::lsp::handlers::document_links::resolve_link_target(
            &target.raw_target,
            Some(&target.base_path),
            target.base_uri.as_ref(),
        )?
    } else {
        crate::lsp::handlers::document_links::resolve_link_target(
            raw_target,
            Some(doc_path),
            Some(uri),
        )?
    };

    let path = resolved.to_file_path()?;
    if !is_markdown_family_path(&path) {
        return None;
    }
    Some(resolved)
}

fn is_markdown_family_path(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    matches!(ext, "md" | "qmd" | "Rmd" | "markdown")
}

fn linked_doc_preview_markdown(target_text: &str, target_path: &Path) -> Option<String> {
    let tree = crate::parse(target_text, None);
    let document = Document::cast(tree)?;
    let blocks: Vec<_> = document.blocks().collect();
    let title = blocks
        .iter()
        .find_map(|node| Heading::cast(node.clone()))
        .map(|h| h.title_or("(empty)"));

    let heading_ranges: HashSet<rowan::TextRange> = blocks
        .iter()
        .filter(|node| node.kind() == crate::syntax::SyntaxKind::HEADING)
        .map(|node| node.text_range())
        .collect();
    let snippet = blocks.iter().find_map(|node| {
        if heading_ranges.contains(&node.text_range()) {
            return None;
        }
        let normalized = normalize_preview_text(node.text().to_string().trim());
        (!normalized.is_empty()).then_some(crop_preview(&normalized, HOVER_PREVIEW_MAX_CHARS))
    });

    if title.is_none() && snippet.is_none() {
        return None;
    }
    let file_name = target_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document");
    let mut out = format!("**Linked document:** `{file_name}`");
    if let Some(title) = title {
        out.push_str(&format!("\n\n**Title:** {}", title));
    }
    if let Some(snippet) = snippet {
        out.push_str(&format!("\n\n{}", snippet));
    }
    Some(out)
}

/// Format a bibliography entry for hover display.
///
/// Works with any bibliography format (BibTeX, CSL-JSON, CSL-YAML, RIS).
fn format_bibliography_entry(entry: &crate::bib::BibEntry) -> String {
    let author = entry
        .fields
        .get("author")
        .or_else(|| entry.fields.get("editor"))
        .map(|s| s.as_str())
        .unwrap_or_default();

    let year = entry
        .fields
        .get("year")
        .or_else(|| entry.fields.get("date"))
        .or_else(|| entry.fields.get("issued"))
        .map(|s| s.as_str())
        .unwrap_or_default();

    let title = entry
        .fields
        .get("title")
        .or_else(|| entry.fields.get("booktitle"))
        .map(|s| s.as_str())
        .unwrap_or_default();

    let container = entry
        .fields
        .get("journal")
        .or_else(|| entry.fields.get("journaltitle"))
        .or_else(|| entry.fields.get("container-title"))
        .or_else(|| entry.fields.get("publisher"))
        .map(|s| s.as_str())
        .unwrap_or_default();

    let locator = build_locator_unified(entry);

    let mut summary = String::new();
    if !author.is_empty() {
        summary.push_str(author);
    }
    if !year.is_empty() {
        if !summary.is_empty() {
            summary.push_str(" (");
            summary.push_str(year);
            summary.push(')');
        } else {
            summary.push_str(year);
        }
    }
    if !title.is_empty() {
        if !summary.is_empty() {
            summary.push_str(". ");
        }
        summary.push_str(&format!("*{}*", title));
    }
    if !container.is_empty() {
        summary.push_str(". ");
        summary.push_str(container);
    }
    if !locator.is_empty() {
        summary.push_str(", ");
        summary.push_str(&locator);
    }

    summary.trim().to_string()
}

fn build_locator_unified(entry: &crate::bib::BibEntry) -> String {
    let volume = entry
        .fields
        .get("volume")
        .map(|s| s.as_str())
        .unwrap_or_default();
    let number = entry
        .fields
        .get("number")
        .or_else(|| entry.fields.get("issue"))
        .map(|s| s.as_str())
        .unwrap_or_default();
    let pages = entry
        .fields
        .get("pages")
        .or_else(|| entry.fields.get("page"))
        .map(|s| s.as_str())
        .unwrap_or_default();

    let mut parts = Vec::new();
    if !volume.is_empty() {
        if !number.is_empty() {
            parts.push(format!("{}({})", volume, number));
        } else {
            parts.push(volume.to_string());
        }
    } else if !number.is_empty() {
        parts.push(number.to_string());
    }
    if !pages.is_empty() {
        parts.push(pages.to_string());
    }
    parts.join(", ")
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use crate::syntax::AstNode;

    #[test]
    fn test_hover_on_footnote_reference() {
        let input = "Text with footnote[^1]\n\n[^1]: This is the footnote content.";
        let root = parse(input, None);

        // Find the footnote reference node
        let footnote_ref = root
            .descendants()
            .find_map(crate::syntax::FootnoteReference::cast)
            .expect("Should find footnote reference")
            .syntax()
            .clone();

        // Extract the label
        let (label, is_footnote) =
            helpers::extract_reference_label(&footnote_ref).expect("Should extract label");
        assert_eq!(label, "1");
        assert!(is_footnote);

        let db = crate::salsa::SalsaDb::default();
        let extensions = crate::config::Extensions::default();
        let index = crate::salsa::symbol_usage_index_from_tree(&db, &root, &extensions);
        let range = index
            .footnote_definitions(&label)
            .and_then(|ranges| ranges.first())
            .copied()
            .expect("Should find definition range");
        let footnote_def = root
            .descendants()
            .filter_map(FootnoteDefinition::cast)
            .find(|def| def.syntax().text_range() == range)
            .expect("Should find footnote definition by range");

        // Extract content
        let content = footnote_def.content();
        assert!(content.contains("This is the footnote content"));
    }

    #[test]
    fn test_hover_multiline_footnote() {
        let input = "Text[^1]\n\n[^1]: First line\n    Second line";
        let root = parse(input, None);

        let db = crate::salsa::SalsaDb::default();
        let extensions = crate::config::Extensions::default();
        let index = crate::salsa::symbol_usage_index_from_tree(&db, &root, &extensions);
        let range = index
            .footnote_definitions("1")
            .and_then(|ranges| ranges.first())
            .copied()
            .expect("Should find definition range");
        let footnote_def = root
            .descendants()
            .filter_map(FootnoteDefinition::cast)
            .find(|def| def.syntax().text_range() == range)
            .expect("Should find footnote definition by range");

        let content = footnote_def.content();
        assert!(content.contains("First line"));
        assert!(content.contains("Second line"));
    }

    #[test]
    fn test_no_definition_found() {
        let input = "Text with footnote[^missing]";
        let root = parse(input, None);

        let db = crate::salsa::SalsaDb::default();
        let extensions = crate::config::Extensions::default();
        let index = crate::salsa::symbol_usage_index_from_tree(&db, &root, &extensions);
        assert!(index.footnote_definitions("missing").is_none());
    }

    #[test]
    fn test_footnote_with_formatting() {
        let input = "[^1]: Text with *emphasis* and `code`.";
        let root = parse(input, None);

        let db = crate::salsa::SalsaDb::default();
        let extensions = crate::config::Extensions::default();
        let index = crate::salsa::symbol_usage_index_from_tree(&db, &root, &extensions);
        let range = index
            .footnote_definitions("1")
            .and_then(|ranges| ranges.first())
            .copied()
            .expect("Should find definition range");
        let footnote_def = root
            .descendants()
            .filter_map(FootnoteDefinition::cast)
            .find(|def| def.syntax().text_range() == range)
            .expect("Should find footnote definition by range");

        let content = footnote_def.content();
        assert!(content.contains("*emphasis*"));
        assert!(content.contains("`code`"));
    }

    #[test]
    fn footnote_definition_index_contains_definition_range() {
        let db = crate::salsa::SalsaDb::default();
        let text = "Text[^a]\n\n[^a]: Hello world\n";
        let tree = parse(text, None);
        let extensions = crate::config::Extensions::default();
        let index = crate::salsa::symbol_usage_index_from_tree(&db, &tree, &extensions);
        assert_eq!(
            index.footnote_definitions("a").map(|ranges| ranges.len()),
            Some(1)
        );
    }
}
