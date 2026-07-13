//! YAML metadata block parsing utilities.

use crate::options::Flavor;
use crate::parser::diagnostics::{Diagnostics, SyntaxError, SyntaxErrorSource};
use crate::parser::utils::helpers::{emit_line_tokens, strip_newline};
use crate::parser::utils::tree_copy::copy_green_children;
use crate::parser::yaml::{YamlValidationContext, locate_yaml_diagnostic_ctx, parse_stream};
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::{GreenNode, GreenNodeBuilder, TextRange};

/// Try to parse a YAML metadata block starting at the given position.
/// Returns the new position after the block if successful, None otherwise.
///
/// A YAML block:
/// - Starts with `---` (not followed by blank line)
/// - Ends with `---` or `...`
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
    let closing_pos = find_yaml_block_closing_pos(lines, pos, at_document_start)?;
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
    if let Some((diag, start, end)) = locate_yaml_diagnostic_ctx(content, "", yaml_ctx) {
        return Some(YamlContentOutcome::Invalid {
            message: diag.message,
            start,
            end,
        });
    }
    let stream = parse_stream(content);
    if !yaml_ctx.consumers().is_empty() && !top_level_is_mapping_or_null(&stream) {
        return None;
    }
    Some(YamlContentOutcome::Valid {
        stream: stream.green().into_owned(),
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

/// Whether a `YAML_SCALAR` node is a plain null scalar (`~`, `null`, `Null`,
/// `NULL`). Quoted forms keep their quotes in `YAML_SCALAR_TEXT`, so they
/// correctly resolve to strings, not null.
fn scalar_is_null(scalar: &SyntaxNode) -> bool {
    let mut text = String::new();
    for token in scalar.children_with_tokens().filter_map(|t| t.into_token()) {
        if token.kind() == SyntaxKind::YAML_SCALAR_TEXT {
            text.push_str(token.text());
        }
    }
    matches!(text.trim(), "~" | "null" | "Null" | "NULL")
}

pub(crate) fn find_yaml_block_closing_pos(
    lines: &[&str],
    pos: usize,
    at_document_start: bool,
) -> Option<usize> {
    if pos >= lines.len() {
        return None;
    }

    let line = lines[pos];

    // Must start with ---
    if line.trim() != "---" {
        return None;
    }

    // If not at document start, previous line must be blank
    if !at_document_start && pos > 0 {
        let prev_line = lines[pos - 1];
        if !prev_line.trim().is_empty() {
            return None;
        }
    }

    // Check that next line (if exists) is NOT blank (this distinguishes from horizontal rule)
    if pos + 1 < lines.len() {
        let next_line = lines[pos + 1];
        if next_line.trim().is_empty() {
            // This is likely a horizontal rule, not YAML
            return None;
        }
    } else {
        // No content after ---, can't be a YAML block
        return None;
    }

    // Find a closing delimiter before emitting; otherwise this is not a valid YAML block.
    let mut closing_pos = None;
    for (i, content_line) in lines.iter().enumerate().skip(pos + 1) {
        if content_line.trim() == "---" || content_line.trim() == "..." {
            closing_pos = Some(i);
            break;
        }
    }
    closing_pos
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
    // Start metadata node
    builder.start_node(SyntaxKind::YAML_METADATA.into());

    // Opening delimiter - strip newline before emitting
    let (text, newline_str) = strip_newline(lines[pos]);
    builder.token(SyntaxKind::YAML_METADATA_DELIM.into(), text);
    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

    builder.start_node(SyntaxKind::YAML_METADATA_CONTENT.into());

    // Embed the in-tree YAML CST under YAML_METADATA_CONTENT when the
    // content validates. On validation failure, fall back to the
    // opaque line-token shape so downstream re-parse (and the host
    // CST snapshot of malformed YAML) keep their current behavior.
    //
    // The stored stream is a `YAML_STREAM` wrapping one or more
    // `YAML_DOCUMENT` children. The wrapper is the YAML-spec stream
    // container — but inside frontmatter the host's
    // `YAML_METADATA_CONTENT` already plays that role (and
    // `find_yaml_block_closing_pos` guarantees a single document by
    // stopping at the first internal `---` / `...`). Splice the stream's
    // children in directly to avoid the redundant wrapper.
    match outcome {
        YamlContentOutcome::Invalid {
            message,
            start,
            end,
        } => {
            // Malformed frontmatter YAML: record the syntax error at its host
            // position (the parser already has the verdict), then fall back to
            // the opaque line-token shape. The content begins at
            // `lines[pos + 1]`, a subslice of the host input, so its host
            // start is the pointer offset from line 0; offsets are identity
            // (no per-line prefix).
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
    if pos != 0 || lines.is_empty() {
        return None;
    }

    let first_line = lines[0];
    if !first_line.trim_start().starts_with('%') {
        return None;
    }

    // Start title block node
    builder.start_node(SyntaxKind::PANDOC_TITLE_BLOCK.into());

    let mut current_pos = 0;
    let mut field_count = 0;

    // Parse up to 3 fields (title, author, date)
    while current_pos < lines.len() && field_count < 3 {
        let line = lines[current_pos];

        // Check if this line starts a field (begins with %)
        if line.trim_start().starts_with('%') {
            emit_line_tokens(builder, line);
            field_count += 1;
            current_pos += 1;

            // Collect continuation lines (start with leading space, not with %)
            while current_pos < lines.len() {
                let cont_line = lines[current_pos];
                if cont_line.is_empty() {
                    // Blank line ends title block
                    break;
                }
                if cont_line.trim_start().starts_with('%') {
                    // Next field
                    break;
                }
                if cont_line.starts_with(' ') || cont_line.starts_with('\t') {
                    // Continuation line
                    emit_line_tokens(builder, cont_line);
                    current_pos += 1;
                } else {
                    // Non-continuation, non-% line ends title block
                    break;
                }
            }
        } else {
            // Line doesn't start with %, title block ends
            break;
        }
    }

    builder.finish_node(); // PandocTitleBlock

    if field_count > 0 {
        Some(current_pos)
    } else {
        None
    }
}

fn mmd_key_value(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once(':')?;
    let key_trimmed = key.trim();
    if key_trimmed.is_empty() {
        return None;
    }
    Some((key_trimmed.to_string(), value.trim().to_string()))
}

/// Try to parse a MultiMarkdown title block starting at the beginning of document.
/// Returns the new position after the block if successful, None otherwise.
///
/// A MultiMarkdown title block:
/// - Must be at document start (pos == 0)
/// - Contains one or more `Key: Value` lines
/// - The first field value must be non-empty
/// - Continuation lines start with leading space or tab
/// - Terminates with a blank line
pub(crate) fn try_parse_mmd_title_block(
    lines: &[&str],
    pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
) -> Option<usize> {
    if pos != 0 || lines.is_empty() {
        return None;
    }

    let mut current_pos = pos;

    // First line must be a key-value pair with non-empty value.
    let first = lines[current_pos];
    let (_first_key, first_value) = mmd_key_value(first)?;
    if first_value.is_empty() {
        return None;
    }

    builder.start_node(SyntaxKind::MMD_TITLE_BLOCK.into());

    while current_pos < lines.len() {
        let line = lines[current_pos];

        if line.trim().is_empty() {
            break;
        }

        if mmd_key_value(line).is_none() {
            builder.finish_node();
            return None;
        }

        emit_line_tokens(builder, line);
        current_pos += 1;

        // Optional continuation lines (must be indented and not key-value starts).
        while current_pos < lines.len() {
            let cont_line = lines[current_pos];
            if cont_line.trim().is_empty() {
                break;
            }

            let trimmed = cont_line.trim_start();
            if mmd_key_value(trimmed).is_some() {
                break;
            }

            if cont_line.starts_with(' ') || cont_line.starts_with('\t') {
                emit_line_tokens(builder, cont_line);
                current_pos += 1;
            } else {
                builder.finish_node();
                return None;
            }
        }
    }

    if current_pos >= lines.len() || !lines[current_pos].trim().is_empty() {
        builder.finish_node();
        return None;
    }

    emit_line_tokens(builder, lines[current_pos]);
    current_pos += 1;

    builder.finish_node(); // MMD_TITLE_BLOCK
    Some(current_pos)
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
        let result = find_yaml_block_closing_pos(&lines, 0, true);
        assert_eq!(result, Some(2));
    }

    #[test]
    fn test_nonmapping_yaml_is_not_metadata() {
        // Pandoc backtracks when the content is well-formed YAML whose top
        // level is not a mapping (scalar, sequence): the lines reparse as
        // ordinary blocks (simple table, or HR + paragraph).
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
        // Pandoc accepts a top-level mapping, and null or comments-only
        // content (empty metadata).
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
        // GFM/CommonMark frontmatter has no asserted YAML metadata consumer,
        // so the mapping gate does not apply there.
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
        // Pandoc hard-errors on YAML parse exceptions (no fallback
        // interpretation exists); the lossless analog is keeping the
        // metadata node and reporting a syntax error.
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
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_mmd_title_block(&lines, 0, &mut builder);
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
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_mmd_title_block(&lines, 0, &mut builder);
        assert_eq!(result, Some(5));
    }

    #[test]
    fn test_mmd_title_requires_non_empty_first_value() {
        let lines = vec!["Title:", "Author: Jane Doe", "", "Body"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_mmd_title_block(&lines, 0, &mut builder);
        assert_eq!(result, None);
    }

    #[test]
    fn test_mmd_title_requires_trailing_blank_line() {
        let lines = vec!["Title: My Title", "Author: Jane Doe"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_mmd_title_block(&lines, 0, &mut builder);
        assert_eq!(result, None);
    }

    #[test]
    fn test_mmd_title_not_at_start() {
        let lines = vec!["Body", "Title: My Title", ""];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_mmd_title_block(&lines, 1, &mut builder);
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
