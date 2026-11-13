use crate::config::Extensions;
use crate::linter::code_block_collector::SnippetKind;
use crate::syntax::{
    AstNode, CellOptionResolution, CodeBlock, Heading, InlineExecutable, MystDirective, SyntaxKind,
    SyntaxNode,
};
use std::collections::HashMap;

/// A block or inline expression with its location in the document.
#[derive(Debug, Clone)]
pub struct CodeSnippet {
    pub kind: SnippetKind,
    /// Programming language of the block
    pub language: String,
    /// Content of the code block (without fences), dedented: container
    /// prefix bytes (`> ` markers, list item indent) carried inside the
    /// block as `LINE_PREFIX` tokens are skipped so external tools see
    /// valid code.
    pub content: String,
    /// Starting line number in the document (1-indexed)
    pub start_line: usize,
    /// Byte offset range of the content in the original document, including
    /// the first line's container prefix for blocks.
    pub original_range: std::ops::Range<usize>,
    /// For each line of `content`, the byte offset in the original document
    /// where that line's content starts — after the line's container prefix.
    /// Dedenting makes plain start-plus-relative-offset arithmetic wrong on
    /// prefixed lines, so offset mapping walks these instead.
    pub line_starts: Vec<usize>,
}

/// A node's text with each line's container-prefix (`LINE_PREFIX`) tokens
/// skipped, paired with the original byte offset at which each produced
/// line starts.
fn dedented_text_with_line_starts(node: &SyntaxNode) -> (String, Vec<usize>) {
    let mut text = String::new();
    let mut line_starts = Vec::new();
    let mut at_line_start = true;

    for token in node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
    {
        if token.kind() == SyntaxKind::LINE_PREFIX {
            continue;
        }
        let token_text = token.text();
        let base: usize = token.text_range().start().into();
        let mut pos = 0;
        while pos < token_text.len() {
            if at_line_start {
                line_starts.push(base + pos);
                at_line_start = false;
            }
            match token_text[pos..].find('\n') {
                Some(rel) => {
                    pos += rel + 1;
                    at_line_start = true;
                }
                None => break,
            }
        }
        text.push_str(token_text);
    }

    (text, line_starts)
}

/// Collect lintable blocks and executable inline expressions in document order.
pub fn collect_code_snippets(tree: &SyntaxNode, input: &str) -> HashMap<String, Vec<CodeSnippet>> {
    let mut blocks: HashMap<String, Vec<CodeSnippet>> = HashMap::new();

    for node in tree.descendants() {
        let block = if let Some(inline) = InlineExecutable::cast(node.clone()) {
            extract_inline_expression(&inline, input)
        } else {
            match node.kind() {
                SyntaxKind::CODE_BLOCK => extract_code_block(&node, input),
                SyntaxKind::MYST_DIRECTIVE => extract_myst_directive_block(&node, input),
                _ => None,
            }
        };
        if let Some(block) = block {
            blocks
                .entry(block.language.clone())
                .or_default()
                .push(block);
        }
    }

    blocks
}

fn extract_inline_expression(inline: &InlineExecutable, input: &str) -> Option<CodeSnippet> {
    let language = inline.language()?;
    let range = inline.code_range()?;
    let start: usize = range.start().into();
    let mut content = String::new();
    let mut line_starts = Vec::new();
    let mut at_line_start = true;
    for segment in inline.code_source_segments() {
        let base: usize = segment.text_range().start().into();
        for (offset, byte) in segment.text().bytes().enumerate() {
            if at_line_start {
                line_starts.push(base + offset);
            }
            at_line_start = byte == b'\n';
        }
        content.push_str(segment.text());
    }
    Some(CodeSnippet {
        kind: SnippetKind::Inline,
        language,
        content,
        start_line: offset_to_line(input, start),
        original_range: start..usize::from(range.end()),
        line_starts,
    })
}

/// Extract a MyST code directive body as a lintable code block, keyed by the
/// directive argument (e.g. `python` in `` ```{code-block} python ``). Returns
/// `None` for other directives or when the language or body is empty.
/// The body's byte range is preserved so external
/// diagnostics map back onto the original source.
fn extract_myst_directive_block(node: &SyntaxNode, input: &str) -> Option<CodeSnippet> {
    let directive = MystDirective::cast(node.clone())?;
    if !directive.is_code() {
        return None;
    }
    let argument = directive.argument()?;
    let language = argument.strip_prefix('.').unwrap_or(&argument).to_string();
    let body_node = directive
        .syntax()
        .children()
        .find(|child| child.kind() == SyntaxKind::MYST_DIRECTIVE_BODY)?;
    let (content, line_starts) = dedented_text_with_line_starts(&body_node);

    if language.is_empty() || content.is_empty() {
        return None;
    }

    let range = body_node.text_range();
    let start: usize = range.start().into();
    let end: usize = range.end().into();

    Some(CodeSnippet {
        kind: SnippetKind::DisplayBlock,
        language,
        content,
        start_line: offset_to_line(input, start),
        original_range: start..end,
        line_starts,
    })
}

