use crate::config::WrapMode;
use crate::formatter::preserve::{preserve_lines_normalized, preserve_lines_unprefixed_normalized};
use crate::formatter::smart::normalize_smart_punctuation;
use crate::formatter::utils::is_block_element;
use crate::formatter::{Formatter, paragraphs};
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

impl Formatter {
    pub(super) fn format_paragraph(&mut self, node: &SyntaxNode, indent: usize) {
        let line_width = self.config.line_width;
        if indent == 0
            && node
                .prev_sibling()
                .is_some_and(|prev| prev.kind() == SyntaxKind::LIST)
            && self.output.ends_with('\n')
            && !self.output.ends_with("\n\n")
        {
            self.output.push('\n');
        }

        let para_start = self.output.len();
        let text = node.text().to_string();
        log::trace!("Formatting paragraph, text length: {}", text.len());
        let paragraph_indent = " ".repeat(indent);

        if self.is_grid_table_continuation_paragraph(node) {
            if indent > 0 {
                for (i, line) in text.lines().enumerate() {
                    if i > 0 {
                        self.output.push('\n');
                    }
                    self.output.push_str(&paragraph_indent);
                    self.output.push_str(
                        normalize_smart_punctuation(
                            line.trim_start(),
                            self.config.formatter_extensions.smart,
                            self.config.formatter_extensions.smart_quotes,
                        )
                        .as_ref(),
                    );
                }
            } else {
                self.output.push_str(
                    normalize_smart_punctuation(
                        &text,
                        self.config.formatter_extensions.smart,
                        self.config.formatter_extensions.smart_quotes,
                    )
                    .as_ref(),
                );
            }
            if !self.output.ends_with('\n') {
                self.output.push('\n');
            }
            return;
        }

        if self.config.formatter_extensions.bookdown_references
            && paragraphs::is_bookdown_text_reference(node)
        {
            if indent > 0 {
                for (i, line) in text.lines().enumerate() {
                    if i > 0 {
                        self.output.push('\n');
                    }
                    self.output.push_str(&paragraph_indent);
                    self.output.push_str(
                        normalize_smart_punctuation(
                            line.trim_start(),
                            self.config.formatter_extensions.smart,
                            self.config.formatter_extensions.smart_quotes,
                        )
                        .as_ref(),
                    );
                }
            } else {
                self.output.push_str(
                    normalize_smart_punctuation(
                        &text,
                        self.config.formatter_extensions.smart,
                        self.config.formatter_extensions.smart_quotes,
                    )
                    .as_ref(),
                );
            }
            if !self.output.ends_with('\n') {
                self.output.push('\n');
            }
            return;
        }

        let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
        let preserve_newlines_for_latex =
            self.fenced_div_depth > 0 && self.contains_latex_command(node);
        if preserve_newlines_for_latex && self.fenced_div_depth > 0 {
            if indent > 0 {
                for (i, line) in text.lines().enumerate() {
                    if i > 0 {
                        self.output.push('\n');
                    }
                    self.output.push_str(&paragraph_indent);
                    self.output.push_str(
                        normalize_smart_punctuation(
                            line.trim_start(),
                            self.config.formatter_extensions.smart,
                            self.config.formatter_extensions.smart_quotes,
                        )
                        .as_ref(),
                    );
                }
            } else {
                self.output.push_str(
                    normalize_smart_punctuation(
                        &text,
                        self.config.formatter_extensions.smart,
                        self.config.formatter_extensions.smart_quotes,
                    )
                    .as_ref(),
                );
            }
            if !self.output.ends_with('\n') {
                self.output.push('\n');
            }
            return;
        }
        log::trace!(
            "Paragraph wrap mode: {:?}, line_width: {}",
            wrap_mode,
            line_width
        );
        match wrap_mode {
            WrapMode::Preserve => {
                log::trace!("Preserving paragraph line breaks");
                for (i, line) in preserve_lines_normalized(node, &self.config)
                    .iter()
                    .enumerate()
                {
                    if i > 0 {
                        self.output.push('\n');
                    }
                    if indent > 0 {
                        self.output.push_str(&paragraph_indent);
                    }
                    self.output.push_str(if indent > 0 {
                        paragraphs::trim_prose_indent(node, line)
                    } else {
                        line
                    });
                }
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
            }
            WrapMode::Reflow => {
                log::trace!("Reflowing paragraph to {} width", line_width);
                let lines = self.wrapped_lines_for_paragraph(node, line_width);

                for (i, line) in lines.iter().enumerate() {
                    if i > 0 {
                        self.output.push('\n');
                    }
                    if indent > 0 {
                        self.output.push_str(&paragraph_indent);
                    }
                    self.output.push_str(line);
                }
            }
            WrapMode::Sentence | WrapMode::Semantic => {
                let lines = if matches!(wrap_mode, WrapMode::Semantic) {
                    log::trace!("Wrapping paragraph by semantic line breaks");
                    self.semantic_lines_for_paragraph(node)
                } else {
                    log::trace!("Wrapping paragraph by sentence");
                    self.sentence_lines_for_paragraph(node)
                };

                for (i, line) in lines.iter().enumerate() {
                    if i > 0 {
                        self.output.push('\n');
                    }
                    if indent > 0 {
                        self.output.push_str(&paragraph_indent);
                    }
                    self.output.push_str(line);
                }
            }
        }

        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }

