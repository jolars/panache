use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BufferedBodyBlock {
    Term,
    Block,
}

impl<'a> Parser<'a> {
    /// Whether a definition marker on this line has to *end* the list item
    /// content above it rather than continue it.
    ///
    /// Pandoc reads a definition marker where a block may start, and inside a
    /// list item its `endline` refuses to cross a block start, so the marker
    /// is neither a soft nor a lazy continuation of the text above it. It is
    /// not a definition either: a term is a one-line block, and the block
    /// above this line is still open, so no term precedes the marker. What is
    /// left is an ordinary paragraph, placed by the marker's own indent:
    ///
    /// - `- a\n  b\n  : def` -> `BulletList [[Plain "a b", Plain ": def"]]`,
    ///   the marker reaching the item's content column and staying inside it;
    /// - `- Term\n: def` -> `BulletList [[Plain "Term"]]` + `Para ": def"`,
    ///   the dedented marker landing outside the item altogether.
    ///
    /// Only a list item guards its `endline` this way. At top level and inside
    /// a blockquote pandoc keeps `a\nb\n: def` a single paragraph, so this
    /// asks for a list item whose content is still buffered — the state the
    /// two cases above share, and the one an already-open definition list
    /// never reaches (its marker is claimed as a `Definition` upstream).
    pub(super) fn definition_marker_breaks_open_list_item_block(&self, content: &str) -> bool {
        if !self.config.extensions.definition_lists || !self.is_list_item_content_open() {
            return false;
        }
        let content_col = paragraphs::current_content_col(&self.containers);
        let Some((marker, ..)) =
            definition_lists::definition_marker_in_list_frame(content, Some(content_col))
        else {
            return false;
        };
        if marker == ':' && self.config.extensions.table_captions {
            let prefix = ContainerPrefix::from_stack(&self.containers.stack, false, self.config);
            let window = StrippedLines::new(&self.lines, self.pos, &prefix);
            return !crate::parser::blocks::tables::is_caption_followed_by_table(&window, self.pos);
        }
        true
    }

    /// What a definition marker on this line does to the block of the
    /// definition body it sits in, or `None` when the line is not such a
    /// marker.
    ///
    /// This is the definition-body analogue of
    /// [`Self::definition_marker_breaks_open_list_item_block`]. Pandoc re-reads
    /// a definition body as its own block sequence starting at the body's
    /// content column, so a marker reaching that column is read where a block
    /// may start and `endline` refuses to cross it — the marker is neither a
    /// soft nor a lazy continuation of the text above it. What is left depends
    /// on the shape of the block it stands over; see [`BufferedBodyBlock`].
    ///
    /// Reaching the content column is the whole test: an indented marker is
    /// *always* inside the body, so a body whose block is already closed (or
    /// which has no block yet) still keeps it — as text, since there is no
    /// one-line block below it to be its term. Only a marker *dedented* below
    /// the content column is a second definition of the same term
    /// (`T\n:   a\n  : def`), and that shape is deliberately left to the
    /// `Definition` arm of `DefinitionListParser::detect_prepared`.
    ///
    /// `content_indent` is the body's content column, already stripped off
    /// `stripped_content`; `content` still carries it, so the dedent test reads
    /// the original indent.
    pub(super) fn definition_marker_over_open_body_block(
        &self,
        content: &str,
        stripped_content: &str,
        content_indent: usize,
    ) -> Option<BufferedBodyBlock> {
        if !self.config.extensions.definition_lists || content_indent == 0 {
            return None;
        }
        let Some(Container::Definition {
            plain_open,
            plain_buffer,
            ..
        }) = self.containers.last()
        else {
            return None;
        };
        if !resolve_content_indent(content, content_indent).reaches_frame() {
            return None;
        }
        let (marker, ..) = definition_lists::try_parse_definition_marker(stripped_content)?;
        if marker == ':' && self.config.extensions.table_captions {
            let prefix = ContainerPrefix::from_stack(&self.containers.stack, false, self.config);
            let window = StrippedLines::new(&self.lines, self.pos, &prefix);
            if crate::parser::blocks::tables::is_caption_followed_by_table(&window, self.pos) {
                return None;
            }
        }
        let buffered = if *plain_open {
            plain_buffer.raw_text()
        } else {
            String::new()
        };
        let buffered = buffered.trim_end_matches(['\r', '\n']);
        if buffered.trim().is_empty() || buffered.contains('\n') {
            Some(BufferedBodyBlock::Block)
        } else {
            Some(BufferedBodyBlock::Term)
        }
    }

