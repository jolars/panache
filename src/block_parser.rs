use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

mod blockquotes;
mod code_blocks;
mod headings;
mod paragraphs;
mod resolvers;
mod utils;

use code_blocks::try_parse_fenced_code_block;
use headings::try_parse_atx_heading;
use paragraphs::try_parse_paragraph;
use resolvers::resolve_containers;

fn init_logger() {
    let _ = env_logger::builder().is_test(true).try_init();
}

pub struct BlockParser<'a> {
    lines: Vec<&'a str>,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
}

impl<'a> BlockParser<'a> {
    pub fn new(input: &'a str) -> Self {
        let lines: Vec<&str> = input.lines().collect();
        Self {
            lines,
            pos: 0,
            builder: GreenNodeBuilder::new(),
        }
    }

    fn has_blank_line_before(&self) -> bool {
        if self.pos == 0 {
            true
        } else {
            self.lines[self.pos - 1].trim().is_empty()
        }
    }

    fn try_parse_atx_heading(&mut self) -> bool {
        let has_blank = self.has_blank_line_before();
        if let Some(new_pos) =
            try_parse_atx_heading(&self.lines, self.pos, &mut self.builder, has_blank)
        {
            self.pos = new_pos;
            true
        } else {
            false
        }
    }

    pub fn try_parse_blank_line(&mut self) -> bool {
        log::debug!("Trying to parse blank line at position {}", self.pos);

        if self.pos >= self.lines.len() {
            return false;
        }

        let line = self.lines[self.pos];

        if line.trim().is_empty() {
            self.builder.start_node(SyntaxKind::BlankLine.into());
            self.builder.token(SyntaxKind::BlankLine.into(), line);
            self.builder.finish_node();
            self.pos += 1;

            log::debug!("Parsed blank line at position {}", self.pos);

            return true;
        }

        false
    }

    pub fn try_parse_fenced_code_block(&mut self) -> bool {
        let has_blank = self.has_blank_line_before();
        if let Some(new_pos) =
            try_parse_fenced_code_block(&self.lines, self.pos, &mut self.builder, has_blank)
        {
            self.pos = new_pos;
            true
        } else {
            false
        }
    }

    pub fn try_parse_paragraph(&mut self) -> bool {
        if let Some(new_pos) = try_parse_paragraph(&self.lines, self.pos, &mut self.builder) {
            self.pos = new_pos;
            true
        } else {
            false
        }
    }

    pub fn parse(mut self) -> SyntaxNode {
        #[cfg(debug_assertions)]
        {
            init_logger();
        }

        self.builder.start_node(SyntaxKind::ROOT.into());
        self.parse_document();
        self.builder.finish_node();

        let flat_tree = SyntaxNode::new_root(self.builder.finish());

        // Second pass: resolve container blocks
        resolve_containers(flat_tree)
    }

    fn parse_document(&mut self) {
        self.builder.start_node(SyntaxKind::DOCUMENT.into());

        log::debug!("Starting document parse");

        while self.pos < self.lines.len() {
            let line = self.lines[self.pos];

            log::debug!("Parsing line {}: {}", self.pos + 1, line);

            if self.try_parse_blank_line() {
                continue;
            }

            if self.try_parse_atx_heading() {
                continue;
            }

            if self.try_parse_fenced_code_block() {
                continue;
            }

            if self.try_parse_paragraph() {
                continue;
            }

            // If no other block matched, just skip the line (could be improved)
            self.pos += 1;
        }

        self.builder.finish_node();
    }
}

#[cfg(test)]
mod tests {
    mod blanklines;
    mod blockquotes;
    mod code_blocks;
    mod headings;
    mod helpers;
}
