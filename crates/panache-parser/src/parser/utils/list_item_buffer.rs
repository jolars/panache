//! Buffer for accumulating list item content before emission.
//!
//! This module provides infrastructure for buffering list item content during parsing,
//! allowing us to determine tight vs loose lists and parse inline elements correctly.

use crate::options::{Dialect, ParserOptions};
use crate::parser::blocks::container_prefix::{
    ContainerPrefixLine, ContainerPrefixState, emit_container_prefix_tokens,
};
use crate::parser::blocks::headings::{emit_atx_heading, try_parse_atx_heading};
use crate::parser::blocks::horizontal_rules::{emit_horizontal_rule, try_parse_horizontal_rule};
use crate::parser::blocks::html_blocks::{
    HtmlBlockType, count_tag_balance, is_pandoc_matched_pair_tag, try_parse_html_block_start,
};
use crate::parser::utils::inline_emission;
use crate::parser::utils::text_buffer::ParagraphBuffer;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::{GreenNodeBuilder, TextSize};

/// A segment in the list item buffer - either text content or a blank line.
#[derive(Debug, Clone)]
pub(crate) enum ListItemContent {
    /// Text content (includes newlines for losslessness)
    Text(String),
    /// Structural blockquote marker emitted inside buffered list-item text.
    BlockquoteMarker {
        leading_spaces: usize,
        has_trailing_space: bool,
    },
}

/// Buffer for accumulating list item content before emission.
///
/// Collects text, blank lines, and structural elements as we parse list item
/// continuation lines. When the list item closes, we can:
/// 1. Determine if it's tight (Plain) or loose (PARAGRAPH)
/// 2. Parse inline elements correctly across continuation lines
/// 3. Emit the complete structure
#[derive(Debug, Default, Clone)]
pub(crate) struct ListItemBuffer {
    /// Segments of content in order
    segments: Vec<ListItemContent>,
}

impl ListItemBuffer {
    /// Create a new empty list item buffer.
    pub(crate) fn new() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    /// Push text content to the buffer.
    pub(crate) fn push_text(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text.is_empty() {
            return;
        }
        self.segments.push(ListItemContent::Text(text));
    }

    pub(crate) fn push_blockquote_marker(
        &mut self,
        leading_spaces: usize,
        has_trailing_space: bool,
    ) {
        self.segments.push(ListItemContent::BlockquoteMarker {
            leading_spaces,
            has_trailing_space,
        });
    }

    /// Check if buffer is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Get the number of segments in the buffer (for debugging).
    pub(crate) fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Return the text of the first segment, if it is a `Text` segment.
    pub(crate) fn first_text(&self) -> Option<&str> {
        match self.segments.first()? {
            ListItemContent::Text(t) => Some(t.as_str()),
            ListItemContent::BlockquoteMarker { .. } => None,
        }
    }

    /// If the buffered text begins with a Pandoc matched-pair HTML open
    /// tag (e.g. `<div ...>`, `<section>`, `<pre>`, `<video>`) whose
    /// opens outnumber its closes in the buffered text, return the tag
    /// name. Used by the block dispatcher to suppress the close-form
    /// dispatch that would otherwise interrupt the LIST_ITEM buffer at
    /// `</div>` / `</pre>` / etc. — letting the buffer accumulate the
    /// full matched-pair text so the emit-time structural lift sees both
    /// open and close.
    ///
    /// Only fires under Pandoc dialect. Under CommonMark, list items
    /// keep their existing behavior (inline HTML inside Plain).
    pub(crate) fn unclosed_pandoc_matched_pair_tag(
        &self,
        config: &ParserOptions,
    ) -> Option<String> {
        if config.dialect != Dialect::Pandoc {
            return None;
        }
        let first = self.first_text()?;
        let first_line_with_nl = first.split_inclusive('\n').next()?;
        let first_line_no_nl = first_line_with_nl
            .strip_suffix("\r\n")
            .or_else(|| first_line_with_nl.strip_suffix('\n'))
            .unwrap_or(first_line_with_nl);
        let HtmlBlockType::BlockTag {
            tag_name,
            is_closing: false,
            ..
        } = try_parse_html_block_start(first_line_no_nl, false)?
        else {
            return None;
        };
        if !is_pandoc_matched_pair_tag(&tag_name) {
            return None;
        }
        let mut opens = 0usize;
        let mut closes = 0usize;
        for segment in &self.segments {
            if let ListItemContent::Text(t) = segment {
                let (o, c) = count_tag_balance(t, &tag_name);
                opens += o;
                closes += c;
            }
        }
        if opens > closes { Some(tag_name) } else { None }
    }

