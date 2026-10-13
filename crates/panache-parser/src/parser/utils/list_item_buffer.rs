//! Buffer for accumulating list item content before emission.
//!
//! This module provides infrastructure for buffering list item content during parsing,
//! allowing us to determine tight vs loose lists and parse inline elements correctly.

use crate::options::{Dialect, ParserOptions};
use crate::parser::blocks::container_prefix::{
    ContainerPrefixLine, ContainerPrefixState, emit_grafted_token,
};
use crate::parser::blocks::figures::paragraph_is_standalone_image;
use crate::parser::blocks::headings::{emit_atx_heading, try_parse_atx_heading};
use crate::parser::blocks::horizontal_rules::{emit_horizontal_rule, try_parse_horizontal_rule};
use crate::parser::blocks::html_blocks::{
    HtmlBlockType, count_tag_balance, is_pandoc_matched_pair_tag, try_parse_html_block_start,
};
use crate::parser::blocks::paragraphs::update_display_math_state;
use crate::parser::blocks::tables::try_parse_pipe_separator;
use crate::parser::inlines::code_spans::pending_code_span_openers;
use crate::parser::utils::container_stack::{
    OpenDisplayMath, gobble_chain_prefix_len as item_indent_prefix_len,
};
use crate::parser::utils::helpers::{is_blank_line, trim_end_newlines};
use crate::parser::utils::inline_emission;
use crate::parser::utils::text_buffer::ParagraphBuffer;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::{GreenNodeBuilder, TextSize};

/// A segment in the list item buffer - either text content or a blank line.
#[derive(Debug, Clone)]
pub(crate) enum ListItemContent {
    /// Text content (includes newlines for losslessness)
    Text(String),
    /// Container bytes held out of an opaque inline region.
    Prefix(String),
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
    /// The already-emitted checkbox starts a paragraph even when the
    /// remaining text resembles a heading or thematic break.
    has_task_checkbox: bool,
    /// Display-math region (`$$ ... $$`, `\[ ... \]`, `\\[ ... \\]`) left
    /// open by the buffered lines. Consulted by the parser's list-item hold
    /// so block detection cannot split an open region into a `TEX_BLOCK`
    /// (same failure mode the paragraph tracker fixes at top level).
    open_display_math: Option<OpenDisplayMath>,
    /// Matched-pair HTML open tag surviving a mid-item partial flush.
    ///
    /// `clear()` folds the segments' unclosed tag in here before draining,
    /// so the markdown-in-html signal outlives the buffered text it was
    /// computed from. Read via [`Self::open_matched_pair_tag`] by
    /// `ContainerPrefix::from_stack` to arm the HTML-closer line-extent
    /// terminator for containers pushed above the item.
    carried_unclosed_html_tag: Option<String>,
}

impl ListItemBuffer {
    /// Create a new empty list item buffer.
    pub(crate) fn new() -> Self {
        Self {
            segments: Vec::new(),
            has_task_checkbox: false,
            open_display_math: None,
            carried_unclosed_html_tag: None,
        }
    }

    pub(crate) fn mark_task_paragraph(&mut self) {
        self.has_task_checkbox = true;
    }

    pub(crate) fn is_task_paragraph(&self) -> bool {
        self.has_task_checkbox
    }

    /// Push text content to the buffer, tracking display-math delimiters
    /// per line (the marker-line seed can carry multiple lines).
    pub(crate) fn push_text(&mut self, text: impl Into<String>, config: &ParserOptions) {
        let text = text.into();
        if text.is_empty() {
            return;
        }
        for line in text.split_inclusive('\n') {
            update_display_math_state(trim_end_newlines(line), &mut self.open_display_math, config);
        }
        self.segments.push(ListItemContent::Text(text));
    }

    pub(crate) fn push_prefix(&mut self, prefix: &str) {
        if !prefix.is_empty() {
            self.segments
                .push(ListItemContent::Prefix(prefix.to_owned()));
        }
    }

    /// Whether the buffered lines left a display-math region open.
    pub(crate) fn has_open_display_math(&self) -> bool {
        self.open_display_math.is_some()
    }

