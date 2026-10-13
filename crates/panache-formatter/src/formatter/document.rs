use crate::formatter::Formatter;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

use super::utils::is_block_element;

impl Formatter {
    pub(super) fn format_document(&mut self, node: &SyntaxNode, indent: usize) {
        for element in node.children_with_tokens() {
            match element {
                NodeOrToken::Node(child) if self.should_process_top_level_node(&child) => {
                    self.format_node_sync(&child, indent);
                }
                NodeOrToken::Token(token) => match token.kind() {
                    SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => {}
                    SyntaxKind::BLANK_LINE if !self.output.is_empty() => self.output.push('\n'),
                    SyntaxKind::NONBREAKING_SPACE => self.output.push_str(r"\ "),
                    _ => self.output.push_str(token.text()),
                },
                _ => {}
            }
        }
    }

    pub(super) fn format_heading_block(&mut self, node: &SyntaxNode, indent: usize) {
        log::trace!("Formatting heading");
        if let Some(previous) = node.prev_sibling()
            && is_block_element(previous.kind())
            && !self.output.is_empty()
            && self.output.ends_with('\n')
            && !self.output.ends_with("\n\n")
        {
            self.output.push('\n');
        }

        self.output.push_str(&" ".repeat(indent));
        self.output.push_str(&self.format_heading(node, indent));
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

    pub(super) fn format_horizontal_rule(&mut self, node: &SyntaxNode, indent: usize) {
        if !self.output.is_empty() && self.output.ends_with('\n') && !self.output.ends_with("\n\n")
        {
            self.output.push('\n');
        }
        self.output.push_str(&" ".repeat(indent));
        self.output
            .push_str(&self.horizontal_rule_text(self.config.line_width.saturating_sub(indent)));
        self.output.push('\n');

        if let Some(next) = node.next_sibling()
            && is_block_element(next.kind())
            && !self.output.ends_with("\n\n")
        {
            self.output.push('\n');
            self.consecutive_blank_lines = 1;
        }
    }

    pub(super) fn format_reference_definition(&mut self, node: &SyntaxNode) {
        self.output.push_str(node.text().to_string().trim_end());
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }

        if let Some(next) = node.next_sibling()
            && is_block_element(next.kind())
            && next.kind() != SyntaxKind::REFERENCE_DEFINITION
            && next.kind() != SyntaxKind::FOOTNOTE_DEFINITION
            && !self.output.ends_with("\n\n")
        {
            self.output.push('\n');
        }
    }

    pub(super) fn format_metadata_block(&mut self, node: &SyntaxNode) {
        let text = node.text().to_string();
        self.output.push_str(&text);
        if !text.ends_with('\n') {
            self.output.push('\n');
        }
        if let Some(next) = node.next_sibling()
            && is_block_element(next.kind())
            && !self.output.ends_with("\n\n")
        {
            self.output.push('\n');
            self.consecutive_blank_lines = 1;
        }
    }

    pub(super) fn format_blank_line(&mut self) {
        if !self.output.is_empty() && self.consecutive_blank_lines < 1 {
            self.output.push('\n');
            self.consecutive_blank_lines += 1;
        }
    }
}
