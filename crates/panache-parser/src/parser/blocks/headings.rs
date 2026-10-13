//! ATX heading parsing utilities.

use crate::options::{Dialect, ParserOptions};
use crate::parser::inlines::hard_breaks;
use crate::parser::inlines::sink::InlineSink;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use crate::parser::utils::attributes::{
    emit_attribute_node, try_parse_trailing_attributes_with_pos,
};
use crate::parser::utils::helpers::trim_end_spaces_tabs;
use crate::parser::utils::inline_emission;

fn try_parse_mmd_header_identifier_with_pos(content: &str) -> Option<(String, usize, usize)> {
    let trimmed = trim_end_spaces_tabs(content);
    let end = trimmed.len();
    let bytes = trimmed.as_bytes();

    if end == 0 || bytes[end - 1] != b']' {
        return None;
    }

    let start = trimmed[..end - 1].rfind('[')?;
    let raw = &trimmed[start..end];
    let inner = &raw[1..raw.len() - 1];
    if inner.trim().is_empty() {
        return None;
    }

    let normalized = inner.split_whitespace().collect::<String>().to_lowercase();
    if normalized.is_empty() {
        return None;
    }

    Some((normalized, start, end))
}

/// Split a trailing backslash line break off a heading's inline text.
///
/// Under the Pandoc dialect the newline that ends a heading line belongs to the
/// heading's inline stream, so `# foo\` reads as `Header 1 [Str "foo",
/// LineBreak]` exactly the way `foo\` does mid-paragraph. Heading emission
/// hands the inline scanner only the text of the line, so the `\`+newline pair
/// never reaches `escapes.rs`; the break has to be recognized here instead. The
/// newline token is the caller's to emit, so the returned break covers only the
/// backslash and whatever whitespace precedes it.
///
/// Whitespace in front of the backslash is part of the break, not content:
/// `# foo \` is `[Str "foo", LineBreak]`, with no `Space`. It rides along in
/// the break token so no byte is lost.
///
/// The last backslash escapes the line ending only if the trailing backslash
/// run is odd: `# foo\\` is `Str "foo\"`, `# foo\\\` is `Str "foo\",
/// LineBreak`. CommonMark keeps the backslash literal in every case, hence the
/// dialect gate.
fn split_trailing_line_break<'a>(
    content: &'a str,
    config: &ParserOptions,
) -> (&'a str, Option<&'a str>) {
    if config.dialect != Dialect::Pandoc {
        return (content, None);
    }

    let backslashes = content.bytes().rev().take_while(|&b| b == b'\\').count();
    if backslashes % 2 == 0 {
        return (content, None);
    }

    let break_start = hard_breaks::ws_run_start(content.as_bytes(), 0, content.len() - 1);
    (&content[..break_start], Some(&content[break_start..]))
}

/// Emit a heading's `HEADING_CONTENT` node, splitting off a trailing backslash
/// line break when the Pandoc dialect calls for one.
///
/// `at_line_end` is false when a closing `#` run or an attribute block follows
/// the content, since then the backslash is not against the line ending and
/// pandoc reads it as an ordinary escape.
fn emit_heading_content(
    builder: &mut impl InlineSink,
    text_content: &str,
    at_line_end: bool,
    config: &ParserOptions,
) {
    let (text_content, hard_line_break) = if at_line_end {
        split_trailing_line_break(text_content, config)
    } else {
        (text_content, None)
    };

    builder.start_node(SyntaxKind::HEADING_CONTENT.into());
    if !text_content.is_empty() {
        inline_emission::emit_inlines(builder, text_content, config, false);
    }
    if let Some(hard_line_break) = hard_line_break {
        builder.token(SyntaxKind::HARD_LINE_BREAK.into(), hard_line_break);
    }
    builder.finish_node();
}

