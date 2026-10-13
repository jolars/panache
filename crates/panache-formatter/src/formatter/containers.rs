use crate::config::WrapMode;
use crate::formatter::Formatter;
use crate::formatter::paragraphs::trim_prose_indent;
use crate::formatter::preserve::preserve_lines_unprefixed_normalized;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

impl Formatter {
    pub(super) fn format_admonition(&mut self, node: &SyntaxNode, indent: usize) {
        let mut marker = String::new();
        let mut type_str = String::new();
        let mut title_str = String::new();
        let mut body = Vec::new();

        for element in node.children_with_tokens() {
            match element {
                NodeOrToken::Token(token) => match token.kind() {
                    SyntaxKind::ADMONITION_MARKER => marker.push_str(token.text()),
                    SyntaxKind::ADMONITION_TYPE => type_str.push_str(token.text()),
                    SyntaxKind::ADMONITION_TITLE => title_str.push_str(token.text()),
                    _ => {}
                },
                NodeOrToken::Node(child) => body.push(child),
            }
        }

        self.output.push_str(&" ".repeat(indent));
        self.output.push_str(marker.trim());
        let normalized_type = type_str.split_whitespace().collect::<Vec<_>>().join(" ");
        if !normalized_type.is_empty() {
            self.output.push(' ');
            self.output.push_str(&normalized_type);
        }
        if !title_str.trim().is_empty() {
            self.output.push(' ');
            self.output.push_str(title_str.trim());
        }
        self.output.push('\n');

        let child_indent = indent + 4;
        let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
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

        let mut previous_was_blank = false;
        for child in &body[leading..end] {
            match child.kind() {
                SyntaxKind::BLANK_LINE => {
                    if !previous_was_blank {
                        self.output.push('\n');
                        previous_was_blank = true;
                    }
                    continue;
                }
                SyntaxKind::PARAGRAPH => {
                    let para_start = self.output.len();
                    let available_width = self.config.line_width.saturating_sub(child_indent);
                    let lines = match wrap_mode {
                        WrapMode::Preserve => {
                            preserve_lines_unprefixed_normalized(child, &self.config)
                                .iter()
                                .map(|line| trim_prose_indent(child, line).to_string())
                                .collect()
                        }
                        WrapMode::Reflow => {
                            self.wrapped_lines_for_paragraph(child, available_width)
                        }
                        WrapMode::Sentence => self.sentence_lines_for_paragraph(child),
                        WrapMode::Semantic => self.semantic_lines_for_paragraph(child),
                    };
                    for line in lines {
                        self.output.push_str(&" ".repeat(child_indent));
                        self.output.push_str(trim_prose_indent(child, &line));
                        self.output.push('\n');
                    }
                    self.guard_definition_marker_start(para_start, child_indent);
                }
                SyntaxKind::CODE_BLOCK => self.format_indented_code_block(child, child_indent),
                _ => self.format_node_sync(child, child_indent),
            }
            previous_was_blank = false;
        }

        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        self.consecutive_blank_lines = 0;
    }