    /// Whether the blank line at `self.pos` closes a definition body block
    /// that the *next* line promotes to a term.
    ///
    /// The no-blank-line case is [`Self::definition_marker_over_open_body_block`],
    /// which reads the marker line itself. Here the marker is still ahead, so
    /// the same question is asked by lookahead: pandoc lets a term keep one
    /// blank line before its definition, so the blank does not detach the
    /// marker from the block above it, and a one-line block is a term
    /// (`T\n:   a\n\n    :   b` nests a definition list on `a`).
    ///
    /// The lookahead must run *before* the flush: promotion re-opens the
    /// buffered bytes as a `TERM`, and once they reach the builder as a
    /// `PLAIN` there is nothing left to retag.
    pub(super) fn blank_line_promotes_buffered_definition_term(&self) -> bool {
        if !self.config.extensions.definition_lists {
            return false;
        }
        let content_indent = self.content_container_indent_to_strip();
        if content_indent == 0 {
            return false;
        }
        let Some(Container::Definition {
            plain_open: true,
            plain_buffer,
            ..
        }) = self.containers.last()
        else {
            return false;
        };
        let buffered = plain_buffer.raw_text();
        let buffered = buffered.trim_end_matches(['\r', '\n']);
        if buffered.trim().is_empty() || buffered.contains('\n') {
            return false;
        }
        let prefix = ContainerPrefix::from_stack(&self.containers.stack, false, self.config);
        let stripped = StrippedLines::new(&self.lines, self.pos, &prefix);
        if definition_lists::next_line_is_definition_marker(&stripped, self.pos) != Some(0) {
            return false;
        }
        let marker_line =
            strip_n_blockquote_markers(self.lines[self.pos + 1], self.current_blockquote_depth());
        resolve_content_indent(marker_line, content_indent).reaches_frame()
    }

    pub(super) fn promote_buffered_definition_term(&mut self) {
        let Some(Container::Definition {
            plain_open,
            plain_buffer,
            ..
        }) = self.containers.stack.last_mut()
        else {
            return;
        };
        let term_line = plain_buffer.raw_text();
        plain_buffer.clear();
        *plain_open = false;

        self.builder.start_node(SyntaxKind::DEFINITION_LIST.into());
        self.containers.push(Container::DefinitionList {});
        self.builder.start_node(SyntaxKind::DEFINITION_ITEM.into());
        self.containers.push(Container::DefinitionItem {});
        definition_lists::emit_term(&mut self.builder, &term_line, None, self.config);
    }