    /// Determine if this list item has blank lines between content.
    ///
    /// Used to decide between Plain (tight) and PARAGRAPH (loose).
    /// Returns true if there's a blank line followed by more content.
    pub(crate) fn has_blank_lines_between_content(&self) -> bool {
        log::trace!(
            "has_blank_lines_between_content: segments={} result=false",
            self.segments.len()
        );

        false
    }

    /// Get concatenated text for inline parsing (excludes blank lines).
    fn get_text_for_parsing(&self) -> String {
        let mut result = String::new();
        for segment in &self.segments {
            if let ListItemContent::Text(text) = segment {
                result.push_str(text);
            }
        }
        result
    }

    fn to_paragraph_buffer(&self) -> ParagraphBuffer {
        let mut paragraph_buffer = ParagraphBuffer::new();
        for segment in &self.segments {
            match segment {
                ListItemContent::Text(text) => paragraph_buffer.push_text(text),
                ListItemContent::BlockquoteMarker {
                    leading_spaces,
                    has_trailing_space,
                } => paragraph_buffer.push_marker(*leading_spaces, *has_trailing_space),
            }
        }
        paragraph_buffer
    }

    /// Emit the buffered content as a Plain or PARAGRAPH block.
    ///
    /// If `use_paragraph` is true, wraps in PARAGRAPH (loose list).
    /// If false, wraps in PLAIN (tight list).
    ///
    /// `content_col` is the enclosing list-item's content column (or 0
    /// outside a list-item). The HTML-block first-line structural lift
    /// uses it to strip the list-item leading indent from continuation
    /// lines before reparsing the body, so `<div>` body parses as
    /// pandoc's `Para` (matched-pair under stripped indent) instead of
    /// `Plain` (the indented-close demotion), and so verbatim-tag
    /// content (`<pre>`, `<style>`, etc.) projects without the leading
    /// indent baked into the RawBlock text. The stripped bytes are
    /// re-emitted as `WHITESPACE` tokens at line starts during graft
    /// so the CST stays byte-equal to source.
    pub(crate) fn emit_as_block(
        &self,
        builder: &mut GreenNodeBuilder<'static>,
        use_paragraph: bool,
        config: &ParserOptions,
        content_col: usize,
        suppress_footnote_refs: bool,
    ) {
        if self.is_empty() {
            return;
        }

        // Get text and parse inline elements
        let text = self.get_text_for_parsing();

        if !text.is_empty() {
            let line_without_newline = text
                .strip_suffix("\r\n")
                .or_else(|| text.strip_suffix('\n'));
            if let Some(line) = line_without_newline
                && !line.contains('\n')
                && !line.contains('\r')
            {
                if let Some(level) = try_parse_atx_heading(line) {
                    emit_atx_heading(builder, &text, level, config);
                    return;
                }
                if try_parse_horizontal_rule(line).is_some() {
                    emit_horizontal_rule(builder, &text);
                    return;
                }
            }

            // Multi-line case: first line is an ATX heading, rest is plain
            // continuation. Pandoc treats `- # Heading\n  Some text` as a
            // list item containing Header + Plain, not a single Plain spanning
            // both lines.
            if self
                .segments
                .iter()
                .all(|s| matches!(s, ListItemContent::Text(_)))
                && let Some(first_nl) = text.find('\n')
            {
                let first_line = &text[..first_nl];
                let after_first = &text[first_nl + 1..];
                if !after_first.is_empty()
                    && let Some(level) = try_parse_atx_heading(first_line)
                {
                    let heading_bytes = &text[..first_nl + 1];
                    emit_atx_heading(builder, heading_bytes, level, config);

                    let block_kind = if use_paragraph {
                        SyntaxKind::PARAGRAPH
                    } else {
                        SyntaxKind::PLAIN
                    };
                    builder.start_node(block_kind.into());
                    inline_emission::emit_inlines(
                        builder,
                        after_first,
                        config,
                        suppress_footnote_refs,
                    );
                    builder.finish_node();
                    return;
                }
            }

            // Pandoc HTML-block-first-line structural lift: when the buffered
            // text begins with a matched HTML block (same-line `<div>...</div>`,
            // single-line comment, `<pre>foo</pre>`, etc.) and the entire
            // buffer is consumed by that block, reparse and graft the inner
            // block as a direct LIST_ITEM child. Without this lift, the
            // dispatcher's inline-HTML path takes over and emits
            // `Plain[RawInline <tag>, body, RawInline </tag>]` instead of
            // `Div [...]` or `RawBlock <tag>`.
            //
            // Multi-line cases where the close tag lives in a sibling
            // HTML_BLOCK (because the dispatcher recognizes Pandoc strict-
            // block close forms as block starts and breaks the buffer) are
            // not handled here — the gate rejects HTML_BLOCK_DIV with only
            // one HTML_BLOCK_TAG child. That sub-target stays open.
            if config.dialect == Dialect::Pandoc
                && self
                    .segments
                    .iter()
                    .all(|s| matches!(s, ListItemContent::Text(_)))
                && try_emit_html_block_lift(builder, &text, config, content_col, use_paragraph)
            {
                return;
            }

            // Structural block lift for marker-line tables and fenced divs.
            // Pandoc recognizes `- | a | b |\n  | - | - |` and `- ::: note\n
            // ...\n  :::` as nested Table / Div; without lifting, the buffer
            // would emit them as PLAIN with raw `|` / `:` text. Mirrors the
            // HTML lift above: strip list-item indent from continuation
            // lines, reparse via the block dispatcher, accept a single root
            // node whose kind is in the allowlist and that consumes the
            // whole buffer.
            if self
                .segments
                .iter()
                .all(|s| matches!(s, ListItemContent::Text(_)))
                && try_emit_table_or_div_lift(builder, &text, config, content_col)
            {
                return;
            }
        }

        let block_kind = if use_paragraph {
            SyntaxKind::PARAGRAPH
        } else {
            SyntaxKind::PLAIN
        };

        builder.start_node(block_kind.into());

        let paragraph_buffer = self.to_paragraph_buffer();
        if !paragraph_buffer.is_empty() {
            paragraph_buffer.emit_with_inlines(builder, config, suppress_footnote_refs);
        } else if !text.is_empty() {
            inline_emission::emit_inlines(builder, &text, config, suppress_footnote_refs);
        }

        builder.finish_node(); // Close PLAIN or PARAGRAPH
    }