    pub(super) fn format_footnote_definition(&mut self, node: &SyntaxNode, indent: usize) {
        let mut marker = String::new();
        let mut children = Vec::new();
        for element in node.children_with_tokens() {
            match element {
                NodeOrToken::Token(token)
                    if matches!(
                        token.kind(),
                        SyntaxKind::FOOTNOTE_REFERENCE
                            | SyntaxKind::FOOTNOTE_LABEL_START
                            | SyntaxKind::FOOTNOTE_LABEL_ID
                            | SyntaxKind::FOOTNOTE_LABEL_END
                            | SyntaxKind::FOOTNOTE_LABEL_COLON
                    ) =>
                {
                    marker.push_str(token.text());
                }
                NodeOrToken::Node(child) => children.push(child),
                _ => {}
            }
        }

        self.output.push_str(&" ".repeat(indent));
        self.output.push_str(marker.trim_end());
        let child_indent = indent + 4;
        let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
        let mut first = true;
        let mut pending_blank_lines = 0usize;

        for child in &children {
            if child.kind() == SyntaxKind::BLANK_LINE {
                pending_blank_lines = pending_blank_lines.saturating_add(1);
                continue;
            }
            if !first && pending_blank_lines > 0 && !self.output.ends_with("\n\n") {
                self.output.push('\n');
            }
            pending_blank_lines = 0;

            if first {
                first = false;
                if child.kind() == SyntaxKind::PARAGRAPH {
                    let first_width = self
                        .config
                        .line_width
                        .saturating_sub(indent + marker.len() + 1);
                    let rest_width = self.config.line_width.saturating_sub(child_indent);
                    let widths = [first_width, rest_width];
                    let lines = match wrap_mode {
                        WrapMode::Preserve => {
                            preserve_lines_unprefixed_normalized(child, &self.config)
                        }
                        WrapMode::Reflow => {
                            self.wrapped_lines_for_paragraph_with_widths(child, &widths)
                        }
                        WrapMode::Sentence => self.sentence_lines_for_paragraph(child),
                        WrapMode::Semantic => self.semantic_lines_for_paragraph(child),
                    };
                    if !lines.is_empty() {
                        self.output.push(' ');
                        self.output.push_str(trim_prose_indent(child, &lines[0]));
                        self.output.push('\n');
                        for line in lines.iter().skip(1) {
                            self.output.push_str(&" ".repeat(child_indent));
                            self.output.push_str(trim_prose_indent(child, line));
                            self.output.push('\n');
                        }
                        continue;
                    }
                } else if matches!(
                    child.kind(),
                    SyntaxKind::DEFINITION_LIST
                        | SyntaxKind::HTML_BLOCK
                        | SyntaxKind::HTML_BLOCK_RAW
                        | SyntaxKind::HTML_BLOCK_DIV
                ) {
                    self.output.push(' ');
                    self.format_node_sync(child, child_indent);
                    continue;
                }
                self.output.push('\n');
            }

            match child.kind() {
                SyntaxKind::PARAGRAPH => {
                    let start = self.output.len();
                    let available_width = self.config.line_width.saturating_sub(child_indent);
                    match wrap_mode {
                        WrapMode::Preserve => {
                            for line in preserve_lines_unprefixed_normalized(child, &self.config) {
                                self.output.push_str(&" ".repeat(child_indent));
                                self.output.push_str(trim_prose_indent(child, &line));
                                self.output.push('\n');
                            }
                        }
                        WrapMode::Reflow => {
                            for line in self.wrapped_lines_for_paragraph(child, available_width) {
                                self.output.push_str(&" ".repeat(child_indent));
                                self.output.push_str(trim_prose_indent(child, &line));
                                self.output.push('\n');
                            }
                        }
                        WrapMode::Sentence | WrapMode::Semantic => {
                            let lines = if matches!(wrap_mode, WrapMode::Semantic) {
                                self.semantic_lines_for_paragraph(child)
                            } else {
                                self.sentence_lines_for_paragraph(child)
                            };
                            for line in lines {
                                self.output.push_str(&" ".repeat(child_indent));
                                self.output.push_str(trim_prose_indent(child, &line));
                                self.output.push('\n');
                            }
                        }
                    }
                    self.guard_definition_marker_start(start, child_indent);
                }
                SyntaxKind::BLANK_LINE => self.output.push('\n'),
                SyntaxKind::CODE_BLOCK => self.format_footnote_code_block(child, child_indent),
                _ => {
                    let saved_output = self.output.clone();
                    self.output.clear();
                    self.format_node_sync(child, child_indent);
                    let formatted = self.output.clone();
                    self.output = saved_output;
                    self.output.push_str(&formatted);
                }
            }
        }

        if children.is_empty() {
            self.output.push('\n');
        }
        if node
            .next_sibling()
            .is_some_and(|next| next.kind() == SyntaxKind::FOOTNOTE_DEFINITION)
            && !self.output.ends_with("\n\n")
        {
            self.output.push('\n');
        }
    }

    fn format_footnote_code_block(&mut self, node: &SyntaxNode, indent: usize) {
        let mut lines = Vec::new();
        for child in node.children() {
            if child.kind() != SyntaxKind::CODE_CONTENT {
                continue;
            }
            let mut line = String::new();
            for element in child.children_with_tokens() {
                let NodeOrToken::Token(token) = element else {
                    continue;
                };
                match token.kind() {
                    SyntaxKind::TEXT => line.push_str(token.text()),
                    SyntaxKind::NEWLINE => {
                        lines.push(std::mem::take(&mut line));
                    }
                    _ => {}
                }
            }
            if !line.is_empty() {
                lines.push(line);
            }
        }
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        self.output.push_str(&" ".repeat(indent));
        self.output.push_str("```\n");
        for line in lines {
            if !line.is_empty() {
                self.output.push_str(&" ".repeat(indent));
                self.output.push_str(&line);
            }
            self.output.push('\n');
        }
        self.output.push_str(&" ".repeat(indent));
        self.output.push_str("```\n");
    }
}