/// Try to parse an ATX heading from content, returns heading level (1-6) if found.
pub fn try_parse_atx_heading(content: &str) -> Option<usize> {
    let line = if let Some(stripped) = content.strip_suffix("\r\n") {
        stripped
    } else if let Some(stripped) = content.strip_suffix('\n') {
        stripped
    } else {
        content
    };
    let trimmed = line.trim_start();

    let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
    if hash_count == 0 || hash_count > 6 {
        return None;
    }

    let after_hashes = &trimmed[hash_count..];
    if !after_hashes.is_empty() && !after_hashes.starts_with(' ') && !after_hashes.starts_with('\t')
    {
        return None;
    }

    let leading_spaces = line.len() - trimmed.len();
    if leading_spaces > 3 {
        return None;
    }

    Some(hash_count)
}

/// Try to parse a setext heading from lines, returns (level, underline_char) if found.
///
/// Setext headings consist of:
/// 1. A non-empty text line (heading content)
/// 2. An underline of `=` (level 1) or `-` (level 2) characters
///
/// Rules:
/// - Underline can be any non-zero length (CommonMark §4.3 / Pandoc both)
/// - Underline can have leading/trailing spaces (up to 3 leading spaces)
/// - All underline characters must be the same (`=` or `-`)
/// - Text line cannot be indented 4+ spaces (would be code block)
/// - Text line cannot be empty/blank
pub fn try_parse_setext_heading(lines: &[&str], pos: usize) -> Option<(usize, char)> {
    if pos >= lines.len() {
        return None;
    }

    let text_line = lines[pos];
    let next_pos = pos + 1;
    if next_pos >= lines.len() {
        return None;
    }

    let underline = lines[next_pos];

    if crate::parser::utils::helpers::is_blank_line(text_line) {
        return None;
    }

    let leading_spaces = text_line.len() - text_line.trim_start().len();
    if leading_spaces >= 4 {
        return None;
    }

    let underline_trimmed = underline.trim();

    if underline_trimmed.is_empty() {
        return None;
    }

    let first_char = underline_trimmed.chars().next()?;
    if first_char != '=' && first_char != '-' {
        return None;
    }

    if !underline_trimmed.chars().all(|c| c == first_char) {
        return None;
    }

    let underline_leading_spaces = underline.len() - underline.trim_start().len();
    if underline_leading_spaces >= 4 {
        return None;
    }

    let level = if first_char == '=' { 1 } else { 2 };

    Some((level, first_char))
}

/// Emit the body of a setext heading (HEADING_CONTENT + underline + newlines).
///
/// The caller is responsible for the surrounding `HEADING` start/finish node.
/// This split lets multi-line setext headings retroactively wrap a previously
/// open paragraph by combining its buffered content with the underline line.
pub(crate) fn emit_setext_heading_body(
    builder: &mut GreenNodeBuilder<'static>,
    text_line: &str,
    underline_line: &str,
    _level: usize,
    config: &ParserOptions,
) {
    emit_setext_heading_text(builder, text_line, config);
    emit_setext_underline(builder, underline_line);
}

