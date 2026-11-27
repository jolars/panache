use crate::formatter::Formatter;
use crate::syntax::{DivFenceOpen, DivInfo, FencedDiv, SyntaxKind, SyntaxNode};
use rowan::{NodeOrToken, ast::AstNode};

use super::utils::is_block_element;

impl Formatter {
    pub(super) fn format_fenced_div(&mut self, node: &SyntaxNode, indent: usize) {
        let Some(div) = FencedDiv::cast(node.clone()) else {
            self.output.push_str(&node.text().to_string());
            return;
        };
        if div.opening_fence().is_some_and(opening_has_trailing_text) {
            self.output.push_str(&node.text().to_string());
            if !self.output.ends_with('\n') {
                self.output.push('\n');
            }
            return;
        }
        let has_close = div.has_closing_fence();
        let body: Vec<_> = node
            .children()
            .filter(|child| {
                !matches!(
                    child.kind(),
                    SyntaxKind::DIV_FENCE_OPEN | SyntaxKind::DIV_INFO | SyntaxKind::DIV_FENCE_CLOSE
                )
            })
            .collect();
        let has_content = body
            .iter()
            .any(|child| child.kind() != SyntaxKind::BLANK_LINE);
        if !has_close && !has_content {
            let text = div
                .opening_fence()
                .map(|open| open.syntax().text().to_string())
                .unwrap_or_else(|| node.text().to_string());
            self.output.push_str(text.trim_end_matches('\n'));
            if !self.output.ends_with('\n') {
                self.output.push('\n');
            }
            return;
        }
        let source_colons = div
            .opening_fence()
            .map(|open| {
                open.syntax()
                    .text()
                    .to_string()
                    .trim_start()
                    .chars()
                    .take_while(|&ch| ch == ':')
                    .count()
            })
            .unwrap_or(3)
            .max(3);
        let in_list_item = node
            .ancestors()
            .any(|ancestor| ancestor.kind() == SyntaxKind::LIST_ITEM);
        let opening_colons = if in_list_item {
            source_colons
        } else {
            3 + self.fenced_div_depth * 2
        };
        self.output.push_str(&" ".repeat(indent));
        self.output.push_str(&":".repeat(opening_colons));
        if let Some(attributes) = div
            .info()
            .map(normalize_div_info)
            .filter(|text| !text.is_empty())
        {
            self.output.push(' ');
            self.output.push_str(&attributes);
        }
        self.output.push('\n');
        self.fenced_div_depth += 1;
        let leading = body
            .iter()
            .take_while(|child| child.kind() == SyntaxKind::BLANK_LINE)
            .count();
        let trailing = body
            .iter()
            .rev()
            .take_while(|child| child.kind() == SyntaxKind::BLANK_LINE)
            .count();
        let end = body.len().saturating_sub(trailing).max(leading);
        let first_kind = body
            .iter()
            .find(|child| child.kind() != SyntaxKind::BLANK_LINE)
            .map(|child| child.kind());
        let mut previous_was_blank = false;
        for (index, child) in body[leading..end].iter().enumerate() {
            if child.kind() == SyntaxKind::BLANK_LINE {
                if index < leading
                    && matches!(first_kind, Some(SyntaxKind::LIST | SyntaxKind::LIST_ITEM))
                {
                    continue;
                }
                if !previous_was_blank {
                    self.output.push('\n');
                    previous_was_blank = true;
                }
                continue;
            }
            previous_was_blank = false;
            if child.kind() == SyntaxKind::CODE_BLOCK && indent > 0 {
                self.format_indented_code_block(child, indent);
                if let Some(next) = body[leading..end].get(index + 1)
                    && (next.kind() == SyntaxKind::FENCED_DIV
                        || matches!(next.kind(), SyntaxKind::PARAGRAPH | SyntaxKind::PLAIN)
                            && next.text().to_string().trim_start().starts_with(":::"))
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }
            } else {
                self.format_node_sync(child, indent);
            }
        }
        self.fenced_div_depth -= 1;
        if body
            .iter()
            .rev()
            .find(|child| child.kind() != SyntaxKind::BLANK_LINE)
            .is_some_and(|child| {
                child.kind() == SyntaxKind::HORIZONTAL_RULE
                    && !child
                        .prev_sibling()
                        .as_ref()
                        .is_some_and(super::blocks::line_block_in_list_div)
            })
            && self.output.ends_with('\n')
            && !self.output.ends_with("\n\n")
        {
            self.output.push('\n');
        }
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        self.output.push_str(&" ".repeat(indent));
        self.output.push_str(&":".repeat(opening_colons));
        self.output.push('\n');
        self.consecutive_blank_lines = 0;
        if let Some(next) = node.next_sibling()
            && is_block_element(next.kind())
            && !self.output.ends_with("\n\n")
            && (!in_list_item
                || matches!(
                    next.kind(),
                    SyntaxKind::PARAGRAPH | SyntaxKind::PLAIN | SyntaxKind::LIST
                ))
        {
            self.output.push('\n');
            self.consecutive_blank_lines = 1;
        }
    }
}

fn normalize_div_info(info: DivInfo) -> String {
    let text = info.text();
    if !text.starts_with('{') || !text.ends_with('}') {
        return text;
    }

    // Only the gaps are whitespace tokens; quoted values retain their source bytes.
    let mut output = String::new();
    let mut pending_space = false;
    for child in info.syntax().children_with_tokens() {
        if child.kind() == SyntaxKind::WHITESPACE {
            pending_space = true;
            continue;
        }
        let text = child.to_string();
        if pending_space && output != "{" && text != "}" && !output.is_empty() {
            output.push(' ');
        }
        output.push_str(&text);
        pending_space = false;
    }
    output
}

fn opening_has_trailing_text(open: DivFenceOpen) -> bool {
    let mut saw_info = false;
    for child in open.syntax().children_with_tokens() {
        match child {
            NodeOrToken::Node(node) if node.kind() == SyntaxKind::DIV_INFO => saw_info = true,
            NodeOrToken::Token(token) if saw_info && token.kind() == SyntaxKind::TEXT => {
                let trimmed = token.text().trim();
                if !trimmed.is_empty() && !trimmed.chars().all(|ch| ch == ':') {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}
