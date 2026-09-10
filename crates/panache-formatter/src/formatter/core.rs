use crate::config::{Config, HorizontalRuleStyle, WrapMode};
use crate::directives::DirectiveTracker;
use crate::syntax::{SyntaxKind, SyntaxNode};
use panache_parser::parser::blocks::definition_lists::try_parse_definition_marker;
use panache_parser::parser::blocks::headings::try_parse_atx_heading;
use panache_parser::parser::blocks::horizontal_rules::try_parse_horizontal_rule;
use panache_parser::parser::utils::attributes::{AttrComponent, attribute_content_spans};
use rowan::NodeOrToken;

use super::code_blocks;
use super::code_blocks::FormattedCodeMap;
use super::headings;
use super::inline;
use super::inline_layout;
use super::paragraphs;
use super::sentence_wrap::SentenceProfileCache;
use super::tables;
use super::utils::is_structural_block;

pub struct Formatter {
    pub(super) output: String,
    pub(super) config: Config,
    pub(super) sentence_profile: SentenceProfileCache,
    pub(super) consecutive_blank_lines: usize,
    pub(super) fenced_div_depth: usize,
    pub(super) formatted_code: FormattedCodeMap,
    /// Stack of max marker widths for nested lists (for right-aligning markers)
    pub(super) max_marker_widths: Vec<usize>,
    /// Optional byte range to format (start, end). If None, format entire document.
    range: Option<(usize, usize)>,
    /// Track ignore directives for formatting
    pub(super) directive_tracker: DirectiveTracker,
    /// Depth of ignore region (for preserving content exactly)
    pub(super) ignore_region_start: Option<usize>,
    /// Structured rendering context for nested blockquote containers.
    pub(super) blockquote_context: Option<BlockquoteContext>,
}

#[derive(Clone, Debug)]
pub(super) struct BlockquoteContext {
    pub(super) in_list_continuation: bool,
}

impl Formatter {
    pub fn new(
        config: Config,
        formatted_code: FormattedCodeMap,
        range: Option<(usize, usize)>,
    ) -> Self {
        Self {
            output: String::with_capacity(8192),
            config,
            sentence_profile: SentenceProfileCache::default(),
            consecutive_blank_lines: 0,
            fenced_div_depth: 0,
            formatted_code,
            max_marker_widths: Vec::new(),
            range,
            directive_tracker: DirectiveTracker::new(),
            ignore_region_start: None,
            blockquote_context: None,
        }
    }
    pub fn format(mut self, node: &SyntaxNode) -> String {
        self.format_node_sync(node, 0);
        self.output
    }

    fn is_in_range(&self, node: &SyntaxNode) -> bool {
        if let Some((range_start, range_end)) = self.range {
            let node_start: usize = node.text_range().start().into();
            let node_end: usize = node.text_range().end().into();

            node_start < range_end && node_end > range_start
        } else {
            true
        }
    }

    pub(super) fn should_process_top_level_node(&self, node: &SyntaxNode) -> bool {
        if self.range.is_none() {
            return true;
        }

        if node.kind() == SyntaxKind::DOCUMENT {
            return true;
        }

        if is_structural_block(node.kind()) {
            return self.is_in_range(node);
        }

        false
    }

    pub(super) fn format_inline_node(&self, node: &SyntaxNode) -> String {
        inline::format_inline_node(node, &self.config)
    }

    pub(super) fn wrapped_lines_for_paragraph(
        &self,
        node: &SyntaxNode,
        width: usize,
    ) -> Vec<String> {
        inline_layout::wrapped_lines_for_paragraph(
            &self.config,
            &self.sentence_profile,
            node,
            width,
            &|n| self.format_inline_node(n),
        )
    }

    pub(super) fn wrapped_lines_for_paragraph_with_widths(
        &self,
        node: &SyntaxNode,
        widths: &[usize],
    ) -> Vec<String> {
        inline_layout::wrapped_lines_for_paragraph_with_widths(
            &self.config,
            &self.sentence_profile,
            node,
            widths,
            &|n| self.format_inline_node(n),
        )
    }