/// Emit a setext heading's text half: `HEADING_CONTENT`, any trailing
/// attributes, and the newline that ends the text line.
///
/// Split from [`emit_setext_underline`] so a caller can emit the underline
/// line's container prefix (blockquote markers, list indent) *between* the
/// two — the underline is a second source line, and only the dispatch line's
/// prefix is emitted upstream by the parser core.
pub(crate) fn emit_setext_heading_text(
    builder: &mut impl InlineSink,
    text_line: &str,
    config: &ParserOptions,
) {
    let (text_without_newline, text_newline_str) =
        if let Some(stripped) = text_line.strip_suffix("\r\n") {
            (stripped, "\r\n")
        } else if let Some(stripped) = text_line.strip_suffix('\n') {
            (stripped, "\n")
        } else {
            (text_line, "")
        };

    let text_trimmed = text_without_newline.trim_start();
    let leading_spaces = text_without_newline.len() - text_trimmed.len();

    if leading_spaces > 0 {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &text_without_newline[..leading_spaces],
        );
    }

    let trailing_attrs = if config.extensions.header_attributes {
        try_parse_trailing_attributes_with_pos(text_trimmed)
    } else {
        None
    };

    let (text_content, attr_text, space_before_attrs) =
        if let Some((_attrs, text_before, start_brace_pos)) = trailing_attrs {
            let space = &text_trimmed[text_before.len()..start_brace_pos];
            let raw_attrs = &text_trimmed[start_brace_pos..];
            (text_before, Some(raw_attrs), space)
        } else if config.extensions.mmd_header_identifiers {
            if let Some((_normalized, start_bracket_pos, end_bracket_pos)) =
                try_parse_mmd_header_identifier_with_pos(text_trimmed)
            {
                let text_before = trim_end_spaces_tabs(&text_trimmed[..start_bracket_pos]);
                let space = &text_trimmed[text_before.len()..start_bracket_pos];
                let raw_attrs = &text_trimmed[start_bracket_pos..end_bracket_pos];
                (text_before, Some(raw_attrs), space)
            } else {
                (text_trimmed, None, "")
            }
        } else {
            (text_trimmed, None, "")
        };

    emit_heading_content(builder, text_content, attr_text.is_none(), config);

    if !space_before_attrs.is_empty() {
        builder.token(SyntaxKind::WHITESPACE.into(), space_before_attrs);
    }

    if let Some(attr_text) = attr_text {
        emit_attribute_node(builder, attr_text);
    }

    if !text_newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), text_newline_str);
    }
}

/// Emit a setext heading's underline half: leading spaces, the
/// `SETEXT_HEADING_UNDERLINE` node, and the trailing newline. See
/// [`emit_setext_heading_text`] for why this is a separate entry point.
pub(crate) fn emit_setext_underline(builder: &mut GreenNodeBuilder<'static>, underline_line: &str) {
    let (underline_without_newline, underline_newline_str) =
        if let Some(stripped) = underline_line.strip_suffix("\r\n") {
            (stripped, "\r\n")
        } else if let Some(stripped) = underline_line.strip_suffix('\n') {
            (stripped, "\n")
        } else {
            (underline_line, "")
        };

    let underline_trimmed = underline_without_newline.trim_start();
    let underline_leading_spaces = underline_without_newline.len() - underline_trimmed.len();

    if underline_leading_spaces > 0 {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &underline_without_newline[..underline_leading_spaces],
        );
    }

    builder.start_node(SyntaxKind::SETEXT_HEADING_UNDERLINE.into());
    builder.token(
        SyntaxKind::SETEXT_HEADING_UNDERLINE.into(),
        underline_trimmed,
    );
    builder.finish_node();

    if !underline_newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), underline_newline_str);
    }
}

/// The tail of an ATX heading line --- everything after the opening marker and
/// the spaces behind it --- split into content and closing decoration.
///
/// Concatenating the fields in declaration order reconstructs the input, so
/// emission stays byte-lossless.
struct AtxTail<'a> {
    content: &'a str,
    /// Whitespace between the content and an mmd `[id]` identifier.
    mmd_gap: &'a str,
    /// MultiMarkdown `[id]` identifier, which sits in front of the closing run.
    mmd_attrs: Option<&'a str>,
    /// Whitespace in front of the closing `#` run.
    closing_gap: &'a str,
    closing_run: &'a str,
    /// Whitespace between the closing run and the attribute block, or the
    /// line's trailing whitespace when there is no block.
    attr_gap: &'a str,
    /// Trailing `{...}` attribute block, including the line's trailing
    /// whitespace.
    attrs: Option<&'a str>,
}

impl<'a> AtxTail<'a> {
    fn all_content(content: &'a str) -> Self {
        Self {
            content,
            mmd_gap: "",
            mmd_attrs: None,
            closing_gap: "",
            closing_run: "",
            attr_gap: "",
            attrs: None,
        }
    }

    fn content_ends_the_line(&self) -> bool {
        self.mmd_attrs.is_none() && self.closing_run.is_empty() && self.attrs.is_none()
    }
}

