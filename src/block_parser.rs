use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

mod blockquotes;
mod code_blocks;
mod container_stack;
mod headings;
mod lists;
mod paragraphs;
mod resolvers;
mod utils;

use code_blocks::try_parse_fenced_code_block;
use container_stack::{Container, ContainerStack, byte_index_at_column, leading_indent};
use lists::{markers_match, try_parse_list_marker};

fn init_logger() {
    let _ = env_logger::builder().is_test(true).try_init();
}

/// Check if line starts with a blockquote marker (up to 3 spaces + >).
/// Returns (marker_end_byte, content_start_byte) if found.
fn try_parse_blockquote_marker(line: &str) -> Option<(usize, usize)> {
    let bytes = line.as_bytes();
    let mut i = 0;

    // Skip up to 3 spaces
    let mut spaces = 0;
    while i < bytes.len() && bytes[i] == b' ' && spaces < 3 {
        spaces += 1;
        i += 1;
    }

    // Must have > next
    if i >= bytes.len() || bytes[i] != b'>' {
        return None;
    }
    let marker_end = i + 1;

    // Optional space after >
    let content_start = if marker_end < bytes.len() && bytes[marker_end] == b' ' {
        marker_end + 1
    } else {
        marker_end
    };

    Some((marker_end, content_start))
}

pub struct BlockParser<'a> {
    lines: Vec<&'a str>,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    containers: ContainerStack,
}

impl<'a> BlockParser<'a> {
    pub fn new(input: &'a str) -> Self {
        let lines: Vec<&str> = input.lines().collect();
        Self {
            lines,
            pos: 0,
            builder: GreenNodeBuilder::new(),
            containers: ContainerStack::new(),
        }
    }

    pub fn parse(mut self) -> SyntaxNode {
        #[cfg(debug_assertions)]
        {
            init_logger();
        }

        self.builder.start_node(SyntaxKind::ROOT.into());
        self.parse_document_stack();
        self.builder.finish_node(); // ROOT

        SyntaxNode::new_root(self.builder.finish())
    }

    fn parse_document_stack(&mut self) {
        self.builder.start_node(SyntaxKind::DOCUMENT.into());

        log::debug!("Starting document parse");

        while self.pos < self.lines.len() {
            let line = self.lines[self.pos];

            log::debug!("Parsing line {}: {}", self.pos + 1, line);

            if self.parse_line(line) {
                continue;
            }
            self.pos += 1;
        }

        self.containers.close_to(0, &mut self.builder);
        self.builder.finish_node(); // DOCUMENT
    }

    /// Check if we need a blank line before starting a new blockquote.
    /// Returns true if a blockquote can start here.
    fn can_start_blockquote(&self) -> bool {
        // At start of document, no blank line needed
        if self.pos == 0 {
            return true;
        }
        // After a blank line, can start blockquote
        if self.pos > 0 && self.lines[self.pos - 1].trim().is_empty() {
            return true;
        }
        // If we're already in a blockquote, nested blockquotes need blank line too
        // (blank_before_blockquote extension)
        false
    }

