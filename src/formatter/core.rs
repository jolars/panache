use crate::config::{BlankLines, Config, WrapMode};
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

    /// Check if we should process a direct child of ROOT/DOCUMENT
    /// When range filtering is active, only process nodes that overlap with the range
    fn should_process_top_level_node(&self, node: &SyntaxNode) -> bool {
        // If no range specified, process everything
        if self.range.is_none() {
            return true;
        }

        // Always process ROOT and DOCUMENT nodes (containers)
        if node.kind() == SyntaxKind::ROOT || node.kind() == SyntaxKind::DOCUMENT {
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
        if node.kind() != SyntaxKind::BlankLine {
            self.consecutive_blank_lines = 0;
        }

        let line_width = self.config.line_width;

        match node.kind() {
            SyntaxKind::ROOT | SyntaxKind::DOCUMENT => {
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
                            SyntaxKind::BlankLine => {
                                self.output.push('\n');
                            }
                            SyntaxKind::EscapedChar => {
                                // Token already includes backslash (e.g., "\*")
                                self.output.push_str(t.text());
                            }
                            SyntaxKind::ImageLinkStart
                            | SyntaxKind::LinkStart
                            | SyntaxKind::LatexCommand => {
                                self.output.push_str(t.text());
                            }
                            _ => self.output.push_str(t.text()),
                        },
                    }
                }
            }

            SyntaxKind::Heading => {
                log::trace!("Formatting heading");
                // Determine level
                let mut level = 1;
                let mut attributes = String::new();

                // First pass: get level and attributes
                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::AtxHeadingMarker => {
                            let t = child.text().to_string();
                            level = t.chars().take_while(|&c| c == '#').count().clamp(1, 6);
                        }
                        SyntaxKind::SetextHeadingUnderline => {
                            let t = child.text().to_string();
                            if t.chars().all(|c| c == '=') {
                                level = 1;
                            } else {
                                level = 2;
                            }
                        }
                        SyntaxKind::Attribute => {
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
                    if child.kind() == SyntaxKind::HeadingContent {
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

            SyntaxKind::HorizontalRule => {
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

            SyntaxKind::ReferenceDefinition => {
                // Output reference definition as-is: [label]: url "title"
                let text = node.text().to_string();
                self.output.push_str(text.trim_end());
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }

                // Ensure blank line after if followed by non-reference block element
                if let Some(next) = node.next_sibling()
                    && is_block_element(next.kind())
                    && next.kind() != SyntaxKind::ReferenceDefinition
                    && next.kind() != SyntaxKind::FootnoteDefinition
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }
            }

            SyntaxKind::FootnoteDefinition => {
                // Format footnote definition with proper indentation
                // Extract marker and children first
                let mut marker = String::new();
                let mut child_blocks = Vec::new();

                for element in node.children_with_tokens() {
                    match element {
                        NodeOrToken::Token(token)
                            if token.kind() == SyntaxKind::FootnoteReference =>
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
                    if !first && prev_was_code_or_list && child.kind() != SyntaxKind::BlankLine {
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
                        SyntaxKind::BlankLine => {
                            self.output.push('\n');
                        }
                        SyntaxKind::CodeBlock => {
                            // Format code blocks as fenced blocks with indentation
                            // Extract code content, stripping WHITESPACE tokens (indentation)
                            let mut code_lines = Vec::new();
                            for code_child in child.children() {
                                if code_child.kind() == SyntaxKind::CodeContent {
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
                        matches!(child.kind(), SyntaxKind::CodeBlock | SyntaxKind::List);
                }

                // If no child blocks, just end with newline
                if child_blocks.is_empty() {
                    self.output.push('\n');
                }

                // Add blank line after footnote definition (matching Pandoc's behavior)
                if let Some(next) = node.next_sibling() {
                    let next_kind = next.kind();
                    if next_kind == SyntaxKind::FootnoteDefinition && !self.output.ends_with("\n\n")
                    {
                        self.output.push('\n');
                    }
                }
            }

            SyntaxKind::LatexEnvironment => {
                // Output the environment exactly as written
                let text = node.text().to_string();
                self.output.push_str(&text);
                if !text.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::HtmlBlock => {
                // Output HTML block exactly as written
                let text = node.text().to_string();
                self.output.push_str(&text);
                if !text.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::Comment => {
                let text = node.text().to_string();
                self.output.push_str(&text);
                if !text.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::LatexCommand => {
                // Standalone LaTeX commands - preserve exactly as written
                let text = node.text().to_string();
                self.output.push_str(&text);
                // Don't add extra newlines for standalone LaTeX commands
            }

            SyntaxKind::BlockQuote => {
                log::trace!("Formatting blockquote");
                // Determine nesting depth by counting ancestor BlockQuote nodes (including self)
                let mut depth = 0usize;
                let mut cur = Some(node.clone());
                while let Some(n) = cur {
                    if n.kind() == SyntaxKind::BlockQuote {
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
                        SyntaxKind::BlockQuoteMarker => continue,

                        SyntaxKind::PARAGRAPH => match wrap_mode {
                            WrapMode::Preserve => {
                                // Build paragraph text while skipping BlockQuoteMarker tokens
                                // (they're in the tree for losslessness but we add prefixes dynamically)
                                let mut lines_text = String::new();
                                let mut skip_next_whitespace = false;
                                for item in child.children_with_tokens() {
                                    match item {
                                        NodeOrToken::Token(t)
                                            if t.kind() == SyntaxKind::BlockQuoteMarker =>
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
                        SyntaxKind::BlankLine => {
                            self.output.push_str(blank_prefix);
                            self.output.push('\n');
                        }
                        SyntaxKind::HorizontalRule => {
                            self.output.push_str(&content_prefix);
                            self.output.push_str("---");
                            self.output.push('\n');
                        }
                        SyntaxKind::Heading => {
                            // Format heading with blockquote prefix
                            let heading_text = self.format_heading(&child);
                            for line in heading_text.lines() {
                                self.output.push_str(&content_prefix);
                                self.output.push_str(line);
                                self.output.push('\n');
                            }
                        }
                        SyntaxKind::List => {
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
                        SyntaxKind::CodeBlock => {
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

                // If paragraph contains display math across lines ($$\n...\n$$), preserve as-is
                // Check that it's actually dollar signs, not just any characters
                let has_multiline_display_math = text.contains("$$\n") || text.contains("\n$$");
                if has_multiline_display_math {
                    log::debug!("Paragraph has multiline display math, preserving");
                    self.output.push_str(&text);
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    return;
                }

                // Check if paragraph contains inline display math ($$...$$)
                // Only reformat if it's on a single line
                if self.contains_inline_display_math(node) {
                    log::debug!("Paragraph has inline display math");
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

                        if !self.output.ends_with('\n') {
                            self.output.push('\n');
                        }
                    }
                }
            }

            SyntaxKind::List => {
                self.format_list(node, indent);
            }

            SyntaxKind::DefinitionList => {
                // Add blank line before top-level definition lists
                if indent == 0 && !self.output.is_empty() && !self.output.ends_with("\n\n") {
                    self.output.push('\n');
                }
                for child in node.children() {
                    if child.kind() == SyntaxKind::BlankLine {
                        continue;
                    }
                    self.format_node_sync(&child, indent);
                }
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::LineBlock => {
                log::debug!("Formatting line block");
                // Add blank line before line blocks if not at start
                if !self.output.is_empty() && !self.output.ends_with("\n\n") {
                    self.output.push('\n');
                }

                // Format each line preserving line breaks and leading spaces
                for child in node.children() {
                    if child.kind() == SyntaxKind::LineBlockLine {
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

            SyntaxKind::DefinitionItem => {
                // Format term and definitions in compact format (no blank lines)
                for child in node.children() {
                    if child.kind() == SyntaxKind::BlankLine {
                        continue; // Skip blank lines for compact format
                    }
                    self.format_node_sync(&child, indent);
                }
            }

            SyntaxKind::Term => {
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

            SyntaxKind::Definition => {
                // Format definition with marker and content
                // Definition content is indented 4 spaces from the margin
                let def_indent = indent + 4;
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
                                SyntaxKind::BlankLine => {
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
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::DefinitionMarker => {
                            // Skip - we already added `:   `
                        }
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::WHITESPACE => {
                            // Skip - we normalize spacing
                        }
                        NodeOrToken::Node(n) => {
                            // Handle continuation content with proper indentation
                            match n.kind() {
                                SyntaxKind::CodeBlock => {
                                    // Add blank line before code block if needed
                                    if !self.output.ends_with("\n\n") {
                                        self.output.push('\n');
                                    }
                                    self.format_indented_code_block(n, def_indent);
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
                                SyntaxKind::BlankLine => {
                                    // Preserve blank lines in definition content
                                    // Only skip blank lines before the first paragraph if it's lazy
                                    let is_before_first_para =
                                        first_para_idx.is_some_and(|idx| i < idx);

                                    if !is_before_first_para && !self.output.ends_with("\n\n") {
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

            SyntaxKind::SimpleTable | SyntaxKind::MultilineTable => {
                // Handle table with proper caption formatting
                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::TableCaption => {
                            // Normalize caption prefix to "Table: "
                            for caption_child in child.children_with_tokens() {
                                match caption_child {
                                    rowan::NodeOrToken::Token(token)
                                        if token.kind() == SyntaxKind::TableCaptionPrefix =>
                                    {
                                        // Always emit normalized "Table: " prefix
                                        self.output.push_str("Table: ");
                                    }
                                    rowan::NodeOrToken::Token(token) => {
                                        self.output.push_str(token.text());
                                    }
                                    rowan::NodeOrToken::Node(node) => {
                                        self.output.push_str(&node.text().to_string());
                                    }
                                }
                            }
                        }
                        _ => {
                            // For other table parts, preserve as-is
                            self.output.push_str(&child.text().to_string());
                        }
                    }
                }
            }

            SyntaxKind::PipeTable => {
                // Format pipe table with proper alignment
                let formatted = tables::format_pipe_table(node, &self.config);
                self.output.push_str(&formatted);
            }

            SyntaxKind::InlineMath => {
                // Check if this is display math (has BlockMathMarker)
                let is_display_math = node.children_with_tokens().any(|t| {
                    matches!(t, NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::BlockMathMarker)
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
                            if tok.kind() == SyntaxKind::InlineMathMarker
                                || tok.kind() == SyntaxKind::BlockMathMarker =>
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

            SyntaxKind::ListItem => {
                self.format_list_item(node, indent);
            }

            SyntaxKind::FencedDiv => {
                // Use more colons for nested divs: 3 base + 2 per depth level
                let colon_count = 3 + (self.fenced_div_depth * 2);
                let colons = ":".repeat(colon_count);

                let mut attributes = None;

                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::DivFenceOpen => {
                            // Extract attributes from DivInfo child node
                            for fence_child in child.children() {
                                if fence_child.kind() == SyntaxKind::DivInfo {
                                    attributes = Some(fence_child.text().to_string());
                                    break;
                                }
                            }
                        }

                        SyntaxKind::DivFenceClose => {
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
                        SyntaxKind::DivFenceOpen | SyntaxKind::DivInfo | SyntaxKind::DivFenceClose
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

            SyntaxKind::InlineMathMarker => {
                // Output inline math as $...$ or $$...$$ (on the same line)
                self.output.push_str(node.text().to_string().trim());
            }

            SyntaxKind::MathBlock => {
                let mut label = None;
                let mut math_content = None;
                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::MathContent => {
                            math_content = Some(child.text().to_string());
                        }
                        SyntaxKind::Attribute => {
                            label = Some(child.text().to_string().trim().to_string());
                        }
                        _ => {}
                    }
                }
                // Opening fence
                self.output.push_str("$$\n");
                // Math content
                if let Some(content) = math_content {
                    let math_indent = self.config.math_indent;
                    for line in content.trim().lines() {
                        self.output.push_str(&" ".repeat(math_indent));
                        self.output.push_str(line.trim_end());
                        self.output.push('\n');
                    }
                }
                // Closing fence (with label if present)
                self.output.push_str("$$");
                if let Some(lbl) = label {
                    self.output.push(' ');
                    self.output.push_str(&lbl);
                }
                self.output.push('\n');
            }

            SyntaxKind::CodeBlock => {
                log::trace!("Formatting code block");
                // Normalize code blocks to use backticks
                self.format_code_block(node);
            }

            SyntaxKind::YamlMetadata | SyntaxKind::PandocTitleBlock => {
                // Preserve these blocks as-is
                let text = node.text().to_string();
                self.output.push_str(&text);
                // Ensure these blocks end with appropriate spacing
                if !text.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::BlankLine => {
                // Apply blank_lines config to collapse consecutive blank lines
                match self.config.blank_lines {
                    BlankLines::Preserve => {
                        // Always output blank line
                        self.output.push('\n');
                        self.consecutive_blank_lines += 1;
                    }
                    BlankLines::Collapse => {
                        // Only output if we haven't already output one blank line
                        if self.consecutive_blank_lines == 0 {
                            self.output.push('\n');
                            self.consecutive_blank_lines = 1;
                        }
                        // Otherwise skip this blank line (collapsing to one)
                    }
                }
            }

            SyntaxKind::Emphasis => {
                // Normalize emphasis to always use single asterisks
                self.output.push('*');
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => self.format_node_sync(&n, indent),
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::EmphasisMarker {
                                self.output.push_str(t.text());
                            }
                        }
                    }
                }
                self.output.push('*');
            }

            SyntaxKind::Strong => {
                // Normalize strong emphasis to always use double asterisks
                self.output.push_str("**");
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => self.format_node_sync(&n, indent),
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::StrongMarker {
                                self.output.push_str(t.text());
                            }
                        }
                    }
                }
                self.output.push_str("**");
            }

            SyntaxKind::Strikeout => {
                // Format strikeout with tildes
                self.output.push_str("~~");
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => self.format_node_sync(&n, indent),
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::StrikeoutMarker {
                                self.output.push_str(t.text());
                            }
                        }
                    }
                }
                self.output.push_str("~~");
            }

            SyntaxKind::Superscript => {
                // Format superscript with carets
                self.output.push('^');
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => self.format_node_sync(&n, indent),
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::SuperscriptMarker {
                                self.output.push_str(t.text());
                            }
                        }
                    }
                }
                self.output.push('^');
            }

            SyntaxKind::Subscript => {
                // Format subscript with tildes
                self.output.push('~');
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => self.format_node_sync(&n, indent),
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::SubscriptMarker {
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
