use crate::config::WrapMode;
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
                && t.kind() == SyntaxKind::ListMarker
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

    /// Check if a marker should be right-aligned (Roman numerals and alphabetic markers)
    pub(super) fn is_alignable_marker(marker: &str) -> bool {
        // Don't align example lists (they start with '(@')
        if marker.starts_with("(@") {
            return false;
        }

        // Don't align bullet lists
        if marker.len() == 1 && (marker == "-" || marker == "*" || marker == "+") {
            return false;
        }

        // Align all ordered list styles with letters or Roman numerals:
        // Period: a., i., A., I.
        // Right-paren: a), i), A), I)
        // Parens: (a), (i), (A), (I)
        if marker.len() < 2 {
            return false;
        }

        // Check if the marker contains a letter (handles all three delimiter styles)
        marker.chars().any(|c| c.is_alphabetic())
    }

    /// Calculate the maximum marker width for all direct ListItem children of a List
    /// Returns 0 if markers shouldn't be aligned
    pub(super) fn calculate_max_marker_width(list_node: &SyntaxNode) -> usize {
        let markers: Vec<String> = list_node
            .children()
            .filter(|child| child.kind() == SyntaxKind::ListItem)
            .filter_map(|item| Self::extract_list_marker(&item))
            .collect();

        // Check if any marker is alignable
        if !markers.iter().any(|m| Self::is_alignable_marker(m)) {
            return 0;
        }

        // Return max width of alignable markers
        markers
            .iter()
            .filter(|m| Self::is_alignable_marker(m))
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

        // Calculate marker padding (for right-alignment)
        let marker_padding = if Self::is_alignable_marker(&marker) && max_marker_width > 0 {
            max_marker_width.saturating_sub(marker.len())
        } else {
            0
        };

        // Spaces after marker (minimum 1, or 2 for uppercase letter markers)
        let spaces_after = if marker.len() == 2
            && marker.starts_with(|c: char| c.is_ascii_uppercase())
            && marker.ends_with('.')
        {
            2
        } else {
            1
        };

        // Check for task checkbox (adds 4 more characters: "[x] ")
        let has_checkbox = item_node.children_with_tokens().any(|el| {
            if let NodeOrToken::Token(t) = el {
                t.kind() == SyntaxKind::TaskCheckbox
            } else {
                false
            }
        });
        let checkbox_width = if has_checkbox { 4 } else { 0 };

        marker_padding + marker.len() + spaces_after + checkbox_width
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
        // Add blank line before top-level lists (indent == 0) that follow content.
        // Don't add for nested lists (indent > 0) as they follow their parent item's content.
        if indent == 0 && !self.output.is_empty() && !self.output.ends_with("\n\n") {
            self.output.push('\n');
        }

        // Calculate max marker width for right-alignment
        let max_marker_width = Self::calculate_max_marker_width(node);
        self.max_marker_widths.push(max_marker_width);

        let mut prev_was_item = false;
        let mut prev_was_blank = false;
        let mut prev_item_had_trailing_blank = false;
        let mut last_item_content_indent = 0;

        for child in node.children() {
            if child.kind() == SyntaxKind::ListItem {
                // Only strip double newlines if:
                // 1. There was no explicit blank line before this item (at List level)
                // 2. The previous ListItem didn't have a trailing BlankLine child
                if prev_was_item && !prev_was_blank && !prev_item_had_trailing_blank {
                    while self.output.ends_with("\n\n") {
                        self.output.pop();
                    }
                }

                // Check if this list item has a trailing BlankLine child
                prev_item_had_trailing_blank = child
                    .children()
                    .last()
                    .map(|last_child| last_child.kind() == SyntaxKind::BlankLine)
                    .unwrap_or(false);

                prev_was_item = true;
                prev_was_blank = false;

                // Calculate content indent for this list item (marker + space)
                last_item_content_indent =
                    indent + Self::calculate_list_item_content_indent(&child, max_marker_width);
            }

            // Preserve blank lines between list items
            if child.kind() == SyntaxKind::BlankLine {
                self.output.push('\n');
                prev_was_blank = true;
                continue;
            }

            // Paragraphs that are siblings of ListItems are continuation content
            // Format them with the last list item's content indentation
            if child.kind() == SyntaxKind::PARAGRAPH && prev_was_item {
                self.format_list_continuation_paragraph(&child, last_item_content_indent);
            } else if child.kind() == SyntaxKind::CodeBlock && prev_was_item {
                // Code blocks that are siblings of ListItems are also continuation content
                // Format them with indentation
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
                rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::ListMarker => {
                    seen_marker = true;
                }
                rowan::NodeOrToken::Node(n)
                    if matches!(n.kind(), SyntaxKind::Plain | SyntaxKind::PARAGRAPH) =>
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
                    SyntaxKind::ListMarker => {
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
                    SyntaxKind::TaskCheckbox => {
                        checkbox = Some(t.text().to_string());
                    }
                    _ => {}
                }
            }
        }

        // Get max marker width for this list level
        let max_marker_width = self.max_marker_widths.last().copied().unwrap_or(0);

        // Calculate leading spaces for right-alignment of markers
        // and standard spacing after marker
        let (marker_padding, spaces_after_marker) =
            if Self::is_alignable_marker(&marker) && max_marker_width > 0 {
                // Right-align markers by adding leading spaces
                let padding = max_marker_width.saturating_sub(marker.len());

                // Check if this is uppercase letter with period (needs minimum 2 spaces)
                let min_spaces = if marker.len() == 2
                    && marker.starts_with(|c: char| c.is_ascii_uppercase())
                    && marker.ends_with('.')
                {
                    2
                } else {
                    1
                };

                (padding, min_spaces)
            } else {
                // Non-alignable markers: no padding
                let spaces = if marker.len() == 2
                    && marker.starts_with(|c: char| c.is_ascii_uppercase())
                    && marker.ends_with('.')
                {
                    2
                } else {
                    1
                };
                (0, spaces)
            };

        let total_indent = indent;

        // Calculate checkbox width if present (checkbox + space after)
        let checkbox_width = if let Some(ref cb) = checkbox {
            cb.len() + 1 // "[x] " is 4 characters
        } else {
            0
        };

        let hanging =
            marker_padding + marker.len() + spaces_after_marker + total_indent + checkbox_width;
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
                self.output.push_str(&" ".repeat(marker_padding));
                self.output.push_str(&marker);
                self.output.push_str(&" ".repeat(spaces_after_marker));

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
                SyntaxKind::Plain | SyntaxKind::PARAGRAPH => {
                    // These blocks are already handled by word wrapping above if they're
                    // direct children. Only process Plain/PARAGRAPH if it comes after a BlankLine
                    // (indicating it's a true continuation paragraph, not the first content).
                    let has_blank_before = child
                        .prev_sibling()
                        .map(|prev| prev.kind() == SyntaxKind::BlankLine)
                        .unwrap_or(false);

                    if has_blank_before {
                        let content_indent = total_indent
                            + marker_padding
                            + marker.len()
                            + spaces_after_marker
                            + checkbox_width;
                        self.format_list_continuation_paragraph(&child, content_indent);
                    }
                    // Otherwise skip - already handled
                }
                SyntaxKind::List => {
                    // Nested list indent includes: total_indent + marker_padding + marker + 1 space + checkbox
                    self.format_node_sync(
                        &child,
                        total_indent + marker_padding + marker.len() + 1 + checkbox_width,
                    );
                }
                SyntaxKind::CodeBlock => {
                    // Code blocks in list items need indentation
                    let content_indent = total_indent
                        + marker_padding
                        + marker.len()
                        + spaces_after_marker
                        + checkbox_width;
                    self.format_indented_code_block(&child, content_indent);
                }
                SyntaxKind::BlankLine => {
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