    pub(super) fn handle_definition_list_effect(
        &mut self,
        block_match: &crate::parser::block_dispatcher::PreparedBlockMatch,
        content: &str,
        indent_to_emit: Option<&str>,
    ) -> usize {
        use crate::parser::block_dispatcher::DefinitionPrepared;

        let prepared = block_match
            .payload
            .as_ref()
            .and_then(|p| p.downcast_ref::<DefinitionPrepared>());
        let Some(prepared) = prepared else {
            return 0;
        };

        let mut extras: usize = 0;
        match prepared {
            DefinitionPrepared::Definition {
                marker_char,
                indent,
                spaces_after,
                spaces_after_cols,
                has_content,
            } => {
                self.emit_buffered_plain_if_needed();

                while matches!(self.containers.last(), Some(Container::ListItem { .. })) {
                    self.close_containers_to(self.containers.depth() - 1);
                }
                while matches!(self.containers.last(), Some(Container::List { .. })) {
                    self.close_containers_to(self.containers.depth() - 1);
                }

                if matches!(self.containers.last(), Some(Container::Definition { .. })) {
                    self.close_containers_to(self.containers.depth() - 1);
                }

                if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                    self.close_containers_to(self.containers.depth() - 1);
                }

                if definition_lists::in_definition_list(&self.containers)
                    && !matches!(
                        self.containers.last(),
                        Some(Container::DefinitionItem { .. })
                    )
                {
                    self.builder.start_node(SyntaxKind::DEFINITION_ITEM.into());
                    self.containers.push(Container::DefinitionItem {});
                }

                if !definition_lists::in_definition_list(&self.containers) {
                    self.builder.start_node(SyntaxKind::DEFINITION_LIST.into());
                    self.containers.push(Container::DefinitionList {});
                }

                if !matches!(
                    self.containers.last(),
                    Some(Container::DefinitionItem { .. })
                ) {
                    self.builder.start_node(SyntaxKind::DEFINITION_ITEM.into());
                    self.containers.push(Container::DefinitionItem {});
                }

                self.builder.start_node(SyntaxKind::DEFINITION.into());

                if let Some(indent_str) = indent_to_emit {
                    self.builder
                        .token(SyntaxKind::WHITESPACE.into(), indent_str);
                }

                let indent_bytes = byte_index_at_column(content, *indent);
                emit_definition_marker(&mut self.builder, *marker_char, &content[..indent_bytes]);
                if *spaces_after > 0 {
                    let space_start = indent_bytes + 1;
                    let space_end = space_start + *spaces_after;
                    if space_end <= content.len() {
                        self.builder.token(
                            SyntaxKind::WHITESPACE.into(),
                            &content[space_start..space_end],
                        );
                    }
                }

                if !*has_content {
                    let current_line = self.lines[self.pos];
                    let (_, newline_str) = strip_newline(current_line);
                    if !newline_str.is_empty() {
                        self.builder.token(SyntaxKind::NEWLINE.into(), newline_str);
                    }
                }

                let content_col = *indent + 1 + *spaces_after_cols;
                let content_start_bytes = indent_bytes + 1 + *spaces_after;
                let after_marker_and_spaces = content.get(content_start_bytes..).unwrap_or("");
                let mut plain_buffer = ParagraphBuffer::new();
                let mut definition_pushed = false;

                if *has_content {
                    let current_line = self.lines[self.pos];
                    let (trimmed_content, _) = strip_newline(content);

                    let content_start = content_start_bytes.min(trimmed_content.len());
                    let content_slice = &trimmed_content[content_start..];
                    let content_line = &content[content_start_bytes.min(content.len())..];

                    let (blockquote_depth, inner_blockquote_content) =
                        count_blockquote_markers(content_line);

                    let should_start_list_from_first_line = self
                        .lines
                        .get(self.pos + 1)
                        .map(|next_line| {
                            let (next_without_newline, _) = strip_newline(next_line);
                            if next_without_newline.trim().is_empty() {
                                return true;
                            }

                            let (next_indent_cols, _) = leading_indent(next_without_newline);
                            next_indent_cols >= content_col
                        })
                        .unwrap_or(true);

                    if blockquote_depth > 0 {
                        self.containers.push(Container::Definition {
                            content_col,
                            plain_open: false,
                            plain_buffer: ParagraphBuffer::new(),
                        });
                        definition_pushed = true;

                        let marker_info = parse_blockquote_marker_info(content_line);
                        for level in 0..blockquote_depth {
                            self.builder.start_node(SyntaxKind::BLOCK_QUOTE.into());
                            if let Some(info) = marker_info.get(level) {
                                blockquotes::emit_one_blockquote_marker(
                                    &mut self.builder,
                                    info.leading_spaces,
                                    info.has_trailing_space,
                                );
                            }
                            self.containers.push(Container::BlockQuote {});
                        }

                        if !inner_blockquote_content.trim().is_empty() {
                            paragraphs::start_paragraph_if_needed(
                                &mut self.containers,
                                &mut self.builder,
                            );
                            paragraphs::append_paragraph_line(
                                &mut self.containers,
                                &mut self.builder,
                                inner_blockquote_content,
                                self.config,
                            );
                        }
                    } else if let Some(marker_match) = try_parse_list_marker(
                        content_slice,
                        self.config,
                        lists::open_list_hint_at_indent(
                            &self.containers,
                            leading_indent(content_slice).0,
                        ),
                    ) && should_start_list_from_first_line
                    {
                        self.containers.push(Container::Definition {
                            content_col,
                            plain_open: false,
                            plain_buffer: ParagraphBuffer::new(),
                        });
                        definition_pushed = true;

                        let (indent_cols, indent_bytes) = leading_indent(content_line);
                        self.builder.start_node(SyntaxKind::LIST.into());
                        self.containers.push(Container::List {
                            marker: marker_match.marker.clone(),
                            base_indent_cols: indent_cols,
                            has_blank_between_items: false,
                        });

                        let list_item = ListItemEmissionInput {
                            content: content_line,
                            marker_len: marker_match.marker_len,
                            spaces_after_cols: marker_match.spaces_after_cols,
                            spaces_after_bytes: marker_match.spaces_after_bytes,
                            indent_cols,
                            indent_bytes,
                            virtual_marker_space: marker_match.virtual_marker_space,
                        };

                        let finish = if let Some(nested_marker) = is_content_nested_bullet_marker(
                            content_line,
                            marker_match.marker_len,
                            marker_match.spaces_after_bytes,
                        ) {
                            lists::add_list_item_with_nested_empty_list(
                                &mut self.containers,
                                &mut self.builder,
                                &list_item,
                                nested_marker,
                                self.config,
                            );
                            lists::ListItemFinish::Done
                        } else {
                            lists::add_list_item(
                                &mut self.containers,
                                &mut self.builder,
                                &list_item,
                                self.config,
                            )
                        };
                        extras = self.dispatch_bq_after_list_item(finish);
                    } else if let Some(fence) =
                        code_blocks::try_parse_fence_open(content_slice, self.config.dialect)
                    {
                        self.containers.push(Container::Definition {
                            content_col,
                            plain_open: false,
                            plain_buffer: ParagraphBuffer::new(),
                        });
                        definition_pushed = true;

                        let bq_depth = self.current_blockquote_depth();
                        if let Some(indent_str) = indent_to_emit {
                            self.builder
                                .token(SyntaxKind::WHITESPACE.into(), indent_str);
                        }
                        let fence_line = content[content_start..].to_string();
                        let prefix = ContainerPrefix::from_scalars(
                            bq_depth,
                            0,
                            bq_depth > 0,
                            content_col,
                            false,
                            self.config.dialect,
                        );
                        let window = StrippedLines::new(&self.lines, self.pos, &prefix);
                        let new_pos = if self.config.extensions.tex_math_gfm
                            && code_blocks::is_gfm_math_fence(&fence)
                        {
                            code_blocks::parse_fenced_math_block(
                                &mut self.builder,
                                &window,
                                fence,
                                Some(&fence_line),
                                self.config.dialect,
                            )
                        } else {
                            code_blocks::parse_fenced_code_block(
                                &mut self.builder,
                                &window,
                                fence,
                                Some(&fence_line),
                                &self.diagnostics,
                                self.config.flavor,
                            )
                        };
                        extras = new_pos.saturating_sub(self.pos).saturating_sub(1);
                    } else if let Some(html_extras) =
                        self.try_dispatch_definition_html_block(content_line, content_col)
                    {
                        self.containers.push(Container::Definition {
                            content_col,
                            plain_open: false,
                            plain_buffer: ParagraphBuffer::new(),
                        });
                        definition_pushed = true;
                        extras = html_extras;
                    } else {
                        let (_, newline_str) = strip_newline(current_line);
                        let (content_without_newline, _) = strip_newline(after_marker_and_spaces);
                        plain_buffer.push_text(content_without_newline);
                        plain_buffer.push_text(newline_str);
                    }
                }

                if !definition_pushed {
                    self.containers.push(Container::Definition {
                        content_col,
                        plain_open: *has_content,
                        plain_buffer,
                    });
                    extras = self
                        .try_buffer_definition_hugo_continuations(after_marker_and_spaces)
                        .unwrap_or(0);
                }
            }
            DefinitionPrepared::Term { blank_count } => {
                self.emit_buffered_plain_if_needed();

                if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                    self.close_containers_to(self.containers.depth() - 1);
                }

                if !definition_lists::in_definition_list(&self.containers) {
                    self.builder.start_node(SyntaxKind::DEFINITION_LIST.into());
                    self.containers.push(Container::DefinitionList {});
                }

                while matches!(
                    self.containers.last(),
                    Some(Container::Definition { .. }) | Some(Container::DefinitionItem { .. })
                ) {
                    self.close_containers_to(self.containers.depth() - 1);
                }

                self.builder.start_node(SyntaxKind::DEFINITION_ITEM.into());
                self.containers.push(Container::DefinitionItem {});

                emit_term(&mut self.builder, content, indent_to_emit, self.config);
                self.emit_term_lookahead_blank_lines(*blank_count);
                extras = *blank_count;
            }
        };
        extras
    }

    pub(super) fn try_buffer_definition_hugo_continuations(
        &mut self,
        first: &str,
    ) -> Option<usize> {
        if !self.config.extensions.hugo_shortcodes || !first.contains("{{") {
            return None;
        }
        // Definition prose bypasses ordinary dispatch. Hold the entire call
        // before its continuation lines can become Markdown blocks.
        let prefix = ContainerPrefix::from_stack(&self.containers.stack, false, self.config);
        let lines = StrippedLines::new(&self.lines, self.pos, &prefix)
            .with_hugo_lookahead(&self.hugo_lookahead);
        let prepared = super::super::blocks::hugo::prepare(&lines, true, Some(first))?;
        let Some(Container::Definition { plain_buffer, .. }) = self.containers.stack.last_mut()
        else {
            return None;
        };
        for i in 1..prepared.line_count {
            let raw = self.lines[self.pos + i];
            let tail = lines.peek_prefix_at(self.pos + i);
            plain_buffer.push_indent(&raw[..raw.len() - tail.len()]);
            plain_buffer.push_text(tail);
        }
        Some(prepared.line_count - 1)
    }

    /// Emit the blank lines a definition-list term look-ahead skipped over.
    ///
    /// The look-ahead runs on container-stripped lines, so inside a blockquote
    /// a "blank" line still carries its `>` markers in the source. Split them
    /// off as `BLOCK_QUOTE_MARKER` tokens the way the main blank-line path
    /// does, instead of burying them in the `BLANK_LINE` token.
    /// Open a definition list whose term is the list item's own first content
    /// line, i.e. the text on the list-marker line.
    ///
    /// `- Term\n  : def\n` is `BulletList [[DefinitionList [(Term, [[Plain
    /// def]])]]]` for pandoc: it reparses item contents as a fresh block sequence,
    /// so the term is found there rather than by the block dispatcher, which never
    /// sees the marker line (`ListParser` claims it first). This is the list
    /// analogue of the footnote branch in `handle_footnote_open_effect`.
    ///
    /// Declines whenever pandoc's reader would reach a block before
    /// `definitionList`: an ATX heading or a thematic break on the marker line
    /// keeps it, and so does more than one buffered line — a term is always a
    /// one-line block. An empty buffer means the item's content went down the
    /// blockquote-dispatch path, which has its own definition handling.
    ///
    /// Returns the number of source lines consumed beyond the marker line.
    pub(super) fn maybe_open_definition_term_in_new_list_item(&mut self) -> Option<usize> {
        if !self.config.extensions.definition_lists {
            return None;
        }
        let Some(Container::ListItem {
            content_col,
            buffer,
            marker_only,
            ..
        }) = self.containers.stack.last()
        else {
            return None;
        };
        if *marker_only {
            return None;
        }
        let content_col = *content_col;
        let text = buffer.sole_text_segment()?.to_string();

        let mut lines_it = text.split_inclusive('\n');
        let first_line = lines_it.next()?;
        if lines_it.next().is_some() {
            return None;
        }
        let (detect, _) = strip_newline(first_line);
        if detect.trim().is_empty()
            || try_parse_atx_heading(detect).is_some()
            || try_parse_horizontal_rule(detect).is_some()
            || html_blocks::try_parse_html_block_start(detect, false).is_some()
        {
            return None;
        }

        let prefix = ContainerPrefix::from_stack(&self.containers.stack, false, self.config)
            .without_innermost_list_advance();
        let window = StrippedLines::new(&self.lines, self.pos, &prefix);
        let blank_count = first_content_line_term_lookahead(
            &window,
            self.pos,
            content_col,
            self.config.extensions.table_captions,
        )?;

        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut() {
            buffer.clear();
        }
        self.builder.start_node(SyntaxKind::DEFINITION_LIST.into());
        self.containers.push(Container::DefinitionList {});
        self.builder.start_node(SyntaxKind::DEFINITION_ITEM.into());
        self.containers.push(Container::DefinitionItem {});
        emit_term(&mut self.builder, &text, None, self.config);
        self.emit_term_lookahead_blank_lines(blank_count);
        Some(blank_count)
    }

    pub(super) fn emit_term_lookahead_blank_lines(&mut self, blank_count: usize) {
        let bq_depth = self.current_blockquote_depth();
        for i in 0..blank_count {
            let blank_pos = self.pos + 1 + i;
            if blank_pos >= self.lines.len() {
                continue;
            }
            let blank_line = self.lines[blank_pos];
            let (line_depth, _) = blockquotes::count_blockquote_markers(blank_line);
            let depth = line_depth.min(bq_depth);
            let content = if depth > 0 {
                let marker_info = parse_blockquote_marker_info(blank_line);
                self.emit_blockquote_markers(&marker_info, depth);
                strip_n_blockquote_markers(blank_line, depth)
            } else {
                blank_line
            };
            self.builder.start_node(SyntaxKind::BLANK_LINE.into());
            self.builder.token(SyntaxKind::BLANK_LINE.into(), content);
            self.builder.finish_node();
        }
    }

    /// Close nested definition-list levels that a dedented definition marker
    /// on the current line has left.
    ///
    /// A marker is a block start, so pandoc reads it in the frame of whichever
    /// body it lands in. `T\n:   a\n    : def\n  : sibling` puts `: sibling`
    /// back on `T`: column 2 does not reach the frame of the list nested in
    /// `T`'s body, which starts at the body's own content column. Plain text
    /// is *not* a block start and stays a lazy continuation of the innermost
    /// body, so only a marker line unwinds anything.
    ///
    /// Runs before the content-container strip, since the levels it closes are
    /// what that strip is measured from. A single, unnested definition list
    /// cannot be dedented out of this way — its own marker arm handles a
    /// sibling definition — so this needs two levels to do anything.
    pub(super) fn close_dedented_definition_lists(&mut self, content: &str) {
        if !self.config.extensions.definition_lists
            || self
                .containers
                .stack
                .iter()
                .filter(|c| matches!(c, Container::DefinitionList { .. }))
                .count()
                < 2
        {
            return;
        }

        let (without_newline, _) = strip_newline(content);
        let (indent_cols, _) = leading_indent(without_newline);

        let mut content_frame = 0usize;
        let mut item_frame = 0usize;
        let mut levels: Vec<(usize, usize)> = Vec::new();
        for (idx, container) in self.containers.stack.iter().enumerate() {
            match container {
                Container::FootnoteDefinition { content_col }
                | Container::Admonition { content_col }
                | Container::Definition { content_col, .. } => {
                    content_frame += *content_col;
                    item_frame = 0;
                }
                Container::ListItem { content_col, .. } => item_frame = *content_col,
                Container::DefinitionList { .. } => levels.push((idx, content_frame + item_frame)),
                _ => {}
            }
        }

        let target = levels.iter().rposition(|(_, frame)| {
            indent_cols >= *frame
                && definition_lists::try_parse_definition_marker(
                    &without_newline[byte_index_at_column(without_newline, *frame)..],
                )
                .is_some()
        });

        if let Some(target) = target
            && let Some((first_closed, _)) = levels.get(target + 1)
        {
            self.close_containers_to(*first_closed);
        }
    }
}

