//! YAML metadata block parsing utilities.

use crate::options::Flavor;
use crate::parser::diagnostics::{Diagnostics, SyntaxError, SyntaxErrorSource};
use crate::parser::utils::helpers::{emit_line_tokens, strip_newline};
use crate::parser::utils::tree_copy::copy_green_children;
use crate::parser::yaml::{YamlValidationContext, parse_stream, validate_yaml_with_context_tree};
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::{GreenNode, GreenNodeBuilder, TextRange};

/// Try to parse a YAML metadata block starting at the given position.
/// Returns the new position after the block if successful, None otherwise.
///
/// A YAML block:
/// - Starts with `---` at column 0 (not followed by blank line)
/// - Ends with `---` or `...`, also at column 0
/// - At document start OR preceded by blank line
/// - Content passes [`prepare_yaml_content`]'s metadata gate
pub(crate) fn try_parse_yaml_block(
    lines: &[&str],
    pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
    at_document_start: bool,
    diags: &Diagnostics,
    flavor: Flavor,
) -> Option<usize> {
    let closing_pos = find_yaml_block_closing_pos(lines, pos, at_document_start, |i| lines[i])?;
    let content = collect_yaml_content(lines, pos, closing_pos);
    let outcome = prepare_yaml_content(&content, flavor)?;
    emit_yaml_block(lines, pos, closing_pos, builder, diags, &outcome)
}

/// Reconstruct the content between the delimiters as a contiguous byte
/// string. The lines returned by `split_lines_inclusive` are non-overlapping
/// slices of the original input that retain their trailing LF / CRLF, so
/// concatenating them rebuilds the source bytes exactly (including CRLF).
pub(crate) fn collect_yaml_content(lines: &[&str], pos: usize, closing_pos: usize) -> String {
    let mut content = String::new();
    for content_line in lines.iter().take(closing_pos).skip(pos + 1) {
        content.push_str(content_line);
    }
    content
}

/// Validation + parse outcome for YAML metadata content, computed once at
/// detection and carried to emission (via `YamlMetadataPrepared`) so the
/// content is never re-parsed.
#[derive(Debug, Clone)]
pub(crate) enum YamlContentOutcome {
    /// Content failed validation: emit opaque line tokens plus a syntax
    /// error at content-relative `start..end`. Pandoc hard-errors here, so
    /// there is no alternative interpretation to fall back to.
    Invalid {
        message: &'static str,
        start: usize,
        end: usize,
    },
    /// Content validated: embed the parsed `YAML_STREAM`'s children.
    Valid { stream: GreenNode },
}

/// Validate and parse metadata-block content, applying pandoc's metadata
/// gate: pandoc accepts frontmatter whose YAML parses to a top-level mapping
/// or null (empty/comments-only), backtracks to another block interpretation
/// (simple table, HR + paragraph) when it parses to anything else, and
/// hard-errors on a YAML parse exception. Returns `None` for the backtrack
/// case so the dispatcher falls through to the other block parsers; the
/// hard-error case maps to [`YamlContentOutcome::Invalid`].
///
/// The gate only applies to flavors with an asserted frontmatter YAML
/// consumer (pandoc-family); GFM/CommonMark-family frontmatter stays lenient.
pub(crate) fn prepare_yaml_content(content: &str, flavor: Flavor) -> Option<YamlContentOutcome> {
    let yaml_ctx = YamlValidationContext::frontmatter(flavor);
    let (diag, tree) = validate_yaml_with_context_tree(content, yaml_ctx);
    if let Some(diag) = diag {
        let start = diag.byte_start.min(content.len());
        let end = diag.byte_end.min(content.len()).max(start);
        return Some(YamlContentOutcome::Invalid {
            message: diag.message,
            start,
            end,
        });
    }
    let stream = match tree {
        Some(tree) => tree,
        None => parse_stream(content),
    };
    if !yaml_ctx.consumers().is_empty() && !top_level_is_mapping_or_null(&stream) {
        return None;
    }
    Some(YamlContentOutcome::Valid {
        stream: stream.green().to_owned(),
    })
}

