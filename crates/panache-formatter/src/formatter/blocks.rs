use crate::config::{SmartPunctuation, WrapMode};
use crate::formatter::preserve::{preserve_lines_normalized, preserve_lines_unprefixed_normalized};
use crate::formatter::smart::{smart_node_text, smart_text_map};
use crate::formatter::utils::is_block_element;
use crate::formatter::{FormatContext, Formatter, paragraphs};
use crate::syntax::{AstNode, LineBlock, LineBlockLine, SyntaxKind, SyntaxNode};
use panache_parser::parser::blocks::tables::try_parse_pipe_separator;
use panache_parser::parser::inlines::core::parse_inline_text_recursive;
use rowan::{GreenNodeBuilder, NodeOrToken};

pub(super) fn logical_line_block_lines(block: &LineBlock) -> Vec<Vec<LineBlockLine>> {
    let mut lines: Vec<Vec<LineBlockLine>> = Vec::new();
    for line in block.syntax().children().filter_map(LineBlockLine::cast) {
        let has_marker = line
            .syntax()
            .children_with_tokens()
            .any(|elem| elem.kind() == SyntaxKind::LINE_BLOCK_MARKER);
        match lines.last_mut() {
            Some(previous) if !has_marker => previous.push(line),
            _ => lines.push(vec![line]),
        }
    }
    lines
}

fn line_block_content(lines: &[LineBlockLine], config: &FormatContext) -> String {
    let mut content = String::new();
    for (i, line) in lines.iter().enumerate() {
        let mut part = String::new();
        let mut past_prefix = false;
        for elem in line.syntax().children_with_tokens() {
            let kind = elem.kind();
            if !past_prefix && matches!(kind, SyntaxKind::WHITESPACE | SyntaxKind::LINE_PREFIX) {
                continue;
            }
            past_prefix = true;
            match kind {
                SyntaxKind::LINE_BLOCK_MARKER => continue,
                SyntaxKind::NEWLINE => break,
                _ => match &elem {
                    NodeOrToken::Token(t) => part.push_str(t.text()),
                    NodeOrToken::Node(n) => part.push_str(&n.text().to_string()),
                },
            }
        }
        if i == 0 {
            content.push_str(part.trim_end());
        } else if !part.trim().is_empty() {
            if !content.is_empty() {
                content.push(' ');
            }
            content.push_str(part.trim());
        }
    }
    if config.smart_punctuation == SmartPunctuation::Preserve {
        return content;
    }

    // The source CST parses each physical line separately. Resolve inlines
    // across the joined logical line so continued literals stay protected.
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(SyntaxKind::PLAIN.into());
    parse_inline_text_recursive(&mut builder, &content, &config.parser_options(), false);
    builder.finish_node();
    let node = SyntaxNode::new_root(builder.finish());
    let mut context = config.clone();
    context.punctuation = std::sync::Arc::new(smart_text_map(&node, config));
    smart_node_text(&node, &context)
}

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
        log::trace!(
            "Formatting paragraph, text length: {:?}",
            node.text_range().len()
        );
        let paragraph_indent = " ".repeat(indent);

        // Reflowing would break grid table continuations, bookdown text references,
        // and LaTeX in fenced divs, so keep their source lines.
        let keep_line_breaks = self.is_grid_table_continuation_paragraph(node)
            || (self.config.formatter_extensions.bookdown_references
                && paragraphs::is_bookdown_text_reference(node))
            || (self.fenced_div_depth > 0 && self.contains_latex_command(node));
        if keep_line_breaks {
            let text = smart_node_text(node, &self.config);
            if indent > 0 {
                for (i, line) in text.lines().enumerate() {
                    if i > 0 {
                        self.output.push('\n');
                    }
                    self.output.push_str(&paragraph_indent);
                    self.output.push_str(line.trim_start());
                }
            } else {
                self.output.push_str(&text);
            }
            if !self.output.ends_with('\n') {
                self.output.push('\n');
            }
            return;
        }
        let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
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

        let block = LineBlock::cast(node.clone()).expect("line block");
        for lines in logical_line_block_lines(&block) {
            let mut line = line_block_content(&lines, &self.config);
            if self.config.smart_punctuation == SmartPunctuation::Ascii
                && self.config.formatter_extensions.smart
                && self.config.parser_extensions.pipe_tables
                && try_parse_pipe_separator(&format!("| {line}")).is_some()
            {
                // A dash-only logical line can become a table delimiter after
                // continuations are joined, so keep its authored punctuation.
                let mut preserve = self.config.clone();
                preserve.smart_punctuation = SmartPunctuation::Preserve;
                line = line_block_content(&lines, &preserve);
            }
            self.output.push_str(&line_indent);
            if line.trim().is_empty() {
                self.output.push('|');
            } else {
                self.output.push_str("| ");
                self.output.push_str(&line);
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
