use crate::config::WrapMode;
use crate::formatter::indent_utils::{calculate_list_item_indent, is_alignable_marker};
use crate::formatter::wrapping;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;
use textwrap::wrap_algorithms::WrapAlgorithm;

use super::Formatter;

impl Formatter {
    /// Extract the marker text from a ListItem node
    /// Standardizes bullet list markers to "-" for consistency
    pub(super) fn extract_list_marker(node: &SyntaxNode) -> Option<String> {
        for el in node.children_with_tokens() {
            if let NodeOrToken::Token(t) = el
                && t.kind() == SyntaxKind::LIST_MARKER
            {
                let marker = t.text().to_string();
                // Standardize bullet list markers: convert *, +, - to "-"
                if marker.len() == 1 && matches!(marker.as_str(), "-" | "*" | "+") {
                    return Some("-".to_string());
                }
                return Some(marker);
            }
        }
        None
    }

    /// Calculate the maximum marker width for all direct ListItem children of a List
    /// Returns 0 if markers shouldn't be aligned
    pub(super) fn calculate_max_marker_width(list_node: &SyntaxNode) -> usize {
        let markers: Vec<String> = list_node
            .children()
            .filter(|child| child.kind() == SyntaxKind::LIST_ITEM)
            .filter_map(|item| Self::extract_list_marker(&item))
            .collect();

        // Check if any marker is alignable
        if !markers.iter().any(|m| is_alignable_marker(m)) {
            return 0;
        }

        // Return max width of alignable markers
        markers
            .iter()
            .filter(|m| is_alignable_marker(m))
            .map(|m| m.len())
            .max()
            .unwrap_or(0)
    }

    /// Calculate the content indentation offset for a list item (marker + padding + space)
    /// This is the column where the list item's content starts relative to the list's base indent
    pub(super) fn calculate_list_item_content_indent(
        item_node: &SyntaxNode,
        max_marker_width: usize,
    ) -> usize {
        let marker = Self::extract_list_marker(item_node).unwrap_or_default();

        // Check for task checkbox (adds 4 more characters: "[x] ")
        let has_checkbox = item_node.children_with_tokens().any(|el| {
            if let NodeOrToken::Token(t) = el {
                t.kind() == SyntaxKind::TASK_CHECKBOX
            } else {
                false
            }
        });

        let indent = calculate_list_item_indent(&marker, max_marker_width, has_checkbox);
        indent.content_offset()
    }

    /// Format a paragraph that is a continuation of a list item.
    /// Strips existing indentation from the text and applies the correct list item indentation.
    pub(super) fn format_list_continuation_paragraph(&mut self, node: &SyntaxNode, indent: usize) {
        let text = node.text().to_string();
        let line_width = self.config.line_width.saturating_sub(indent);
        let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);