pub(super) fn emit_definition_plain_or_heading(
    builder: &mut GreenNodeBuilder<'static>,
    buffer: &ParagraphBuffer,
    config: &ParserOptions,
    suppress_footnote_refs: bool,
) {
    if let Some(tail) = buffer.try_emit_hugo_heading(builder, config) {
        if !tail.is_empty() {
            builder.start_node(SyntaxKind::PLAIN.into());
            tail.emit_with_inlines(builder, config, suppress_footnote_refs);
            builder.finish_node();
        }
        return;
    }
    let text = buffer.raw_text();
    let text = text.as_str();
    let line_without_newline = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'));
    if let Some(line) = line_without_newline
        && !line.contains('\n')
        && !line.contains('\r')
        && let Some(level) = try_parse_atx_heading(line)
    {
        emit_atx_heading(builder, text, level, config);
        return;
    }

    if let Some(first_nl) = text.find('\n') {
        let first_line = &text[..first_nl];
        let after_first = &text[first_nl + 1..];
        if !after_first.is_empty()
            && let Some(level) = try_parse_atx_heading(first_line)
        {
            let heading_bytes = &text[..first_nl + 1];
            emit_atx_heading(builder, heading_bytes, level, config);
            builder.start_node(SyntaxKind::PLAIN.into());
            buffer.split_at_raw(first_nl + 1).emit_with_inlines(
                builder,
                config,
                suppress_footnote_refs,
            );
            builder.finish_node();
            return;
        }
    }

    let block_kind = if paragraph_is_standalone_image(text, config) {
        SyntaxKind::FIGURE
    } else {
        SyntaxKind::PLAIN
    };
    builder.start_node(block_kind.into());
    buffer.emit_with_inlines(builder, config, suppress_footnote_refs);
    builder.finish_node();
}