fn closing_run_len(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == b'#' {
        let backslashes = bytes[..end - 1]
            .iter()
            .rev()
            .take_while(|&&b| b == b'\\')
            .count();
        if backslashes % 2 == 1 {
            break;
        }
        end -= 1;
    }
    bytes.len() - end
}

/// Split an ATX heading line's tail the way pandoc's `atxClosing` reads it.
///
/// pandoc parses the closing decoration as an optional mmd `[id]`, then a run
/// of `#`, then an optional `{...}` attribute block, all anchored to the end of
/// the line. The run therefore comes *before* the block, and a block in front
/// of a run is ordinary content: `# foo # {#id}` carries the id, while
/// `# foo {#id} #` is `Header 1 (foo-id) [Str "foo", Space, Str "{#id}"]` ---
/// braces and all, with the id auto-generated from that text.
///
/// The block is only syntax while `header_attributes` is on. Under
/// `commonmark`, `gfm`, and `markdown_mmd` the braces are content
/// (`# foo {#id}` is `[Str "foo", Space, Str "{#id}"]`), which also frees the
/// formatter from re-emitting a closing run in front of them --- the run is
/// load-bearing only while the block is live. The mmd `[id]` identifier rides
/// on its own extension, so `markdown_mmd` still reads `# foo [My ID]`.
fn split_atx_tail<'a>(
    heading_text: &'a str,
    spaces_after_marker: usize,
    config: &ParserOptions,
) -> AtxTail<'a> {
    let mut tail = AtxTail::all_content(heading_text);
    if config.extensions.hugo_shortcodes && ends_in_incomplete_shortcode(heading_text) {
        return tail;
    }
    let mut rest = heading_text;

    if config.extensions.header_attributes
        && let Some((_attrs, text_before, open_brace)) =
            try_parse_trailing_attributes_with_pos(rest)
    {
        tail.attrs = Some(&rest[open_brace..]);
        tail.attr_gap = &rest[text_before.len()..open_brace];
        rest = text_before;
    }

    let run_end = trim_end_spaces_tabs(rest).len();
    let hashes = closing_run_len(&rest[..run_end]);
    if hashes > 0 {
        let before_run = &rest[..run_end - hashes];
        let preceded_by_ws = config.dialect != Dialect::CommonMark
            || before_run
                .chars()
                .last()
                .is_some_and(|c| c == ' ' || c == '\t')
            || (before_run.is_empty() && spaces_after_marker > 0);
        if preceded_by_ws {
            tail.closing_run = &rest[run_end - hashes..run_end];
            if tail.attrs.is_none() {
                tail.attr_gap = &rest[run_end..];
            }
            let content_end = trim_end_spaces_tabs(before_run).len();
            tail.closing_gap = &before_run[content_end..];
            rest = &before_run[..content_end];
        }
    }

    if tail.attrs.is_none()
        && config.extensions.mmd_header_identifiers
        && let Some((_normalized, start_bracket, end_bracket)) =
            try_parse_mmd_header_identifier_with_pos(rest)
    {
        let content = trim_end_spaces_tabs(&rest[..start_bracket]);
        tail.mmd_gap = &rest[content.len()..start_bracket];
        tail.mmd_attrs = Some(&rest[start_bracket..end_bracket]);
        if end_bracket < rest.len() {
            tail.closing_gap = &rest[end_bracket..];
        }
        rest = content;
    }

    tail.content = rest;
    tail
}

fn ends_in_incomplete_shortcode(text: &str) -> bool {
    if !text.contains("{{<") && !text.contains("{{%") {
        return false;
    }
    let index = crate::parser::inlines::hugo::HugoIndex::new(text);
    let mut pos = 0;
    while pos < text.len() {
        if text.as_bytes()[pos] == b'\\' {
            pos += 1;
            if pos < text.len() {
                pos += text[pos..].chars().next().unwrap().len_utf8();
            }
        } else if text.as_bytes()[pos] == b'`'
            && let Some((len, _, _, _)) =
                crate::parser::inlines::code_spans::try_parse_code_span(&text[pos..])
        {
            pos += len;
        } else if let Some(span) = index.scan_at(text, pos) {
            if !span.opening.complete {
                // Decoration cannot begin inside a shortcode that consumes the suffix.
                return true;
            }
            pos += span.end;
        } else {
            pos += text[pos..].chars().next().unwrap().len_utf8();
        }
    }
    false
}