    /// Clear the buffer for reuse.
    pub(crate) fn clear(&mut self) {
        self.segments.clear();
    }
}

/// Attempt the Pandoc HTML-block-first-line structural lift on the
/// buffered list-item text. Returns `true` if `text` was emitted as
/// one or more HTML block CST nodes (no surrounding PLAIN/PARAGRAPH
/// wrapper). Returns `false` if the lift gate rejected the case;
/// the caller falls through to its default Plain/Paragraph emission.
///
/// The gate is strict: the inner reparse must produce exactly one
/// top-level HTML_BLOCK or HTML_BLOCK_DIV that consumes every byte
/// of `text` (modulo list-item indent stripping — see `content_col`).
/// For HTML_BLOCK_DIV, a matched open+close is required (>= 2
/// `HTML_BLOCK_TAG` children). This avoids lifting unclosed shapes
/// (where the close tag would live in a separate sibling HTML_BLOCK),
/// which would produce a structurally incomplete CST.
///
/// When `content_col > 0`, continuation lines have up to `content_col`
/// leading spaces stripped before the inner reparse, mirroring
/// pandoc's list-item indent normalization. The stripped bytes are
/// re-injected as `WHITESPACE` tokens at the start of each continuation
/// line during graft so the result is byte-equal to the original
/// buffer text.
fn try_emit_html_block_lift(
    builder: &mut GreenNodeBuilder<'static>,
    text: &str,
    config: &ParserOptions,
    content_col: usize,
    use_paragraph: bool,
) -> bool {
    let first_line = text.split_inclusive('\n').next().unwrap_or(text);
    let first_line_no_nl = first_line
        .strip_suffix("\r\n")
        .or_else(|| first_line.strip_suffix('\n'))
        .unwrap_or(first_line);
    if try_parse_html_block_start(first_line_no_nl, false).is_none() {
        return false;
    }

    let (parse_text, prefixes) = if content_col > 0 {
        strip_list_item_indent(text, content_col)
    } else {
        (text.to_string(), Vec::new())
    };

    let refdefs = config.refdef_labels.clone().unwrap_or_default();
    let inner_root = crate::parser::parse_with_refdefs(&parse_text, Some(config.clone()), refdefs);

    let children: Vec<SyntaxNode> = inner_root.children().collect();
    if children.is_empty() {
        return false;
    }
    let first = &children[0];
    if !matches!(
        first.kind(),
        SyntaxKind::HTML_BLOCK | SyntaxKind::HTML_BLOCK_DIV
    ) {
        return false;
    }
    let total_end = children.last().unwrap().text_range().end();
    if total_end != TextSize::of(parse_text.as_str()) {
        return false;
    }

    // Single-child path: existing same-line / fully-contained lift.
    // Multi-child path: trailing-text split — the inner dispatcher
    // produced sibling block(s) after the HTML_BLOCK / HTML_BLOCK_DIV.
    // Sources:
    //   - `try_parse_comment_pi_with_trailing_split` for `<!--…--> trail`
    //     and `<?…?> trail` (HTML_BLOCK + PARAGRAPH).
    //   - Same-line div / non-div strict-block lift's trailing branch
    //     for `<div>foo</div>bar` (HTML_BLOCK_DIV + PARAGRAPH) and
    //     `<form>foo</form>bar` (also HTML_BLOCK + PARAGRAPH after the
    //     existing strict-block matched-pair lift fires).
    // The trailing PARAGRAPH is retagged to PLAIN for tight list items
    // so the item shape matches pandoc (`[RawBlock, Plain[trailing]]`
    // for tight, `[RawBlock, Para[...]]` for loose). N>2 children would
    // require Para→Plain SoftBreak fusion across HTML-block boundaries
    // (0390 blocked); leave those shapes to the inline path until that
    // gap closes.
    let multi_child_trailing = if children.len() == 1 {
        false
    } else if children.len() == 2
        && matches!(
            first.kind(),
            SyntaxKind::HTML_BLOCK | SyntaxKind::HTML_BLOCK_DIV
        )
        && children[1].kind() == SyntaxKind::PARAGRAPH
    {
        true
    } else {
        return false;
    };

    if first.kind() == SyntaxKind::HTML_BLOCK_DIV {
        let html_block_tag_count = first
            .children()
            .filter(|c| c.kind() == SyntaxKind::HTML_BLOCK_TAG)
            .count();
        if html_block_tag_count < 2 {
            return false;
        }
    }

    let prefix_lines: Vec<ContainerPrefixLine> = prefixes
        .into_iter()
        .map(ContainerPrefixLine::list_only)
        .collect();
    let mut prefix_state = ContainerPrefixState::new(prefix_lines);
    if multi_child_trailing {
        graft_node(builder, first, &mut prefix_state);
        let trailing_kind = if use_paragraph {
            SyntaxKind::PARAGRAPH
        } else {
            SyntaxKind::PLAIN
        };
        graft_node_retag_root(builder, &children[1], &mut prefix_state, trailing_kind);
    } else {
        graft_node(builder, first, &mut prefix_state);
    }
    true
}