    /// Backtick runs in the buffered text that are still waiting for a closer.
    ///
    /// The list-item analogue of
    /// [`ParagraphBuffer::pending_code_span_openers`]: buffered item content is
    /// an open paragraph by another name, so a run left open there governs
    /// block detection on the closing line just the same.
    pub(crate) fn pending_code_span_openers(&self) -> Vec<usize> {
        let mut text = String::new();
        for segment in &self.segments {
            if let ListItemContent::Text(t) = segment {
                text.push_str(t);
            }
        }
        pending_code_span_openers(&text)
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

    /// Number of source lines held in `Text` segments, counted over their
    /// concatenation (a segment need not end at a line boundary). A trailing
    /// line without a newline counts as one; structural marker segments
    /// contribute nothing.
    pub(crate) fn buffered_line_count(&self) -> usize {
        let mut lines = 0usize;
        let mut ends_mid_line = false;
        for segment in &self.segments {
            if let ListItemContent::Text(t) = segment {
                lines += t.matches('\n').count();
                if !t.is_empty() {
                    ends_mid_line = !t.ends_with('\n');
                }
            }
        }
        lines + usize::from(ends_mid_line)
    }

    /// Number of segments in the buffer. Counts structural marker segments
    /// too, so it must not be read as a line count — use
    /// [`Self::buffered_line_count`] for that.
    #[cfg(test)]
    pub(crate) fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// True when every segment is `Text` — no structural marker segments.
    pub(crate) fn is_text_only(&self) -> bool {
        self.segments
            .iter()
            .all(|s| matches!(s, ListItemContent::Text(_)))
    }

    /// Return the text of the first segment, if it is a `Text` segment.
    pub(crate) fn first_text(&self) -> Option<&str> {
        match self.segments.first()? {
            ListItemContent::Text(t) => Some(t.as_str()),
            ListItemContent::BlockquoteMarker { .. } | ListItemContent::Prefix(_) => None,
        }
    }

    /// The buffer's only `Text` segment, when it leads and every other
    /// segment is a structural blockquote marker.
    ///
    /// A quoted item (`> - a | b`) buffers the *next* line's `>` marker
    /// before that line's text reaches the block dispatcher, so "the
    /// buffer holds exactly one line so far" cannot be answered by
    /// counting segments.
    pub(crate) fn sole_text_segment(&self) -> Option<&str> {
        let text = self.first_text()?;
        self.segments[1..]
            .iter()
            .all(|s| matches!(s, ListItemContent::BlockquoteMarker { .. }))
            .then_some(text)
    }

    /// The structural blockquote-marker segments buffered after the leading
    /// text segment, in push order. A consumer that re-emits the buffered
    /// line itself (rather than draining the buffer) reads these back out
    /// before `clear()` and re-injects them, keeping the parse lossless.
    pub(crate) fn trailing_blockquote_markers(&self) -> Vec<(usize, bool)> {
        self.segments
            .iter()
            .skip(1)
            .filter_map(|s| match s {
                ListItemContent::BlockquoteMarker {
                    leading_spaces,
                    has_trailing_space,
                } => Some((*leading_spaces, *has_trailing_space)),
                ListItemContent::Text(_) | ListItemContent::Prefix(_) => None,
            })
            .collect()
    }

    fn blockquote_prefixes(&self) -> Vec<String> {
        let mut prefixes: Vec<String> = Vec::new();
        if !self
            .segments
            .iter()
            .any(|s| matches!(s, ListItemContent::BlockquoteMarker { .. }))
        {
            return prefixes;
        }
        let mut line = 0usize;
        for segment in &self.segments {
            match segment {
                ListItemContent::Text(text) => line += text.matches('\n').count(),
                ListItemContent::Prefix(_) => {}
                ListItemContent::BlockquoteMarker {
                    leading_spaces,
                    has_trailing_space,
                } => {
                    if prefixes.len() <= line {
                        prefixes.resize(line + 1, String::new());
                    }
                    let entry = &mut prefixes[line];
                    entry.extend(std::iter::repeat_n(' ', *leading_spaces));
                    entry.push('>');
                    if *has_trailing_space {
                        entry.push(' ');
                    }
                }
            }
        }
        prefixes
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
        self.segments_unclosed_matched_pair_tag()
    }

    /// Dialect-free body of [`Self::unclosed_pandoc_matched_pair_tag`]:
    /// the tag opened on the *current segments'* first line, if any.
    fn segments_unclosed_matched_pair_tag(&self) -> Option<String> {
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

    fn combined_unclosed_tag(&self) -> Option<String> {
        if let Some(fresh) = self.segments_unclosed_matched_pair_tag() {
            return Some(fresh);
        }
        let carried = self.carried_unclosed_html_tag.as_deref()?;
        let mut opens = 0usize;
        let mut closes = 0usize;
        for segment in &self.segments {
            if let ListItemContent::Text(t) = segment {
                let (o, c) = count_tag_balance(t, carried);
                opens += o;
                closes += c;
            }
        }
        (1 + opens > closes).then(|| carried.to_string())
    }

    /// Like [`Self::unclosed_pandoc_matched_pair_tag`], but surviving the
    /// mid-item partial flush via the carried field, so it still answers
    /// after an interrupting block (blockquote, nested list, ...) emptied
    /// the segments. Used by `ContainerPrefix::from_stack` to arm the
    /// HTML-closer line-extent terminator (pandoc's
    /// `notFollowedByHtmlCloser`) for containers pushed above the item.
    ///
    /// The dispatcher gate keeps using the segments-only accessor: the
    /// carried tag must not change how post-flush close-form lines
    /// dispatch, only where container line runs end.
    pub(crate) fn open_matched_pair_tag(&self, config: &ParserOptions) -> Option<String> {
        if config.dialect != Dialect::Pandoc {
            return None;
        }
        self.combined_unclosed_tag()
    }

    /// Whether the buffered text ends in an open paragraph, which a lazy
    /// continuation line could extend. False when the buffer holds no
    /// non-blank text (an item that so far is only its marker), or only a
    /// single line that emits as an ATX heading or thematic break.
    /// `content_col` is the item's content column: continuation lines carry
    /// up to that much item indentation, which isn't part of the block.
    pub(crate) fn has_open_paragraph(&self, content_col: usize) -> bool {
        if self.has_task_checkbox {
            return true;
        }

        let mut saw_nonblank_line = false;
        let mut line_opens_paragraph = |line: &str| {
            if is_blank_line(line) {
                return false;
            }
            if saw_nonblank_line {
                return true;
            }
            saw_nonblank_line = true;
            let line = trim_end_newlines(line);
            let item_indent = line
                .bytes()
                .take(content_col)
                .take_while(|&b| b == b' ')
                .count();
            let line = &line[item_indent..];
            try_parse_atx_heading(line).is_none() && try_parse_horizontal_rule(line).is_none()
        };

        // Most segments contain complete lines. Only join fragments when a
        // source line crosses segments, and stop once a paragraph is known.
        let mut partial_line = String::new();
        for segment in &self.segments {
            let ListItemContent::Text(text) = segment else {
                continue;
            };
            for fragment in text.split_inclusive('\n') {
                if partial_line.is_empty() && fragment.ends_with('\n') {
                    if line_opens_paragraph(fragment) {
                        return true;
                    }
                } else {
                    partial_line.push_str(fragment);
                    if fragment.ends_with('\n') {
                        if line_opens_paragraph(&partial_line) {
                            return true;
                        }
                        partial_line.clear();
                    }
                }
            }
        }
        line_opens_paragraph(&partial_line)
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

    fn get_text_for_parsing(&self) -> String {
        let mut result = String::new();
        for segment in &self.segments {
            if let ListItemContent::Text(text) = segment {
                result.push_str(text);
            }
        }
        result
    }

    /// Build the inline-parsing buffer, holding each continuation line's
    /// item indent out of the text as an [`ParagraphBuffer::push_indent`]
    /// segment.
    ///
    /// Pandoc's `listLine` gobbles the item's content column off every line
    /// after the marker line before the item's raw text is reparsed, so
    /// interior whitespace inside an inline construct is measured from the
    /// content column: ``- a\n   `x\n   y` `` is `Code "x  y"`, not
    /// `Code "x    y"`. Buffering the raw line would bake those columns into
    /// the code span. The held-out bytes are spliced back as `WHITESPACE`
    /// tokens at emission, mirroring how the blockquote path re-injects its
    /// `>` markers.
    ///
    /// `gobble` is the whole enclosing container chain, not just this item's
    /// column: the buffered lines are raw source, so an enclosing footnote
    /// definition's or outer item's share has not been taken off them yet.
    pub(crate) fn to_paragraph_buffer(&self, gobble: &[usize]) -> ParagraphBuffer {
        let mut paragraph_buffer = ParagraphBuffer::new();
        let mut at_line_start = false;
        for segment in &self.segments {
            match segment {
                ListItemContent::Text(text) => {
                    if gobble.is_empty() || (!at_line_start && !text.contains('\n')) {
                        paragraph_buffer.push_text(text);
                        continue;
                    }
                    for line in text.split_inclusive('\n') {
                        if at_line_start {
                            let consumed = item_indent_prefix_len(line, gobble);
                            paragraph_buffer.push_indent(&line[..consumed]);
                            paragraph_buffer.push_text(&line[consumed..]);
                        } else {
                            paragraph_buffer.push_text(line);
                        }
                        at_line_start = line.ends_with('\n');
                    }
                }
                ListItemContent::Prefix(prefix) => {
                    paragraph_buffer.push_indent(prefix);
                    at_line_start = false;
                }
                ListItemContent::BlockquoteMarker {
                    leading_spaces,
                    has_trailing_space,
                } => {
                    paragraph_buffer.push_marker(*leading_spaces, *has_trailing_space);
                    at_line_start = false;
                }
            }
        }
        paragraph_buffer
    }

    /// Emit the buffered content as a Plain or PARAGRAPH block.
    ///
    /// If `use_paragraph` is true, wraps in PARAGRAPH (loose list).
    /// If false, wraps in PLAIN (tight list).
    ///
    /// `gobble` is the enclosing container indent chain (empty outside a
    /// list-item). The HTML-block first-line structural lift
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
        gobble: &[usize],
        suppress_footnote_refs: bool,
        allow_unclosed_div: bool,
    ) {
        if self.is_empty() {
            return;
        }

        // Dispatch may buffer the next block's quote prefix before flushing
        // this block. Those bytes belong outside the prose wrapper, including
        // when a heading lift returns before ordinary inline emission.
        let text_end = self
            .segments
            .iter()
            .rposition(|segment| matches!(segment, ListItemContent::Text(_)))
            .map_or(0, |i| i + 1);
        if text_end < self.segments.len() {
            if text_end > 0 {
                let mut content = self.clone();
                content.segments.truncate(text_end);
                content.emit_as_block(
                    builder,
                    use_paragraph,
                    config,
                    gobble,
                    suppress_footnote_refs,
                    allow_unclosed_div,
                );
            }
            let mut trivia = Self::new();
            trivia.segments = self.segments[text_end..].to_vec();
            trivia.to_paragraph_buffer(gobble).emit_with_inlines(
                builder,
                config,
                suppress_footnote_refs,
            );
            return;
        }

        let text = self.get_text_for_parsing();

        if !text.is_empty() && !self.has_task_checkbox {
            let line_without_newline = text
                .strip_suffix("\r\n")
                .or_else(|| text.strip_suffix('\n'));
            if let Some(line) = line_without_newline
                && !line.contains('\n')
                && !line.contains('\r')
            {
                let detect_line = &line[item_indent_prefix_len(line, gobble)..];
                if let Some(level) = try_parse_atx_heading(detect_line) {
                    emit_atx_heading(builder, &text, level, config);
                    return;
                }
                if try_parse_horizontal_rule(detect_line).is_some() {
                    emit_horizontal_rule(builder, &text);
                    return;
                }
            }

            let bq_prefixes = self.blockquote_prefixes();
            let line0_has_bq = bq_prefixes.first().is_some_and(|p| !p.is_empty());

            if !line0_has_bq
                && config.extensions.hugo_shortcodes
                && let Some(tail) = self
                    .to_paragraph_buffer(gobble)
                    .try_emit_hugo_heading(builder, config)
            {
                if !tail.is_empty() {
                    builder.start_node(
                        if use_paragraph {
                            SyntaxKind::PARAGRAPH
                        } else {
                            SyntaxKind::PLAIN
                        }
                        .into(),
                    );
                    tail.emit_with_inlines(builder, config, suppress_footnote_refs);
                    builder.finish_node();
                }
                return;
            }

            if !line0_has_bq && let Some(first_nl) = text.find('\n') {
                let first_line = &text[..first_nl];
                let after_first = &text[first_nl + 1..];
                let detect_first = &first_line[item_indent_prefix_len(first_line, gobble)..];
                if !after_first.is_empty()
                    && let Some(level) = try_parse_atx_heading(detect_first)
                {
                    let heading_bytes = &text[..first_nl + 1];
                    emit_atx_heading(builder, heading_bytes, level, config);

                    let block_kind = if use_paragraph {
                        SyntaxKind::PARAGRAPH
                    } else {
                        SyntaxKind::PLAIN
                    };
                    builder.start_node(block_kind.into());
                    if self.is_text_only() {
                        inline_emission::emit_inlines(
                            builder,
                            after_first,
                            config,
                            suppress_footnote_refs,
                        );
                    } else {
                        self.trailing_after_first_line(config)
                            .to_paragraph_buffer(gobble)
                            .emit_with_inlines(builder, config, suppress_footnote_refs);
                    }
                    builder.finish_node();
                    return;
                }
            }

            if config.dialect == Dialect::Pandoc
                && !line0_has_bq
                && try_emit_html_block_lift(
                    builder,
                    &text,
                    config,
                    gobble,
                    &bq_prefixes,
                    use_paragraph,
                    "",
                    allow_unclosed_div,
                )
            {
                return;
            }

            if try_emit_table_or_div_lift(builder, &text, config, gobble, &bq_prefixes) {
                return;
            }
        }

        let block_kind = if paragraph_is_standalone_image(&text, config) {
            SyntaxKind::FIGURE
        } else if use_paragraph {
            SyntaxKind::PARAGRAPH
        } else {
            SyntaxKind::PLAIN
        };

        builder.start_node(block_kind.into());

        let paragraph_buffer = self.to_paragraph_buffer(gobble);
        if !paragraph_buffer.is_empty() {
            paragraph_buffer.emit_with_inlines(builder, config, suppress_footnote_refs);
        } else if !text.is_empty() {
            inline_emission::emit_inlines(builder, &text, config, suppress_footnote_refs);
        }

        builder.finish_node(); // Close FIGURE, PLAIN, or PARAGRAPH
    }

    /// Whether the buffered lines are the head of a grid table that is
    /// still open: the first non-blank line is a full grid separator (the
    /// table opened at the buffer's start, where the emit-time lift can
    /// see it) and the last line is a row or row boundary that expects
    /// more table under it. Backs the parser core's partial-separator
    /// hold, which decides whether a `+   +---+` continuation line is
    /// table structure or a nested `+` list marker.
    ///
    /// `content_col` is the item's content column; continuation lines
    /// carry it while the marker-line seed does not, so each line is
    /// dedented by at most that much before classification.
    pub(crate) fn is_open_grid_table(&self, content_col: usize) -> bool {
        use crate::parser::blocks::container_prefix::strip_list_indent;
        use crate::parser::blocks::tables::{
            is_grid_content_row, try_parse_grid_partial_separator, try_parse_grid_separator,
        };

        let text = self.get_text_for_parsing();
        let mut lines = text
            .lines()
            .map(|line| strip_list_indent(line, content_col))
            .filter(|line| !line.trim().is_empty());
        let Some(first) = lines.next() else {
            return false;
        };
        if try_parse_grid_separator(first).is_none() {
            return false;
        }
        let last = lines.next_back().unwrap_or(first);
        is_grid_content_row(last) || try_parse_grid_partial_separator(last).is_some()
    }

    /// The buffered content after the first line of the concatenated text,
    /// as a new buffer: the trailing `Text` bytes plus the structural
    /// blockquote markers between them, in segment order. Backs the
    /// multi-line ATX split for marker-holding buffers, whose trailing
    /// block must re-inject the held-out `>` bytes.
    fn trailing_after_first_line(&self, config: &ParserOptions) -> ListItemBuffer {
        let mut trailing = ListItemBuffer::new();
        let mut past_first_line = false;
        for segment in &self.segments {
            match segment {
                ListItemContent::Text(text) => {
                    if past_first_line {
                        trailing.push_text(text.clone(), config);
                    } else if let Some(nl) = text.find('\n') {
                        past_first_line = true;
                        trailing.push_text(&text[nl + 1..], config);
                    }
                }
                ListItemContent::Prefix(prefix) => trailing.push_prefix(prefix),
                ListItemContent::BlockquoteMarker {
                    leading_spaces,
                    has_trailing_space,
                } => {
                    trailing.push_blockquote_marker(*leading_spaces, *has_trailing_space);
                }
            }
        }
        trailing
    }

    /// Clear the buffer for reuse. Also drops any open display-math state:
    /// every clear site starts a fresh paragraph-like chunk (blank-line
    /// flush, first-line conversion, setext fold), and a blank line ends
    /// the math region just like it ends a paragraph.
    ///
    /// The unclosed matched-pair HTML tag is the exception: it is folded
    /// into the carried field first, because pandoc's markdown-in-html
    /// span outlives the paragraph chunk that opened it.
    pub(crate) fn clear(&mut self) {
        self.carried_unclosed_html_tag = self.combined_unclosed_tag();
        self.segments.clear();
        self.has_task_checkbox = false;
        self.open_display_math = None;
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
/// of `text` (modulo list-item indent stripping — see `gobble`).
/// For HTML_BLOCK_DIV, a matched open+close is required (>= 2
/// `HTML_BLOCK_TAG` children). This avoids lifting unclosed shapes
/// (where the close tag would live in a separate sibling HTML_BLOCK),
/// which would produce a structurally incomplete CST.
///
/// When `gobble` is non-empty, continuation lines have that container
/// indent chain stripped before the inner reparse, mirroring
/// pandoc's list-item indent normalization. The stripped bytes are
/// re-injected as `WHITESPACE` tokens at the start of each continuation
/// line during graft so the result is byte-equal to the original
/// buffer text.
///
/// `line0_prefix` is re-injected at the very start of the grafted block
/// (before the open tag's first token, so it lands *inside* the block).
/// List-item and marker-line callers pass `""` — their line-0 indent was
/// already emitted upstream as the list marker / definition marker. The
/// later-line content-container caller passes the stripped content indent
/// so the block's first line carries it too (the formatter dumps HTML
/// blocks verbatim, so the indent must live inside the block).
///
/// `bq_prefixes` are the per-line blockquote marker bytes the buffer held
/// out of `text` (empty outside a quote); they are re-injected ahead of
/// the item indent at graft time, which is the order they sit in on a
/// quoted item's line (`>   after`). Same contract as the table/div lift.
#[allow(clippy::too_many_arguments)]
pub(crate) fn try_emit_html_block_lift(
    builder: &mut GreenNodeBuilder<'static>,
    text: &str,
    config: &ParserOptions,
    gobble: &[usize],
    bq_prefixes: &[String],
    use_paragraph: bool,
    line0_prefix: &str,
    allow_unclosed_div: bool,
) -> bool {
    let first_line = text.split_inclusive('\n').next().unwrap_or(text);
    let first_line_no_nl = first_line
        .strip_suffix("\r\n")
        .or_else(|| first_line.strip_suffix('\n'))
        .unwrap_or(first_line);
    if try_parse_html_block_start(first_line_no_nl, false).is_none() {
        return false;
    }

    let (parse_text, mut prefixes) = if gobble.is_empty() {
        (text.to_string(), Vec::new())
    } else {
        strip_list_item_indent(text, gobble)
    };
    if !line0_prefix.is_empty() {
        if prefixes.is_empty() {
            prefixes.push(line0_prefix.to_string());
        } else {
            prefixes[0] = line0_prefix.to_string();
        }
    }

    let prefix_lines: Vec<ContainerPrefixLine> = (0..prefixes.len().max(bq_prefixes.len()))
        .map(|i| {
            ContainerPrefixLine::bq_then_list(
                bq_prefixes.get(i).cloned().unwrap_or_default(),
                prefixes.get(i).cloned().unwrap_or_default(),
            )
        })
        .collect();
    emit_html_block_lift_from_stripped(
        builder,
        &parse_text,
        config,
        prefix_lines,
        use_paragraph,
        allow_unclosed_div,
    )
}

/// Reparse-validate-graft core shared by [`try_emit_html_block_lift`] and
/// the blockquote-nested content-container caller. `parse_text` is the
/// already-dedented block text (all container prefixes stripped);
/// `prefix_lines` re-injects the per-line prefix bytes (list indent
/// and/or `> ` markers) at graft time for losslessness. Line 0's prefix
/// is honored too — callers that already emitted the line-0 prefix (e.g.
/// an enclosing blockquote marker) pass an empty entry there.
pub(crate) fn emit_html_block_lift_from_stripped(
    builder: &mut GreenNodeBuilder<'static>,
    parse_text: &str,
    config: &ParserOptions,
    prefix_lines: Vec<ContainerPrefixLine>,
    use_paragraph: bool,
    allow_unclosed_div: bool,
) -> bool {
    let first_line = parse_text
        .split_inclusive('\n')
        .next()
        .unwrap_or(parse_text);
    let first_line_no_nl = first_line
        .strip_suffix("\r\n")
        .or_else(|| first_line.strip_suffix('\n'))
        .unwrap_or(first_line);
    if try_parse_html_block_start(first_line_no_nl, false).is_none() {
        return false;
    }

    let refdefs = config.refdef_labels.clone().unwrap_or_default();
    let inner_root = crate::parser::parse_with_refdefs(parse_text, Some(config.clone()), refdefs);

    let children: Vec<SyntaxNode> = inner_root.children().collect();
    if children.is_empty() {
        return false;
    }
    let first = &children[0];
    if !matches!(
        first.kind(),
        SyntaxKind::HTML_BLOCK | SyntaxKind::HTML_BLOCK_RAW | SyntaxKind::HTML_BLOCK_DIV
    ) {
        return false;
    }
    let total_end = children.last().unwrap().text_range().end();
    if total_end != TextSize::of(parse_text) {
        return false;
    }

    let multi_child_trailing = if children.len() == 1 {
        false
    } else if children.len() == 2
        && matches!(
            first.kind(),
            SyntaxKind::HTML_BLOCK | SyntaxKind::HTML_BLOCK_RAW | SyntaxKind::HTML_BLOCK_DIV
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
        let ok = html_block_tag_count >= 2
            || (allow_unclosed_div
                && html_block_tag_count == 1
                && !first
                    .children()
                    .any(|c| c.kind() == SyntaxKind::HTML_BLOCK_CONTENT));
        if !ok {
            return false;
        }
    }

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

fn try_emit_table_or_div_lift(
    builder: &mut GreenNodeBuilder<'static>,
    text: &str,
    config: &ParserOptions,
    gobble: &[usize],
    bq_prefixes: &[String],
) -> bool {
    let leading_newline = if text.starts_with("\r\n") {
        "\r\n"
    } else if text.starts_with('\n') {
        "\n"
    } else {
        ""
    };
    let body = &text[leading_newline.len()..];
    let bq_prefixes = if leading_newline.is_empty() {
        bq_prefixes
    } else {
        bq_prefixes.get(1..).unwrap_or_default()
    };

    let (parse_text, list_prefixes) = if gobble.is_empty() {
        (body.to_string(), Vec::new())
    } else {
        strip_list_item_indent_from(body, gobble, leading_newline.is_empty())
    };
    if !opens_table_or_div(&parse_text) {
        return false;
    }

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

    let prefix_lines: Vec<ContainerPrefixLine> = (0..list_prefixes.len().max(bq_prefixes.len()))
        .map(|i| {
            ContainerPrefixLine::bq_then_list(
                bq_prefixes.get(i).cloned().unwrap_or_default(),
                list_prefixes.get(i).cloned().unwrap_or_default(),
            )
        })
        .collect();
    let mut prefix_state = ContainerPrefixState::new(prefix_lines);
    if !leading_newline.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), leading_newline);
    }
    graft_node(builder, first, &mut prefix_state);
    true
}

/// Cheap pre-filter for [`try_emit_table_or_div_lift`]: does this text
/// even *look* like it opens a table or a fenced div? The reparse the
/// lift performs is the expensive part and every buffered list item
/// reaches it, so prose has to be rejected before one is spent.
///
/// A grid table, a `|`-fenced pipe table, a caption line, and a fenced
/// div are all identified by their first byte. A leading-pipe-less pipe
/// table (`a | b` / `---|---`) is not — pandoc's `pipeTable` accepts it
/// all the same — so it is recognized by a `|` in the header plus a
/// delimiter row directly under it.
fn opens_table_or_div(text: &str) -> bool {
    let mut lines = text.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return false;
    };
    let trimmed = trim_end_newlines(first).trim_start();
    if matches!(trimmed.as_bytes().first(), Some(b'|' | b'+' | b':')) {
        return true;
    }
    trimmed.contains('|')
        && lines
            .next()
            .and_then(|second| try_parse_pipe_separator(trim_end_newlines(second)))
            .is_some()
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

/// Strip the container indent chain `gobble` off each continuation
/// line of `text` (lines after the first). The first line is left
/// untouched — its leading columns are owned by the list marker and
/// its post-marker spaces. Returns the stripped text plus a per-line
/// prefix vector for losslessness re-injection during graft.
fn strip_list_item_indent(text: &str, gobble: &[usize]) -> (String, Vec<String>) {
    strip_list_item_indent_from(text, gobble, true)
}

/// [`strip_list_item_indent`] with the marker-line convention made
/// explicit. `skip_first_line` is false when the caller already peeled the
/// marker line off `text` (see the marker-only branch in
/// [`try_emit_table_or_div_lift`]), so line 0 carries the item's indent
/// like every other continuation line and must be stripped too.
fn strip_list_item_indent_from(
    text: &str,
    gobble: &[usize],
    skip_first_line: bool,
) -> (String, Vec<String>) {
    let mut stripped = String::with_capacity(text.len());
    let mut prefixes: Vec<String> = Vec::new();
    for (i, line) in text.split_inclusive('\n').enumerate() {
        if i == 0 && skip_first_line {
            prefixes.push(String::new());
            stripped.push_str(line);
            continue;
        }
        let consumed = item_indent_prefix_len(line, gobble);
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
        buffer.push_text("Hello, world!", &ParserOptions::default());
        assert!(!buffer.is_empty());
        assert!(!buffer.has_blank_lines_between_content());
        assert_eq!(buffer.get_text_for_parsing(), "Hello, world!");
    }

    #[test]
    fn test_push_multiple_text_segments() {
        let mut buffer = ListItemBuffer::new();
        let config = ParserOptions::default();
        buffer.push_text("Line 1\n", &config);
        buffer.push_text("Line 2\n", &config);
        buffer.push_text("Line 3", &config);
        assert_eq!(buffer.get_text_for_parsing(), "Line 1\nLine 2\nLine 3");
    }

    #[test]
    fn test_clear_buffer() {
        let mut buffer = ListItemBuffer::new();
        buffer.push_text("Some text", &ParserOptions::default());
        assert!(!buffer.is_empty());

        buffer.clear();
        assert!(buffer.is_empty());
        assert_eq!(buffer.get_text_for_parsing(), "");
    }

    #[test]
    fn paragraph_probe_handles_fragmented_lines() {
        let config = ParserOptions::default();
        let mut buffer = ListItemBuffer::new();
        buffer.push_text("\t\n  #", &config);
        buffer.push_text(" heading\r", &config);
        assert!(!buffer.has_open_paragraph(2));
        buffer.push_text("\n", &config);
        assert!(!buffer.has_open_paragraph(2));
        buffer.push_blockquote_marker(0, true);
        buffer.push_text("  continuation", &config);
        assert!(buffer.has_open_paragraph(2));
    }

    #[test]
    fn task_paragraph_context_ends_on_clear() {
        let config = ParserOptions::default();
        let mut buffer = ListItemBuffer::new();
        buffer.mark_task_paragraph();
        buffer.push_text(" ***\n", &config);
        assert!(buffer.has_open_paragraph(2));
        buffer.clear();
        buffer.push_text("  # heading\n", &config);
        assert!(!buffer.has_open_paragraph(2));
    }

    #[test]
    fn test_empty_text_ignored() {
        let mut buffer = ListItemBuffer::new();
        buffer.push_text("", &ParserOptions::default());
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_sole_text_segment_sees_past_blockquote_markers() {
        let config = ParserOptions::default();
        let mut buffer = ListItemBuffer::new();
        buffer.push_text("a | b\n", &config);
        buffer.push_blockquote_marker(0, true);
        assert_eq!(buffer.sole_text_segment(), Some("a | b\n"));

        buffer.push_text("  - | -\n", &config);
        assert_eq!(buffer.sole_text_segment(), None);
    }

    #[test]
    fn test_buffered_line_count_counts_lines_not_segments() {
        let config = ParserOptions::default();
        let mut buffer = ListItemBuffer::new();
        assert_eq!(buffer.buffered_line_count(), 0);

        buffer.push_text("a | b\n", &config);
        buffer.push_blockquote_marker(0, true);
        buffer.push_blockquote_marker(0, true);
        assert_eq!(buffer.buffered_line_count(), 1);
        assert_eq!(buffer.segment_count(), 3);

        buffer.push_text("  - | -\n", &config);
        assert_eq!(buffer.buffered_line_count(), 2);

        let mut buffer = ListItemBuffer::new();
        buffer.push_text("a\nb\n", &config);
        assert_eq!(buffer.segment_count(), 1);
        assert_eq!(buffer.buffered_line_count(), 2);

        let mut buffer = ListItemBuffer::new();
        buffer.push_text("a\n", &config);
        buffer.push_text("b", &config);
        assert_eq!(buffer.buffered_line_count(), 2);
    }

    #[test]
    fn test_is_text_only_sees_markers() {
        let config = ParserOptions::default();
        let mut buffer = ListItemBuffer::new();
        assert!(buffer.is_text_only());

        buffer.push_text("a\n", &config);
        assert!(buffer.is_text_only());

        buffer.push_blockquote_marker(0, true);
        assert!(!buffer.is_text_only());
    }

    #[test]
    fn test_blockquote_prefixes_are_indexed_by_line() {
        let config = ParserOptions::default();
        let mut buffer = ListItemBuffer::new();
        assert!(buffer.blockquote_prefixes().is_empty());

        buffer.push_text("a | b\n", &config);
        buffer.push_blockquote_marker(0, true);
        buffer.push_blockquote_marker(0, true);
        buffer.push_text("  - | -\n", &config);
        assert_eq!(
            buffer.blockquote_prefixes(),
            vec!["".to_string(), "> > ".to_string()]
        );
    }

    #[test]
    fn test_display_math_state_tracks_and_resets_on_clear() {
        let mut config = ParserOptions::default();
        config.extensions.tex_math_single_backslash = true;

        let mut buffer = ListItemBuffer::new();
        buffer.push_text("\\[\n", &config);
        assert!(buffer.has_open_display_math());
        buffer.push_text("x = 1 \\]\n", &config);
        assert!(!buffer.has_open_display_math());

        buffer.push_text("$$\n", &config);
        assert!(buffer.has_open_display_math());
        buffer.clear();
        assert!(!buffer.has_open_display_math());
    }
}