/// Whether re-emitting `content` as an ATX heading's content would read part of
/// it back as trailing decoration rather than as content.
///
/// The formatter drops a heading's closing `#` run, which is only safe while
/// the content it leaves behind cannot pass for decoration itself: pandoc reads
/// the attribute block and the run from the end of the line, so `# foo {#id} #`
/// and `# foo # #` both lose meaning when the run goes. Callers that get `true`
/// here have to keep a run in front of the line ending.
pub fn content_reads_as_decoration(content: &str, config: &ParserOptions) -> bool {
    let tail = split_atx_tail(content, 1, config);
    tail.attrs.is_some() || !tail.closing_run.is_empty()
}

/// Emit an ATX heading node to the builder.
pub(crate) fn emit_atx_heading(
    builder: &mut impl InlineSink,
    content: &str,
    level: usize,
    config: &ParserOptions,
) {
    builder.start_node(SyntaxKind::HEADING.into());

    let (content_without_newline, newline_str) =
        if let Some(stripped) = content.strip_suffix("\r\n") {
            (stripped, "\r\n")
        } else if let Some(stripped) = content.strip_suffix('\n') {
            (stripped, "\n")
        } else {
            (content, "")
        };

    let trimmed = content_without_newline.trim_start();
    let leading_spaces = content_without_newline.len() - trimmed.len();

    if leading_spaces > 0 {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &content_without_newline[..leading_spaces],
        );
    }

    builder.start_node(SyntaxKind::ATX_HEADING_MARKER.into());
    builder.token(SyntaxKind::ATX_HEADING_MARKER.into(), &trimmed[..level]);
    builder.finish_node();

    let after_marker = &trimmed[level..];
    let spaces_after_marker_count = after_marker
        .find(|c: char| !c.is_whitespace())
        .unwrap_or(after_marker.len());

    if spaces_after_marker_count > 0 {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &after_marker[..spaces_after_marker_count],
        );
    }

    let heading_text = &after_marker[spaces_after_marker_count..];

    let tail = split_atx_tail(heading_text, spaces_after_marker_count, config);

    emit_heading_content(builder, tail.content, tail.content_ends_the_line(), config);

    if !tail.mmd_gap.is_empty() {
        builder.token(SyntaxKind::WHITESPACE.into(), tail.mmd_gap);
    }
    if let Some(mmd_attrs) = tail.mmd_attrs {
        emit_attribute_node(builder, mmd_attrs);
    }
    if !tail.closing_gap.is_empty() {
        builder.token(SyntaxKind::WHITESPACE.into(), tail.closing_gap);
    }
    if !tail.closing_run.is_empty() {
        builder.token(SyntaxKind::ATX_HEADING_MARKER.into(), tail.closing_run);
    }
    if !tail.attr_gap.is_empty() {
        builder.token(SyntaxKind::WHITESPACE.into(), tail.attr_gap);
    }
    if let Some(attrs) = tail.attrs {
        emit_attribute_node(builder, attrs);
    }

    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

    builder.finish_node(); // Heading
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_heading() {
        assert_eq!(try_parse_atx_heading("# Heading"), Some(1));
    }

    #[test]
    fn test_level_3_heading() {
        assert_eq!(try_parse_atx_heading("### Level 3"), Some(3));
    }

    #[test]
    fn test_heading_with_leading_spaces() {
        assert_eq!(try_parse_atx_heading("   # Heading"), Some(1));
    }

    #[test]
    fn test_atx_heading_with_attributes_losslessness() {
        use crate::ParserOptions;

        let input = "# Test {#id}\n";
        let config = ParserOptions::default();
        let tree = crate::parse(input, Some(config));

        assert_eq!(
            tree.text().to_string(),
            input,
            "Parser must preserve all bytes including space before attributes"
        );

        let heading = tree.first_child().unwrap();
        assert_eq!(heading.kind(), SyntaxKind::HEADING);

        let mut found_whitespace = false;
        for child in heading.children_with_tokens() {
            if child.kind() == SyntaxKind::WHITESPACE
                && let Some(token) = child.as_token()
            {
                let start: usize = token.text_range().start().into();
                if token.text() == " " && start == 6 {
                    found_whitespace = true;
                    break;
                }
            }
        }
        assert!(
            found_whitespace,
            "Whitespace token between heading content and attributes must be present"
        );
    }

    #[test]
    fn test_atx_heading_closing_hashes_are_lossless() {
        let input = "### Extension: `smart` ###\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        assert_eq!(tree.text().to_string(), input);
    }

    fn commonmark_options() -> ParserOptions {
        let flavor = crate::options::Flavor::CommonMark;
        ParserOptions {
            flavor,
            dialect: Dialect::for_flavor(flavor),
            extensions: crate::options::Extensions::for_flavor(flavor),
            ..ParserOptions::default()
        }
    }

    fn split(text: &str, config: &ParserOptions) -> (String, String) {
        let tail = split_atx_tail(text, 1, config);
        (tail.content.to_string(), tail.closing_run.to_string())
    }

    #[test]
    fn pandoc_closes_a_heading_on_a_run_with_no_space_in_front() {
        let config = ParserOptions::default();
        assert_eq!(split("foo#", &config), ("foo".into(), "#".into()));
        assert_eq!(split("foo###", &config), ("foo".into(), "###".into()));
        assert_eq!(
            split("foo {#id}#", &config),
            ("foo {#id}".into(), "#".into())
        );
    }

    #[test]
    fn commonmark_requires_a_space_in_front_of_a_closing_run() {
        let config = commonmark_options();
        assert_eq!(split("foo#", &config), ("foo#".into(), String::new()));
        assert_eq!(split("foo###", &config), ("foo###".into(), String::new()));
        assert_eq!(split("foo #", &config), ("foo".into(), "#".into()));
    }

    #[test]
    fn a_brace_block_needs_the_header_attributes_extension() {
        let config = commonmark_options();
        assert_eq!(
            split("foo {#id}", &config),
            ("foo {#id}".into(), String::new())
        );
        assert_eq!(
            split("garply#{#id}", &config),
            ("garply#{#id}".into(), String::new())
        );
        let config = ParserOptions::default();
        assert_eq!(split("foo {#id}", &config), ("foo".into(), String::new()));
    }

    #[test]
    fn a_gated_off_brace_block_emits_no_attribute_node() {
        for (input, flavor, want_attribute) in [
            ("# foo {#id}\n", crate::options::Flavor::CommonMark, false),
            (
                "foo {#id}\n===\n",
                crate::options::Flavor::CommonMark,
                false,
            ),
            ("# foo {#id}\n", crate::options::Flavor::Pandoc, true),
            ("foo {#id}\n===\n", crate::options::Flavor::Pandoc, true),
        ] {
            let config = ParserOptions {
                flavor,
                dialect: Dialect::for_flavor(flavor),
                extensions: crate::options::Extensions::for_flavor(flavor),
                ..ParserOptions::default()
            };
            let tree = crate::parse(input, Some(config));
            assert_eq!(tree.text().to_string(), input);
            let has_attribute = tree
                .descendants()
                .any(|n| n.kind() == SyntaxKind::ATTRIBUTE);
            assert_eq!(
                has_attribute,
                want_attribute,
                "{input:?} under {flavor:?} should{} carry an ATTRIBUTE node",
                if want_attribute { "" } else { " not" }
            );
        }
    }

    #[test]
    fn an_escaped_hash_ends_the_closing_run() {
        let config = ParserOptions::default();
        assert_eq!(split("foo \\##", &config), ("foo \\#".into(), "#".into()));
        assert_eq!(split("foo\\###", &config), ("foo\\#".into(), "##".into()));
        assert_eq!(split("foo\\#", &config), ("foo\\#".into(), String::new()));
        assert_eq!(split("foo\\\\##", &config), ("foo\\\\".into(), "##".into()));
    }

    #[test]
    fn a_closing_run_still_ends_the_line() {
        let config = ParserOptions::default();
        assert_eq!(
            split("foo # bar #", &config),
            ("foo # bar".into(), "#".into())
        );
        assert_eq!(split("foo#bar", &config), ("foo#bar".into(), String::new()));
    }

    #[test]
    fn test_four_spaces_not_heading() {
        assert_eq!(try_parse_atx_heading("    # Not heading"), None);
    }

    #[test]
    fn test_no_space_after_hash() {
        assert_eq!(try_parse_atx_heading("#NoSpace"), None);
    }

    #[test]
    fn test_empty_heading() {
        assert_eq!(try_parse_atx_heading("# "), Some(1));
    }

    #[test]
    fn test_level_7_invalid() {
        assert_eq!(try_parse_atx_heading("####### Too many"), None);
    }

    #[test]
    fn test_setext_level_1() {
        let lines = vec!["Heading", "======="];
        assert_eq!(try_parse_setext_heading(&lines, 0), Some((1, '=')));
    }

    #[test]
    fn test_setext_level_2() {
        let lines = vec!["Heading", "-------"];
        assert_eq!(try_parse_setext_heading(&lines, 0), Some((2, '-')));
    }

    #[test]
    fn test_setext_any_underline_length() {
        let lines = vec!["Heading", "="];
        assert_eq!(try_parse_setext_heading(&lines, 0), Some((1, '=')));

        let lines = vec!["Heading", "=="];
        assert_eq!(try_parse_setext_heading(&lines, 0), Some((1, '=')));

        let lines = vec!["Heading", "==="];
        assert_eq!(try_parse_setext_heading(&lines, 0), Some((1, '=')));
    }

    #[test]
    fn test_setext_mixed_chars_invalid() {
        let lines = vec!["Heading", "==-=="];
        assert_eq!(try_parse_setext_heading(&lines, 0), None);
    }

    #[test]
    fn test_setext_with_leading_spaces() {
        let lines = vec!["Heading", "   ======="];
        assert_eq!(try_parse_setext_heading(&lines, 0), Some((1, '=')));
    }

    #[test]
    fn test_setext_with_trailing_spaces() {
        let lines = vec!["Heading", "=======   "];
        assert_eq!(try_parse_setext_heading(&lines, 0), Some((1, '=')));
    }

    #[test]
    fn test_setext_empty_text_line() {
        let lines = vec!["", "======="];
        assert_eq!(try_parse_setext_heading(&lines, 0), None);
    }

    #[test]
    fn test_setext_no_next_line() {
        let lines = vec!["Heading"];
        assert_eq!(try_parse_setext_heading(&lines, 0), None);
    }

    #[test]
    fn test_setext_four_spaces_indent() {
        let lines = vec!["    Heading", "    ======="];
        assert_eq!(try_parse_setext_heading(&lines, 0), None);
    }

    #[test]
    fn test_setext_long_underline() {
        let underline = "=".repeat(100);
        let lines = vec!["Heading", underline.as_str()];
        assert_eq!(try_parse_setext_heading(&lines, 0), Some((1, '=')));
    }

    #[test]
    fn test_parse_mmd_header_identifier_normalizes_like_pandoc() {
        let parsed = try_parse_mmd_header_identifier_with_pos("A heading [My ID]")
            .expect("should parse mmd header identifier");
        assert_eq!(parsed.0, "myid");
        assert_eq!(parsed.1, 10);
    }
}