/// Structural lift for pipe tables, grid tables, and fenced divs whose
/// opener sits on the list-item marker line (or on the first non-blank
/// continuation line of a buffered list item). Returns `true` when the
/// buffered text was emitted as a single LIST_ITEM-child block. The
/// strict single-root + total-end-coverage gate makes "lift failed"
/// indistinguishable from "buffer is not actually a table/div" — the
/// caller falls through to its PLAIN/PARAGRAPH wrapper.
fn try_emit_table_or_div_lift(
    builder: &mut GreenNodeBuilder<'static>,
    text: &str,
    config: &ParserOptions,
    content_col: usize,
) -> bool {
    let first_line = text.split_inclusive('\n').next().unwrap_or(text);
    let first_line_no_nl = first_line
        .strip_suffix("\r\n")
        .or_else(|| first_line.strip_suffix('\n'))
        .unwrap_or(first_line);
    let trimmed = first_line_no_nl.trim_start();
    let first_byte = trimmed.as_bytes().first().copied();
    if !matches!(first_byte, Some(b'|') | Some(b'+') | Some(b':')) {
        return false;
    }

    let (parse_text, prefixes) = if content_col > 0 {
        strip_list_item_indent(text, content_col)
    } else {
        (text.to_string(), Vec::new())
    };

    let refdefs = config.refdef_labels.clone().unwrap_or_default();
    let inner_root = crate::parser::parse_with_refdefs(&parse_text, Some(config.clone()), refdefs);

    let children: Vec<SyntaxNode> = inner_root.children().collect();
    if children.len() != 1 {
        return false;
    }
    let first = &children[0];
    if !matches!(
        first.kind(),
        SyntaxKind::PIPE_TABLE | SyntaxKind::GRID_TABLE | SyntaxKind::FENCED_DIV
    ) {
        return false;
    }
    if first.text_range().end() != TextSize::of(parse_text.as_str()) {
        return false;
    }

    let prefix_lines: Vec<ContainerPrefixLine> = prefixes
        .into_iter()
        .map(ContainerPrefixLine::list_only)
        .collect();
    let mut prefix_state = ContainerPrefixState::new(prefix_lines);
    graft_node(builder, first, &mut prefix_state);
    true
}