        match wrap_mode {
            WrapMode::Preserve => {
                // Strip existing indentation and apply list item indentation
                for line in text.lines() {
                    self.output.push_str(&" ".repeat(indent));
                    self.output.push_str(line.trim_start());
                    self.output.push('\n');
                }
            }
            WrapMode::Reflow => {
                // Wrap with list item indentation
                let lines = self.wrapped_lines_for_paragraph(node, line_width);
                for line in lines {
                    self.output.push_str(&" ".repeat(indent));
                    self.output.push_str(&line);
                    self.output.push('\n');
                }
            }
        }
    }

    /// Format a List node
    pub(super) fn format_list(&mut self, node: &SyntaxNode, indent: usize) {
        // Add blank line before top-level lists (indent == 0) that follow content,
        // UNLESS the previous sibling was also a List (they're separate lists in the source).
        // If previous was a List, it already has spacing and we normalize markers instead.
        let prev_is_list = node
            .prev_sibling()
            .map(|prev| matches!(prev.kind(), SyntaxKind::LIST | SyntaxKind::BLANK_LINE))
            .unwrap_or(false);

        if indent == 0 && !self.output.is_empty() && !self.output.ends_with("\n\n") && !prev_is_list
        {
            self.output.push('\n');
        }

        // Calculate max marker width for right-alignment
        let max_marker_width = Self::calculate_max_marker_width(node);
        self.max_marker_widths.push(max_marker_width);

        // Detect if this is a loose list by checking for PARAGRAPH wrapper nodes
        // (Parser marks loose lists with PARAGRAPH, tight lists with Plain)
        let is_loose = node
            .children()
            .find(|child| child.kind() == SyntaxKind::LIST_ITEM)
            .and_then(|item| {
                item.children()
                    .find(|c| matches!(c.kind(), SyntaxKind::PARAGRAPH | SyntaxKind::PLAIN))
            })
            .map(|wrapper| wrapper.kind() == SyntaxKind::PARAGRAPH)
            .unwrap_or(false);

        log::debug!("Formatting list: is_loose={}", is_loose);

        let mut item_count = 0;
        let total_items = node
            .children()
            .filter(|c| c.kind() == SyntaxKind::LIST_ITEM)
            .count();

        let mut last_item_content_indent = 0;

        for child in node.children() {
            if child.kind() == SyntaxKind::LIST_ITEM {
                item_count += 1;

                // Strip double newlines ONLY between items within a tight list
                // Don't strip if we're at the first item (preserves spacing before the list)
                if !is_loose && item_count > 1 {
                    while self.output.ends_with("\n\n") {
                        self.output.pop();
                    }
                }

                // Calculate content indent for this list item (marker + space)
                last_item_content_indent =
                    indent + Self::calculate_list_item_content_indent(&child, max_marker_width);

                self.format_node_sync(&child, indent);

                // Add blank line after each item for loose lists (except last)
                if is_loose && item_count < total_items && !self.output.ends_with("\n\n") {
                    self.output.push('\n');
                }
            } else if child.kind() == SyntaxKind::BLANK_LINE {
                // Skip BlankLine nodes - we're normalizing spacing based on loose/tight
                continue;
            } else if child.kind() == SyntaxKind::PARAGRAPH {
                // Paragraphs that are siblings of ListItems are continuation content
                self.format_list_continuation_paragraph(&child, last_item_content_indent);
            } else if child.kind() == SyntaxKind::CODE_BLOCK {
                // Code blocks that are siblings of ListItems are also continuation content
                self.format_indented_code_block(&child, last_item_content_indent);
            } else {
                self.format_node_sync(&child, indent);
            }
        }

        // Pop the max marker width off the stack
        self.max_marker_widths.pop();

        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
    }

    /// Find Plain or PARAGRAPH child in a ListItem node.
    /// These nodes wrap the text content in Pandoc-style AST.
    /// For nested lists, skip Plain nodes that appear before the ListMarker
    /// (these contain only indentation whitespace).
    fn find_content_node(node: &SyntaxNode) -> Option<SyntaxNode> {
        let mut seen_marker = false;
        for el in node.children_with_tokens() {
            match el {
                rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::LIST_MARKER => {
                    seen_marker = true;
                }
                rowan::NodeOrToken::Node(n)
                    if matches!(n.kind(), SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH) =>
                {
                    // Only return Plain/PARAGRAPH nodes that come after the marker
                    if seen_marker {
                        return Some(n);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Format a ListItem node
    pub(super) fn format_list_item(&mut self, node: &SyntaxNode, indent: usize) {
        // Compute indent, marker, and checkbox from leading tokens
        let mut marker = String::new();
        let mut original_marker = String::new(); // Track original for word removal
        let mut checkbox = None;
        // NOTE: We ignore WHITESPACE tokens for list indentation calculation.
        // The WHITESPACE tokens are emitted by the parser for losslessness, but the
        // formatter should use the `indent` parameter (which represents nesting level)
        // to determine output indentation, not the source indentation from WHITESPACE tokens.

        for el in node.children_with_tokens() {
            if let NodeOrToken::Token(t) = el {
                match t.kind() {
                    SyntaxKind::WHITESPACE => {
                        // Skip - we don't accumulate source indentation
                        // The `indent` parameter determines output indentation
                    }
                    SyntaxKind::LIST_MARKER => {
                        let raw_marker = t.text().to_string();
                        original_marker = raw_marker.clone();
                        // Standardize bullet list markers to "-"
                        marker = if raw_marker.len() == 1
                            && matches!(raw_marker.as_str(), "-" | "*" | "+")
                        {
                            "-".to_string()
                        } else {
                            raw_marker
                        };
                    }
                    SyntaxKind::TASK_CHECKBOX => {
                        checkbox = Some(t.text().to_string());
                    }
                    _ => {}
                }
            }
        }

        // Get max marker width for this list level
        let max_marker_width = self.max_marker_widths.last().copied().unwrap_or(0);

        // Calculate indentation using the utility
        let list_indent = calculate_list_item_indent(&marker, max_marker_width, checkbox.is_some());

        let total_indent = indent;
        let hanging = list_indent.hanging_indent(total_indent);
        let available_width = self.config.line_width.saturating_sub(hanging);

        // Build words from Plain/PARAGRAPH content node if present, otherwise from entire ListItem
        let mut arena: Vec<Box<str>> = Vec::new();
        let content_node = Self::find_content_node(node);

        let words = if let Some(content) = content_node {
            // Extract words from Plain/PARAGRAPH child (postprocessor wraps all content in one node)
            wrapping::build_words(&self.config, &content, &mut arena, |n| {
                self.format_inline_node(n)
            })
        } else {
            // Backwards compatibility: scan entire ListItem and remove marker/checkbox
            let mut node_words = wrapping::build_words(&self.config, node, &mut arena, |n| {
                self.format_inline_node(n)
            });

            // Remove the original marker from words (not the standardized one)
            if let Some(first) = node_words.first()
                && first.word == original_marker
            {
                // Remove the marker; we will print it ourselves with a following space
                node_words.remove(0);
            }

            // Remove checkbox from words if present
            if checkbox.is_some()
                && let Some(first) = node_words.first()
            {
                let trimmed = first.word.trim_start();
                if trimmed.starts_with('[') && trimmed.len() >= 3 {
                    node_words.remove(0);
                }
            }

            node_words
        };

        let algo = WrapAlgorithm::new();
        let line_widths = [available_width];
        let lines = algo.wrap(&words, &line_widths);

        log::trace!(
            "ListItem wrapping: {} lines, hanging indent={}",
            lines.len(),
            hanging
        );

        for (i, line) in lines.iter().enumerate() {
            log::trace!("  Line {}: {} words", i, line.len());
            if i == 0 {
                // First line: output indent + marker padding + marker + spaces + checkbox
                self.output.push_str(&" ".repeat(total_indent));
                self.output
                    .push_str(&" ".repeat(list_indent.marker_padding));
                self.output.push_str(&marker);
                self.output.push_str(&" ".repeat(list_indent.spaces_after));

                // Output checkbox if present
                if let Some(ref cb) = checkbox {
                    self.output.push_str(cb);
                    self.output.push(' ');
                }
            } else {
                // Hanging indent includes all leading whitespace
                self.output.push_str(&" ".repeat(hanging));
            }
            for (j, w) in line.iter().enumerate() {
                self.output.push_str(w.word);
                if j + 1 < line.len() {
                    self.output.push_str(w.whitespace);
                } else {
                    self.output.push_str(w.penalty);
                }
            }
            self.output.push('\n');
        }

        // Format nested blocks inside this list item aligned to the content column.
        // Skip Plain/PARAGRAPH nodes that were already processed for word wrapping.
        for child in node.children() {
            match child.kind() {
                SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH => {
                    // These blocks are already handled by word wrapping above if they're
                    // direct children. Only process Plain/PARAGRAPH if it comes after a BlankLine
                    // (indicating it's a true continuation paragraph, not the first content).
                    let has_blank_before = child
                        .prev_sibling()
                        .map(|prev| prev.kind() == SyntaxKind::BLANK_LINE)
                        .unwrap_or(false);

                    if has_blank_before {
                        let content_indent = list_indent.hanging_indent(total_indent);
                        self.format_list_continuation_paragraph(&child, content_indent);
                    }
                    // Otherwise skip - already handled
                }
                SyntaxKind::LIST => {
                    // Nested list indent: base + marker + 1 space + checkbox
                    self.format_node_sync(
                        &child,
                        total_indent + list_indent.marker_width + 1 + list_indent.checkbox_width,
                    );
                }
                SyntaxKind::CODE_BLOCK => {
                    // Code blocks in list items need indentation
                    let content_indent = list_indent.hanging_indent(total_indent);
                    self.format_indented_code_block(&child, content_indent);
                }
                SyntaxKind::BLANK_LINE => {
                    // Blank lines within list items
                    self.output.push('\n');
                }
                _ => {
                    // Other block elements
                }
            }
        }
    }
}