    pub(super) fn sentence_lines_for_paragraph(&self, node: &SyntaxNode) -> Vec<String> {
        inline_layout::sentence_lines_for_paragraph(
            &self.config,
            &self.sentence_profile,
            node,
            &|n| self.format_inline_node(n),
        )
    }

    pub(super) fn semantic_lines_for_paragraph(&self, node: &SyntaxNode) -> Vec<String> {
        inline_layout::semantic_lines_for_paragraph(
            &self.config,
            &self.sentence_profile,
            node,
            &|n| self.format_inline_node(n),
        )
    }

    pub(super) fn format_heading(&self, node: &SyntaxNode) -> String {
        headings::format_heading(node, &self.config)
    }

    pub(super) fn contains_latex_command(&self, node: &SyntaxNode) -> bool {
        paragraphs::contains_latex_command(node)
    }

    pub(super) fn is_grid_table_continuation_paragraph(&self, node: &SyntaxNode) -> bool {
        if node.kind() != SyntaxKind::PARAGRAPH {
            return false;
        }
        let text = node.text().to_string();
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.trim().is_empty())
            .collect();
        if lines.len() < 2 {
            return false;
        }
        lines.iter().all(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with('|') || trimmed.starts_with('+')
        }) && lines.iter().any(|line| line.contains("+-"))
            && lines.iter().any(|line| line.trim_start().starts_with('|'))
    }

    pub(super) fn is_grid_table_caption_definition_list(&self, node: &SyntaxNode) -> bool {
        if node.kind() != SyntaxKind::DEFINITION_LIST {
            return false;
        }
        if !node
            .text()
            .to_string()
            .lines()
            .any(|line| line.trim_start().starts_with(':'))
        {
            return false;
        }
        if let Some(prev) = node.prev_sibling() {
            return prev.kind() == SyntaxKind::GRID_TABLE
                || self.is_grid_table_continuation_paragraph(&prev);
        }
        false
    }

    pub(super) fn horizontal_rule_text(&self, available_width: usize) -> String {
        match self.config.horizontal_rule_style {
            HorizontalRuleStyle::LineWidth => "-".repeat(available_width.max(3)),
            HorizontalRuleStyle::Compact => "---".to_string(),
        }
    }

    fn starts_with_list_marker(text: &str) -> bool {
        text.starts_with("- ")
            || text.starts_with("* ")
            || text.starts_with("+ ")
            || text.starts_with("(@")
            || {
                let mut chars = text.chars().peekable();
                let mut saw_digit = false;
                while let Some(ch) = chars.peek().copied() {
                    if ch.is_ascii_digit() {
                        saw_digit = true;
                        chars.next();
                    } else {
                        break;
                    }
                }
                saw_digit && matches!(chars.peek().copied(), Some('.') | Some(')'))
            }
    }

    pub(super) fn paragraph_starts_with_atx_heading_candidate(&self, node: &SyntaxNode) -> bool {
        if node.kind() != SyntaxKind::PARAGRAPH {
            return false;
        }
        let text = node.text().to_string();
        let first_line = text.lines().next().unwrap_or_default();
        let trimmed = first_line.trim_start_matches([' ', '\t']);
        let leading_hashes = trimmed.chars().take_while(|&c| c == '#').count();
        (1..=6).contains(&leading_hashes)
            && trimmed[leading_hashes..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
    }

    pub(super) fn leading_atx_heading_with_remainder(
        &self,
        node: &SyntaxNode,
    ) -> Option<(String, String)> {
        if !matches!(node.kind(), SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH) {
            return None;
        }

        let text = node.text().to_string();
        let mut lines = text.lines();
        let first_line = lines.next()?.trim_start_matches([' ', '\t']);
        try_parse_atx_heading(first_line)?;

        let remainder = lines
            .flat_map(str::split_whitespace)
            .collect::<Vec<_>>()
            .join(" ");

        if remainder.is_empty() {
            return None;
        }

        Some((first_line.trim_end().to_string(), remainder))
    }

    pub(super) fn wrap_text_for_indent(&self, text: &str, indent: usize) -> Vec<String> {
        let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
        let width = self.config.line_width.saturating_sub(indent);
        match wrap_mode {
            WrapMode::Preserve | WrapMode::Semantic => vec![text.to_string()],
            WrapMode::Reflow | WrapMode::Sentence => {
                inline_layout::wrap_text_first_fit(text, width)
            }
        }
    }

    pub(super) fn format_code_block(&mut self, node: &SyntaxNode) {
        code_blocks::format_code_block(node, &self.config, &self.formatted_code, &mut self.output);
    }

    pub(super) fn format_code_block_to_string(&mut self, node: &SyntaxNode) -> String {
        let saved_output = self.output.clone();
        self.output.clear();
        self.format_code_block(node);
        let formatted = self.output.clone();
        self.output = saved_output;
        formatted
    }

    fn strip_leading_columns(line: &str, columns: usize) -> String {
        let mut cols = 0usize;
        let mut idx = 0usize;

        for (byte_idx, ch) in line.char_indices() {
            if cols >= columns {
                idx = byte_idx;
                break;
            }

            match ch {
                ' ' => {
                    cols += 1;
                    idx = byte_idx + ch.len_utf8();
                }
                '\t' => {
                    cols += 4 - (cols % 4);
                    idx = byte_idx + ch.len_utf8();
                }
                _ => {
                    idx = byte_idx;
                    break;
                }
            }
        }

        if cols >= columns {
            line[idx..].to_string()
        } else if line.chars().all(|c| matches!(c, ' ' | '\t')) {
            String::new()
        } else {
            line[idx..].to_string()
        }
    }

    pub(super) fn format_container_code_block(
        &mut self,
        node: &SyntaxNode,
        first_line_prefix: &str,
        continuation_indent: usize,
        trim_first_line_start: bool,
        strip_content_columns: Option<usize>,
        indent_blank_content_lines: bool,
    ) {
        let formatted = self.format_code_block_to_string(node);

        let mut lines = formatted.lines();
        if let Some(first_line) = lines.next() {
            self.output.push_str(first_line_prefix);
            if trim_first_line_start {
                self.output.push_str(first_line.trim_start());
            } else {
                self.output.push_str(first_line);
            }
            self.output.push('\n');
        }

        let mut remaining: Vec<&str> = lines.collect();
        if remaining.is_empty() {
            return;
        }

        let closing = remaining.pop().unwrap();

        let continuation_prefix = " ".repeat(continuation_indent);
        for line in remaining {
            if line.trim().is_empty() && !indent_blank_content_lines {
                self.output.push('\n');
                continue;
            }

            self.output.push_str(&continuation_prefix);
            match strip_content_columns {
                Some(cols) => self
                    .output
                    .push_str(&Self::strip_leading_columns(line, cols)),
                None => self.output.push_str(line),
            }
            self.output.push('\n');
        }

        self.output.push_str(&continuation_prefix);
        match strip_content_columns {
            Some(cols) => self
                .output
                .push_str(&Self::strip_leading_columns(closing, cols)),
            None => self.output.push_str(closing),
        }
        self.output.push('\n');
    }

    /// Format a code block that is a continuation of a definition or list item.
    /// Adds indentation prefix to each line of the fenced code block.
    pub(super) fn format_indented_code_block(&mut self, node: &SyntaxNode, indent: usize) {
        let is_fenced = node
            .children()
            .any(|child| child.kind() == SyntaxKind::CODE_FENCE_OPEN);
        let in_list_item = node
            .ancestors()
            .any(|ancestor| ancestor.kind() == SyntaxKind::LIST_ITEM);
        let code_text = node.text().to_string();
        let should_preserve_raw_indented = !is_fenced
            && in_list_item
            && (code_text.contains("```")
                || code_text.contains("<details")
                || code_text.contains("</details>"));
        if should_preserve_raw_indented {
            self.output.push_str(&code_text);
            if !self.output.ends_with('\n') {
                self.output.push('\n');
            }
            return;
        }

        let indent_str = " ".repeat(indent);

        let strip_content_columns = (!is_fenced)
            .then(|| Self::container_content_offset(node))
            .filter(|cols| *cols > 0);
        self.format_container_code_block(
            node,
            &indent_str,
            indent,
            false,
            strip_content_columns,
            false,
        );

        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
    }

    /// Visual column at which the content of `node`'s enclosing container
    /// starts *in the source*.
    ///
    /// The parser leaves that offset on every body line of an indented code
    /// block, so it is what has to come off before the block is re-prefixed at
    /// its formatted indent. Mirrors `definition_content_offset` and
    /// `list_item_content_offset` in the pandoc-native projector
    /// (`panache_parser::to_pandoc_ast`); the two must agree on what a code
    /// block's payload is.
    pub(super) fn container_content_offset(node: &SyntaxNode) -> usize {
        let Some(parent) = node.parent() else {
            return 0;
        };
        match parent.kind() {
            SyntaxKind::DEFINITION => {
                Self::marker_content_offset(&parent, SyntaxKind::DEFINITION_MARKER)
            }
            SyntaxKind::LIST_ITEM => {
                let parent_ws = match parent.prev_sibling_or_token() {
                    Some(NodeOrToken::Token(t)) if t.kind() == SyntaxKind::WHITESPACE => {
                        Self::advance_col(0, t.text())
                    }
                    _ => 0,
                };
                parent_ws + Self::marker_content_offset(&parent, SyntaxKind::LIST_MARKER)
            }
            _ => 0,
        }
    }

    fn marker_content_offset(container: &SyntaxNode, marker: SyntaxKind) -> usize {
        let mut col = 0usize;
        let mut saw_marker = false;
        for el in container.children_with_tokens() {
            match el {
                NodeOrToken::Token(t) if t.kind() == marker => {
                    col = Self::advance_col(col, t.text());
                    saw_marker = true;
                }
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::WHITESPACE => {
                    col = Self::advance_col(col, t.text());
                    if saw_marker {
                        return col;
                    }
                }
                _ if saw_marker => return col,
                _ => {}
            }
        }
        col
    }

    fn advance_col(start: usize, s: &str) -> usize {
        let mut col = start;
        for c in s.chars() {
            if c == '\t' {
                col = (col / 4 + 1) * 4;
            } else {
                col += 1;
            }
        }
        col
    }

    pub(super) fn code_block_leading_indent(node: &SyntaxNode) -> String {
        node.children_with_tokens()
            .take_while(
                |item| matches!(item, NodeOrToken::Token(t) if t.kind() == SyntaxKind::WHITESPACE),
            )
            .filter_map(|item| match item {
                NodeOrToken::Token(t) => Some(t.text().to_string()),
                _ => None,
            })
            .collect::<String>()
    }

    /// Format `node` into a scratch buffer instead of `self.output`.
    ///
    /// Callers that re-prefix the result line by line (blockquote children,
    /// most of all) need the rendering in isolation. `width_reduction` shrinks
    /// `line_width` for the duration so wrapped content still fits once the
    /// prefix is put back in front of it.
    pub(super) fn render_to_buffer(
        &mut self,
        node: &SyntaxNode,
        indent: usize,
        width_reduction: usize,
    ) -> String {
        let saved_output = std::mem::take(&mut self.output);
        let saved_line_width = self.config.line_width;
        self.config.line_width = saved_line_width.saturating_sub(width_reduction);

        self.format_node_sync(node, indent);

        self.config.line_width = saved_line_width;
        std::mem::replace(&mut self.output, saved_output)
    }

    pub(super) fn append_blockquote_prefixed_block(
        &mut self,
        rendered: &str,
        content_prefix: &str,
        blank_prefix: &str,
        leading_indent: Option<&str>,
    ) {
        for line in rendered.lines() {
            if line.is_empty() {
                self.output.push_str(blank_prefix);
            } else {
                self.output.push_str(content_prefix);
                if let Some(indent) = leading_indent
                    && !indent.is_empty()
                {
                    self.output.push_str(indent);
                }
                self.output.push_str(line);
            }
            self.output.push('\n');
        }
    }

    /// Re-prefix a rendered child that may itself contain a nested blockquote.
    ///
    /// A nested `BLOCK_QUOTE` derives its prefix from its own ancestor depth,
    /// so its lines arrive already carrying `> ` for every enclosing quote and
    /// only need the base indent. Unlike
    /// [`Self::append_blockquote_prefixed_block`], which is for content that
    /// can legitimately start a line with `>` (code, raw HTML), this trusts
    /// that a leading `> ` means "already quoted".
    pub(super) fn append_blockquote_prefixed_nested_block(
        &mut self,
        rendered: &str,
        base_indent: &str,
        content_prefix: &str,
        blank_prefix: &str,
    ) {
        for line in rendered.lines() {
            if line.is_empty() {
                self.output.push_str(blank_prefix);
            } else if line.starts_with("> ") {
                self.output.push_str(base_indent);
                self.output.push_str(line);
            } else {
                self.output.push_str(content_prefix);
                self.output.push_str(line);
            }
            self.output.push('\n');
        }
    }

    pub(super) fn append_blockquote_prefixed_list_output(
        &mut self,
        list_output: &str,
        base_indent: &str,
        content_prefix: &str,
        blank_prefix: &str,
    ) -> bool {
        let mut in_list_item_continuation = false;
        for line in list_output.lines() {
            let trimmed_line = line.trim_start();
            let starts_with_list_marker = Self::starts_with_list_marker(trimmed_line)
                && !tables::is_partial_grid_separator(trimmed_line);
            if trimmed_line.is_empty() {
                self.output.push_str(blank_prefix);
                in_list_item_continuation = false;
            } else if line.starts_with("> ") {
                let trimmed_rest = line.trim_start_matches("> ").trim_start();
                if trimmed_rest.is_empty() {
                    self.output.push_str(blank_prefix);
                    in_list_item_continuation = false;
                    self.output.push('\n');
                    continue;
                }
                self.output.push_str(base_indent);
                self.output.push_str(line);
                in_list_item_continuation = Self::starts_with_list_marker(trimmed_rest)
                    && !tables::is_partial_grid_separator(trimmed_rest);
            } else {
                self.output.push_str(content_prefix);
                self.output.push_str(line);
                in_list_item_continuation = starts_with_list_marker
                    || (in_list_item_continuation && line.starts_with(char::is_whitespace));
            }
            self.output.push('\n');
        }

        in_list_item_continuation
    }

    /// Smart punctuation turns `—`→`---` and `–`→`--`. When a paragraph's
    /// whole content normalizes to dashes, the emitted line re-parses as a
    /// thematic break (or setext underline) — a semantic + idempotency break
    /// (one pandoc itself shares). When that happens, re-emit the paragraph
    /// with smart off so the lossless unicode dash is preserved. The smart-off
    /// rendering is adopted only when it actually clears the marker, so a
    /// paragraph that genuinely contains a `***`/`___`/`- - -` line (not
    /// produced by smart) is left untouched.
    pub(super) fn guard_dash_block_marker(
        &mut self,
        start: usize,
        node: &SyntaxNode,
        indent: usize,
    ) {
        if !self.config.formatter_extensions.smart
            || !Self::produces_dash_block_marker(&self.output[start..])
        {
            return;
        }

        let original = self.output[start..].to_string();
        self.output.truncate(start);

        let mut cfg = self.config.clone();
        cfg.formatter_extensions.smart = false;
        let saved = std::mem::replace(&mut self.config, cfg);
        self.format_node_sync(node, indent);
        self.config = saved;

        if Self::produces_dash_block_marker(&self.output[start..]) {
            self.output.truncate(start);
            self.output.push_str(&original);
        }
    }

    /// A paragraph whose first emitted line is a `:` or `~` definition marker
    /// re-parses as a `DEFINITION` as soon as the block above it is a single
    /// line: the parser promotes that line to the `TERM`. Reflow manufactures
    /// exactly that situation out of a multi-line paragraph, so escape the
    /// marker to keep `format(format(x)) == format(x)`. `\: def` is
    /// `Para [":", Space, "def"]` for pandoc and panache alike.
    ///
    /// Reads the emitted text rather than the source, because the reparse sees
    /// the emitted text — a paragraph that is still multi-line after wrapping
    /// cannot supply a term and is left alone.
    ///
    /// `content_indent` is the content-container indent the calling arm
    /// prepends to every line (a footnote body's four columns, a list item's
    /// hanging column). The parser strips it before measuring the marker's own
    /// 0-3 space allowance, so the guard must too, or a marker at a content
    /// column of 4 reads as indented code and is never guarded.
    pub(super) fn guard_definition_marker_start(&mut self, start: usize, content_indent: usize) {
        if !self.config.parser_extensions.definition_lists {
            return;
        }
        let Some(first) = self.output[start..].lines().next() else {
            return;
        };
        let container_indent = first
            .bytes()
            .take_while(|byte| *byte == b' ')
            .count()
            .min(content_indent);
        let prefix_len = container_indent + Self::block_prefix_len(&first[container_indent..]);
        let Some((marker, ..)) = try_parse_definition_marker(&first[prefix_len..]) else {
            return;
        };
        if !Self::preceding_block_is_one_line(&self.output[..start]) {
            return;
        }
        let marker = first[prefix_len..]
            .find(marker)
            .expect("marker parsed above")
            .saturating_add(prefix_len);
        self.output.insert(start + marker, '\\');
    }

    fn block_prefix_len(line: &str) -> usize {
        let bytes = line.as_bytes();
        let mut i = 0;
        let mut saw_marker = false;
        loop {
            let start = i;
            while i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'>' {
                i += 1;
                if i < bytes.len() && bytes[i] == b' ' {
                    i += 1;
                }
                saw_marker = true;
                continue;
            }
            return if saw_marker { start } else { 0 };
        }
    }

    fn preceding_block_is_one_line(before: &str) -> bool {
        let is_blank = |line: &str| line[Self::block_prefix_len(line)..].trim().is_empty();
        let ends_the_block_above = |line: &str| {
            is_blank(line)
                || inline_layout::looks_like_div_fence_line(
                    line[Self::block_prefix_len(line)..].trim_start(),
                )
        };
        let mut lines = before.lines().rev().skip_while(|l| is_blank(l));
        lines.next().is_some() && lines.next().is_none_or(ends_the_block_above)
    }

    fn produces_dash_block_marker(text: &str) -> bool {
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if try_parse_horizontal_rule(line).is_some() {
                return true;
            }
            if i > 0 && trimmed.chars().all(|c| c == '-') && !lines[i - 1].trim().is_empty() {
                return true;
            }
        }
        false
    }

    pub(super) fn format_node_sync(&mut self, node: &SyntaxNode, indent: usize) {
        if self.directive_tracker.is_formatting_ignored()
            && node.kind() != SyntaxKind::DOCUMENT
            && node.kind() != SyntaxKind::COMMENT
            && node.kind() != SyntaxKind::HTML_BLOCK
            && node.kind() != SyntaxKind::HTML_BLOCK_RAW
            && node.kind() != SyntaxKind::HTML_BLOCK_DIV
        {
            let text = node.text().to_string();
            self.output.push_str(&text);
            for directive in crate::directives::collect_inline_directives(node) {
                self.directive_tracker.process_directive(&directive);
            }
            return;
        }

        if matches!(node.kind(), SyntaxKind::PARAGRAPH | SyntaxKind::PLAIN) {
            let inline_directives = crate::directives::collect_inline_directives(node);
            if !inline_directives.is_empty() {
                let affects_formatting = inline_directives.iter().any(|d| match d {
                    crate::directives::Directive::Start(kind)
                    | crate::directives::Directive::End(kind) => kind.affects_formatting(),
                });
                if affects_formatting {
                    let text = node.text().to_string();
                    self.output.push_str(&text);
                    if !text.ends_with('\n') {
                        self.output.push('\n');
                    }
                    for directive in inline_directives {
                        self.directive_tracker.process_directive(&directive);
                    }
                    return;
                }
                for directive in inline_directives {
                    self.directive_tracker.process_directive(&directive);
                }
            }
        }

        if node.kind() != SyntaxKind::BLANK_LINE {
            self.consecutive_blank_lines = 0;
        }

        match node.kind() {
            SyntaxKind::DOCUMENT => self.format_document(node, indent),

            SyntaxKind::HEADING => self.format_heading_block(node, indent),

            SyntaxKind::HORIZONTAL_RULE => self.format_horizontal_rule(node, indent),

            SyntaxKind::REFERENCE_DEFINITION => self.format_reference_definition(node),

            SyntaxKind::ADMONITION => self.format_admonition(node, indent),

            SyntaxKind::FOOTNOTE_DEFINITION => self.format_footnote_definition(node, indent),

            SyntaxKind::HTML_BLOCK | SyntaxKind::HTML_BLOCK_RAW | SyntaxKind::HTML_BLOCK_DIV => {
                self.format_html_block(node)
            }
            SyntaxKind::COMMENT => self.format_comment(node),
            SyntaxKind::LATEX_COMMAND => self.format_latex_command(node),
            SyntaxKind::TEX_BLOCK => self.format_tex_block(node),

            SyntaxKind::BLOCK_QUOTE => self.format_block_quote(node, indent),

            SyntaxKind::PARAGRAPH => self.format_paragraph(node, indent),

            SyntaxKind::FIGURE => self.format_figure(node, indent),

            SyntaxKind::PLAIN => self.format_plain(node, indent),

            SyntaxKind::LIST => {
                self.format_list(node, indent);
            }

            SyntaxKind::DEFINITION_LIST => self.format_definition_list(node, indent),

            SyntaxKind::LINE_BLOCK => self.format_line_block(node, indent),

            SyntaxKind::DEFINITION_ITEM => self.format_definition_item(node, indent),

            SyntaxKind::TERM => self.format_term(node, indent),

            SyntaxKind::DEFINITION => self.format_definition(node, indent),

            SyntaxKind::SIMPLE_TABLE
            | SyntaxKind::MULTILINE_TABLE
            | SyntaxKind::PIPE_TABLE
            | SyntaxKind::GRID_TABLE => self.format_table(node, indent),

            SyntaxKind::INLINE_MATH => self.format_inline_math(node),

            SyntaxKind::LIST_ITEM => {
                self.format_list_item(node, indent);
            }

            SyntaxKind::FENCED_DIV => self.format_fenced_div(node, indent),

            SyntaxKind::INLINE_MATH_MARKER => self.format_inline_math_marker(node),

            SyntaxKind::DISPLAY_MATH => self.format_display_math(node),

            SyntaxKind::CODE_BLOCK => self.format_block_code(node),

            SyntaxKind::YAML_METADATA
            | SyntaxKind::PANDOC_TITLE_BLOCK
            | SyntaxKind::MMD_TITLE_BLOCK => self.format_metadata_block(node),
            SyntaxKind::BLANK_LINE => self.format_blank_line(),

            SyntaxKind::EMPHASIS => {
                self.format_delimited_inline(node, indent, "*", SyntaxKind::EMPHASIS_MARKER)
            }
            SyntaxKind::STRONG => {
                self.format_delimited_inline(node, indent, "**", SyntaxKind::STRONG_MARKER)
            }
            SyntaxKind::STRIKEOUT => {
                self.format_delimited_inline(node, indent, "~~", SyntaxKind::STRIKEOUT_MARKER)
            }
            SyntaxKind::SUPERSCRIPT => {
                self.format_delimited_inline(node, indent, "^", SyntaxKind::SUPERSCRIPT_MARKER)
            }
            SyntaxKind::SUBSCRIPT => {
                self.format_delimited_inline(node, indent, "~", SyntaxKind::SUBSCRIPT_MARKER)
            }

            SyntaxKind::MYST_DIRECTIVE => self.format_myst_directive(node, indent),

            SyntaxKind::MYST_TARGET
            | SyntaxKind::MYST_COMMENT
            | SyntaxKind::MYST_BLOCK_BREAK
            | SyntaxKind::SVELTE_BLOCK => self.format_raw_block(node),

            _ => {
                self.output.push_str(&node.text().to_string());
            }
        }
    }
}