        self.guard_dash_block_marker(para_start, node, indent);
        self.guard_definition_marker_start(para_start, indent);
    }

    pub(super) fn format_figure(&mut self, node: &SyntaxNode, indent: usize) {
        log::trace!("Formatting figure");
        let text = self.format_inline_node(node);
        let trimmed = text.trim();
        if indent > 0 && !self.output.ends_with(":   ") {
            self.output.push_str(&" ".repeat(indent));
        }
        self.output.push_str(trimmed);
        self.output.push('\n');
    }

    pub(super) fn format_plain(&mut self, node: &SyntaxNode, indent: usize) {
        let line_width = self.config.line_width;
        let text = node.text().to_string();
        log::trace!("Formatting Plain block, text length: {}", text.len());

        let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
        let needs_indent = indent > 0
            && (self.output.ends_with('\n') || self.output.is_empty())
            && !self.output.ends_with(":   ");
        match wrap_mode {
            WrapMode::Preserve => {
                let in_definition = self.output.ends_with(":   ");
                let lines = if in_definition {
                    // The normalized marker sets a new content column, so
                    // rebuild container indentation around opaque payloads.
                    preserve_lines_unprefixed_normalized(node, &self.config)
                } else {
                    preserve_lines_normalized(node, &self.config)
                };
                for (i, line) in lines.iter().enumerate() {
                    if needs_indent {
                        self.output.push_str(&" ".repeat(indent));
                    } else if i > 0 {
                        self.output.push('\n');
                        if in_definition {
                            self.output.push_str(&" ".repeat(indent));
                        }
                    }
                    self.output
                        .push_str(if needs_indent || (in_definition && i > 0) {
                            paragraphs::trim_prose_indent(node, line)
                        } else {
                            line
                        });
                    if needs_indent {
                        self.output.push('\n');
                    }
                }
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
            }
            WrapMode::Reflow => {
                log::trace!("Reflowing Plain block to {} width", line_width);
                let in_definition = self.output.ends_with(":   ");
                let preserve_ambiguous_definition_emphasis =
                    in_definition && text.contains(r"\|*") && text.contains(".*");
                let lines = if in_definition {
                    if preserve_ambiguous_definition_emphasis {
                        text.lines().map(ToString::to_string).collect()
                    } else {
                        let marker_len = ":   ".len();
                        let marker_indent = indent.saturating_sub(4);
                        let first_line_space =
                            line_width.saturating_sub(marker_indent + marker_len);
                        let continuation_width = line_width.saturating_sub(indent);
                        let widths = [first_line_space, continuation_width];
                        self.wrapped_lines_for_paragraph_with_widths(node, &widths)
                    }
                } else {
                    self.wrapped_lines_for_paragraph(node, line_width)
                };

                for (i, line) in lines.iter().enumerate() {
                    if i > 0 {
                        self.output.push('\n');
                        self.output.push_str(&" ".repeat(indent));
                    } else if needs_indent {
                        self.output.push_str(&" ".repeat(indent));
                    }
                    let rendered = if i > 0 && indent > 0 {
                        paragraphs::trim_prose_indent(node, line)
                    } else {
                        line.as_str()
                    };
                    self.output.push_str(rendered);
                }

                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
            }
            WrapMode::Sentence | WrapMode::Semantic => {
                let in_definition = self.output.ends_with(":   ");
                let preserve_ambiguous_definition_emphasis =
                    in_definition && text.contains(r"\|*") && text.contains(".*");
                let lines = if preserve_ambiguous_definition_emphasis {
                    text.lines().map(ToString::to_string).collect()
                } else if matches!(wrap_mode, WrapMode::Semantic) {
                    log::trace!("Wrapping Plain block by semantic line breaks");
                    self.semantic_lines_for_paragraph(node)
                } else {
                    log::trace!("Wrapping Plain block by sentence");
                    self.sentence_lines_for_paragraph(node)
                };

                for (i, line) in lines.iter().enumerate() {
                    if i > 0 {
                        self.output.push('\n');
                        self.output.push_str(&" ".repeat(indent));
                    } else if needs_indent {
                        self.output.push_str(&" ".repeat(indent));
                    }
                    let rendered = if i > 0 && indent > 0 {
                        paragraphs::trim_prose_indent(node, line)
                    } else {
                        line.as_str()
                    };
                    self.output.push_str(rendered);
                }

                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
            }
        }
    }

    pub(super) fn format_line_block(&mut self, node: &SyntaxNode, indent: usize) {
        log::trace!("Formatting line block");
        if !self.output.is_empty() && !self.output.ends_with("\n\n") {
            self.output.push('\n');
        }

        let line_indent = " ".repeat(indent);

        let mut rendered: Vec<String> = Vec::new();
        for child in node.children() {
            if child.kind() != SyntaxKind::LINE_BLOCK_LINE {
                continue;
            }
            let mut content = String::new();
            let mut has_marker = false;
            let mut past_prefix = false;
            for elem in child.children_with_tokens() {
                let kind = elem.kind();
                if !past_prefix && matches!(kind, SyntaxKind::WHITESPACE | SyntaxKind::LINE_PREFIX)
                {
                    continue;
                }
                past_prefix = true;
                match kind {
                    SyntaxKind::LINE_BLOCK_MARKER => {
                        has_marker = true;
                        continue;
                    }
                    SyntaxKind::NEWLINE => break,
                    _ => match &elem {
                        NodeOrToken::Token(t) => content.push_str(t.text()),
                        NodeOrToken::Node(n) => content.push_str(&n.text().to_string()),
                    },
                }
            }
            match rendered.last_mut() {
                Some(previous) if !has_marker => {
                    let continuation = content.trim();
                    if !continuation.is_empty() {
                        if !previous.is_empty() {
                            previous.push(' ');
                        }
                        previous.push_str(continuation);
                    }
                }
                _ => rendered.push(content.trim_end().to_string()),
            }
        }

        for line in &rendered {
            self.output.push_str(&line_indent);
            if line.trim().is_empty() {
                self.output.push('|');
            } else {
                self.output.push_str("| ");
                self.output.push_str(line);
            }
            self.output.push('\n');
        }

        if let Some(next) = node.next_sibling()
            && is_block_element(next.kind())
            && !self.output.ends_with("\n\n")
        {
            self.output.push('\n');
        }
    }
}
