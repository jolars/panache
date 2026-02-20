use crate::config::{Config, WrapMode};
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;
use std::collections::HashMap;

use super::code_blocks;
use super::headings;
use super::inline;
use super::paragraphs;
use super::tables;
use super::utils::{is_block_element, is_structural_block};
use super::wrapping;

pub struct Formatter {
    pub(super) output: String,
    pub(super) config: Config,
    pub(super) consecutive_blank_lines: usize,
    pub(super) fenced_div_depth: usize,
    pub(super) formatted_code: HashMap<String, String>,
    /// Stack of max marker widths for nested lists (for right-aligning markers)
    pub(super) max_marker_widths: Vec<usize>,
    /// Optional byte range to format (start, end). If None, format entire document.
    range: Option<(usize, usize)>,
}

impl Formatter {
    pub fn new(
        config: Config,
        formatted_code: HashMap<String, String>,
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
        wrapping::wrapped_lines_for_paragraph(&self.config, node, width, |n| {
            self.format_inline_node(n)
        })
    }

    // Delegate to headings module
    fn format_heading(&self, node: &SyntaxNode) -> String {
        headings::format_heading(node)
    }

    // Delegate to paragraphs module
    fn contains_inline_display_math(&self, node: &SyntaxNode) -> bool {
        paragraphs::contains_inline_display_math(node)
    }

    // Delegate to paragraphs module
    fn format_paragraph_with_display_math(
        &mut self,
        node: &SyntaxNode,
        _indent: usize,
        line_width: usize,
    ) {
        paragraphs::format_paragraph_with_display_math(
            node,
            line_width,
            &self.config,
            &mut self.output,
        );
    }

    // Delegate to code_blocks module
    fn format_code_block(&mut self, node: &SyntaxNode) {
        code_blocks::format_code_block(node, &self.config, &self.formatted_code, &mut self.output);
    }

    /// Format a code block that is a continuation of a definition or list item.
    /// Adds indentation prefix to each line of the fenced code block.
    pub(super) fn format_indented_code_block(&mut self, node: &SyntaxNode, indent: usize) {
        let indent_str = " ".repeat(indent);

        // Save current output and format code block to temp buffer
        let saved_output = self.output.clone();
        self.output.clear();

        // Use the standard code block formatter
        self.format_code_block(node);

        // Get the formatted output and restore original
        let code_output = self.output.clone();
        self.output = saved_output;

        // Add indentation to each line
        for line in code_output.lines() {
            self.output.push_str(&indent_str);
            self.output.push_str(line);
            self.output.push('\n');
        }

        // Ensure we end with exactly one newline
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
    }