/// Look ahead from `pos+1` past at most one blank line for a definition marker
/// line at `content_col` indent. Returns the blank-line count consumed before
/// the marker, or `None` if no marker is found at the next non-blank line.
///
/// The one-blank limit is the term rule
/// [`next_line_is_definition_marker`](definition_lists::next_line_is_definition_marker)
/// applies, in the container's own frame: `- T\n\n\n  : b` is a bullet item
/// holding two paragraphs, not a term and its definition.
///
/// Used by `handle_footnote_open_effect` and
/// `maybe_open_definition_term_in_new_list_item` to decide whether a
/// container's *first content line* should open a definition-list term:
/// pandoc reparses container contents as a fresh block sequence, so it treats
/// `[^1]: Term\n\n    :   Definition\n` as a `Note [DefinitionList ...]` and
/// `- Term\n  : def\n` as a `BulletList [[DefinitionList ...]]`, not as a
/// paragraph followed by a separate def list with no term.
///
/// `lines` is absolute-indexed. Pass a [`StrippedLines`] when a blockquote
/// prefix is open, so `> - Term` / `>   : def` is measured on the quote's
/// content rather than on its markers.
pub(super) fn first_content_line_term_lookahead(
    lines: &(impl LineView + ?Sized),
    pos: usize,
    content_col: usize,
    table_captions_enabled: bool,
) -> Option<usize> {
    let mut check_pos = pos + 1;
    let mut blank_count = 0;
    while check_pos < lines.line_count() {
        let line = lines.line(check_pos);
        let (trimmed, _) = strip_newline(line);
        if trimmed.trim().is_empty() {
            blank_count += 1;
            if blank_count > definition_lists::MAX_BLANKS_BEFORE_DEFINITION {
                return None;
            }
            check_pos += 1;
            continue;
        }
        if !resolve_content_indent(trimmed, content_col).reaches_frame() {
            return None;
        }
        let strip_bytes = byte_index_at_column(trimmed, content_col);
        if strip_bytes > trimmed.len() {
            return None;
        }
        let stripped = &trimmed[strip_bytes..];
        if let Some((marker, ..)) = definition_lists::try_parse_definition_marker(stripped) {
            if marker == ':' && table_captions_enabled {
                let view = crate::parser::blocks::tables::ContentColStripView {
                    inner: lines,
                    content_col,
                };
                if crate::parser::blocks::tables::is_caption_followed_by_table(&view, check_pos) {
                    return None;
                }
            }
            return Some(blank_count);
        }
        return None;
    }
    None
}
