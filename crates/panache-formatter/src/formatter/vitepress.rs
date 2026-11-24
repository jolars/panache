use panache_parser::vitepress_container_closing_fence_len;
use rowan::ast::AstNode;

use crate::syntax::{
    Alert, SyntaxKind, SyntaxNode, VitePressContainer, text_without_line_prefixes,
};

use super::Formatter;

impl Formatter {
    pub(super) fn format_vitepress_alert(&mut self, node: &SyntaxNode, indent: usize) {
        let alert = Alert::cast(node.clone()).expect("VitePress alert");
        self.output.push_str(&" ".repeat(indent));
        self.output.push_str(&alert.marker().expect("alert marker"));
        if let Some(title) = alert.title() {
            self.output.push(' ');
            self.output.push_str(title.text());
        }
        self.output.push('\n');
        for child in node.children() {
            self.format_node_sync(&child, indent);
        }
    }

    pub(super) fn format_vitepress_container(&mut self, node: &SyntaxNode, indent: usize) {
        let container = VitePressContainer::cast(node.clone()).expect("VitePress container");
        let open = container
            .opening_fence()
            .expect("VitePress container opener");
        let fence_start = self.output.len() + indent;
        let fence_len = self.write_vitepress_fence(&open, indent, 0);
        let body_start = self.output.len();

        for child in container.body() {
            if child.kind() == SyntaxKind::CODE_BLOCK
                && !self.directive_tracker.is_formatting_ignored()
            {
                self.format_vitepress_container_code(&child, indent);
            } else {
                self.format_node_sync(&child, indent);
            }
        }

        // VitePress looks for a closing fence before parsing any body blocks.
        // A descendant can expose a colon run when its indentation changes.
        let prefix = " ".repeat(indent);
        let body_fence_len = self.output[body_start..]
            .lines()
            .filter_map(|line| line.strip_prefix(&prefix))
            .map(vitepress_container_closing_fence_len)
            .max()
            .unwrap_or(0);
        let required_len = fence_len.max(body_fence_len + 1);
        if required_len > fence_len {
            self.output
                .insert_str(fence_start, &":".repeat(required_len - fence_len));
        }
        if let Some(close) = container.closing_fence() {
            self.write_vitepress_fence(&close, indent, required_len);
        }
    }

    fn format_vitepress_container_code(&mut self, code: &SyntaxNode, indent: usize) {
        let fenced = code
            .children()
            .any(|child| child.kind() == SyntaxKind::CODE_FENCE_OPEN);
        let closed = code
            .children()
            .any(|child| child.kind() == SyntaxKind::CODE_FENCE_CLOSE);
        let rendered = if closed {
            self.format_code_block_to_string(code)
        } else {
            String::new()
        };
        // Code indentation can keep a colon run from closing the container.
        // Preserve its source geometry if formatting would expose such a line.
        let preserve = !closed
            || rendered
                .lines()
                .any(|line| vitepress_container_closing_fence_len(line) > 0);
        let body = if preserve {
            text_without_line_prefixes(code)
        } else {
            rendered
        };
        // The generic indented-code parser may leave the list offset on
        // the whitespace tokens when the extension is used in other flavors.
        let source_offset = if fenced || self.config.flavor == crate::config::Flavor::VitePress {
            0
        } else {
            self.container_content_offset(code)
        };
        for line in body.lines() {
            if !line.is_empty() {
                self.output.push_str(&" ".repeat(indent));
                self.output
                    .push_str(&Self::strip_leading_columns(line, source_offset));
            }
            self.output.push('\n');
        }
        self.consecutive_blank_lines = 0;
    }

    fn write_vitepress_fence(
        &mut self,
        fence: &SyntaxNode,
        indent: usize,
        minimum_len: usize,
    ) -> usize {
        self.output.push_str(&" ".repeat(indent));
        let mut tokens = fence
            .children_with_tokens()
            .skip_while(|element| element.kind() != SyntaxKind::VITEPRESS_CONTAINER_FENCE)
            .take_while(|element| element.kind() != SyntaxKind::NEWLINE);
        let marker = tokens
            .next()
            .and_then(|element| element.into_token())
            .expect("VitePress container fence");
        let len = marker.text().len().max(minimum_len);
        self.output.extend(std::iter::repeat_n(':', len));
        for element in tokens {
            match element {
                rowan::NodeOrToken::Token(token) => self.output.push_str(token.text()),
                rowan::NodeOrToken::Node(node) => self.output.push_str(&node.text().to_string()),
            }
        }
        self.output.push('\n');
        self.consecutive_blank_lines = 0;
        len
    }
}