/// Whether the first document in a parsed `YAML_STREAM` has a top-level
/// mapping or null value (pandoc's acceptance rule for metadata blocks).
/// A stream or document with no content node (empty, comments-only) counts
/// as null.
fn top_level_is_mapping_or_null(stream: &SyntaxNode) -> bool {
    let Some(document) = stream
        .children()
        .find(|n| n.kind() == SyntaxKind::YAML_DOCUMENT)
    else {
        return true;
    };
    let Some(value) = document.children().find(|n| {
        matches!(
            n.kind(),
            SyntaxKind::YAML_BLOCK_MAP
                | SyntaxKind::YAML_FLOW_MAP
                | SyntaxKind::YAML_BLOCK_SEQUENCE
                | SyntaxKind::YAML_FLOW_SEQUENCE
                | SyntaxKind::YAML_SCALAR
        )
    }) else {
        return true;
    };
    match value.kind() {
        SyntaxKind::YAML_BLOCK_MAP | SyntaxKind::YAML_FLOW_MAP => true,
        SyntaxKind::YAML_SCALAR => scalar_is_null(&value),
        _ => false,
    }
}

fn scalar_is_null(scalar: &SyntaxNode) -> bool {
    let mut text = String::new();
    for token in scalar.children_with_tokens().filter_map(|t| t.into_token()) {
        if token.kind() == SyntaxKind::YAML_SCALAR_TEXT {
            text.push_str(token.text());
        }
    }
    matches!(text.trim(), "~" | "null" | "Null" | "NULL")
}

/// Whether `line` is the metadata delimiter `marker` at column 0 of its
/// container's content.
///
/// Pandoc's `yamlMetaBlock` opens with `string "---" >> blankline` and its
/// closer (`stopLine`) has the same shape; neither is preceded by a
/// leading-space parser, so *any* indentation disqualifies the line and it
/// reparses as an ordinary block (indented code at 4+ columns, a simple
/// table rule or thematic break below that). Trailing spaces and tabs are
/// fine --- `blankline` skips them.
fn is_delim(line: &str, marker: &str) -> bool {
    line.strip_prefix(marker)
        .is_some_and(|rest| rest.trim().is_empty())
}

/// Whether `line` opens a YAML metadata block. Callers must pass the line
/// with its container prefix stripped, so column 0 is the container's
/// content column.
pub(crate) fn is_metadata_open_delim(line: &str) -> bool {
    is_delim(line, "---")
}

/// Whether `line` closes a YAML metadata block (pandoc's `stopLine`).
fn is_metadata_close_delim(line: &str) -> bool {
    is_delim(line, "---") || is_delim(line, "...")
}

/// Locate the closing delimiter of the metadata block opening at `pos`.
///
/// `strip` maps an ABSOLUTE line index to that line with its container
/// prefix removed; delimiter recognition is anchored at column 0 of the
/// stripped text, so a metadata block nested in a list item is measured
/// against the item's content column, not the document's.
pub(crate) fn find_yaml_block_closing_pos<'a>(
    lines: &[&'a str],
    pos: usize,
    at_document_start: bool,
    strip: impl Fn(usize) -> &'a str,
) -> Option<usize> {
    if pos >= lines.len() {
        return None;
    }

    if !is_metadata_open_delim(strip(pos)) {
        return None;
    }

    if !at_document_start && pos > 0 {
        let prev_line = lines[pos - 1];
        if !prev_line.trim().is_empty() {
            return None;
        }
    }

    if pos + 1 < lines.len() {
        let next_line = lines[pos + 1];
        if next_line.trim().is_empty() {
            return None;
        }
    } else {
        return None;
    }

    (pos + 1..lines.len()).find(|&i| is_metadata_close_delim(strip(i)))
}

pub(crate) fn emit_yaml_block(
    lines: &[&str],
    pos: usize,
    closing_pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
    diags: &Diagnostics,
    outcome: &YamlContentOutcome,
) -> Option<usize> {
    if pos >= lines.len() || closing_pos <= pos || closing_pos >= lines.len() {
        return None;
    }
    builder.start_node(SyntaxKind::YAML_METADATA.into());

    let (text, newline_str) = strip_newline(lines[pos]);
    builder.token(SyntaxKind::YAML_METADATA_DELIM.into(), text);
    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

    builder.start_node(SyntaxKind::YAML_METADATA_CONTENT.into());

    match outcome {
        YamlContentOutcome::Invalid {
            message,
            start,
            end,
        } => {
            let host_start = lines[pos + 1].as_ptr() as usize - lines[0].as_ptr() as usize;
            diags.push(SyntaxError {
                range: TextRange::new(
                    ((host_start + start) as u32).into(),
                    ((host_start + end) as u32).into(),
                ),
                message: message.to_string(),
                source: SyntaxErrorSource::Yaml,
            });
            for content_line in lines.iter().take(closing_pos).skip(pos + 1) {
                emit_line_tokens(builder, content_line);
            }
        }
        YamlContentOutcome::Valid { stream } => {
            copy_green_children(builder, stream);
        }
    }
    builder.finish_node(); // YAML_METADATA_CONTENT

    let (closing_text, closing_newline) = strip_newline(lines[closing_pos]);
    builder.token(SyntaxKind::YAML_METADATA_DELIM.into(), closing_text);
    if !closing_newline.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), closing_newline);
    }

    builder.finish_node(); // YamlMetadata

    Some(closing_pos + 1)
}