fn extract_code_block(node: &SyntaxNode, input: &str) -> Option<CodeSnippet> {
    let block = CodeBlock::cast(node.clone())?;
    let language = block.language()?.trim_start_matches('.').to_string();
    let kind = match block.executable_cell() {
        Some(cell) => {
            let options = cell.resolved_options();
            let literal_bool = |key: &str, expected: &str| {
                options.iter().any(|option| {
                    option.key() == key
                        && matches!(option.resolution(), CellOptionResolution::Resolved(value)
                            if !value.is_quoted()
                                && !matches!(value.yaml_tag(), Some("!expr" | "!r"))
                                && value.cooked_value().is_some_and(|text| text.trim().eq_ignore_ascii_case(expected)))
                })
            };
            if literal_bool("eval", "false") {
                SnippetKind::DisplayBlock
            } else if language == "r" && literal_bool("error", "true") {
                SnippetKind::ErrorHandledBlock
            } else {
                SnippetKind::Block
            }
        }
        None => SnippetKind::DisplayBlock,
    };
    let mut content = String::new();
    let mut line_starts = Vec::new();
    let mut at_line_start = true;
    let mut end = 0;
    for segment in block.code_source_segments() {
        let base: usize = segment.text_range().start().into();
        for (offset, byte) in segment.text().bytes().enumerate() {
            if at_line_start {
                line_starts.push(base + offset);
            }
            at_line_start = byte == b'\n';
        }
        content.push_str(segment.text());
        end = usize::from(segment.text_range().end());
    }
    if language.is_empty() || content.is_empty() {
        return None;
    }
    // Retain the first line's prefix so even a one-line container block is
    // recognized as dedented when deciding whether a fix preserves structure.
    let start = input[..*line_starts.first()?]
        .rfind('\n')
        .map_or(0, |offset| offset + 1);

    Some(CodeSnippet {
        kind,
        language,
        content,
        start_line: offset_to_line(input, start),
        original_range: start..end,
        line_starts,
    })
}

/// Convert byte offset to 1-indexed line number.
pub fn offset_to_line(input: &str, offset: usize) -> usize {
    // Count how many newlines precede this offset
    let newline_count = input[..offset].chars().filter(|&c| c == '\n').count();
    // Line number is newlines + 1
    newline_count + 1
}

/// Normalize a label for case-insensitive matching.
/// Collapses whitespace and converts to lowercase.
///
/// Used for reference definitions and footnote IDs to ensure
/// case-insensitive and whitespace-normalized matching.
pub fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Normalize cross-reference and anchor labels for case-sensitive matching.
///
/// Unlike `normalize_label`, this preserves letter case.
pub fn normalize_anchor_label(label: &str) -> String {
    label.trim().to_string()
}

pub fn crossref_resolution_labels(label: &str, bookdown_references: bool) -> Vec<String> {
    let normalized = normalize_anchor_label(label);
    let mut labels = vec![normalized.clone()];

    if !bookdown_references {
        return labels;
    }

    if let Some((_, unprefixed)) = normalized.split_once(':')
        && !unprefixed.is_empty()
        && crate::parser::inlines::citations::has_bookdown_prefix(&normalized)
        && !labels.iter().any(|candidate| candidate == unprefixed)
    {
        labels.push(unprefixed.to_string());
    }

    labels
}

#[cfg(feature = "lsp")]
pub fn crossref_symbol_labels(label: &str, bookdown_references: bool) -> Vec<String> {
    let mut labels = crossref_resolution_labels(label, bookdown_references);

    if !bookdown_references {
        return labels;
    }

    let normalized = normalize_anchor_label(label);
    let is_prefixed = crate::parser::inlines::citations::has_bookdown_prefix(&normalized);
    if is_prefixed {
        return labels;
    }

    for prefix in crate::parser::inlines::citations::BOOKDOWN_LABEL_PREFIXES {
        let candidate = format!("{}:{}", prefix, normalized);
        if !labels.iter().any(|existing| existing == &candidate) {
            labels.push(candidate);
        }
    }

    labels
}

#[derive(Debug, Clone)]
pub struct ImplicitHeadingId {
    pub id: String,
    pub heading: SyntaxNode,
}

