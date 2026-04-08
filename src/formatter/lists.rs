use crate::config::WrapMode;
use crate::formatter::indent_utils::{calculate_list_item_indent, is_alignable_marker};
use crate::formatter::wrapping::{self, WrapStrategy};
use crate::syntax::{BlockQuote, SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

use super::Formatter;

impl Formatter {
    fn normalize_task_checkbox(checkbox: &str) -> String {
        if checkbox == "[X]" {
            "[x]".to_string()
        } else {
            checkbox.to_string()
        }
    }

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

    /// Check if a nested list is empty (contains only one item with no text content)
    fn is_empty_nested_list(list_node: &SyntaxNode) -> bool {
        let items: Vec<_> = list_node
            .children()
            .filter(|c| c.kind() == SyntaxKind::LIST_ITEM)
            .collect();

        // Must have exactly one item
        if items.len() != 1 {
            return false;
        }

        let item = &items[0];

        // Check if item has any text content or nested structures
        for child in item.children_with_tokens() {
            match child {
                NodeOrToken::Token(t) => {
                    // Has text content beyond marker/whitespace/newline
                    if matches!(t.kind(), SyntaxKind::TEXT | SyntaxKind::ESCAPED_CHAR) {
                        return false;
                    }
                }
                NodeOrToken::Node(n) => {
                    // Has nested blocks (PLAIN/PARAGRAPH/LIST/etc)
                    if !matches!(
                        n.kind(),
                        SyntaxKind::LIST_MARKER | SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE
                    ) {
                        return false;
                    }
                }
            }
        }

        true
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
            WrapMode::Sentence => {
                let lines = self.sentence_lines_for_paragraph(node);
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
        // Keep one normalized separator between adjacent top-level lists to match Pandoc output.
        if indent == 0
            && self.fenced_div_depth == 0
            && !self.output.is_empty()
            && !self.output.ends_with("\n\n")
        {
            self.output.push('\n');
        }

        // Calculate max marker width for right-alignment
        let max_marker_width = Self::calculate_max_marker_width(node);
        self.max_marker_widths.push(max_marker_width);

        // Decide loose/tight at the *list* level.
        // Parser may emit PLAIN for most list item text; we treat lists as loose
        // if there are explicit blank lines between items in the CST.
        let list_children: Vec<_> = node.children().collect();
        let has_blank_between_items = list_children.iter().enumerate().any(|(idx, child)| {
            if child.kind() != SyntaxKind::BLANK_LINE {
                return false;
            }
            let prev_is_item = idx > 0 && list_children[idx - 1].kind() == SyntaxKind::LIST_ITEM;
            let next_is_item = idx + 1 < list_children.len()
                && list_children[idx + 1].kind() == SyntaxKind::LIST_ITEM;
            prev_is_item && next_is_item
        });
        let has_nested_lists = list_children.iter().any(|child| {
            child.kind() == SyntaxKind::LIST_ITEM
                && child
                    .children()
                    .any(|item_child| item_child.kind() == SyntaxKind::LIST)
        });
        let has_blockquote_children = list_children.iter().any(|child| {
            child.kind() == SyntaxKind::LIST_ITEM
                && child
                    .children()
                    .any(|item_child| matches!(item_child.kind(), SyntaxKind::BLOCK_QUOTE))
        });
        let is_loose = (has_blank_between_items || has_blockquote_children) && !has_nested_lists;

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

                // Calculate content indent for this list item (marker + space)
                last_item_content_indent =
                    indent + Self::calculate_list_item_content_indent(&child, max_marker_width);

                self.format_node_sync(&child, indent);

                // Add blank line after each item for loose lists (except last)
                if is_loose && item_count < total_items && !self.output.ends_with("\n\n") {
                    self.output.push('\n');
                }
            } else if child.kind() == SyntaxKind::BLANK_LINE {
                // Preserve explicit separators when not treating this list as globally loose.
                let prev_is_item = child
                    .prev_sibling()
                    .map(|n| n.kind() == SyntaxKind::LIST_ITEM)
                    .unwrap_or(false);
                let next_is_item = child
                    .next_sibling()
                    .map(|n| n.kind() == SyntaxKind::LIST_ITEM)
                    .unwrap_or(false);
                if !is_loose && prev_is_item && next_is_item && !self.output.ends_with("\n\n") {
                    self.output.push('\n');
                }
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
        // Pre-pass: Process any directive comments to update tracker state
        for child in node.children() {
            if matches!(child.kind(), SyntaxKind::HTML_BLOCK | SyntaxKind::COMMENT)
                && let Some(directive) = crate::directives::extract_directive_from_node(&child)
            {
                self.directive_tracker.process_directive(&directive);
            }
        }

        // Compute indent, marker, and checkbox from leading tokens
        let mut marker = String::new();
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
                        checkbox = Some(Self::normalize_task_checkbox(t.text()));
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

        let first_non_blank_child = node
            .children()
            .find(|child| child.kind() != SyntaxKind::BLANK_LINE);
        if let Some(leading_heading) = first_non_blank_child
            && leading_heading.kind() == SyntaxKind::HEADING
        {
            self.output.push_str(&" ".repeat(total_indent));
            self.output
                .push_str(&" ".repeat(list_indent.marker_padding));
            self.output.push_str(&marker);
            self.output.push_str(&" ".repeat(list_indent.spaces_after));
            if let Some(ref cb) = checkbox {
                self.output.push_str(cb);
                self.output.push(' ');
            }
            self.output.push_str(&self.format_heading(&leading_heading));
            self.output.push('\n');

            let has_following_blocks = node
                .children()
                .any(|child| child != leading_heading && child.kind() != SyntaxKind::BLANK_LINE);
            if has_following_blocks {
                self.output.push('\n');
            }

            for child in node.children() {
                if child == leading_heading || child.kind() == SyntaxKind::BLANK_LINE {
                    continue;
                }

                match child.kind() {
                    SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH => {
                        self.format_list_continuation_paragraph(&child, hanging);
                    }
                    SyntaxKind::LIST => {
                        self.format_node_sync(&child, hanging);
                    }
                    SyntaxKind::CODE_BLOCK => {
                        self.format_indented_code_block(&child, hanging);
                    }
                    _ => {
                        self.format_node_sync(&child, hanging);
                    }
                }
            }
            return;
        }

        // Build source node for wrapping from Plain/PARAGRAPH content node if present.
        let content_node = Self::find_content_node(node);

        let content_has_hard_breaks = content_node
            .as_ref()
            .map(|content| {
                content
                    .descendants_with_tokens()
                    .any(|el| el.kind() == SyntaxKind::HARD_LINE_BREAK)
            })
            .unwrap_or(false);

        let wrap_source = content_node.as_ref();

        // Check if this item contains only an empty nested list (special case formatting)
        let has_only_empty_nested_list = node
            .children()
            .any(|c| c.kind() == SyntaxKind::LIST && Self::is_empty_nested_list(&c))
            && wrap_source.is_none_or(|source| source.text().to_string().trim().is_empty());

        let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
        let in_blockquote = BlockQuote::contains_node(node);
        let line_widths = [available_width];
        let lines = match wrap_mode {
            WrapMode::Preserve => Vec::new(),
            WrapMode::Sentence => Vec::new(),
            WrapMode::Reflow => wrap_source
                .map(|source| {
                    wrapping::wrapped_lines_for_node(
                        &self.config,
                        source,
                        &line_widths,
                        &|n| self.format_inline_node(n),
                        WrapStrategy::ListReflow { in_blockquote },
                    )
                })
                .unwrap_or_default(),
        };
        let preserve_lines = match wrap_mode {
            WrapMode::Preserve => {
                let source = content_node
                    .as_ref()
                    .map(|content| content.text().to_string())
                    .unwrap_or_default();
                Some(source.lines().map(ToString::to_string).collect::<Vec<_>>())
            }
            _ => None,
        };
        let sentence_lines: Option<Vec<String>> = match wrap_mode {
            WrapMode::Sentence => Some(
                wrap_source
                    .map(|source| {
                        wrapping::wrapped_lines_for_node(
                            &self.config,
                            source,
                            &[],
                            &|n| self.format_inline_node(n),
                            WrapStrategy::ListSentence { in_blockquote },
                        )
                    })
                    .unwrap_or_default(),
            ),
            _ => None,
        };

        let heading_with_remainder = content_node
            .as_ref()
            .and_then(|content| self.leading_atx_heading_with_remainder(content));
        let content_starts_with_blockquote = content_node
            .as_ref()
            .map(|content| content.text().to_string().trim_start().starts_with('>'))
            .unwrap_or(false);

        log::trace!(
            "ListItem wrapping: {} lines, hanging indent={}",
            lines.len(),
            hanging
        );

        if let Some((heading_line, remainder)) = heading_with_remainder {
            self.output.push_str(&" ".repeat(total_indent));
            self.output
                .push_str(&" ".repeat(list_indent.marker_padding));
            self.output.push_str(&marker);
            self.output.push_str(&" ".repeat(list_indent.spaces_after));
            if let Some(ref cb) = checkbox {
                self.output.push_str(cb);
                self.output.push(' ');
            }
            self.output.push_str(&heading_line);
            self.output.push('\n');
            self.output.push('\n');

            for line in self.wrap_text_for_indent(&remainder, hanging) {
                self.output.push_str(&" ".repeat(hanging));
                self.output.push_str(line.trim_start());
                self.output.push('\n');
            }
        } else if let Some(preserve_lines) = &preserve_lines {
            for (i, line) in preserve_lines.iter().enumerate() {
                if i == 0 {
                    self.output.push_str(&" ".repeat(total_indent));
                    self.output
                        .push_str(&" ".repeat(list_indent.marker_padding));
                    self.output.push_str(&marker);
                    self.output.push_str(&" ".repeat(list_indent.spaces_after));
                    if let Some(ref cb) = checkbox {
                        self.output.push_str(cb);
                        self.output.push(' ');
                    }
                } else {
                    self.output.push_str(&" ".repeat(hanging));
                }
                self.output.push_str(line.trim_start());
                if !has_only_empty_nested_list {
                    self.output.push('\n');
                }
            }
        } else if let Some(sentence_lines) = &sentence_lines {
            for (i, text) in sentence_lines.iter().enumerate() {
                log::trace!("  Line {}: sentence line", i);
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
                if i > 0 {
                    self.output.push_str(text.trim_start());
                } else {
                    let normalized = text
                        .replace("<summary>\n\t", "<summary>\n    ")
                        .replace("<summary>\n  ", "<summary>\n    ");
                    self.output.push_str(&normalized);
                }
                if !has_only_empty_nested_list {
                    self.output.push('\n');
                }
            }
        } else {
            for (i, line) in lines.iter().enumerate() {
                log::trace!("  Line {}: {} chars", i, line.len());
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
                let mut rendered_line = if i > 0 {
                    line.trim_start().to_string()
                } else {
                    line.to_string()
                };
                rendered_line = rendered_line
                    .replace("<summary>\n\t", "<summary>\n    ")
                    .replace("<summary>\n  ", "<summary>\n    ");
                if rendered_line.contains('\n') {
                    for (idx, segment) in rendered_line.split('\n').enumerate() {
                        let segment = if content_has_hard_breaks {
                            segment
                        } else {
                            segment.trim_end()
                        };
                        if idx == 0 {
                            self.output.push_str(segment);
                        } else {
                            let trimmed = segment.trim_start();
                            if !trimmed.is_empty() {
                                self.output.push('\n');
                                self.output.push_str(&" ".repeat(hanging));
                                self.output.push_str(trimmed);
                            }
                        }
                    }
                } else {
                    self.output.push_str(&rendered_line);
                }
                // Only output newline if this item doesn't have an inline empty nested list
                if !has_only_empty_nested_list {
                    self.output.push('\n');
                }
            }
        }

        // Special case: if no lines were wrapped but we have empty nested list, still output marker
        if lines.is_empty() && has_only_empty_nested_list {
            self.output.push_str(&" ".repeat(total_indent));
            self.output
                .push_str(&" ".repeat(list_indent.marker_padding));
            self.output.push_str(&marker);
            self.output.push(' '); // Space before nested marker
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

                    // Also process if we're in an ignore region (content should be preserved exactly)
                    let in_ignore_region = self.directive_tracker.is_formatting_ignored();

                    if has_blank_before || in_ignore_region {
                        let content_indent = list_indent.hanging_indent(total_indent);
                        // If in ignore region, just call format_node_sync which preserves content
                        // The indent parameter isn't used when in ignore mode, so we don't add it
                        if in_ignore_region {
                            self.format_node_sync(&child, 0);
                        } else {
                            self.format_list_continuation_paragraph(&child, content_indent);
                        }
                    }
                    // Otherwise skip - already handled
                }
                SyntaxKind::LIST => {
                    // Check if this is an empty nested list (only has one item with no content)
                    if Self::is_empty_nested_list(&child) {
                        // Format inline: output nested marker and newline
                        let nested_marker = Self::extract_list_marker(
                            &child
                                .children()
                                .find(|c| c.kind() == SyntaxKind::LIST_ITEM)
                                .unwrap(),
                        )
                        .unwrap_or_else(|| "-".to_string());
                        self.output.push_str(&nested_marker);
                        self.output.push('\n');
                    } else {
                        // Normal nested list: indent on next line
                        self.format_node_sync(&child, list_indent.hanging_indent(total_indent));
                    }
                }
                SyntaxKind::CODE_BLOCK => {
                    // Code blocks in list items need indentation
                    let content_indent = list_indent.hanging_indent(total_indent);
                    self.format_indented_code_block(&child, content_indent);
                }
                SyntaxKind::BLOCK_QUOTE => {
                    let follows_primary_content = child
                        .prev_sibling()
                        .map(|prev| {
                            matches!(prev.kind(), SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH)
                        })
                        .unwrap_or(false);

                    if content_starts_with_blockquote && follows_primary_content {
                        if self.output.ends_with('\n') {
                            self.output.pop();
                        }

                        let mut pieces: Vec<String> = Vec::new();
                        let child_text = child.text().to_string();
                        for line in child_text.lines() {
                            let trimmed = line.trim_start();
                            let content = if let Some(rest) = trimmed.strip_prefix('>') {
                                rest.trim_start()
                            } else {
                                trimmed
                            };
                            if !content.is_empty() {
                                pieces.push(content.to_string());
                            }
                        }

                        if !pieces.is_empty() {
                            self.output.push(' ');
                            self.output.push_str(&pieces.join(" "));
                        }
                        self.output.push('\n');
                    } else {
                        let content_indent = list_indent.hanging_indent(total_indent);
                        self.format_node_sync(&child, content_indent);
                    }
                }
                SyntaxKind::BLANK_LINE => {
                    // Blank lines within list items
                    self.output.push('\n');
                }
                _ => {
                    // Other block elements - format with proper indentation
                    let content_indent = list_indent.hanging_indent(total_indent);
                    self.format_node_sync(&child, content_indent);
                }
            }
        }
    }
}
