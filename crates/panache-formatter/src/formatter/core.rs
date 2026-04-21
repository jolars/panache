use crate::config::{Config, WrapMode};
use crate::directives::{DirectiveTracker, extract_directive_from_node};
use crate::syntax::{BlockQuote, DefinitionItem, DisplayMath, FencedDiv, SyntaxKind, SyntaxNode};
use panache_parser::parser::blocks::headings::{try_parse_atx_heading, try_parse_setext_heading};
use panache_parser::parser::utils::attributes::parse_attribute_content;
use rowan::NodeOrToken;
use rowan::ast::AstNode;

use super::code_blocks;
use super::code_blocks::FormattedCodeMap;
use super::headings;
use super::inline;
use super::inline_layout;
use super::paragraphs;
use super::smart::normalize_smart_punctuation;
use super::tables;
use super::utils::{is_block_element, is_structural_block};

pub struct Formatter {
    pub(super) output: String,
    pub(super) config: Config,
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
    ignore_region_start: Option<usize>,
    /// Structured rendering context for nested blockquote containers.
    blockquote_context: Option<BlockquoteContext>,
}

#[derive(Clone, Debug)]
struct BlockquoteContext {
    in_list_continuation: bool,
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

    /// Check if a node overlaps with the formatting range
    fn is_in_range(&self, node: &SyntaxNode) -> bool {
        if let Some((range_start, range_end)) = self.range {
            let node_start: usize = node.text_range().start().into();
            let node_end: usize = node.text_range().end().into();

            // Node overlaps with range if it starts before range ends and ends after range starts
            node_start < range_end && node_end > range_start
        } else {
            // No range specified, format everything
            true
        }
    }

    /// Check if we should process a direct child of DOCUMENT
    /// When range filtering is active, only process nodes that overlap with the range
    fn should_process_top_level_node(&self, node: &SyntaxNode) -> bool {
        // If no range specified, process everything
        if self.range.is_none() {
            return true;
        }

        // Always process DOCUMENT node (container)
        if node.kind() == SyntaxKind::DOCUMENT {
            return true;
        }

        // For structural block elements, check if they overlap with the range
        if is_structural_block(node.kind()) {
            return self.is_in_range(node);
        }

        // For non-block elements (tokens), don't include them
        false
    }

    // Delegate to extracted wrapping module
    pub(super) fn format_inline_node(&self, node: &SyntaxNode) -> String {
        inline::format_inline_node(node, &self.config)
    }

    // Delegate to wrapping module
    pub(super) fn wrapped_lines_for_paragraph(
        &self,
        node: &SyntaxNode,
        width: usize,
    ) -> Vec<String> {
        let text = node.text().to_string();
        if text.contains("[@") && text.contains("]:") {
            return text.lines().map(ToString::to_string).collect();
        }
        inline_layout::wrapped_lines_for_paragraph(&self.config, node, width, &|n| {
            self.format_inline_node(n)
        })
    }

    pub(super) fn wrapped_lines_for_paragraph_with_widths(
        &self,
        node: &SyntaxNode,
        widths: &[usize],
    ) -> Vec<String> {
        inline_layout::wrapped_lines_for_paragraph_with_widths(&self.config, node, widths, &|n| {
            self.format_inline_node(n)
        })
    }

    pub(super) fn sentence_lines_for_paragraph(&self, node: &SyntaxNode) -> Vec<String> {
        inline_layout::sentence_lines_for_paragraph(&self.config, node, &|n| {
            self.format_inline_node(n)
        })
    }

    // Delegate to headings module
    pub(super) fn format_heading(&self, node: &SyntaxNode) -> String {
        headings::format_heading(node, &self.config)
    }

    fn contains_latex_command(&self, node: &SyntaxNode) -> bool {
        paragraphs::contains_latex_command(node)
    }

