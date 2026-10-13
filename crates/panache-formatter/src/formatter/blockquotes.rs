use crate::config::WrapMode;
use crate::formatter::Formatter;
use crate::formatter::core::BlockquoteContext;
use crate::formatter::preserve::{preserve_lines, preserve_lines_unprefixed};
use crate::formatter::utils::is_block_element;
use crate::syntax::{SyntaxKind, SyntaxNode};

impl Formatter {
    pub(super) fn format_block_quote(&mut self, node: &SyntaxNode, indent: usize) {
        log::trace!("Formatting blockquote");
        let depth = node
            .ancestors()
            .take_while(|ancestor| ancestor.kind() != SyntaxKind::LIST_ITEM)
            .filter(|ancestor| ancestor.kind() == SyntaxKind::BLOCK_QUOTE)
            .count()
            .max(1);

        let base_indent = " ".repeat(indent);
        let content_prefix = format!("{}{}", base_indent, "> ".repeat(depth)); // includes trailing space
        let blank_prefix = content_prefix.trim_end().to_string(); // no trailing space

        let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
        let blockquote_children: Vec<_> = node.children().collect();
        let saved_blockquote_context = self.blockquote_context.clone();
        self.blockquote_context = Some(BlockquoteContext {
            in_list_continuation: false,
        });

        for child in &blockquote_children {
            match child.kind() {
                SyntaxKind::BLOCK_QUOTE_MARKER => continue,

                SyntaxKind::PARAGRAPH | SyntaxKind::PLAIN => {
                    let para_start = self.output.len();
                    match wrap_mode {
                        WrapMode::Preserve => {
                            let escaped = self.config.formatter_extensions.escaped_line_breaks;
                            for line in preserve_lines_unprefixed(child, escaped) {
                                if line.is_empty() {
                                    self.output.push_str(content_prefix.trim_end());
                                } else {
                                    self.output.push_str(&content_prefix);
                                    self.output.push_str(&line);
                                }
                                self.output.push('\n');
                            }
                        }
                        WrapMode::Reflow => {
                            let width = self.config.line_width.saturating_sub(content_prefix.len());
                            let lines = self.wrapped_lines_for_paragraph(child, width);
                            for line in lines {
                                self.output.push_str(&content_prefix);
                                self.output.push_str(&line);
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
                                self.output.push_str(&content_prefix);
                                self.output.push_str(&line);
                                self.output.push('\n');
                            }
                        }
                    }
                    self.guard_definition_marker_start(para_start, 0);
                }
                SyntaxKind::ALERT => {
                    let marker = child
                        .children_with_tokens()
                        .filter_map(|item| item.into_token())
                        .find(|tok| tok.kind() == SyntaxKind::ALERT_MARKER)
                        .map(|tok| tok.text().to_string())
                        .unwrap_or_else(|| "[!NOTE]".to_string());

                    self.output.push_str(&content_prefix);
                    self.output.push_str(&marker);
                    self.output.push('\n');

                    for alert_child in child.children() {
                        match alert_child.kind() {
                            SyntaxKind::PARAGRAPH => match wrap_mode {
                                WrapMode::Preserve => {
                                    let escaped =
                                        self.config.formatter_extensions.escaped_line_breaks;
                                    for line in preserve_lines(&alert_child, escaped) {
                                        if line.is_empty() {
                                            self.output.push_str(content_prefix.trim_end());
                                        } else {
                                            self.output.push_str(&content_prefix);
                                            self.output.push_str(&line);
                                        }
                                        self.output.push('\n');
                                    }
                                }
                                WrapMode::Reflow => {
                                    let width =
                                        self.config.line_width.saturating_sub(content_prefix.len());
                                    for line in
                                        self.wrapped_lines_for_paragraph(&alert_child, width)
                                    {
                                        self.output.push_str(&content_prefix);
                                        self.output.push_str(&line);
                                        self.output.push('\n');
                                    }
                                }
                                WrapMode::Sentence | WrapMode::Semantic => {
                                    let lines = if matches!(wrap_mode, WrapMode::Semantic) {
                                        self.semantic_lines_for_paragraph(&alert_child)
                                    } else {
                                        self.sentence_lines_for_paragraph(&alert_child)
                                    };
                                    for line in lines {
                                        self.output.push_str(&content_prefix);
                                        self.output.push_str(&line);
                                        self.output.push('\n');
                                    }
                                }
                            },
                            SyntaxKind::BLANK_LINE => {
                                self.output.push_str(&blank_prefix);
                                self.output.push('\n');
                            }
                            _ => {
                                let rendered = self.render_to_buffer(
                                    &alert_child,
                                    indent,
                                    content_prefix.len(),
                                );
                                self.append_blockquote_prefixed_nested_block(
                                    &rendered,
                                    &base_indent,
                                    &content_prefix,
                                    &blank_prefix,
                                );
                            }
                        }
                    }
                }
                SyntaxKind::BLANK_LINE => {
                    self.output.push_str(&blank_prefix);
                    self.output.push('\n');
                }
                SyntaxKind::HORIZONTAL_RULE => {
                    self.output.push_str(&content_prefix);
                    let available_width =
                        self.config.line_width.saturating_sub(content_prefix.len());
                    self.output
                        .push_str(&self.horizontal_rule_text(available_width));
                    self.output.push('\n');
                }
                SyntaxKind::HEADING => {
                    let heading_text = self.format_heading(child, 0);
                    for line in heading_text.lines() {
                        self.output.push_str(&content_prefix);
                        self.output.push_str(line);
                        self.output.push('\n');
                    }
                    if let Some(next) = child.next_sibling()
                        && next.kind() != SyntaxKind::BLANK_LINE
                        && is_block_element(next.kind())
                    {
                        self.output.push_str(&blank_prefix);
                        self.output.push('\n');
                    }
                }
                SyntaxKind::LIST => {
                    let list_output = self.render_to_buffer(child, 0, content_prefix.len());

                    let ends_in_list_continuation = self.append_blockquote_prefixed_list_output(
                        &list_output,
                        &base_indent,
                        &content_prefix,
                        &blank_prefix,
                    );
                    if let Some(ctx) = self.blockquote_context.as_mut() {
                        ctx.in_list_continuation = ends_in_list_continuation;
                    }
                }
                SyntaxKind::CODE_BLOCK => {
                    let code_block_leading_indent = Self::code_block_leading_indent(child);
                    let code_output = self.render_to_buffer(child, indent, 0);

                    self.append_blockquote_prefixed_block(
                        &code_output,
                        &content_prefix,
                        &blank_prefix,
                        Some(&code_block_leading_indent),
                    );
                    if let Some(ctx) = self.blockquote_context.as_mut() {
                        ctx.in_list_continuation = false;
                    }
                }
                SyntaxKind::HTML_BLOCK
                | SyntaxKind::HTML_BLOCK_RAW
                | SyntaxKind::HTML_BLOCK_DIV => {
                    let html_output = self.render_to_buffer(child, indent, 0);

                    self.append_blockquote_prefixed_block(
                        &html_output,
                        &content_prefix,
                        &blank_prefix,
                        None,
                    );
                    if let Some(ctx) = self.blockquote_context.as_mut() {
                        ctx.in_list_continuation = false;
                    }
                }
                SyntaxKind::HUGO_SHORTCODE_BLOCK => {
                    let rendered = crate::syntax::text_without_line_prefixes(child);
                    for line in rendered.split_inclusive('\n') {
                        self.output.push_str(&content_prefix);
                        self.output.push_str(line);
                    }
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                }
                SyntaxKind::TEX_BLOCK => {
                    let tex_output = self.render_to_buffer(child, indent, 0);

                    self.append_blockquote_prefixed_block(
                        &tex_output,
                        &content_prefix,
                        &blank_prefix,
                        None,
                    );
                }
                SyntaxKind::PIPE_TABLE
                | SyntaxKind::GRID_TABLE
                | SyntaxKind::SIMPLE_TABLE
                | SyntaxKind::MULTILINE_TABLE => {
                    let table_output = self.render_to_buffer(child, 0, 0);

                    let min_indent = table_output
                        .lines()
                        .filter(|line| !line.trim().is_empty())
                        .map(|line| line.len() - line.trim_start().len())
                        .min()
                        .unwrap_or(0);
                    let dedented: String = table_output
                        .lines()
                        .map(|line| {
                            if line.trim().is_empty() {
                                String::new()
                            } else {
                                line[min_indent..].to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    self.append_blockquote_prefixed_block(
                        &dedented,
                        &content_prefix,
                        &blank_prefix,
                        None,
                    );
                    if let Some(ctx) = self.blockquote_context.as_mut() {
                        ctx.in_list_continuation = false;
                    }
                }
                SyntaxKind::LINE_BLOCK => {
                    let line_block_output = self.render_to_buffer(child, 0, 0);

                    self.append_blockquote_prefixed_block(
                        &line_block_output,
                        &content_prefix,
                        &blank_prefix,
                        None,
                    );
                    if let Some(ctx) = self.blockquote_context.as_mut() {
                        ctx.in_list_continuation = false;
                    }
                }
                SyntaxKind::DEFINITION_LIST => {
                    let def_output = self.render_to_buffer(child, indent, 0);

                    self.append_blockquote_prefixed_block(
                        &def_output,
                        &content_prefix,
                        &blank_prefix,
                        None,
                    );
                    if let Some(ctx) = self.blockquote_context.as_mut() {
                        ctx.in_list_continuation = false;
                    }
                }
                SyntaxKind::BLOCK_QUOTE => {
                    self.format_node_sync(child, indent);
                    if let Some(ctx) = self.blockquote_context.as_mut() {
                        ctx.in_list_continuation = false;
                    }
                }
                _ => {
                    let rendered = self.render_to_buffer(child, 0, content_prefix.len());

                    self.append_blockquote_prefixed_nested_block(
                        &rendered,
                        &base_indent,
                        &content_prefix,
                        &blank_prefix,
                    );
                    if let Some(ctx) = self.blockquote_context.as_mut() {
                        ctx.in_list_continuation = false;
                    }
                }
            }
        }
        self.blockquote_context = saved_blockquote_context;
    }
}