fn graft_node_retag_root(
    builder: &mut GreenNodeBuilder<'static>,
    node: &SyntaxNode,
    prefix: &mut Option<ContainerPrefixState>,
    new_kind: SyntaxKind,
) {
    builder.start_node(new_kind.into());
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n) => graft_node(builder, &n, prefix),
            rowan::NodeOrToken::Token(t) => {
                emit_grafted_token(builder, t.kind(), t.text(), prefix);
            }
        }
    }
    builder.finish_node();
}

/// Strip up to `content_col` leading-space bytes from each continuation
/// line of `text` (lines after the first). The first line is left
/// untouched — its leading columns are owned by the list marker and
/// its post-marker spaces. Returns the stripped text plus a per-line
/// prefix vector for losslessness re-injection during graft.
fn strip_list_item_indent(text: &str, content_col: usize) -> (String, Vec<String>) {
    let mut stripped = String::with_capacity(text.len());
    let mut prefixes: Vec<String> = Vec::new();
    for (i, line) in text.split_inclusive('\n').enumerate() {
        if i == 0 {
            prefixes.push(String::new());
            stripped.push_str(line);
            continue;
        }
        let mut consumed = 0usize;
        let mut col = 0usize;
        for &b in line.as_bytes() {
            if col >= content_col {
                break;
            }
            match b {
                b' ' => {
                    col += 1;
                    consumed += 1;
                }
                b'\t' => {
                    let next = (col / 4 + 1) * 4;
                    if next > content_col {
                        break;
                    }
                    col = next;
                    consumed += 1;
                }
                _ => break,
            }
        }
        prefixes.push(line[..consumed].to_string());
        stripped.push_str(&line[consumed..]);
    }
    (stripped, prefixes)
}

fn graft_node(
    builder: &mut GreenNodeBuilder<'static>,
    node: &SyntaxNode,
    prefix: &mut Option<ContainerPrefixState>,
) {
    builder.start_node(node.kind().into());
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n) => graft_node(builder, &n, prefix),
            rowan::NodeOrToken::Token(t) => {
                emit_grafted_token(builder, t.kind(), t.text(), prefix);
            }
        }
    }
    builder.finish_node();
}

fn emit_grafted_token(
    builder: &mut GreenNodeBuilder<'static>,
    kind: SyntaxKind,
    text: &str,
    prefix: &mut Option<ContainerPrefixState>,
) {
    if let Some(state) = prefix.as_mut() {
        if state.at_line_start {
            if let Some(line_prefix) = state.prefixes.get(state.line_idx) {
                emit_container_prefix_tokens(builder, line_prefix);
            }
            state.at_line_start = false;
        }
        builder.token(kind.into(), text);
        if kind == SyntaxKind::NEWLINE || kind == SyntaxKind::BLANK_LINE {
            state.line_idx += 1;
            state.at_line_start = true;
        }
    } else {
        builder.token(kind.into(), text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer_is_empty() {
        let buffer = ListItemBuffer::new();
        assert!(buffer.is_empty());
        assert!(!buffer.has_blank_lines_between_content());
    }

    #[test]
    fn test_push_single_text() {
        let mut buffer = ListItemBuffer::new();
        buffer.push_text("Hello, world!");
        assert!(!buffer.is_empty());
        assert!(!buffer.has_blank_lines_between_content());
        assert_eq!(buffer.get_text_for_parsing(), "Hello, world!");
    }

    #[test]
    fn test_push_multiple_text_segments() {
        let mut buffer = ListItemBuffer::new();
        buffer.push_text("Line 1\n");
        buffer.push_text("Line 2\n");
        buffer.push_text("Line 3");
        assert_eq!(buffer.get_text_for_parsing(), "Line 1\nLine 2\nLine 3");
    }

    #[test]
    fn test_clear_buffer() {
        let mut buffer = ListItemBuffer::new();
        buffer.push_text("Some text");
        assert!(!buffer.is_empty());

        buffer.clear();
        assert!(buffer.is_empty());
        assert_eq!(buffer.get_text_for_parsing(), "");
    }

    #[test]
    fn test_empty_text_ignored() {
        let mut buffer = ListItemBuffer::new();
        buffer.push_text("");
        assert!(buffer.is_empty());
    }
}