    /// Count how many blockquote levels the line has, and return the content after stripping markers.
    fn count_blockquote_markers<'b>(&self, line: &'b str) -> (usize, &'b str) {
        let mut depth = 0;
        let mut remaining = line;

        while let Some((_, content_start)) = try_parse_blockquote_marker(remaining) {
            depth += 1;
            remaining = &remaining[content_start..];
        }

        (depth, remaining)
    }

    /// Get the current blockquote depth from the container stack.
    fn current_blockquote_depth(&self) -> usize {
        self.containers
            .stack
            .iter()
            .filter(|c| matches!(c, Container::BlockQuote { .. }))
            .count()
    }

    /// Close blockquotes down to a target depth.
    fn close_blockquotes_to_depth(&mut self, target_depth: usize) {
        let mut current = self.current_blockquote_depth();
        while current > target_depth {
            // Close everything until we hit a blockquote, then close it
            while !matches!(self.containers.last(), Some(Container::BlockQuote { .. })) {
                if self.containers.depth() == 0 {
                    break;
                }
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }
            if matches!(self.containers.last(), Some(Container::BlockQuote { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
                current -= 1;
            } else {
                break;
            }
        }
    }

    /// Returns true if the line was consumed.
    fn parse_line(&mut self, line: &str) -> bool {
        // Count blockquote markers on this line
        let (bq_depth, inner_content) = self.count_blockquote_markers(line);
        let current_bq_depth = self.current_blockquote_depth();

        // Handle blank lines specially (including blank lines inside blockquotes)
        // A line like ">" with nothing after is a blank line inside a blockquote
        let is_blank = line.trim().is_empty() || (bq_depth > 0 && inner_content.trim().is_empty());

        if is_blank {
            // Close paragraph if open
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // For blank lines inside blockquotes, we need to handle them at the right depth
            // First, adjust blockquote depth if needed
            if bq_depth > current_bq_depth {
                // Open blockquotes
                for _ in current_bq_depth..bq_depth {
                    self.builder.start_node(SyntaxKind::BlockQuote.into());
                    self.containers
                        .push(Container::BlockQuote { content_col: 0 });
                }
            } else if bq_depth < current_bq_depth {
                // Close blockquotes down to bq_depth
                self.close_blockquotes_to_depth(bq_depth);
            }

            // Peek ahead to determine what containers to keep open
            let mut peek = self.pos + 1;
            while peek < self.lines.len() && self.lines[peek].trim().is_empty() {
                peek += 1;
            }

            // Determine what containers to keep open based on next line
            let levels_to_keep = if peek < self.lines.len() {
                self.compute_levels_to_keep(self.lines[peek])
            } else {
                0
            };

            // Close containers down to the level we want to keep
            while self.containers.depth() > levels_to_keep {
                match self.containers.last() {
                    Some(Container::ListItem { .. }) | Some(Container::List { .. }) => {
                        self.containers
                            .close_to(self.containers.depth() - 1, &mut self.builder);
                    }
                    _ => break,
                }
            }

            self.builder.start_node(SyntaxKind::BlankLine.into());
            self.builder
                .token(SyntaxKind::BlankLine.into(), inner_content);
            self.builder.finish_node();
            self.pos += 1;
            return true;
        }

        // Handle blockquote depth changes
        if bq_depth > current_bq_depth {
            // Need to open new blockquote(s)
            // But first check blank_before_blockquote requirement
            if current_bq_depth == 0 && !self.can_start_blockquote() {
                // Can't start blockquote without blank line - treat as paragraph
                self.start_paragraph_if_needed();
                self.append_paragraph_line(line);
                self.pos += 1;
                return true;
            }

            // For nested blockquotes, also need blank line before
            if current_bq_depth > 0
                && bq_depth > current_bq_depth
                && !matches!(self.containers.last(), Some(Container::BlockQuote { .. }))
            {
                // We're in a blockquote but not directly (e.g., in a paragraph inside)
                // Check if previous line inside blockquote was blank
                // For now, require the paragraph to be closed first
                if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                    // Lazy continuation - add to existing paragraph
                    self.append_paragraph_line(inner_content);
                    self.pos += 1;
                    return true;
                }
            }

            // Close paragraph before opening blockquote
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Open blockquotes up to the required depth
            for _ in current_bq_depth..bq_depth {
                self.builder.start_node(SyntaxKind::BlockQuote.into());
                self.containers
                    .push(Container::BlockQuote { content_col: 0 });
            }

            // Now parse the inner content
            return self.parse_inner_content(inner_content);
        } else if bq_depth < current_bq_depth {
            // Need to close some blockquotes, but first check for lazy continuation
            // Lazy continuation: line without > continues a paragraph in a blockquote
            if bq_depth == 0 && matches!(self.containers.last(), Some(Container::Paragraph { .. }))
            {
                // This is lazy continuation - add to existing paragraph
                self.append_paragraph_line(line);
                self.pos += 1;
                return true;
            }

            // Not lazy continuation - close paragraph if open
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Close blockquotes down to the new depth
            self.close_blockquotes_to_depth(bq_depth);

            // Parse the inner content at the new depth
            if bq_depth > 0 {
                return self.parse_inner_content(inner_content);
            } else {
                return self.parse_inner_content(line);
            }
        } else if bq_depth > 0 {
            // Same blockquote depth - continue parsing inner content
            return self.parse_inner_content(inner_content);
        }

        // No blockquote markers - parse as regular content
        // But check for lazy continuation first
        if current_bq_depth > 0
            && matches!(self.containers.last(), Some(Container::Paragraph { .. }))
        {
            // Lazy continuation
            self.append_paragraph_line(line);
            self.pos += 1;
            return true;
        }

        self.parse_inner_content(line)
    }

    /// Compute how many container levels to keep open based on next line content.
    fn compute_levels_to_keep(&self, next_line: &str) -> usize {
        let (next_bq_depth, next_inner) = self.count_blockquote_markers(next_line);
        let (next_indent_cols, _) = leading_indent(next_inner);
        let next_marker = try_parse_list_marker(next_inner);

        let mut keep_level = 0;

        // First, account for blockquotes
        for (i, c) in self.containers.stack.iter().enumerate() {
            match c {
                Container::BlockQuote { .. } => {
                    // Count blockquotes up to this point
                    let bq_count = self.containers.stack[..=i]
                        .iter()
                        .filter(|x| matches!(x, Container::BlockQuote { .. }))
                        .count();
                    if bq_count <= next_bq_depth {
                        keep_level = i + 1;
                    }
                }
                Container::List {
                    marker,
                    base_indent_cols,
                } => {
                    let continues_list = if let Some((ref nm, _, _)) = next_marker {
                        markers_match(marker, nm) && next_indent_cols <= base_indent_cols + 3
                    } else {
                        let item_content_col = self
                            .containers
                            .stack
                            .get(i + 1)
                            .and_then(|c| match c {
                                Container::ListItem { content_col } => Some(*content_col),
                                _ => None,
                            })
                            .unwrap_or(0);
                        next_indent_cols >= item_content_col
                    };
                    if continues_list {
                        keep_level = i + 1;
                    }
                }
                _ => {}
            }
        }

        keep_level
    }

    /// Parse content inside blockquotes (or at top level).
    fn parse_inner_content(&mut self, content: &str) -> bool {
        // Check for heading (needs blank line before, or at start of container)
        let has_blank_before = self.pos == 0
            || self.lines[self.pos - 1].trim().is_empty()
            || matches!(self.containers.last(), Some(Container::BlockQuote { .. }));

        if has_blank_before {
            // Try to parse ATX heading from stripped content
            if let Some(heading_level) = self.try_parse_atx_heading_inline(content) {
                self.emit_atx_heading(content, heading_level);
                self.pos += 1;
                return true;
            }
        }

        // Check for fenced code block (only at top level for now)
        if has_blank_before
            && self.current_blockquote_depth() == 0
            && let Some(new_pos) =
                try_parse_fenced_code_block(&self.lines, self.pos, &mut self.builder, true)
        {
            self.pos = new_pos;
            return true;
        }

        // List marker?
        if let Some((marker, marker_len, spaces_after)) = try_parse_list_marker(content) {
            let (indent_cols, indent_bytes) = leading_indent(content);
            if indent_cols >= 4 && !self.in_list() {
                // Code block at top-level, treat as paragraph
                self.start_paragraph_if_needed();
                self.append_paragraph_line(content);
                self.pos += 1;
                return true;
            }

            // Close paragraph before list item
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Nested list inside current item if indented to content column or beyond
            let current_content_col = self.current_content_col();
            if current_content_col > 0 && indent_cols >= current_content_col {
                self.start_nested_list(
                    content,
                    &marker,
                    marker_len,
                    spaces_after,
                    indent_cols,
                    indent_bytes,
                );
                self.pos += 1;
                return true;
            }

            // Find matching list level
            let matched_level = self.find_matching_list_level(&marker, indent_cols);

            if let Some(level) = matched_level {
                self.continue_list_at_level(level);
            } else {
                self.start_new_list(&marker, indent_cols);
            }

            // Start list item
            self.emit_list_item(
                content,
                &marker,
                marker_len,
                spaces_after,
                indent_cols,
                indent_bytes,
            );
            self.pos += 1;
            return true;
        }

        // Paragraph
        self.start_paragraph_if_needed();
        self.append_paragraph_line(content);
        self.pos += 1;
        true
    }

    /// Try to parse an ATX heading from content, returns heading level if found.
    fn try_parse_atx_heading_inline(&self, content: &str) -> Option<usize> {
        let trimmed = content.trim_start();

        // Must start with 1-6 # characters
        let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
        if hash_count == 0 || hash_count > 6 {
            return None;
        }

        // After hashes, must be end of line, space, or tab
        let after_hashes = &trimmed[hash_count..];
        if !after_hashes.is_empty()
            && !after_hashes.starts_with(' ')
            && !after_hashes.starts_with('\t')
        {
            return None;
        }

        // Check leading spaces (max 3)
        let leading_spaces = content.len() - trimmed.len();
        if leading_spaces > 3 {
            return None;
        }

        Some(hash_count)
    }

    /// Emit an ATX heading node.
    fn emit_atx_heading(&mut self, content: &str, level: usize) {
        self.builder.start_node(SyntaxKind::Heading.into());

        let trimmed = content.trim_start();

        // Marker node for the hashes (must be a node containing a token, not just a token)
        self.builder.start_node(SyntaxKind::AtxHeadingMarker.into());
        self.builder
            .token(SyntaxKind::AtxHeadingMarker.into(), &trimmed[..level]);
        self.builder.finish_node();

        // Get content after marker
        let after_marker = &trimmed[level..];
        let content_start = after_marker
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(after_marker.len());

        // Emit heading content (strip trailing hashes)
        let heading_content = after_marker[content_start..].trim_end();
        let heading_content =
            heading_content.trim_end_matches(|c: char| c == '#' || c.is_whitespace());

        // Heading content node
        self.builder.start_node(SyntaxKind::HeadingContent.into());
        if !heading_content.is_empty() {
            self.builder.token(SyntaxKind::TEXT.into(), heading_content);
        }
        self.builder.finish_node();

        self.builder.finish_node(); // Heading
    }

    fn in_list(&self) -> bool {
        self.containers
            .stack
            .iter()
            .any(|c| matches!(c, Container::List { .. }))
    }

    fn find_matching_list_level(
        &self,
        marker: &lists::ListMarker,
        indent_cols: usize,
    ) -> Option<usize> {
        for (i, c) in self.containers.stack.iter().enumerate() {
            if let Container::List {
                marker: list_marker,
                base_indent_cols,
            } = c
                && markers_match(marker, list_marker)
                && indent_cols <= base_indent_cols + 3
            {
                return Some(i);
            }
        }
        None
    }

    fn continue_list_at_level(&mut self, level: usize) {
        self.containers.close_to(level + 1, &mut self.builder);
        if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
            self.containers
                .close_to(self.containers.depth() - 1, &mut self.builder);
        }
        if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
            self.containers
                .close_to(self.containers.depth() - 1, &mut self.builder);
        }
    }

    fn start_new_list(&mut self, marker: &lists::ListMarker, indent_cols: usize) {
        if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
            self.containers
                .close_to(self.containers.depth() - 1, &mut self.builder);
        }
        while matches!(self.containers.last(), Some(Container::ListItem { .. })) {
            self.containers
                .close_to(self.containers.depth() - 1, &mut self.builder);
        }
        while matches!(self.containers.last(), Some(Container::List { .. })) {
            self.containers
                .close_to(self.containers.depth() - 1, &mut self.builder);
        }
        self.builder.start_node(SyntaxKind::List.into());
        self.containers.push(Container::List {
            marker: marker.clone(),
            base_indent_cols: indent_cols,
        });
    }

    fn start_nested_list(
        &mut self,
        content: &str,
        marker: &lists::ListMarker,
        marker_len: usize,
        spaces_after: usize,
        indent_cols: usize,
        indent_bytes: usize,
    ) {
        if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
            self.containers
                .close_to(self.containers.depth() - 1, &mut self.builder);
        }
        self.builder.start_node(SyntaxKind::List.into());
        self.containers.push(Container::List {
            marker: marker.clone(),
            base_indent_cols: indent_cols,
        });
        self.emit_list_item(
            content,
            marker,
            marker_len,
            spaces_after,
            indent_cols,
            indent_bytes,
        );
    }

    fn emit_list_item(
        &mut self,
        content: &str,
        _marker: &lists::ListMarker,
        marker_len: usize,
        spaces_after: usize,
        indent_cols: usize,
        indent_bytes: usize,
    ) {
        self.builder.start_node(SyntaxKind::ListItem.into());
        let marker_text = &content[indent_bytes..indent_bytes + marker_len];
        self.builder
            .token(SyntaxKind::ListMarker.into(), marker_text);
        if spaces_after > 0 {
            let space_start = indent_bytes + marker_len;
            let space_end = space_start + spaces_after;
            if space_end <= content.len() {
                self.builder.token(
                    SyntaxKind::WHITESPACE.into(),
                    &content[space_start..space_end],
                );
            }
        }
        let content_col = indent_cols + marker_len + spaces_after;
        let content_start = indent_bytes + marker_len + spaces_after;
        self.containers.push(Container::ListItem { content_col });
        if content_start < content.len() {
            self.builder
                .token(SyntaxKind::TEXT.into(), &content[content_start..]);
        }
        self.builder.token(SyntaxKind::NEWLINE.into(), "\n");
    }

    fn start_paragraph_if_needed(&mut self) {
        if !matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
            let content_col = self.current_content_col();
            self.builder.start_node(SyntaxKind::PARAGRAPH.into());
            self.containers.push(Container::Paragraph { content_col });
        }
    }

    fn append_paragraph_line(&mut self, line: &str) {
        let text = self.strip_to_content_col(line);
        self.builder.token(SyntaxKind::TEXT.into(), text);
        self.builder.token(SyntaxKind::NEWLINE.into(), "\n");
    }

    fn current_content_col(&self) -> usize {
        self.containers
            .stack
            .iter()
            .rev()
            .find_map(|c| match c {
                Container::ListItem { content_col } => Some(*content_col),
                _ => None,
            })
            .unwrap_or(0)
    }

    fn strip_to_content_col<'b>(&self, line: &'b str) -> &'b str {
        let target = self.current_content_col();
        if target == 0 {
            return line;
        }
        let (indent_cols, _) = leading_indent(line);
        if indent_cols >= target {
            let idx = byte_index_at_column(line, target);
            &line[idx..]
        } else {
            line.trim_start()
        }
    }
}

#[cfg(test)]
mod tests {
    mod blanklines;
    mod blockquotes;
    mod code_blocks;
    mod headings;
    mod helpers;
    mod lists;
}