    fn is_grid_table_continuation_paragraph(&self, node: &SyntaxNode) -> bool {
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

    fn is_grid_table_caption_definition_list(&self, node: &SyntaxNode) -> bool {
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

    fn horizontal_rule_text(&self, available_width: usize) -> String {
        "-".repeat(available_width.max(3))
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

    fn paragraph_starts_with_atx_heading_candidate(&self, node: &SyntaxNode) -> bool {
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
            WrapMode::Preserve => vec![text.to_string()],
            WrapMode::Reflow | WrapMode::Sentence => {
                inline_layout::wrap_text_first_fit(text, width)
            }
        }
    }

    fn paragraph_starts_with_setext_heading_candidate(&self, node: &SyntaxNode) -> bool {
        if node.kind() != SyntaxKind::PARAGRAPH {
            return false;
        }
        let text = node.text().to_string();
        let mut lines = text.lines();
        let first = lines.next().unwrap_or_default();
        let second = lines.next().unwrap_or_default();
        if second.is_empty() {
            return false;
        }
        try_parse_setext_heading(&[first, second], 0).is_some()
    }

    // Delegate to code_blocks module
    fn format_code_block(&mut self, node: &SyntaxNode) {
        code_blocks::format_code_block(node, &self.config, &self.formatted_code, &mut self.output);
    }

    fn format_code_block_to_string(&mut self, node: &SyntaxNode) -> String {
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

    fn format_container_code_block(
        &mut self,
        node: &SyntaxNode,
        first_line_prefix: &str,
        continuation_indent: usize,
        trim_first_line_start: bool,
        normalize_content_indent: bool,
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
        let content_indent_cols = if normalize_content_indent {
            continuation_indent
        } else {
            0
        };

        let continuation_prefix = " ".repeat(continuation_indent);
        for line in remaining {
            if line.trim().is_empty() && !indent_blank_content_lines {
                self.output.push('\n');
                continue;
            }

            self.output.push_str(&continuation_prefix);
            if normalize_content_indent {
                self.output
                    .push_str(&Self::strip_leading_columns(line, content_indent_cols));
            } else {
                self.output.push_str(line);
            }
            self.output.push('\n');
        }

        self.output.push_str(&continuation_prefix);
        if normalize_content_indent {
            self.output
                .push_str(&Self::strip_leading_columns(closing, content_indent_cols));
        } else {
            self.output.push_str(closing);
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

        self.format_container_code_block(node, &indent_str, indent, false, false, true);

        // Ensure we end with exactly one newline
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
    }

    fn code_block_leading_indent(node: &SyntaxNode) -> String {
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

    fn blockquote_prev_significant_kind(children: &[SyntaxNode], idx: usize) -> Option<SyntaxKind> {
        children[..idx]
            .iter()
            .rev()
            .find(|child| {
                !matches!(
                    child.kind(),
                    SyntaxKind::BLOCK_QUOTE_MARKER | SyntaxKind::BLANK_LINE
                )
            })
            .map(SyntaxNode::kind)
    }

    fn blockquote_next_significant_kind(children: &[SyntaxNode], idx: usize) -> Option<SyntaxKind> {
        children[idx + 1..]
            .iter()
            .find(|child| {
                !matches!(
                    child.kind(),
                    SyntaxKind::BLOCK_QUOTE_MARKER | SyntaxKind::BLANK_LINE
                )
            })
            .map(SyntaxNode::kind)
    }

    fn should_preserve_blockquote_blank_suffix(
        prev_significant_kind: Option<SyntaxKind>,
        next_significant_kind: Option<SyntaxKind>,
        blank_suffix: &str,
        in_list_continuation: bool,
    ) -> bool {
        if blank_suffix.is_empty() || !blank_suffix.chars().all(|ch| ch == ' ') {
            return false;
        }

        matches!(next_significant_kind, Some(SyntaxKind::CODE_BLOCK))
            && (in_list_continuation
                || matches!(
                    prev_significant_kind,
                    Some(SyntaxKind::LIST | SyntaxKind::LIST_ITEM)
                ))
    }

    fn append_blockquote_prefixed_block(
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
                    && !line.starts_with([' ', '\t'])
                {
                    self.output.push_str(indent);
                }
                self.output.push_str(line);
            }
            self.output.push('\n');
        }
    }

    fn append_blockquote_prefixed_list_output(
        &mut self,
        list_output: &str,
        base_indent: &str,
        content_prefix: &str,
        blank_prefix: &str,
    ) -> bool {
        let mut in_list_item_continuation = false;
        for line in list_output.lines() {
            let trimmed_line = line.trim_start();
            let starts_with_list_marker = Self::starts_with_list_marker(trimmed_line);
            if line.is_empty() {
                self.output.push_str(blank_prefix);
                in_list_item_continuation = false;
            } else if line.starts_with("> ") {
                let rest = line.trim_start_matches("> ");
                let trimmed_rest = rest.trim_start();
                if trimmed_rest.is_empty() {
                    self.output.push_str(blank_prefix);
                    in_list_item_continuation = false;
                    self.output.push('\n');
                    continue;
                }
                let starts_with_marker_after_quote = Self::starts_with_list_marker(trimmed_rest);
                if starts_with_marker_after_quote {
                    self.output.push_str(base_indent);
                    self.output.push_str("> ");
                    self.output.push_str(trimmed_rest);
                    in_list_item_continuation = true;
                } else {
                    self.output.push_str(base_indent);
                    self.output.push_str(line);
                    in_list_item_continuation = false;
                }
            } else if starts_with_list_marker {
                self.output.push_str(content_prefix);
                self.output.push_str(trimmed_line);
                in_list_item_continuation = true;
            } else if in_list_item_continuation && line.starts_with(char::is_whitespace) {
                if trimmed_line.is_empty() {
                    self.output.push_str(blank_prefix);
                    in_list_item_continuation = false;
                } else {
                    self.output.push_str(content_prefix);
                    self.output.push_str("  ");
                    self.output.push_str(trimmed_line);
                }
            } else {
                self.output.push_str(content_prefix);
                self.output.push_str(line);
                in_list_item_continuation = false;
            }
            self.output.push('\n');
        }

        in_list_item_continuation
    }

    // The large format_node_sync method - keeping it here for now, can extract later
    pub(super) fn format_node_sync(&mut self, node: &SyntaxNode, indent: usize) {
        // Check if formatting is ignored - if so, preserve content exactly
        // Exception: Always process DOCUMENT, COMMENT, and HTML_BLOCK nodes (may contain directives)
        if self.directive_tracker.is_formatting_ignored()
            && node.kind() != SyntaxKind::DOCUMENT
            && node.kind() != SyntaxKind::COMMENT
            && node.kind() != SyntaxKind::HTML_BLOCK
        {
            let text = node.text().to_string();
            self.output.push_str(&text);
            return;
        }

        // Reset blank line counter when we hit a non-blank node
        if node.kind() != SyntaxKind::BLANK_LINE {
            self.consecutive_blank_lines = 0;
        }

        let line_width = self.config.line_width;

        match node.kind() {
            SyntaxKind::DOCUMENT => {
                for el in node.children_with_tokens() {
                    match el {
                        rowan::NodeOrToken::Node(n) => {
                            // When range filtering is active, only process nodes that overlap
                            if self.should_process_top_level_node(&n) {
                                self.format_node_sync(&n, indent);
                            }
                        }
                        rowan::NodeOrToken::Token(t) => match t.kind() {
                            SyntaxKind::WHITESPACE => {}
                            SyntaxKind::NEWLINE => {}
                            SyntaxKind::BLANK_LINE => {
                                if !self.output.is_empty() {
                                    self.output.push('\n');
                                }
                            }
                            SyntaxKind::ESCAPED_CHAR => {
                                // Token already includes backslash (e.g., "\*")
                                self.output.push_str(t.text());
                            }
                            SyntaxKind::NONBREAKING_SPACE => {
                                // Keep Pandoc escaped-space form for idempotency and losslessness.
                                self.output.push_str(r"\ ");
                            }
                            SyntaxKind::IMAGE_LINK_START
                            | SyntaxKind::LINK_START
                            | SyntaxKind::LATEX_COMMAND => {
                                self.output.push_str(t.text());
                            }
                            _ => self.output.push_str(t.text()),
                        },
                    }
                }
            }

            SyntaxKind::HEADING => {
                log::trace!("Formatting heading");
                // Determine level
                let mut level = 1;
                let mut attributes = String::new();

                // First pass: get level and attributes
                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::ATX_HEADING_MARKER => {
                            let t = child.text().to_string();
                            level = t.chars().take_while(|&c| c == '#').count().clamp(1, 6);
                        }
                        SyntaxKind::SETEXT_HEADING_UNDERLINE => {
                            let t = child.text().to_string();
                            if t.chars().all(|c| c == '=') {
                                level = 1;
                            } else {
                                level = 2;
                            }
                        }
                        SyntaxKind::ATTRIBUTE => {
                            attributes = normalize_attribute_text(&child.text().to_string());
                        }
                        _ => {}
                    }
                }

                // Output heading marker
                self.output.push_str(&"#".repeat(level));
                self.output.push(' ');

                // Second pass: format content by traversing tokens/nodes directly
                // This preserves formatting without adding spaces between inline elements
                let content_start = self.output.len();
                for child in node.children() {
                    if child.kind() == SyntaxKind::HEADING_CONTENT {
                        for element in child.children_with_tokens() {
                            match element {
                                NodeOrToken::Token(t) => {
                                    self.output.push_str(t.text());
                                }
                                NodeOrToken::Node(n) => {
                                    // Format inline nodes (emphasis, code, spans, etc.)
                                    let formatted = self.format_inline_node(&n);
                                    self.output.push_str(&formatted);
                                }
                            }
                        }
                    }
                }

                // Trim trailing whitespace and hashes from content
                let content_end = self.output.len();
                let content = self.output[content_start..content_end].to_string();
                let trimmed = content.trim_end_matches(|c: char| c == '#' || c.is_whitespace());
                self.output.truncate(content_start);
                self.output.push_str(trimmed);

                // Trim trailing whitespace from content
                self.output = self.output.trim_end().to_string();

                // Add attributes if present
                if !attributes.is_empty() {
                    self.output.push(' ');
                    self.output.push_str(&attributes);
                }

                self.output.push('\n');

                if let Some(next) = node.next_sibling()
                    && (is_block_element(next.kind()) || next.kind() == SyntaxKind::HEADING)
                    && !(self.config.formatter_extensions.blank_before_header
                        && self.paragraph_starts_with_atx_heading_candidate(&next))
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }
            }

            SyntaxKind::HORIZONTAL_RULE => {
                // Output normalized horizontal rule using full available width.
                self.output
                    .push_str(&self.horizontal_rule_text(self.config.line_width));
                self.output.push('\n');

                // Ensure blank line after if followed by block element
                if let Some(next) = node.next_sibling()
                    && is_block_element(next.kind())
                    && !self.paragraph_starts_with_setext_heading_candidate(&next)
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                    self.consecutive_blank_lines = 1;
                }
            }

            SyntaxKind::REFERENCE_DEFINITION => {
                // Output reference definition as-is: [label]: url "title"
                let text = node.text().to_string();
                self.output.push_str(text.trim_end());
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }

                // Ensure blank line after if followed by non-reference block element
                if let Some(next) = node.next_sibling()
                    && is_block_element(next.kind())
                    && next.kind() != SyntaxKind::REFERENCE_DEFINITION
                    && next.kind() != SyntaxKind::FOOTNOTE_DEFINITION
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }
            }

            SyntaxKind::FOOTNOTE_DEFINITION => {
                // Format footnote definition with proper indentation
                // Extract marker and children first
                let mut marker = String::new();
                let mut child_blocks = Vec::new();

                for element in node.children_with_tokens() {
                    match element {
                        NodeOrToken::Token(token)
                            if matches!(
                                token.kind(),
                                SyntaxKind::FOOTNOTE_REFERENCE
                                    | SyntaxKind::FOOTNOTE_LABEL_START
                                    | SyntaxKind::FOOTNOTE_LABEL_ID
                                    | SyntaxKind::FOOTNOTE_LABEL_END
                                    | SyntaxKind::FOOTNOTE_LABEL_COLON
                            ) =>
                        {
                            marker.push_str(token.text());
                        }
                        NodeOrToken::Node(child) => {
                            child_blocks.push(child);
                        }
                        _ => {}
                    }
                }

                // Output indent and marker
                self.output.push_str(&" ".repeat(indent));
                self.output.push_str(marker.trim_end());

                // Format child blocks with 4-space indentation
                let child_indent = indent + 4;
                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
                let mut first = true;
                let mut pending_blank_lines = 0usize;

                for child in &child_blocks {
                    if child.kind() == SyntaxKind::BLANK_LINE {
                        pending_blank_lines = pending_blank_lines.saturating_add(1);
                        continue;
                    }

                    // Preserve explicit blank-line separators between footnote child blocks,
                    // but do not invent one when source had only a soft continuation.
                    if !first && pending_blank_lines > 0 && !self.output.ends_with("\n\n") {
                        self.output.push('\n');
                    }
                    pending_blank_lines = 0;

                    if first {
                        first = false;
                        // First paragraph - check if it can go on same line
                        if child.kind() == SyntaxKind::PARAGRAPH {
                            // Calculate how much space is available on first line
                            let marker_len = marker.len();
                            let first_line_space = self
                                .config
                                .line_width
                                .saturating_sub(indent + marker_len + 1);

                            let available_width =
                                self.config.line_width.saturating_sub(child_indent);
                            let widths = [first_line_space, available_width];
                            let lines = match wrap_mode {
                                WrapMode::Preserve => {
                                    let text = child.text().to_string();
                                    text.lines()
                                        .map(|line| {
                                            normalize_smart_punctuation(
                                                line,
                                                self.config.formatter_extensions.smart,
                                                self.config.formatter_extensions.smart_quotes,
                                            )
                                            .to_string()
                                        })
                                        .collect()
                                }
                                WrapMode::Reflow => {
                                    self.wrapped_lines_for_paragraph_with_widths(child, &widths)
                                }
                                WrapMode::Sentence => self.sentence_lines_for_paragraph(child),
                            };

                            if !lines.is_empty() {
                                self.output.push(' ');
                                self.output
                                    .push_str(lines[0].trim_start_matches([' ', '\t']));
                                self.output.push('\n');
                                for line in lines.iter().skip(1) {
                                    self.output.push_str(&" ".repeat(child_indent));
                                    self.output.push_str(line.trim_start_matches([' ', '\t']));
                                    self.output.push('\n');
                                }
                                continue;
                            }
                        }
                    }

                    // Format blocks with indentation
                    match child.kind() {
                        SyntaxKind::PARAGRAPH => {
                            // Handle paragraph with wrapping and indentation
                            let available_width =
                                self.config.line_width.saturating_sub(child_indent);

                            match wrap_mode {
                                WrapMode::Preserve => {
                                    let text = child.text().to_string();
                                    for line in text.lines() {
                                        self.output.push_str(&" ".repeat(child_indent));
                                        self.output.push_str(
                                            normalize_smart_punctuation(
                                                line.trim_start_matches([' ', '\t']),
                                                self.config.formatter_extensions.smart,
                                                self.config.formatter_extensions.smart_quotes,
                                            )
                                            .as_ref(),
                                        );
                                        self.output.push('\n');
                                    }
                                }
                                WrapMode::Reflow => {
                                    let lines =
                                        self.wrapped_lines_for_paragraph(child, available_width);
                                    for line in lines {
                                        self.output.push_str(&" ".repeat(child_indent));
                                        self.output.push_str(line.trim_start_matches([' ', '\t']));
                                        self.output.push('\n');
                                    }
                                }
                                WrapMode::Sentence => {
                                    let lines = self.sentence_lines_for_paragraph(child);
                                    for line in lines {
                                        self.output.push_str(&" ".repeat(child_indent));
                                        self.output.push_str(line.trim_start_matches([' ', '\t']));
                                        self.output.push('\n');
                                    }
                                }
                            }
                        }
                        SyntaxKind::BLANK_LINE => {
                            // Normalize blank lines to just newlines
                            self.output.push('\n');
                        }
                        SyntaxKind::CODE_BLOCK => {
                            // Format code blocks as fenced blocks with indentation
                            // Extract code content, stripping WHITESPACE tokens (indentation)
                            let mut code_lines = Vec::new();
                            for code_child in child.children() {
                                if code_child.kind() == SyntaxKind::CODE_CONTENT {
                                    // Build content line by line, skipping WHITESPACE tokens
                                    let mut line_content = String::new();
                                    for token in code_child.children_with_tokens() {
                                        if let NodeOrToken::Token(t) = token {
                                            match t.kind() {
                                                SyntaxKind::WHITESPACE => {
                                                    // Skip WHITESPACE (indentation preserved for losslessness)
                                                }
                                                SyntaxKind::TEXT => {
                                                    line_content.push_str(t.text());
                                                }
                                                SyntaxKind::NEWLINE => {
                                                    // End of line - save it and start new line
                                                    code_lines.push(line_content.clone());
                                                    line_content.clear();
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    // Don't forget last line if it doesn't end with newline
                                    if !line_content.is_empty() {
                                        code_lines.push(line_content);
                                    }
                                }
                            }

                            // Strip trailing blank lines from code content
                            while code_lines.last().is_some_and(|l| l.is_empty()) {
                                code_lines.pop();
                            }

                            // Output fenced code block with footnote indentation
                            self.output.push_str(&" ".repeat(child_indent));
                            self.output.push_str("```\n");
                            for line in code_lines {
                                if !line.is_empty() {
                                    self.output.push_str(&" ".repeat(child_indent));
                                    self.output.push_str(&line);
                                }
                                self.output.push('\n');
                            }
                            self.output.push_str(&" ".repeat(child_indent));
                            self.output.push_str("```\n");
                        }
                        _ => {
                            // Other blocks (lists, etc.) - format with indentation
                            // format_node_sync(child, child_indent) already accounts for indent,
                            // so we can append its output directly.
                            let saved_output = self.output.clone();
                            self.output.clear();
                            self.format_node_sync(child, child_indent);
                            let formatted = self.output.clone();
                            self.output = saved_output;
                            self.output.push_str(&formatted);
                        }
                    }
                }

                // If no child blocks, just end with newline
                if child_blocks.is_empty() {
                    self.output.push('\n');
                }

                // Add blank line after footnote definition (matching Pandoc's behavior)
                if let Some(next) = node.next_sibling() {
                    let next_kind = next.kind();
                    if next_kind == SyntaxKind::FOOTNOTE_DEFINITION
                        && !self.output.ends_with("\n\n")
                    {
                        self.output.push('\n');
                    }
                }
            }

            SyntaxKind::HTML_BLOCK => {
                // Check if this is a directive comment
                if let Some(directive) = extract_directive_from_node(node) {
                    // Process the directive to update tracker state
                    self.directive_tracker.process_directive(&directive);

                    // Track when we enter an ignore region to preserve content
                    if matches!(directive, crate::directives::Directive::Start(_))
                        && self.directive_tracker.is_formatting_ignored()
                        && self.ignore_region_start.is_none()
                    {
                        self.ignore_region_start = Some(self.output.len());
                    }
                }

                // Output HTML block exactly as written
                let text = node.text().to_string();
                self.output.push_str(&text);
                if !text.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::COMMENT => {
                let text = node.text().to_string();

                // Check if this is a directive
                if let Some(directive) = extract_directive_from_node(node) {
                    // Process the directive to update tracker state
                    self.directive_tracker.process_directive(&directive);

                    // Track when we enter an ignore region to preserve content
                    if matches!(directive, crate::directives::Directive::Start(_))
                        && self.directive_tracker.is_formatting_ignored()
                        && self.ignore_region_start.is_none()
                    {
                        self.ignore_region_start = Some(self.output.len());
                    }
                }

                // Always output the comment itself
                self.output.push_str(&text);
                if !text.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::LATEX_COMMAND => {
                // Standalone LaTeX commands - preserve exactly as written
                let text = node.text().to_string();
                self.output.push_str(&text);
                // Don't add extra newlines for standalone LaTeX commands
            }

            SyntaxKind::TEX_BLOCK => {
                log::trace!("Formatting TeX block");
                // Raw blocks (LaTeX commands, etc.) - preserve verbatim
                // Just output all content as-is
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Token(t) => {
                            self.output.push_str(t.text());
                        }
                        rowan::NodeOrToken::Node(_) => {
                            // No child nodes in the simplified structure
                        }
                    }
                }

                // Ensure newline at end of raw block
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::BLOCK_QUOTE => {
                log::trace!("Formatting blockquote");
                // Determine nesting depth by counting ancestor BlockQuote nodes (including self)
                let depth = BlockQuote::cast(node.clone())
                    .map(|bq| bq.depth())
                    .unwrap_or(1);

                // Prefixes for quoted content and blank quoted lines
                let base_indent = " ".repeat(indent);
                let content_prefix = format!("{}{}", base_indent, "> ".repeat(depth)); // includes trailing space
                let blank_prefix = content_prefix.trim_end().to_string(); // no trailing space

                // Format children (paragraphs, blank lines) with proper > prefix per depth
                // NOTE: BlockQuoteMarker tokens are in the tree for losslessness, but we ignore
                // them during formatting and add prefixes dynamically instead.
                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
                let blockquote_children: Vec<_> = node.children().collect();
                let saved_blockquote_context = self.blockquote_context.clone();
                self.blockquote_context = Some(BlockquoteContext {
                    in_list_continuation: false,
                });

                for (child_idx, child) in blockquote_children.iter().enumerate() {
                    let prev_significant_kind =
                        Self::blockquote_prev_significant_kind(&blockquote_children, child_idx);
                    let next_significant_kind =
                        Self::blockquote_next_significant_kind(&blockquote_children, child_idx);
                    match child.kind() {
                        // Skip BlockQuoteMarker tokens - we add prefixes dynamically
                        SyntaxKind::BLOCK_QUOTE_MARKER => continue,

                        SyntaxKind::PARAGRAPH => match wrap_mode {
                            WrapMode::Preserve => {
                                // Build paragraph text while skipping BlockQuoteMarker tokens
                                // (they're in the tree for losslessness but we add prefixes dynamically)
                                let mut lines_text = String::new();
                                let mut skip_next_whitespace = false;
                                for item in child.children_with_tokens() {
                                    match item {
                                        NodeOrToken::Token(t)
                                            if t.kind() == SyntaxKind::BLOCK_QUOTE_MARKER =>
                                        {
                                            // Skip marker - we add these dynamically
                                            // Also skip the following whitespace (part of marker syntax)
                                            skip_next_whitespace = true;
                                        }
                                        NodeOrToken::Token(t)
                                            if t.kind() == SyntaxKind::WHITESPACE
                                                && skip_next_whitespace =>
                                        {
                                            // Skip whitespace after marker
                                            skip_next_whitespace = false;
                                        }
                                        NodeOrToken::Token(t) => {
                                            skip_next_whitespace = false;
                                            lines_text.push_str(t.text());
                                        }
                                        NodeOrToken::Node(n) => {
                                            skip_next_whitespace = false;
                                            lines_text.push_str(&n.text().to_string());
                                        }
                                    }
                                }

                                for line in lines_text.lines() {
                                    self.output.push_str(&content_prefix);
                                    self.output.push_str(line);
                                    self.output.push('\n');
                                }
                            }
                            WrapMode::Reflow => {
                                let width =
                                    self.config.line_width.saturating_sub(content_prefix.len());
                                let lines = self.wrapped_lines_for_paragraph(child, width);
                                for line in lines {
                                    self.output.push_str(&content_prefix);
                                    self.output.push_str(&line);
                                    self.output.push('\n');
                                }
                            }
                            WrapMode::Sentence => {
                                let lines = self.sentence_lines_for_paragraph(child);
                                for line in lines {
                                    self.output.push_str(&content_prefix);
                                    self.output.push_str(&line);
                                    self.output.push('\n');
                                }
                            }
                        },
                        SyntaxKind::ALERT => {
                            let marker = child
                                .children_with_tokens()
                                .filter_map(|item| item.into_token())
                                .find(|tok| tok.kind() == SyntaxKind::ALERT_MARKER)
                                .map(|tok| tok.text().to_string())
                                .unwrap_or_else(|| "[!NOTE]".to_string());

                            self.output.push_str(&content_prefix);
                            self.output.push_str(&marker);
                            self.output.push('\n');

                            for alert_child in child.children() {
                                match alert_child.kind() {
                                    SyntaxKind::PARAGRAPH => match wrap_mode {
                                        WrapMode::Preserve => {
                                            let text = alert_child.text().to_string();
                                            for line in text.lines() {
                                                self.output.push_str(&content_prefix);
                                                self.output.push_str(line);
                                                self.output.push('\n');
                                            }
                                        }
                                        WrapMode::Reflow => {
                                            let width = self
                                                .config
                                                .line_width
                                                .saturating_sub(content_prefix.len());
                                            for line in self
                                                .wrapped_lines_for_paragraph(&alert_child, width)
                                            {
                                                self.output.push_str(&content_prefix);
                                                self.output.push_str(&line);
                                                self.output.push('\n');
                                            }
                                        }
                                        WrapMode::Sentence => {
                                            for line in
                                                self.sentence_lines_for_paragraph(&alert_child)
                                            {
                                                self.output.push_str(&content_prefix);
                                                self.output.push_str(&line);
                                                self.output.push('\n');
                                            }
                                        }
                                    },
                                    SyntaxKind::BLANK_LINE => {
                                        self.output.push_str(&blank_prefix);
                                        self.output.push('\n');
                                    }
                                    _ => {
                                        let saved_output = self.output.clone();
                                        let saved_line_width = self.config.line_width;
                                        self.output.clear();
                                        self.config.line_width = self
                                            .config
                                            .line_width
                                            .saturating_sub(content_prefix.len());
                                        self.format_node_sync(&alert_child, indent);
                                        let rendered = self.output.clone();
                                        self.config.line_width = saved_line_width;
                                        self.output = saved_output;

                                        for line in rendered.lines() {
                                            if line.is_empty() {
                                                self.output.push_str(&blank_prefix);
                                            } else if line.starts_with("> ") {
                                                self.output.push_str(&base_indent);
                                                self.output.push_str(line);
                                            } else {
                                                self.output.push_str(&content_prefix);
                                                self.output.push_str(line);
                                            }
                                            self.output.push('\n');
                                        }
                                    }
                                }
                            }
                        }
                        SyntaxKind::BLANK_LINE => {
                            let blank_suffix = child.text().to_string();
                            let blank_suffix = blank_suffix.trim_end_matches(['\r', '\n']);
                            if blank_suffix.is_empty() {
                                self.output.push_str(&blank_prefix);
                            } else {
                                if Self::should_preserve_blockquote_blank_suffix(
                                    prev_significant_kind,
                                    next_significant_kind,
                                    blank_suffix,
                                    self.blockquote_context
                                        .as_ref()
                                        .is_some_and(|ctx| ctx.in_list_continuation),
                                ) {
                                    self.output.push_str(&content_prefix);
                                    self.output.push_str(blank_suffix);
                                } else {
                                    self.output.push_str(&blank_prefix);
                                }
                            }
                            self.output.push('\n');
                        }
                        SyntaxKind::HORIZONTAL_RULE => {
                            self.output.push_str(&content_prefix);
                            let available_width =
                                self.config.line_width.saturating_sub(content_prefix.len());
                            self.output
                                .push_str(&self.horizontal_rule_text(available_width));
                            self.output.push('\n');
                        }
                        SyntaxKind::HEADING => {
                            // Format heading with blockquote prefix
                            let heading_text = self.format_heading(child);
                            for line in heading_text.lines() {
                                self.output.push_str(&content_prefix);
                                self.output.push_str(line);
                                self.output.push('\n');
                            }
                            if let Some(next) = child.next_sibling()
                                && next.kind() != SyntaxKind::BLANK_LINE
                                && is_block_element(next.kind())
                            {
                                self.output.push_str(&blank_prefix);
                                self.output.push('\n');
                            }
                        }
                        SyntaxKind::LIST => {
                            // Format list with blockquote prefix
                            // Save current output, format list to temp, then prefix each line
                            let saved_output = self.output.clone();
                            let saved_line_width = self.config.line_width;
                            self.output.clear();
                            self.config.line_width =
                                self.config.line_width.saturating_sub(content_prefix.len());
                            // We trim list-temp indentation before re-prefixing with `content_prefix`.
                            // Format at indent 0 here to avoid double-accounting indentation width.
                            self.format_node_sync(child, 0);
                            let list_output = self.output.clone();
                            self.config.line_width = saved_line_width;
                            self.output = saved_output;

                            let ends_in_list_continuation = self
                                .append_blockquote_prefixed_list_output(
                                    &list_output,
                                    &base_indent,
                                    &content_prefix,
                                    &blank_prefix,
                                );
                            if let Some(ctx) = self.blockquote_context.as_mut() {
                                ctx.in_list_continuation = ends_in_list_continuation;
                            }
                        }
                        SyntaxKind::CODE_BLOCK => {
                            // Format code block with blockquote prefix
                            // Save current output, format code block to temp, then prefix each line
                            let code_block_leading_indent = Self::code_block_leading_indent(child);
                            let saved_output = self.output.clone();
                            self.output.clear();
                            self.format_node_sync(child, indent);
                            let code_output = self.output.clone();
                            self.output = saved_output;

                            self.append_blockquote_prefixed_block(
                                &code_output,
                                &content_prefix,
                                &blank_prefix,
                                Some(&code_block_leading_indent),
                            );
                            if let Some(ctx) = self.blockquote_context.as_mut() {
                                ctx.in_list_continuation = false;
                            }
                        }
                        SyntaxKind::TEX_BLOCK => {
                            // Keep raw TeX content verbatim, but preserve blockquote prefixes.
                            let saved_output = self.output.clone();
                            self.output.clear();
                            self.format_node_sync(child, indent);
                            let tex_output = self.output.clone();
                            self.output = saved_output;

                            self.append_blockquote_prefixed_block(
                                &tex_output,
                                &content_prefix,
                                &blank_prefix,
                                None,
                            );
                        }
                        _ => {
                            // Handle other content within block quotes
                            self.format_node_sync(child, indent);
                            if let Some(ctx) = self.blockquote_context.as_mut() {
                                ctx.in_list_continuation = matches!(
                                    child.kind(),
                                    SyntaxKind::LIST | SyntaxKind::LIST_ITEM
                                );
                            }
                        }
                    }
                }
                self.blockquote_context = saved_blockquote_context;
            }

            SyntaxKind::PARAGRAPH => {
                let text = node.text().to_string();
                log::debug!("Formatting paragraph, text length: {}", text.len());
                let paragraph_indent = " ".repeat(indent);

                if self.is_grid_table_continuation_paragraph(node) {
                    if indent > 0 {
                        for (i, line) in text.lines().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                            }
                            self.output.push_str(&paragraph_indent);
                            self.output.push_str(
                                normalize_smart_punctuation(
                                    line.trim_start(),
                                    self.config.formatter_extensions.smart,
                                    self.config.formatter_extensions.smart_quotes,
                                )
                                .as_ref(),
                            );
                        }
                    } else {
                        self.output.push_str(
                            normalize_smart_punctuation(
                                &text,
                                self.config.formatter_extensions.smart,
                                self.config.formatter_extensions.smart_quotes,
                            )
                            .as_ref(),
                        );
                    }
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    return;
                }

                if self.config.formatter_extensions.bookdown_references
                    && paragraphs::is_bookdown_text_reference(node)
                {
                    if indent > 0 {
                        for (i, line) in text.lines().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                            }
                            self.output.push_str(&paragraph_indent);
                            self.output.push_str(
                                normalize_smart_punctuation(
                                    line.trim_start(),
                                    self.config.formatter_extensions.smart,
                                    self.config.formatter_extensions.smart_quotes,
                                )
                                .as_ref(),
                            );
                        }
                    } else {
                        self.output.push_str(
                            normalize_smart_punctuation(
                                &text,
                                self.config.formatter_extensions.smart,
                                self.config.formatter_extensions.smart_quotes,
                            )
                            .as_ref(),
                        );
                    }
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    return;
                }

                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
                let preserve_newlines_for_latex =
                    self.fenced_div_depth > 0 && self.contains_latex_command(node);
                if preserve_newlines_for_latex && self.fenced_div_depth > 0 {
                    if indent > 0 {
                        for (i, line) in text.lines().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                            }
                            self.output.push_str(&paragraph_indent);
                            self.output.push_str(
                                normalize_smart_punctuation(
                                    line.trim_start(),
                                    self.config.formatter_extensions.smart,
                                    self.config.formatter_extensions.smart_quotes,
                                )
                                .as_ref(),
                            );
                        }
                    } else {
                        self.output.push_str(
                            normalize_smart_punctuation(
                                &text,
                                self.config.formatter_extensions.smart,
                                self.config.formatter_extensions.smart_quotes,
                            )
                            .as_ref(),
                        );
                    }
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    return;
                }
                log::debug!(
                    "Paragraph wrap mode: {:?}, line_width: {}",
                    wrap_mode,
                    line_width
                );
                match wrap_mode {
                    WrapMode::Preserve => {
                        log::trace!("Preserving paragraph line breaks");
                        if indent > 0 {
                            for (i, line) in text.lines().enumerate() {
                                if i > 0 {
                                    self.output.push('\n');
                                }
                                self.output.push_str(&paragraph_indent);
                                self.output.push_str(
                                    normalize_smart_punctuation(
                                        line.trim_start(),
                                        self.config.formatter_extensions.smart,
                                        self.config.formatter_extensions.smart_quotes,
                                    )
                                    .as_ref(),
                                );
                            }
                        } else {
                            self.output.push_str(
                                normalize_smart_punctuation(
                                    &text,
                                    self.config.formatter_extensions.smart,
                                    self.config.formatter_extensions.smart_quotes,
                                )
                                .as_ref(),
                            );
                        }
                        if !self.output.ends_with('\n') {
                            self.output.push('\n');
                        }
                    }
                    WrapMode::Reflow => {
                        log::trace!("Reflowing paragraph to {} width", line_width);
                        let lines = self.wrapped_lines_for_paragraph(node, line_width);

                        for (i, line) in lines.iter().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                            }
                            if indent > 0 {
                                self.output.push_str(&paragraph_indent);
                            }
                            self.output.push_str(line);
                        }
                    }
                    WrapMode::Sentence => {
                        log::trace!("Wrapping paragraph by sentence");
                        let lines = self.sentence_lines_for_paragraph(node);

                        for (i, line) in lines.iter().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                            }
                            if indent > 0 {
                                self.output.push_str(&paragraph_indent);
                            }
                            self.output.push_str(line);
                        }
                    }
                }

                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::FIGURE => {
                // Figure is a standalone image - format the inline content directly
                log::debug!("Formatting figure");
                let text = self.format_inline_node(node);
                let trimmed = text.trim();
                if indent > 0 {
                    self.output.push_str(&" ".repeat(indent));
                }
                self.output.push_str(trimmed);
                self.output.push('\n');
            }

            SyntaxKind::PLAIN => {
                // Plain is like PARAGRAPH but for tight contexts (definition lists, table cells)
                // Apply wrapping with continuation indentation
                let text = node.text().to_string();
                log::debug!("Formatting Plain block, text length: {}", text.len());

                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
                let needs_indent = indent > 0
                    && (self.output.ends_with('\n') || self.output.is_empty())
                    && !self.output.ends_with(":   ");
                match wrap_mode {
                    WrapMode::Preserve => {
                        if needs_indent {
                            for line in text.lines() {
                                self.output.push_str(&" ".repeat(indent));
                                self.output.push_str(
                                    normalize_smart_punctuation(
                                        line.trim_start(),
                                        self.config.formatter_extensions.smart,
                                        self.config.formatter_extensions.smart_quotes,
                                    )
                                    .as_ref(),
                                );
                                self.output.push('\n');
                            }
                        } else {
                            self.output.push_str(
                                normalize_smart_punctuation(
                                    &text,
                                    self.config.formatter_extensions.smart,
                                    self.config.formatter_extensions.smart_quotes,
                                )
                                .as_ref(),
                            );
                            if !self.output.ends_with('\n') {
                                self.output.push('\n');
                            }
                        }
                    }
                    WrapMode::Reflow => {
                        log::trace!("Reflowing Plain block to {} width", line_width);
                        let in_definition = self.output.ends_with(":   ");
                        let preserve_ambiguous_definition_emphasis =
                            in_definition && text.contains(r"\|*") && text.contains(".*");
                        let lines = if in_definition {
                            if preserve_ambiguous_definition_emphasis {
                                text.lines().map(ToString::to_string).collect()
                            } else {
                                let marker_len = ":   ".len();
                                let marker_indent = indent.saturating_sub(4);
                                let first_line_space =
                                    line_width.saturating_sub(marker_indent + marker_len);
                                let continuation_width = line_width.saturating_sub(indent);
                                let widths = [first_line_space, continuation_width];
                                self.wrapped_lines_for_paragraph_with_widths(node, &widths)
                            }
                        } else {
                            self.wrapped_lines_for_paragraph(node, line_width)
                        };

                        for (i, line) in lines.iter().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                                // Add continuation indent for wrapped lines
                                self.output.push_str(&" ".repeat(indent));
                            } else if needs_indent {
                                self.output.push_str(&" ".repeat(indent));
                            }
                            let rendered = if i > 0 && indent > 0 {
                                line.trim_start()
                            } else {
                                line.as_str()
                            };
                            self.output.push_str(rendered);
                        }

                        if !self.output.ends_with('\n') {
                            self.output.push('\n');
                        }
                    }
                    WrapMode::Sentence => {
                        log::trace!("Wrapping Plain block by sentence");
                        let in_definition = self.output.ends_with(":   ");
                        let preserve_ambiguous_definition_emphasis =
                            in_definition && text.contains(r"\|*") && text.contains(".*");
                        let lines = if preserve_ambiguous_definition_emphasis {
                            text.lines().map(ToString::to_string).collect()
                        } else {
                            self.sentence_lines_for_paragraph(node)
                        };

                        for (i, line) in lines.iter().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                                self.output.push_str(&" ".repeat(indent));
                            } else if needs_indent {
                                self.output.push_str(&" ".repeat(indent));
                            }
                            let rendered = if i > 0 && indent > 0 {
                                line.trim_start()
                            } else {
                                line.as_str()
                            };
                            self.output.push_str(rendered);
                        }

                        if !self.output.ends_with('\n') {
                            self.output.push('\n');
                        }
                    }
                }
            }

            SyntaxKind::LIST => {
                self.format_list(node, indent);
            }

            SyntaxKind::DEFINITION_LIST => {
                if self.is_grid_table_caption_definition_list(node) {
                    self.output.push_str(&node.text().to_string());
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    return;
                }
                // Add blank line before top-level definition lists
                if indent == 0 && !self.output.is_empty() && !self.output.ends_with("\n\n") {
                    self.output.push('\n');
                }
                let mut saw_item = false;
                for child in node.children() {
                    if child.kind() == SyntaxKind::BLANK_LINE {
                        continue;
                    }
                    if child.kind() == SyntaxKind::DEFINITION_ITEM {
                        if saw_item && !self.output.ends_with("\n\n") {
                            self.output.push('\n');
                        }
                        saw_item = true;
                    }
                    self.format_node_sync(&child, indent);
                }
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::LINE_BLOCK => {
                log::debug!("Formatting line block");
                // Add blank line before line blocks if not at start
                if !self.output.is_empty() && !self.output.ends_with("\n\n") {
                    self.output.push('\n');
                }

                // Format each line preserving line breaks and leading spaces
                for child in node.children() {
                    if child.kind() == SyntaxKind::LINE_BLOCK_LINE {
                        // Get the text content, preserving leading spaces
                        let text = child.text().to_string();
                        if text.trim_end() == "|" {
                            self.output.push('|');
                            self.output.push('\n');
                            continue;
                        }
                        // The text might start with "| " from the marker, or be continuation
                        // We need to skip the marker if present and output the rest
                        let content = if let Some(stripped) = text.strip_prefix("| ") {
                            stripped
                        } else {
                            // Continuation line - output as-is but with proper marker
                            text.trim_start()
                        };

                        // Output the marker
                        // Check if content is empty or just whitespace/newline
                        let content_trimmed = content.trim();
                        if content_trimmed.is_empty() {
                            // Empty line block line - just output "|"
                            self.output.push('|');
                        } else {
                            // Normal line - output "| " followed by content
                            self.output.push_str("| ");
                            self.output.push_str(content.trim_end());
                        }
                        self.output.push('\n');
                    }
                }

                // Add blank line after if followed by block element
                if let Some(next) = node.next_sibling()
                    && is_block_element(next.kind())
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }
            }

            SyntaxKind::DEFINITION_ITEM => {
                let is_compact_by_structure = DefinitionItem::cast(node.clone())
                    .map(|item| item.is_compact())
                    .unwrap_or(true);
                let mut has_blank_between_term_and_first_definition = false;
                let mut seen_term = false;
                let mut seen_definition = false;

                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::TERM => {
                            seen_term = true;
                        }
                        SyntaxKind::BLANK_LINE => {
                            if seen_term && !seen_definition {
                                has_blank_between_term_and_first_definition = true;
                            }
                        }
                        SyntaxKind::DEFINITION => {
                            seen_definition = true;
                        }
                        _ => {}
                    }
                }

                let is_compact =
                    is_compact_by_structure && !has_blank_between_term_and_first_definition;
                let mut saw_term = false;

                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::BLANK_LINE => {
                            // Ignore source blank lines and normalize based on AST structure.
                        }
                        SyntaxKind::TERM => {
                            self.format_node_sync(&child, indent);
                            saw_term = true;
                        }
                        SyntaxKind::DEFINITION => {
                            if saw_term {
                                if is_compact {
                                    if !self.output.ends_with('\n') {
                                        self.output.push('\n');
                                    }
                                } else if !self.output.ends_with("\n\n") {
                                    self.output.push('\n');
                                }
                            } else if !self.output.is_empty() && !self.output.ends_with('\n') {
                                self.output.push('\n');
                            }
                            self.format_node_sync(&child, indent);
                        }
                        _ => self.format_node_sync(&child, indent),
                    }
                }
            }

            SyntaxKind::TERM => {
                // Format term - just emit text with newline
                for child in node.children_with_tokens() {
                    match child {
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::TEXT => {
                            self.output.push_str(tok.text());
                        }
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::NEWLINE => {
                            self.output.push('\n');
                        }
                        NodeOrToken::Node(n) => {
                            self.format_node_sync(&n, indent);
                        }
                        _ => {}
                    }
                }
            }

            SyntaxKind::DEFINITION => {
                // Format definition with marker and content
                // The definition marker itself is at the base indent level
                // Definition content is indented 4 spaces from the margin
                let def_indent = indent + 4;
                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);

                // Emit base indentation before the marker
                if indent > 0 {
                    self.output.push_str(&" ".repeat(indent));
                }
                self.output.push_str(":   ");

                // Collect children to determine lazy continuation
                let children: Vec<_> = node.children_with_tokens().collect();
                let mut first_para_idx = None;

                // Find first paragraph immediately after initial text (lazy continuation)
                // It's only lazy if there's no BlankLine before it
                let mut text_idx = None;
                for (i, child) in children.iter().enumerate() {
                    if let NodeOrToken::Token(tok) = child
                        && tok.kind() == SyntaxKind::TEXT
                    {
                        text_idx = Some(i);
                    }
                }

                // Check if there's a paragraph immediately after TEXT+NEWLINE (no BlankLine)
                if let Some(tidx) = text_idx {
                    for (i, child) in children.iter().enumerate().skip(tidx + 1) {
                        if let NodeOrToken::Node(n) = child {
                            match n.kind() {
                                SyntaxKind::PARAGRAPH => {
                                    first_para_idx = Some(i);
                                    break;
                                }
                                SyntaxKind::BLANK_LINE => {
                                    // BlankLine before paragraph - not lazy
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                }

                for (i, child) in children.iter().enumerate() {
                    match child {
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::TEXT => {
                            self.output.push_str(tok.text());
                        }
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::NEWLINE => {
                            // If next child is the first lazy paragraph, add space instead
                            if first_para_idx.is_some_and(|idx| i + 1 == idx) {
                                self.output.push(' ');
                            } else {
                                self.output.push('\n');
                            }
                        }
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::DEFINITION_MARKER => {
                            // Skip - we already added `:   `
                        }
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::WHITESPACE => {
                            // Skip - we normalize spacing
                        }
                        NodeOrToken::Node(n) => {
                            // Handle continuation content with proper indentation
                            match n.kind() {
                                SyntaxKind::CODE_BLOCK => {
                                    if self.output.ends_with(":   ") {
                                        self.format_container_code_block(
                                            n, "", def_indent, true, true, false,
                                        );
                                    } else {
                                        // Add blank line before code block if needed
                                        if !self.output.ends_with("\n\n") {
                                            self.output.push('\n');
                                        }
                                        self.format_indented_code_block(n, def_indent);
                                    }
                                }
                                SyntaxKind::PLAIN => {
                                    // Plain block in definition - format inline with potential wrapping
                                    // Already handled by Plain formatter above
                                    if let Some((heading_line, remainder)) =
                                        self.leading_atx_heading_with_remainder(n)
                                    {
                                        self.output.push_str(&heading_line);
                                        self.output.push('\n');
                                        self.output.push('\n');
                                        for line in
                                            self.wrap_text_for_indent(&remainder, def_indent)
                                        {
                                            self.output.push_str(&" ".repeat(def_indent));
                                            self.output.push_str(line.trim_start());
                                            self.output.push('\n');
                                        }
                                    } else {
                                        self.format_node_sync(n, def_indent);
                                    }
                                }
                                SyntaxKind::PARAGRAPH => {
                                    if first_para_idx == Some(i) {
                                        // First paragraph - lazy continuation (inline, wrapped)
                                        let marker_len = ":   ".len();
                                        let first_line_space = self
                                            .config
                                            .line_width
                                            .saturating_sub(indent + marker_len);
                                        let available_width =
                                            self.config.line_width.saturating_sub(def_indent);
                                        let widths = [first_line_space, available_width];

                                        let lines = match wrap_mode {
                                            WrapMode::Preserve => {
                                                let text = n.text().to_string();
                                                text.lines().map(|line| line.to_string()).collect()
                                            }
                                            WrapMode::Reflow => self
                                                .wrapped_lines_for_paragraph_with_widths(
                                                    n, &widths,
                                                ),
                                            WrapMode::Sentence => {
                                                self.sentence_lines_for_paragraph(n)
                                            }
                                        };

                                        if !lines.is_empty() {
                                            self.output.push_str(&lines[0]);
                                            self.output.push('\n');
                                            for line in lines.iter().skip(1) {
                                                self.output.push_str(&" ".repeat(def_indent));
                                                self.output.push_str(line.trim_start());
                                                self.output.push('\n');
                                            }
                                        }
                                    } else {
                                        // Subsequent paragraphs - indented continuation
                                        if !self.output.ends_with("\n\n") {
                                            self.output.push('\n');
                                        }
                                        self.format_list_continuation_paragraph(n, def_indent);
                                    }
                                }
                                SyntaxKind::BLANK_LINE => {
                                    // Normalize blank lines in definitions to just newlines
                                    // (strip trailing whitespace)
                                    let is_before_first_para =
                                        first_para_idx.is_some_and(|idx| i < idx);

                                    if !is_before_first_para {
                                        self.output.push('\n');
                                    }
                                }
                                SyntaxKind::LIST => {
                                    let start = self.output.len();
                                    self.format_node_sync(n, def_indent);

                                    if self.output[..start].ends_with(":   ")
                                        && self.output[start..].starts_with(&" ".repeat(def_indent))
                                    {
                                        self.output.drain(start..start + def_indent);
                                    }
                                }
                                SyntaxKind::BLOCK_QUOTE => {
                                    if self.output.ends_with(":   ") {
                                        let mut pieces: Vec<String> = Vec::new();
                                        let block_text = n.text().to_string();
                                        for line in block_text.lines() {
                                            let trimmed = line.trim_start();
                                            let content =
                                                if let Some(rest) = trimmed.strip_prefix('>') {
                                                    rest.trim_start()
                                                } else {
                                                    trimmed
                                                };
                                            if !content.is_empty() {
                                                pieces.push(content.to_string());
                                            }
                                        }

                                        self.output.push_str("> ");
                                        self.output.push_str(&pieces.join(" "));
                                        self.output.push('\n');

                                        if let Some(next_non_blank) =
                                            node.children().skip(i + 1).find(|sibling| {
                                                sibling.kind() != SyntaxKind::BLANK_LINE
                                            })
                                            && is_block_element(next_non_blank.kind())
                                            && !self.output.ends_with("\n\n")
                                        {
                                            self.output.push('\n');
                                        }
                                    } else {
                                        self.format_node_sync(n, def_indent);
                                    }
                                }
                                _ => {
                                    self.format_node_sync(n, def_indent);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::SIMPLE_TABLE => {
                log::trace!("Formatting simple table");
                let formatted = tables::format_simple_table(node, &self.config);
                self.output.push_str(&formatted);

                // Ensure blank line after if followed by block element
                if let Some(next) = node.next_sibling()
                    && is_block_element(next.kind())
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }
            }

            SyntaxKind::MULTILINE_TABLE => {
                // Format multiline table with proper alignment and column widths
                let formatted = tables::format_multiline_table(node, &self.config);
                self.output.push_str(&formatted);
            }

            SyntaxKind::PIPE_TABLE => {
                // Format pipe table with proper alignment
                let formatted = tables::format_pipe_table(node, &self.config);
                self.output.push_str(&formatted);
            }

            SyntaxKind::GRID_TABLE => {
                if let Some(next) = node.next_sibling()
                    && self.is_grid_table_continuation_paragraph(&next)
                {
                    self.output.push_str(&node.text().to_string());
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    return;
                }
                // Format grid table with proper alignment and borders
                let formatted = tables::format_grid_table(node, &self.config);
                self.output.push_str(&formatted);
            }

            SyntaxKind::INLINE_MATH => {
                // Check if this is display math (has DisplayMathMarker)
                let is_display_math = node.children_with_tokens().any(|t| {
                    matches!(t, NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::DISPLAY_MATH_MARKER)
                });

                // Get the actual content (TEXT token, not node)
                let content = node
                    .children_with_tokens()
                    .find_map(|c| match c {
                        NodeOrToken::Token(t) if t.kind() == SyntaxKind::TEXT => {
                            Some(t.text().to_string())
                        }
                        _ => None,
                    })
                    .unwrap_or_default();

                // Get original marker to determine input format
                let original_marker = node
                    .children_with_tokens()
                    .find_map(|t| match t {
                        NodeOrToken::Token(tok)
                            if tok.kind() == SyntaxKind::INLINE_MATH_MARKER
                                || tok.kind() == SyntaxKind::DISPLAY_MATH_MARKER =>
                        {
                            Some(tok.text().to_string())
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| "$".to_string());

                // Determine output format based on config
                use crate::config::MathDelimiterStyle;
                let (open, close) = match self.config.math_delimiter_style {
                    MathDelimiterStyle::Preserve => {
                        // Keep original format
                        if is_display_math {
                            match original_marker.as_str() {
                                "\\[" => (r"\[", r"\]"),
                                "\\\\[" => (r"\\[", r"\\]"),
                                _ => ("$$", "$$"), // Default to $$
                            }
                        } else {
                            match original_marker.as_str() {
                                "$`" => ("$`", "`$"),
                                r"\(" => (r"\(", r"\)"),
                                r"\\(" => (r"\\(", r"\\)"),
                                _ => ("$", "$"), // Default to $
                            }
                        }
                    }
                    MathDelimiterStyle::Dollars => {
                        // Normalize to dollars
                        if is_display_math {
                            ("$$", "$$")
                        } else {
                            ("$", "$")
                        }
                    }
                    MathDelimiterStyle::Backslash => {
                        // Normalize to single backslash
                        if is_display_math {
                            (r"\[", r"\]")
                        } else {
                            (r"\(", r"\)")
                        }
                    }
                };

                // Output formatted math
                if is_display_math {
                    self.output.push_str(open);
                    self.output.push(' ');
                    self.output.push_str(&content);
                    self.output.push(' ');
                    self.output.push_str(close);
                } else {
                    self.output.push_str(open);
                    self.output.push_str(&content);
                    self.output.push_str(close);
                }
            }

            SyntaxKind::LIST_ITEM => {
                self.format_list_item(node, indent);
            }

            SyntaxKind::FENCED_DIV => {
                let Some(fenced_div) = FencedDiv::cast(node.clone()) else {
                    self.output.push_str(&node.text().to_string());
                    return;
                };

                let opening_has_trailing_inline_text =
                    fenced_div.opening_fence().is_some_and(|open| {
                        let mut saw_info = false;
                        for child in open.syntax().children_with_tokens() {
                            match child {
                                rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::DIV_INFO => {
                                    saw_info = true;
                                }
                                rowan::NodeOrToken::Token(t)
                                    if saw_info && t.kind() == SyntaxKind::TEXT =>
                                {
                                    let trimmed = t.text().trim();
                                    if !trimmed.is_empty() && !trimmed.chars().all(|c| c == ':') {
                                        return true;
                                    }
                                }
                                _ => {}
                            }
                        }
                        false
                    });
                if opening_has_trailing_inline_text {
                    // Preserve malformed one-line div text verbatim to keep parser shape stable
                    // across format passes. Trimming can shift boundary ownership of following
                    // blank lines and break idempotency.
                    self.output.push_str(&node.text().to_string());
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    return;
                }

                let has_close = fenced_div.has_closing_fence();
                let has_content = fenced_div
                    .body_blocks()
                    .any(|child| child.kind() != SyntaxKind::BLANK_LINE);
                let leading_blank_lines = fenced_div
                    .body_blocks()
                    .take_while(|child| child.kind() == SyntaxKind::BLANK_LINE)
                    .count();

                // Preserve malformed one-line divs verbatim. Patterns like
                // `::: {.callout} inline :::` can parse as an opening fence with
                // trailing text but no body/closing node; normalizing them creates
                // unstable nesting and loses inline content.
                if !has_close && !has_content {
                    if let Some(open) = fenced_div.opening_fence() {
                        self.output
                            .push_str(open.syntax().text().to_string().trim_end_matches('\n'));
                    } else {
                        self.output
                            .push_str(node.text().to_string().trim_end_matches('\n'));
                    }
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    return;
                }

                let source_opening_colons = fenced_div
                    .opening_fence()
                    .map(|open| {
                        open.syntax()
                            .text()
                            .to_string()
                            .trim_start()
                            .chars()
                            .take_while(|&c| c == ':')
                            .count()
                    })
                    .unwrap_or(3)
                    .max(3);
                let in_list_item = node
                    .ancestors()
                    .any(|ancestor| ancestor.kind() == SyntaxKind::LIST_ITEM);
                let depth_encoded_colons = 3 + (self.fenced_div_depth * 2);
                let opening_colons = if in_list_item {
                    source_opening_colons
                } else {
                    depth_encoded_colons
                };
                let colons = ":".repeat(opening_colons);

                let attributes = fenced_div.info_text();
                // Emit normalized opening fence
                if !has_close && !has_content {
                    self.output.push_str(&" ".repeat(indent));
                    if let Some(attrs) = &attributes
                        && !attrs.is_empty()
                    {
                        self.output.push_str(&colons);
                        self.output.push(' ');
                        self.output.push_str(attrs);
                        self.output.push('\n');
                        return;
                    }
                } else {
                    self.output.push_str(&" ".repeat(indent));
                    self.output.push_str(&colons);
                    if let Some(attrs) = &attributes
                        && !attrs.is_empty()
                    {
                        self.output.push(' ');
                        self.output.push_str(attrs);
                    }
                    self.output.push('\n');
                }

                // Increment depth for nested content
                self.fenced_div_depth += 1;

                // Process content
                let content_children: Vec<_> = node
                    .children()
                    .filter(|child| {
                        !matches!(
                            child.kind(),
                            SyntaxKind::DIV_FENCE_OPEN
                                | SyntaxKind::DIV_INFO
                                | SyntaxKind::DIV_FENCE_CLOSE
                        )
                    })
                    .collect();

                let end = content_children.len();
                let first_non_blank_kind = content_children[0..end]
                    .iter()
                    .find(|child| child.kind() != SyntaxKind::BLANK_LINE)
                    .map(|child| child.kind());
                let should_strip_leading_blanks = matches!(
                    first_non_blank_kind,
                    Some(
                        SyntaxKind::PARAGRAPH
                            | SyntaxKind::PLAIN
                            | SyntaxKind::LIST
                            | SyntaxKind::LIST_ITEM
                    )
                );
                let start = if should_strip_leading_blanks {
                    leading_blank_lines
                } else {
                    0
                };

                for (idx, child) in content_children[start..end].iter().enumerate() {
                    if child.kind() == SyntaxKind::BLANK_LINE {
                        if idx < leading_blank_lines
                            && matches!(
                                first_non_blank_kind,
                                Some(SyntaxKind::LIST | SyntaxKind::LIST_ITEM)
                            )
                        {
                            continue;
                        }
                        self.output.push('\n');
                    } else if child.kind() == SyntaxKind::CODE_BLOCK && indent > 0 {
                        self.format_indented_code_block(child, indent);
                        if let Some(next) = content_children[start..end].get(idx + 1)
                            && ((next.kind() == SyntaxKind::PARAGRAPH
                                && next.text().to_string().trim_start().starts_with(":::"))
                                || (next.kind() == SyntaxKind::PLAIN
                                    && next.text().to_string().trim_start().starts_with(":::"))
                                || next.kind() == SyntaxKind::FENCED_DIV)
                            && !self.output.ends_with("\n\n")
                        {
                            self.output.push('\n');
                        }
                    } else {
                        self.format_node_sync(child, indent);
                    }
                }

                // Decrement depth after processing content
                self.fenced_div_depth -= 1;

                // Emit closing fence using the opener's colon count.
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
                self.output.push_str(&" ".repeat(indent));
                self.output.push_str(&":".repeat(opening_colons));
                self.output.push('\n');

                // Reset blank line tracking so outer blocks don't suppress separation.
                self.consecutive_blank_lines = 0;

                // Ensure blank line after if followed by block element
                if let Some(next) = node.next_sibling()
                    && is_block_element(next.kind())
                    && !self.output.ends_with("\n\n")
                {
                    // In list items, keep separator for continuation block content
                    // (e.g. list marker or paragraph) but avoid adding extra space
                    // before nested structural blocks that already manage spacing.
                    let needs_separator = if in_list_item {
                        matches!(
                            next.kind(),
                            SyntaxKind::PARAGRAPH | SyntaxKind::PLAIN | SyntaxKind::LIST
                        )
                    } else {
                        true
                    };
                    if needs_separator {
                        self.output.push('\n');
                        self.consecutive_blank_lines = 1;
                    }
                }
            }

            SyntaxKind::INLINE_MATH_MARKER => {
                // Output inline math as $...$ or $$...$$ (on the same line)
                self.output.push_str(node.text().to_string().trim());
            }

            SyntaxKind::DISPLAY_MATH => {
                // Display math ($$...$$) - format on separate lines
                // Even though it's parsed as inline, it should display as block-level

                let Some(display_math) = DisplayMath::cast(node.clone()) else {
                    self.output.push_str(&node.text().to_string());
                    return;
                };

                let math_content = Some(display_math.content());

                // Default to $$ if markers not found
                let opening_value = display_math
                    .opening_marker()
                    .unwrap_or_else(|| "$$".to_string());
                let closing_value = display_math
                    .closing_marker()
                    .unwrap_or_else(|| "$$".to_string());
                let opening = opening_value.as_str();
                let closing_from_tree = closing_value.as_str();
                let is_environment = display_math.is_environment_form();

                // Apply delimiter style preference
                use crate::config::MathDelimiterStyle;
                let (open, close) = if is_environment {
                    (opening, closing_from_tree)
                } else {
                    match self.config.math_delimiter_style {
                        MathDelimiterStyle::Preserve => (opening, closing_from_tree),
                        MathDelimiterStyle::Dollars => ("$$", "$$"),
                        MathDelimiterStyle::Backslash => (r"\[", r"\]"),
                    }
                };

                if is_environment {
                    self.output.push_str(open);
                    if let Some(content) = math_content {
                        self.output.push_str(&content);
                        if !content.ends_with('\n') {
                            self.output.push('\n');
                        }
                    }
                    self.output.push_str(close);
                    self.output.push('\n');
                    return;
                }

                // Opening fence
                self.output.push('\n');
                self.output.push_str(open);
                self.output.push('\n');

                // Math content
                if let Some(content) = math_content {
                    let math_indent = self.config.math_indent;
                    for line in content.trim().lines() {
                        self.output.push_str(&" ".repeat(math_indent));
                        self.output.push_str(line.trim_end());
                        self.output.push('\n');
                    }
                }

                // Closing fence
                self.output.push_str(close);
                self.output.push('\n');
            }

            SyntaxKind::CODE_BLOCK => {
                log::trace!("Formatting code block");

                // Add blank line before code block if it follows a paragraph
                // This matches Pandoc's formatting behavior
                if let Some(prev_sibling) = node.prev_sibling()
                    && prev_sibling.kind() == SyntaxKind::PARAGRAPH
                {
                    // Only add blank line if we don't already have one
                    if !self.output.ends_with("\n\n") && !self.output.ends_with("\n \n") {
                        self.output.push('\n');
                    }
                }

                // Normalize code blocks to use backticks
                self.format_code_block(node);
            }

            SyntaxKind::YAML_METADATA
            | SyntaxKind::PANDOC_TITLE_BLOCK
            | SyntaxKind::MMD_TITLE_BLOCK => {
                // Preserve these blocks as-is
                let text = node.text().to_string();
                self.output.push_str(&text);
                // Ensure these blocks end with appropriate spacing
                if !text.ends_with('\n') {
                    self.output.push('\n');
                }
                // Ensure blank line after if followed by block element.
                if let Some(next) = node.next_sibling()
                    && is_block_element(next.kind())
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                    self.consecutive_blank_lines = 1;
                }
            }

            SyntaxKind::BLANK_LINE => {
                // BlankLine nodes preserve exact whitespace in the CST for losslessness
                // But when formatting, we normalize to just newlines (no trailing spaces)
                // Drop blank lines at beginning of document output.
                if self.output.is_empty() {
                    return;
                }
                // Limit consecutive blank lines to 1
                if self.consecutive_blank_lines < 1 {
                    self.output.push('\n');
                    self.consecutive_blank_lines += 1;
                }
            }

            SyntaxKind::EMPHASIS => {
                // Normalize emphasis to always use single asterisks
                self.output.push('*');
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => self.format_node_sync(&n, indent),
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::EMPHASIS_MARKER {
                                self.output.push_str(t.text());
                            }
                        }
                    }
                }
                self.output.push('*');
            }

            SyntaxKind::STRONG => {
                // Normalize strong emphasis to always use double asterisks
                self.output.push_str("**");
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => self.format_node_sync(&n, indent),
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::STRONG_MARKER {
                                self.output.push_str(t.text());
                            }
                        }
                    }
                }
                self.output.push_str("**");
            }

            SyntaxKind::STRIKEOUT => {
                // Format strikeout with tildes
                self.output.push_str("~~");
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => self.format_node_sync(&n, indent),
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::STRIKEOUT_MARKER {
                                self.output.push_str(t.text());
                            }
                        }
                    }
                }
                self.output.push_str("~~");
            }

            SyntaxKind::SUPERSCRIPT => {
                // Format superscript with carets
                self.output.push('^');
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => self.format_node_sync(&n, indent),
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::SUPERSCRIPT_MARKER {
                                self.output.push_str(t.text());
                            }
                        }
                    }
                }
                self.output.push('^');
            }

            SyntaxKind::SUBSCRIPT => {
                // Format subscript with tildes
                self.output.push('~');
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => self.format_node_sync(&n, indent),
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::SUBSCRIPT_MARKER {
                                self.output.push_str(t.text());
                            }
                        }
                    }
                }
                self.output.push('~');
            }

            _ => {
                // Fallback: append node text (should be rare with children_with_tokens above)
                self.output.push_str(&node.text().to_string());
            }
        }
    }
}

fn normalize_attribute_text(attr_text: &str) -> String {
    let Some(inner) = attr_text
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
    else {
        return attr_text.to_string();
    };
    let Some(attrs) = parse_attribute_content(inner) else {
        return attr_text.to_string();
    };

    let mut out = String::from("{");
    if let Some(id) = attrs.identifier {
        out.push('#');
        out.push_str(&id);
    }
    for class in attrs.classes {
        if out.len() > 1 {
            out.push(' ');
        }
        if class.starts_with('=') {
            out.push_str(&class);
        } else {
            out.push('.');
            out.push_str(&class);
        }
    }
    for (key, value) in attrs.key_values {
        if out.len() > 1 {
            out.push(' ');
        }
        out.push_str(&key);
        out.push('=');
        out.push('"');
        out.push_str(&value.replace('"', "\\\""));
        out.push('"');
    }
    out.push('}');
    out
}
