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
use headings::try_parse_atx_heading;
use lists::{markers_match, try_parse_list_marker};
use resolvers::resolve_containers;

fn init_logger() {
    let _ = env_logger::builder().is_test(true).try_init();
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

        let flat = SyntaxNode::new_root(self.builder.finish());
        // Use resolver (still needed for blockquotes) until blockquote stack is implemented.
        resolve_containers(flat)
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

    /// Returns true if the line was consumed.
    fn parse_line(&mut self, line: &str) -> bool {
        // Blank line: close paragraph if open, emit blank line.
        // Keep list containers open if next non-blank line continues the list.
        if line.trim().is_empty() {
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Peek ahead to determine how many list levels to keep open.
            let mut peek = self.pos + 1;
            while peek < self.lines.len() && self.lines[peek].trim().is_empty() {
                peek += 1;
            }

            // Find how many list levels should stay open.
            let levels_to_keep = if peek < self.lines.len() {
                let next_line = self.lines[peek];
                let (next_indent_cols, _) = leading_indent(next_line);
                let next_marker = try_parse_list_marker(next_line);

                // Find the deepest list level that the next line continues.
                let mut keep_level = 0;
                for (i, c) in self.containers.stack.iter().enumerate() {
                    if let Container::List {
                        marker,
                        base_indent_cols,
                    } = c
                    {
                        // Check if next line is a matching list marker for this level.
                        let continues_list = if let Some((ref nm, _, _)) = next_marker {
                            markers_match(marker, nm) && next_indent_cols <= base_indent_cols + 3
                        } else {
                            // Check if it's a continuation line - need the item's content_col.
                            // Find the ListItem that follows this List in the stack.
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
                            // Keep this list and everything above it open.
                            keep_level = i + 1;
                        }
                    }
                }
                keep_level
            } else {
                0
            };

            // Close lists down to the level we want to keep.
            // First close any list items, then their lists, down to the keep level.
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
            self.builder.token(SyntaxKind::BlankLine.into(), line);
            self.builder.finish_node();
            self.pos += 1;
            return true;
        }

        // Headings and fenced code handled top-level only
        if self.containers.depth() == 0 {
            if let Some(new_pos) =
                try_parse_atx_heading(&self.lines, self.pos, &mut self.builder, true)
            {
                self.pos = new_pos;
                return true;
            }
            if let Some(new_pos) =
                try_parse_fenced_code_block(&self.lines, self.pos, &mut self.builder, true)
            {
                self.pos = new_pos;
                return true;
            }
        }

        // List marker?
        if let Some((marker, marker_len, spaces_after)) = try_parse_list_marker(line) {
            let (indent_cols, indent_bytes) = leading_indent(line);
            if indent_cols >= 4 && self.containers.depth() == 0 {
                return false; // code block at top-level
            }

            // Close paragraph before list item
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Nested list inside current item if indented to content column or beyond.
            let current_content_col = self.current_content_col();
            if current_content_col > 0 && indent_cols >= current_content_col {
                if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                    self.containers
                        .close_to(self.containers.depth() - 1, &mut self.builder);
                }
                self.builder.start_node(SyntaxKind::List.into());
                self.containers.push(Container::List {
                    marker: marker.clone(),
                    base_indent_cols: indent_cols,
                });
                // Start list item
                self.builder.start_node(SyntaxKind::ListItem.into());
                let marker_text = &line[indent_bytes..indent_bytes + marker_len];
                self.builder
                    .token(SyntaxKind::ListMarker.into(), marker_text);
                if spaces_after > 0 {
                    let space_start = indent_bytes + marker_len;
                    let space_end = space_start + spaces_after;
                    if space_end <= line.len() {
                        self.builder
                            .token(SyntaxKind::WHITESPACE.into(), &line[space_start..space_end]);
                    }
                }
                let content_col = indent_cols + marker_len + spaces_after;
                let content_start = indent_bytes + marker_len + spaces_after;
                self.containers.push(Container::ListItem { content_col });
                if content_start < line.len() {
                    self.builder
                        .token(SyntaxKind::TEXT.into(), &line[content_start..]);
                }
                self.builder.token(SyntaxKind::NEWLINE.into(), "\n");
                self.pos += 1;
                return true;
            }

            // Find matching list level (same marker, indent within range)
            let mut matched_level = None;
            for (i, c) in self.containers.stack.iter().enumerate() {
                if let Container::List {
                    marker: list_marker,
                    base_indent_cols,
                } = c
                    && markers_match(&marker, list_marker)
                    && indent_cols <= base_indent_cols + 3
                {
                    matched_level = Some(i);
                }
            }

            if let Some(level) = matched_level {
                // Close deeper than this list, and close current item
                self.containers.close_to(level + 1, &mut self.builder);
                if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                    self.containers
                        .close_to(self.containers.depth() - 1, &mut self.builder);
                }
                if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
                    self.containers
                        .close_to(self.containers.depth() - 1, &mut self.builder);
                }
            } else {
                // New list at this position: close any existing list/item so it becomes a sibling.
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

            // Start list item
            self.builder.start_node(SyntaxKind::ListItem.into());
            let marker_text = &line[indent_bytes..indent_bytes + marker_len];
            self.builder
                .token(SyntaxKind::ListMarker.into(), marker_text);
            if spaces_after > 0 {
                let space_start = indent_bytes + marker_len;
                let space_end = space_start + spaces_after;
                if space_end <= line.len() {
                    self.builder
                        .token(SyntaxKind::WHITESPACE.into(), &line[space_start..space_end]);
                }
            }
            let content_col = indent_cols + marker_len + spaces_after;
            let content_start = indent_bytes + marker_len + spaces_after;
            self.containers.push(Container::ListItem { content_col });
            if content_start < line.len() {
                self.builder
                    .token(SyntaxKind::TEXT.into(), &line[content_start..]);
            }
            self.builder.token(SyntaxKind::NEWLINE.into(), "\n");
            self.pos += 1;
            return true;
        }

        // Paragraph (respect list item indent)
        self.start_paragraph_if_needed();
        self.append_paragraph_line(line);
        self.pos += 1;
        true
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