pub fn implicit_heading_ids(tree: &SyntaxNode, extensions: &Extensions) -> Vec<ImplicitHeadingId> {
    if !extensions.auto_identifiers {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();

    for heading in tree.descendants().filter_map(Heading::cast) {
        let raw_text = heading
            .content()
            .map(|content| content.text())
            .unwrap_or_default();
        let normalized = normalize_label(&raw_text);
        if normalized.is_empty() {
            continue;
        }

        let base = heading_slugify(&normalized, extensions);
        if base.is_empty() {
            continue;
        }

        let count = seen.entry(base.clone()).or_insert(0);
        let id = if *count == 0 {
            base
        } else {
            format!("{}-{}", base, *count)
        };
        *count += 1;

        out.push(ImplicitHeadingId {
            id,
            heading: heading.syntax().clone(),
        });
    }

    out
}

/// Generate an auto identifier from heading text based on extension settings.
///
/// `auto_identifiers` is the master switch, and `gfm_auto_identifiers` only
/// picks the algorithm. Readers that leave it off (`commonmark`, `myst`) give
/// every heading an empty id, so there is no slug to derive at all.
pub fn heading_slugify(text: &str, extensions: &Extensions) -> String {
    if !extensions.auto_identifiers {
        String::new()
    } else if extensions.gfm_auto_identifiers {
        gfm_slugify(text)
    } else {
        pandoc_slugify(text)
    }
}

/// Generate a GitHub-style auto identifier from heading text.
pub fn gfm_slugify(text: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;

    for ch in text.chars() {
        if ch.is_whitespace() {
            if !out.is_empty() && !prev_dash {
                out.push('-');
                prev_dash = true;
            }
            continue;
        }

        for lc in ch.to_lowercase() {
            if lc.is_alphanumeric() || lc == '_' || lc == '-' {
                out.push(lc);
                prev_dash = lc == '-';
            }
        }
    }

    while out.ends_with('-') {
        out.pop();
    }

    out
}

/// Generate a Pandoc-style auto identifier from heading text.
pub fn pandoc_slugify(text: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;

    for ch in text.chars() {
        if ch.is_whitespace() {
            if !out.is_empty() && !prev_dash {
                out.push('-');
                prev_dash = true;
            }
            continue;
        }

        for lc in ch.to_lowercase() {
            if lc.is_alphanumeric() || lc == '_' || lc == '-' || lc == '.' {
                out.push(lc);
                prev_dash = lc == '-';
            }
        }
    }

    while out.ends_with('-') {
        out.pop();
    }

    out
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "lsp")]
    use super::crossref_symbol_labels;
    use super::{crossref_resolution_labels, implicit_heading_ids};

    #[test]
    fn myst_non_code_directives_are_not_code_snippets() {
        let config = crate::Config {
            flavor: crate::config::Flavor::Myst,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Myst),
            ..Default::default()
        };
        for name in [
            "custom-directive",
            "domain:custom",
            "raw",
            "eval-rst",
            "math",
            "toctree",
        ] {
            let input = format!("```{{{name}}} python\nx = 1\n```\n");
            let tree = crate::parse(&input, Some(config.clone()));
            assert!(
                super::collect_code_snippets(&tree, &input).is_empty(),
                "{name}"
            );
        }
    }

    #[test]
    fn crossref_resolution_labels_keep_exact_match() {
        let labels = crossref_resolution_labels("fig-plot", false);
        assert_eq!(labels, vec!["fig-plot".to_string()]);
    }

    #[test]
    fn crossref_resolution_labels_include_unprefixed_bookdown_key() {
        let labels = crossref_resolution_labels("fig:plot", true);
        assert_eq!(labels, vec!["fig:plot".to_string(), "plot".to_string()]);
    }

    #[cfg(feature = "lsp")]
    #[test]
    fn crossref_symbol_labels_include_bookdown_prefixed_variants() {
        let labels = crossref_symbol_labels("plot", true);
        assert!(labels.iter().any(|label| label == "plot"));
        assert!(labels.iter().any(|label| label == "fig:plot"));
        assert!(labels.iter().any(|label| label == "tab:plot"));
    }

    #[test]
    fn implicit_heading_ids_use_pandoc_duplicate_suffixes() {
        let tree = crate::parse("# Heading\n\n# Heading\n\n# Heading\n", None);
        let ids = implicit_heading_ids(&tree, &crate::config::Extensions::default())
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["heading", "heading-1", "heading-2"]);
    }

    #[test]
    fn implicit_heading_ids_use_gfm_slug_algorithm_when_enabled() {
        let tree = crate::parse("# 3. Applications\n", None);
        let ext = crate::config::Extensions {
            gfm_auto_identifiers: true,
            ..crate::config::Extensions::default()
        };
        let ids = implicit_heading_ids(&tree, &ext)
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["3-applications"]);
    }

    #[test]
    fn implicit_heading_ids_are_empty_without_auto_identifiers() {
        let tree = crate::parse("# Plain heading\n", None);
        let ext = crate::config::Extensions {
            auto_identifiers: false,
            ..crate::config::Extensions::default()
        };
        assert!(implicit_heading_ids(&tree, &ext).is_empty());
    }
}