/// Rewrite a Pandoc `{...}` attribute block in canonical form: id first, then
/// classes in source order, then key/value pairs with quoted values.
///
/// Components are rendered from their *source* slices rather than from derived
/// semantics, so a bare `-` (pandoc's shorthand for `.unnumbered`) survives as
/// `-` instead of being expanded — the shorthand is the preferred spelling in
/// non-English documents, and expanding it would rewrite the author's source.
pub(super) fn normalize_attribute_text(attr_text: &str) -> String {
    let Some(inner) = attr_text
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
    else {
        return attr_text.to_string();
    };
    let Some(spans) = attribute_content_spans(inner) else {
        return attr_text.to_string();
    };

    let mut id = String::new();
    let mut classes: Vec<&str> = Vec::new();
    let mut key_values: Vec<String> = Vec::new();
    for comp in &spans.components {
        match comp {
            AttrComponent::Id(r) => id = inner[r.clone()].to_string(),
            AttrComponent::Class(r) | AttrComponent::Unnumbered(r) => {
                classes.push(&inner[r.clone()])
            }
            AttrComponent::KeyValue { key, value, .. } => {
                let value = strip_attr_value_quotes(&inner[value.clone()]);
                key_values.push(format!(
                    "{}=\"{}\"",
                    &inner[key.clone()],
                    value.replace('"', "\\\"")
                ));
            }
        }
    }

    let mut out = String::from("{");
    if !id.is_empty() {
        out.push_str(&id);
    }
    for part in classes.into_iter().map(str::to_string).chain(key_values) {
        if out.len() > 1 {
            out.push(' ');
        }
        out.push_str(&part);
    }
    out.push('}');
    out
}

fn strip_attr_value_quotes(raw: &str) -> &str {
    match raw.as_bytes().first() {
        Some(&q @ (b'"' | b'\'')) => {
            let inner = &raw[1..];
            inner.strip_suffix(q as char).unwrap_or(inner)
        }
        _ => raw,
    }
}

/// Render a `SPAN_ATTRIBUTES` node, collapsing interior whitespace runs to a
/// single space. Reads the node's `.text()` rather than its children, so it is
/// independent of whether the body is structured into `ATTR_*` tokens. This
/// reproduces the historical span normalization (preserve token order, single-
/// space separation) byte-for-byte.
pub(super) fn normalize_span_attributes(node: &SyntaxNode) -> String {
    let text = node.text().to_string();
    let inner = text
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(text.as_str());
    let joined = inner.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("{{{joined}}}")
}