/// Try to parse a Pandoc title block starting at the beginning of document.
/// Returns the new position after the block if successful, None otherwise.
///
/// A Pandoc title block:
/// - Must be at document start (pos == 0)
/// - Has 1-3 lines starting with `%`
/// - Format: % title, % author(s), % date
/// - Continuation lines start with leading space
pub(crate) fn try_parse_pandoc_title_block(
    lines: &[&str],
    pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
) -> Option<usize> {
    if pos != 0 {
        return None;
    }
    let end = pandoc_title_block_end(lines)?;
    emit_pandoc_title_block(&lines[..end], builder);
    Some(end)
}

pub(crate) fn pandoc_title_block_end(lines: &[&str]) -> Option<usize> {
    let mut end = 0;
    for _ in 0..3 {
        if !lines
            .get(end)
            .is_some_and(|line| line.trim_start().starts_with('%'))
        {
            break;
        }
        end += 1;
        while lines.get(end).is_some_and(|line| {
            line.starts_with([' ', '\t']) && !line.trim_start().starts_with('%')
        }) {
            end += 1;
        }
    }
    (end > 0).then_some(end)
}

pub(crate) fn emit_pandoc_title_block(lines: &[&str], builder: &mut GreenNodeBuilder<'static>) {
    builder.start_node(SyntaxKind::PANDOC_TITLE_BLOCK.into());
    for line in lines {
        emit_line_tokens(builder, line);
    }
    builder.finish_node();
}

fn mmd_key_value(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    (!key.trim().is_empty()).then(|| (key.trim(), value.trim()))
}

/// Return the end of a MultiMarkdown title block, including its required blank line.
/// Callers must check that the block starts at the beginning of the document.
pub(crate) fn mmd_title_block_end(lines: &[&str]) -> Option<usize> {
    let (_, value) = mmd_key_value(lines.first()?)?;
    if value.is_empty() {
        return None;
    }
    for (index, line) in lines.iter().enumerate().skip(1) {
        if line.trim().is_empty() {
            return Some(index + 1);
        }
        if !line.starts_with([' ', '\t']) && mmd_key_value(line).is_none() {
            return None;
        }
    }
    None
}