    // The large format_node_sync method - keeping it here for now, can extract later
    #[allow(clippy::too_many_lines)]
    pub(super) fn format_node_sync(&mut self, node: &SyntaxNode, indent: usize) {
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
                                self.output.push('\n');
                            }
                            SyntaxKind::ESCAPED_CHAR => {
                                // Token already includes backslash (e.g., "\*")
                                self.output.push_str(t.text());
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
                            attributes = child.text().to_string();
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
                    && is_block_element(next.kind())
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }
            }

            SyntaxKind::HORIZONTAL_RULE => {
                // Output normalized horizontal rule (always use "---")
                self.output.push_str("---");
                self.output.push('\n');

                // Ensure blank line after if followed by block element
                if let Some(next) = node.next_sibling()
                    && is_block_element(next.kind())
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
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
                            if token.kind() == SyntaxKind::FOOTNOTE_REFERENCE =>
                        {
                            marker = token.text().to_string();
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
                let mut prev_was_code_or_list = false;

                for child in &child_blocks {
                    // Add blank line between blocks (except after BlankLine or at start)
                    if !first && prev_was_code_or_list && child.kind() != SyntaxKind::BLANK_LINE {
                        self.output.push('\n');
                    }

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

                            // Try wrapping the paragraph to see if it fits on one line
                            let lines = self.wrapped_lines_for_paragraph(child, first_line_space);

                            if lines.len() == 1 {
                                // Fits on one line - put on same line as marker
                                self.output.push(' ');
                                self.output.push_str(&lines[0]);
                                self.output.push('\n');
                                continue;
                            }
                        }
                        // Multi-line or non-paragraph first block - indent on next line
                        self.output.push('\n');
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
                                        self.output.push_str(line);
                                        self.output.push('\n');
                                    }
                                }
                                WrapMode::Reflow => {
                                    let lines =
                                        self.wrapped_lines_for_paragraph(child, available_width);
                                    for line in lines {
                                        self.output.push_str(&" ".repeat(child_indent));
                                        self.output.push_str(&line);
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
                            self.format_node_sync(child, child_indent);
                        }
                    }

                    // Track if this was a code block or list for spacing
                    prev_was_code_or_list =
                        matches!(child.kind(), SyntaxKind::CODE_BLOCK | SyntaxKind::LIST);
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

            SyntaxKind::LATEX_ENVIRONMENT => {
                // Output the environment exactly as written
                let text = node.text().to_string();
                self.output.push_str(&text);
                if !text.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::HTML_BLOCK => {
                // Output HTML block exactly as written
                let text = node.text().to_string();
                self.output.push_str(&text);
                if !text.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::COMMENT => {
                let text = node.text().to_string();
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

            SyntaxKind::BLOCKQUOTE => {
                log::trace!("Formatting blockquote");
                // Determine nesting depth by counting ancestor BlockQuote nodes (including self)
                let mut depth = 0usize;
                let mut cur = Some(node.clone());
                while let Some(n) = cur {
                    if n.kind() == SyntaxKind::BLOCKQUOTE {
                        depth += 1;
                    }
                    cur = n.parent();
                }

                // Prefixes for quoted content and blank quoted lines
                let content_prefix = "> ".repeat(depth); // includes trailing space
                let blank_prefix = content_prefix.trim_end(); // no trailing space

                // Format children (paragraphs, blank lines) with proper > prefix per depth
                // NOTE: BlockQuoteMarker tokens are in the tree for losslessness, but we ignore
                // them during formatting and add prefixes dynamically instead.
                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);

                for child in node.children() {
                    match child.kind() {
                        // Skip BlockQuoteMarker tokens - we add prefixes dynamically
                        SyntaxKind::BLOCKQUOTE_MARKER => continue,

                        SyntaxKind::PARAGRAPH => match wrap_mode {
                            WrapMode::Preserve => {
                                // Build paragraph text while skipping BlockQuoteMarker tokens
                                // (they're in the tree for losslessness but we add prefixes dynamically)
                                let mut lines_text = String::new();
                                let mut skip_next_whitespace = false;
                                for item in child.children_with_tokens() {
                                    match item {
                                        NodeOrToken::Token(t)
                                            if t.kind() == SyntaxKind::BLOCKQUOTE_MARKER =>
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
                                let lines = self.wrapped_lines_for_paragraph(&child, width);
                                for line in lines {
                                    self.output.push_str(&content_prefix);
                                    self.output.push_str(&line);
                                    self.output.push('\n');
                                }
                            }
                        },
                        SyntaxKind::BLANK_LINE => {
                            self.output.push_str(blank_prefix);
                            self.output.push('\n');
                        }
                        SyntaxKind::HORIZONTAL_RULE => {
                            self.output.push_str(&content_prefix);
                            self.output.push_str("---");
                            self.output.push('\n');
                        }
                        SyntaxKind::HEADING => {
                            // Format heading with blockquote prefix
                            let heading_text = self.format_heading(&child);
                            for line in heading_text.lines() {
                                self.output.push_str(&content_prefix);
                                self.output.push_str(line);
                                self.output.push('\n');
                            }
                        }
                        SyntaxKind::LIST => {
                            // Format list with blockquote prefix
                            // Save current output, format list to temp, then prefix each line
                            let saved_output = self.output.clone();
                            self.output.clear();
                            self.format_node_sync(&child, indent);
                            let list_output = self.output.clone();
                            self.output = saved_output;

                            for line in list_output.lines() {
                                if line.is_empty() {
                                    self.output.push_str(blank_prefix);
                                } else {
                                    self.output.push_str(&content_prefix);
                                    self.output.push_str(line);
                                }
                                self.output.push('\n');
                            }
                        }
                        SyntaxKind::CODE_BLOCK => {
                            // Format code block with blockquote prefix
                            // Save current output, format code block to temp, then prefix each line
                            let saved_output = self.output.clone();
                            self.output.clear();
                            self.format_node_sync(&child, indent);
                            let code_output = self.output.clone();
                            self.output = saved_output;

                            for line in code_output.lines() {
                                if line.is_empty() {
                                    self.output.push_str(blank_prefix);
                                } else {
                                    self.output.push_str(&content_prefix);
                                    self.output.push_str(line);
                                }
                                self.output.push('\n');
                            }
                        }
                        _ => {
                            // Handle other content within block quotes
                            self.format_node_sync(&child, indent);
                        }
                    }
                }
            }

            SyntaxKind::PARAGRAPH => {
                let text = node.text().to_string();
                log::debug!("Formatting paragraph, text length: {}", text.len());

                // Check if paragraph contains inline display math ($$...$$)
                if self.contains_inline_display_math(node) {
                    log::debug!("Paragraph has display math");
                    self.format_paragraph_with_display_math(node, indent, line_width);
                    return;
                }

                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
                log::debug!(
                    "Paragraph wrap mode: {:?}, line_width: {}",
                    wrap_mode,
                    line_width
                );
                match wrap_mode {
                    WrapMode::Preserve => {
                        log::trace!("Preserving paragraph line breaks");
                        self.output.push_str(&text);
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
                self.output.push_str(text.trim());
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::PLAIN => {
                // Plain is like PARAGRAPH but for tight contexts (definition lists, table cells)
                // Apply wrapping with continuation indentation
                let text = node.text().to_string();
                log::debug!("Formatting Plain block, text length: {}", text.len());

                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
                match wrap_mode {
                    WrapMode::Preserve => {
                        self.output.push_str(&text);
                        if !self.output.ends_with('\n') {
                            self.output.push('\n');
                        }
                    }
                    WrapMode::Reflow => {
                        log::trace!("Reflowing Plain block to {} width", line_width);
                        let lines = self.wrapped_lines_for_paragraph(node, line_width);

                        for (i, line) in lines.iter().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                                // Add continuation indent for wrapped lines
                                self.output.push_str(&" ".repeat(indent));
                            }
                            self.output.push_str(line);
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
                // Add blank line before top-level definition lists
                if indent == 0 && !self.output.is_empty() && !self.output.ends_with("\n\n") {
                    self.output.push('\n');
                }
                for child in node.children() {
                    if child.kind() == SyntaxKind::BLANK_LINE {
                        continue;
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
                // Format term and definitions in compact format (no blank lines)
                for child in node.children() {
                    if child.kind() == SyntaxKind::BLANK_LINE {
                        continue; // Skip blank lines for compact format
                    }
                    self.format_node_sync(&child, indent);
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
                                    // Add blank line before code block if needed
                                    if !self.output.ends_with("\n\n") {
                                        self.output.push('\n');
                                    }
                                    self.format_indented_code_block(n, def_indent);
                                }
                                SyntaxKind::PLAIN => {
                                    // Plain block in definition - format inline with potential wrapping
                                    // Already handled by Plain formatter above
                                    self.format_node_sync(n, def_indent);
                                }
                                SyntaxKind::PARAGRAPH => {
                                    if first_para_idx == Some(i) {
                                        // First paragraph - lazy continuation (inline)
                                        let text = n.text().to_string();
                                        self.output.push_str(text.trim());
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
                // Use more colons for nested divs: 3 base + 2 per depth level
                let colon_count = 3 + (self.fenced_div_depth * 2);
                let colons = ":".repeat(colon_count);

                let mut attributes = None;

                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::DIV_FENCE_OPEN => {
                            // Extract attributes from DivInfo child node
                            for fence_child in child.children() {
                                if fence_child.kind() == SyntaxKind::DIV_INFO {
                                    attributes = Some(fence_child.text().to_string());
                                    break;
                                }
                            }
                        }

                        SyntaxKind::DIV_FENCE_CLOSE => {
                            // Will be handled after content
                        }

                        // Process any other child nodes (paragraphs, nested divs, etc.)
                        _ => {}
                    }
                }

                // Emit normalized opening fence
                if let Some(attrs) = &attributes {
                    self.output.push_str(&colons);
                    self.output.push(' ');
                    self.output.push_str(attrs);
                    self.output.push('\n');
                }

                // Increment depth for nested content
                self.fenced_div_depth += 1;

                // Process content
                for child in node.children() {
                    if !matches!(
                        child.kind(),
                        SyntaxKind::DIV_FENCE_OPEN
                            | SyntaxKind::DIV_INFO
                            | SyntaxKind::DIV_FENCE_CLOSE
                    ) {
                        self.format_node_sync(&child, indent);
                    }
                }

                // Decrement depth after processing content
                self.fenced_div_depth -= 1;

                // Emit normalized closing fence (ensure it's on its own line)
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
                self.output.push_str(&colons);
                self.output.push('\n');
            }

            SyntaxKind::INLINE_MATH_MARKER => {
                // Output inline math as $...$ or $$...$$ (on the same line)
                self.output.push_str(node.text().to_string().trim());
            }

            SyntaxKind::DISPLAY_MATH => {
                // Display math ($$...$$) - format on separate lines
                // Even though it's parsed as inline, it should display as block-level

                let mut math_content = None;
                let mut opening_marker: Option<String> = None;
                let mut closing_marker: Option<String> = None;

                for child in node.children_with_tokens() {
                    if let rowan::NodeOrToken::Token(t) = child {
                        if t.kind() == SyntaxKind::DISPLAY_MATH_MARKER {
                            let marker_text = t.text().to_string();
                            if opening_marker.is_none() {
                                opening_marker = Some(marker_text);
                            } else if closing_marker.is_none() {
                                closing_marker = Some(marker_text);
                            }
                        } else if t.kind() == SyntaxKind::TEXT {
                            math_content = Some(t.text().to_string());
                        }
                    }
                }

                // Default to $$ if markers not found
                let opening = opening_marker.as_deref().unwrap_or("$$");
                let closing_from_tree = closing_marker.as_deref().unwrap_or("$$");

                // Apply delimiter style preference
                use crate::config::MathDelimiterStyle;
                let (open, close) = match self.config.math_delimiter_style {
                    MathDelimiterStyle::Preserve => (opening, closing_from_tree),
                    MathDelimiterStyle::Dollars => ("$$", "$$"),
                    MathDelimiterStyle::Backslash => (r"\[", r"\]"),
                };

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
                // Normalize code blocks to use backticks
                self.format_code_block(node);
            }

            SyntaxKind::YAML_METADATA | SyntaxKind::PANDOC_TITLE_BLOCK => {
                // Preserve these blocks as-is
                let text = node.text().to_string();
                self.output.push_str(&text);
                // Ensure these blocks end with appropriate spacing
                if !text.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::BLANK_LINE => {
                // BlankLine nodes preserve exact whitespace in the CST for losslessness
                // But when formatting, we normalize to just newlines (no trailing spaces)
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