pub(crate) fn emit_mmd_title_block(lines: &[&str], builder: &mut GreenNodeBuilder<'static>) {
    builder.start_node(SyntaxKind::MMD_TITLE_BLOCK.into());
    for line in lines {
        emit_line_tokens(builder, line);
    }
    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yaml_block_at_start() {
        let lines = vec!["---", "title: Test", "---", "Content"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_yaml_block(
            &lines,
            0,
            &mut builder,
            true,
            &Diagnostics::default(),
            Flavor::Pandoc,
        );
        assert_eq!(result, Some(3));
    }

    #[test]
    fn test_yaml_block_not_at_start() {
        let lines = vec!["Paragraph", "", "---", "title: Test", "---", "Content"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_yaml_block(
            &lines,
            2,
            &mut builder,
            false,
            &Diagnostics::default(),
            Flavor::Pandoc,
        );
        assert_eq!(result, Some(5));
    }

    #[test]
    fn test_horizontal_rule_not_yaml() {
        let lines = vec!["---", "", "Content"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_yaml_block(
            &lines,
            0,
            &mut builder,
            true,
            &Diagnostics::default(),
            Flavor::Pandoc,
        );
        assert_eq!(result, None); // Followed by blank line, so not YAML
    }

    #[test]
    fn test_yaml_with_dots_closer() {
        let lines = vec!["---", "title: Test", "...", "Content"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_yaml_block(
            &lines,
            0,
            &mut builder,
            true,
            &Diagnostics::default(),
            Flavor::Pandoc,
        );
        assert_eq!(result, Some(3));
    }

    #[test]
    fn test_yaml_without_closing_delimiter_is_not_yaml_block() {
        let lines = vec!["---", "title: Test", "Content"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_yaml_block(
            &lines,
            0,
            &mut builder,
            true,
            &Diagnostics::default(),
            Flavor::Pandoc,
        );
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_yaml_block_closing_pos() {
        let lines = vec!["---", "title: Test", "---", "Content"];
        let result = find_yaml_block_closing_pos(&lines, 0, true, |i| lines[i]);
        assert_eq!(result, Some(2));
    }

    #[test]
    fn test_indented_opening_delimiter_is_not_metadata() {
        for indent in [" ", "  ", "   ", "    ", "\t"] {
            let opener = format!("{indent}---");
            let content = format!("{indent}title: Test");
            let lines = vec![opener.as_str(), content.as_str(), opener.as_str()];
            let mut builder = GreenNodeBuilder::new();
            let result = try_parse_yaml_block(
                &lines,
                0,
                &mut builder,
                true,
                &Diagnostics::default(),
                Flavor::Pandoc,
            );
            assert_eq!(result, None, "indent {indent:?} should not open metadata");
        }
    }

    #[test]
    fn test_indented_closing_delimiter_does_not_close_metadata() {
        for closer in ["  ---", "    ---", "  ...", "    ..."] {
            let lines = vec!["---", "title: Test", closer];
            let mut builder = GreenNodeBuilder::new();
            let result = try_parse_yaml_block(
                &lines,
                0,
                &mut builder,
                true,
                &Diagnostics::default(),
                Flavor::Pandoc,
            );
            assert_eq!(result, None, "{closer:?} should not close metadata");
        }
    }

    #[test]
    fn test_delimiter_trailing_whitespace_is_allowed() {
        let lines = vec!["---  ", "title: Test", "---\t"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_yaml_block(
            &lines,
            0,
            &mut builder,
            true,
            &Diagnostics::default(),
            Flavor::Pandoc,
        );
        assert_eq!(result, Some(3));
    }

    #[test]
    fn test_metadata_in_list_item_is_measured_from_content_column() {
        let input = "- item\n\n  ---\n  title: x\n  ---\n\n  after\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        assert_eq!(tree.text().to_string(), input);
        assert!(
            tree.descendants()
                .any(|n| n.kind() == SyntaxKind::YAML_METADATA),
            "list-item metadata block should still be recognized"
        );
    }

    #[test]
    fn test_nonmapping_yaml_is_not_metadata() {
        for content in ["- a", "plain prose here", "42", "[a, b]"] {
            let lines = vec!["---", content, "---"];
            let mut builder = GreenNodeBuilder::new();
            let result = try_parse_yaml_block(
                &lines,
                0,
                &mut builder,
                true,
                &Diagnostics::default(),
                Flavor::Pandoc,
            );
            assert_eq!(result, None, "{content:?} should not parse as metadata");
        }
    }

    #[test]
    fn test_mapping_and_null_yaml_is_metadata() {
        for content in ["foo: bar", "{a: 1}", "null", "~", "# comment"] {
            let lines = vec!["---", content, "---"];
            let mut builder = GreenNodeBuilder::new();
            let result = try_parse_yaml_block(
                &lines,
                0,
                &mut builder,
                true,
                &Diagnostics::default(),
                Flavor::Pandoc,
            );
            assert_eq!(result, Some(3), "{content:?} should parse as metadata");
        }
    }

    #[test]
    fn test_nonmapping_yaml_stays_metadata_for_lenient_flavors() {
        let lines = vec!["---", "42", "---"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_yaml_block(
            &lines,
            0,
            &mut builder,
            true,
            &Diagnostics::default(),
            Flavor::Gfm,
        );
        assert_eq!(result, Some(3));
    }

    #[test]
    fn test_invalid_yaml_stays_metadata_with_diagnostic() {
        let lines = vec!["---", "foo: \"bar", "---"];
        let mut builder = GreenNodeBuilder::new();
        let diags = Diagnostics::default();
        let result = try_parse_yaml_block(&lines, 0, &mut builder, true, &diags, Flavor::Pandoc);
        assert_eq!(result, Some(3));
        assert!(
            !diags.take().is_empty(),
            "malformed YAML should surface a syntax error"
        );
    }

    #[test]
    fn test_yaml_block_emits_content_node() {
        let input = "---\ntitle: Test\nlist:\n  - a\n---\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        let metadata = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::YAML_METADATA)
            .expect("yaml metadata node");
        let content = metadata
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_METADATA_CONTENT)
            .expect("yaml metadata content node");
        assert_eq!(content.text().to_string(), "title: Test\nlist:\n  - a\n");
    }

    #[test]
    fn test_pandoc_title_simple() {
        let lines = vec!["% My Title", "% Author", "% Date", "", "Content"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_pandoc_title_block(&lines, 0, &mut builder);
        assert_eq!(result, Some(3));
    }

    #[test]
    fn test_pandoc_title_with_continuation() {
        let lines = vec![
            "% My Title",
            "  on multiple lines",
            "% Author One",
            "  Author Two",
            "% June 15, 2006",
            "",
            "Content",
        ];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_pandoc_title_block(&lines, 0, &mut builder);
        assert_eq!(result, Some(5));
    }

    #[test]
    fn test_pandoc_title_partial() {
        let lines = vec!["% My Title", "%", "% June 15, 2006", "", "Content"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_pandoc_title_block(&lines, 0, &mut builder);
        assert_eq!(result, Some(3));
    }

    #[test]
    fn test_pandoc_title_not_at_start() {
        let lines = vec!["Content", "% Title"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_pandoc_title_block(&lines, 1, &mut builder);
        assert_eq!(result, None);
    }

    #[test]
    fn test_mmd_title_simple() {
        let lines = vec!["Title: My Title", "Author: Jane Doe", "", "Content"];
        let result = mmd_title_block_end(&lines);
        assert_eq!(result, Some(3));
    }

    #[test]
    fn test_mmd_title_with_continuation() {
        let lines = vec![
            "Title: My title",
            "Author: John Doe",
            "Comment: This is a sample mmd title block, with",
            "  a field spanning multiple lines.",
            "",
            "Body",
        ];
        let result = mmd_title_block_end(&lines);
        assert_eq!(result, Some(5));
    }

    #[test]
    fn test_mmd_title_requires_non_empty_first_value() {
        let lines = vec!["Title:", "Author: Jane Doe", "", "Body"];
        let result = mmd_title_block_end(&lines);
        assert_eq!(result, None);
    }

    #[test]
    fn test_mmd_title_requires_trailing_blank_line() {
        let lines = vec!["Title: My Title", "Author: Jane Doe"];
        let result = mmd_title_block_end(&lines);
        assert_eq!(result, None);
    }

    #[test]
    fn test_indented_yaml_delimiters_are_lossless() {
        let input = "    ---\n    title: Test\n    ...\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn test_valid_yaml_content_embeds_yaml_document_subtree() {
        let input = "---\ntitle: Test\nlist:\n  - a\n---\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        assert_eq!(tree.text().to_string(), input);
        let content = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::YAML_METADATA)
            .and_then(|m| {
                m.children()
                    .find(|c| c.kind() == SyntaxKind::YAML_METADATA_CONTENT)
            })
            .expect("yaml metadata content node");
        // YAML_METADATA_CONTENT plays the singleton-stream role; the
        // YAML_STREAM wrapper is dropped during embedding. The direct
        // child is the YAML_DOCUMENT covering the full content range.
        let first_child = content
            .children()
            .next()
            .expect("embedded yaml subtree child");
        assert_eq!(first_child.kind(), SyntaxKind::YAML_DOCUMENT);
        assert_eq!(first_child.text_range(), content.text_range());
        assert!(
            content
                .descendants()
                .all(|n| n.kind() != SyntaxKind::YAML_STREAM),
            "host embed should not carry the redundant YAML_STREAM wrapper"
        );
    }

    #[test]
    fn test_invalid_yaml_content_falls_back_to_line_tokens() {
        // Unterminated single-quoted scalar is rejected by the YAML
        // validator. The host parser must keep the legacy line-token
        // shape so losslessness holds and the downstream re-parse still
        // reports the diagnostic.
        let input = "---\ntitle: 'unterminated\n---\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        assert_eq!(tree.text().to_string(), input);
        let content = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::YAML_METADATA)
            .and_then(|m| {
                m.children()
                    .find(|c| c.kind() == SyntaxKind::YAML_METADATA_CONTENT)
            })
            .expect("yaml metadata content node");
        assert!(
            content
                .children()
                .all(|c| c.kind() != SyntaxKind::YAML_DOCUMENT),
            "invalid YAML must not embed a YAML_DOCUMENT subtree"
        );
    }
}
