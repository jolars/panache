use super::*;

impl<'a> Parser<'a> {
    /// Close enclosing list items (and their containing list) whose
    /// `content_col` exceeds the given indent. Under CommonMark this covers
    /// every interrupting block (HR, ATX heading, fenced code, ...): per §5.2
    /// a line shallower than the item's content column cannot continue the
    /// item, so the item and the surrounding list close before the new block
    /// is emitted at the outer level.
    ///
    /// Pandoc uses the same close but for fenced code *only* — its
    /// `rawListItem` stops collecting at a line `codeBlockFenced` would claim
    /// while still swallowing an under-indented heading or thematic break as
    /// lazy item text. Which blocks qualify is therefore the call site's
    /// question, not this helper's.
    ///
    /// The loop stops at any non-`ListItem` container, so a list *outside* an
    /// enclosing blockquote is never reached from inside it.
    pub(super) fn close_lists_above_indent(&mut self, indent_cols: usize) {
        while let Some(Container::ListItem { content_col, .. }) = self.containers.last() {
            if indent_cols >= *content_col {
                break;
            }
            self.close_containers_to(self.containers.depth() - 1);
            if matches!(self.containers.last(), Some(Container::List { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
            }
        }
    }

    /// Whether `content` (container-stripped, still carrying the item's
    /// content-column indent) is a partial (rowspan) grid separator that
    /// continues the innermost open list item's buffered grid table
    /// (`+   +---+` under `| c | d |`). Such a line parses as a `+` list
    /// marker, but it is table structure — pandoc's grid parser consumes
    /// it before `bulletList` can see it — so every list-decision site on
    /// the item's continuation path must yield to it. A *dedented*
    /// separator fails the content-column check and still ends the item
    /// (pandoc's list-start tolerance wins there; pinned by
    /// `dedented_partial_separator_still_ends_the_list_item`).
    pub(super) fn partial_separator_continues_item_table(&self, content: &str) -> bool {
        use crate::parser::blocks::container_prefix::strip_list_indent;
        use crate::parser::blocks::tables::try_parse_grid_partial_separator;
        let Some(Container::ListItem {
            buffer,
            content_col,
            ..
        }) = self.containers.last()
        else {
            return false;
        };
        leading_indent(content).0 >= *content_col
            && try_parse_grid_partial_separator(strip_list_indent(content, *content_col)).is_some()
            && buffer.is_open_grid_table(*content_col)
    }

    pub(super) fn emit_list_item_buffer_if_needed(&mut self) {
        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut()
            && !buffer.is_empty()
        {
            let buffer_clone = buffer.clone();
            buffer.clear();
            let gobble = self.containers.gobble_chain();
            let use_paragraph = buffer_clone.has_blank_lines_between_content();
            let suppress_footnote_refs = self.in_footnote_definition();
            buffer_clone.emit_as_block(
                &mut self.builder,
                use_paragraph,
                self.config,
                &gobble,
                suppress_footnote_refs,
                false,
            );
        }
    }

    /// CommonMark §5.2: when a list item's first line (after the marker) is a
    /// fenced code block opener, the content of the item *is* the code block —
    /// not buffered text. The list-item open path normally pushes the
    /// post-marker text into the item's buffer; this helper detects an opening
    /// fence in that buffered first line and converts it into a CODE_BLOCK
    /// When `add_list_item` opens an inner BLOCK_QUOTE on the same line as
    /// the list marker (`- > <content>`), it returns the post-`> ` content
    /// instead of stuffing it into a paragraph; we re-dispatch that content
    /// through the block parser so block-level constructs (HTML blocks,
    /// ATX headings, fenced code, …) on the first line of a bq-in-listitem
    /// are recognized properly.
    ///
    /// Returns the number of *extra* lines consumed beyond the list-marker
    /// line itself. The caller already accounts for the marker line in its
    /// `LineDispatch::Consumed(1 + extras)`; if `result` is `Done`, this
    /// returns 0.
    pub(super) fn dispatch_bq_after_list_item(
        &mut self,
        result: crate::parser::blocks::lists::ListItemFinish,
    ) -> usize {
        let crate::parser::blocks::lists::ListItemFinish::BqDispatch { content } = result else {
            return 0;
        };
        let pos_before = self.pos;
        self.dispatch_list_marker_consumed = true;
        let dispatch = self.parse_inner_content(&content, Some(&content));
        self.dispatch_list_marker_consumed = false;
        self.pos = pos_before;
        match dispatch {
            LineDispatch::Consumed(n) => n.saturating_sub(1),
            LineDispatch::Rejected => 0,
        }
    }

    /// inside the LIST_ITEM, consuming subsequent lines until the closing
    /// fence (or end of document under CommonMark dialect, per §4.5).
    ///
    /// Pandoc-markdown also reaches this path: a bare fence still requires a
    /// matching closer to register as a code block, matching
    /// `FencedCodeBlockParser::detect_prepared` (`bare_fence_in_list_with_closer`).
    /// Returns `Some(extras)` when a fence-open is recognized on the buffered
    /// first-line content and the fenced code block was emitted (`extras` is
    /// the number of source lines consumed beyond the list-marker line).
    /// `None` means the helper did not fire and the caller proceeds normally.
    pub(super) fn maybe_open_fenced_code_in_new_list_item(&mut self) -> Option<usize> {
        let Some(Container::ListItem {
            content_col,
            buffer,
            ..
        }) = self.containers.stack.last()
        else {
            return None;
        };
        let content_col = *content_col;
        let text = buffer.sole_text_segment()?;
        let text_owned = text.to_string();
        let fence = code_blocks::try_parse_fence_open(&text_owned, self.config.dialect)?;
        let common_mark_dialect = self.config.dialect == crate::options::Dialect::CommonMark;
        let has_info = !fence.info_string.trim().is_empty();
        let bq_depth = self.current_blockquote_depth();
        let has_matching_closer = self.has_matching_fence_closer(&fence, bq_depth, content_col);
        if !(has_info || has_matching_closer || common_mark_dialect) {
            return None;
        }
        if (fence.fence_char == '`' && !self.config.extensions.backtick_code_blocks)
            || (fence.fence_char == '~' && !self.config.extensions.fenced_code_blocks)
        {
            return None;
        }
        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut() {
            buffer.clear();
        }
        let prefix = ContainerPrefix::from_scalars(
            bq_depth,
            content_col,
            bq_depth > 0,
            0,
            true,
            self.config.dialect,
        );
        let window = StrippedLines::new(&self.lines, self.pos, &prefix);
        let new_pos = code_blocks::parse_fenced_code_block(
            &mut self.builder,
            &window,
            fence,
            Some(&text_owned),
            &self.diagnostics,
            self.config.flavor,
        );
        Some(new_pos.saturating_sub(self.pos).saturating_sub(1))
    }

    /// When a new list item's marker line opens a line block (`- | a`), emit
    /// the line block as the item's content instead of buffering the line as
    /// text.
    ///
    /// Pandoc parses a list item's content as a fresh block sequence, so
    /// `lineBlock` sees `| a` at the item's content column and claims it:
    /// `- | a\n  | b` is `BulletList [[LineBlock [[a], [b]]]]`. The
    /// dispatcher's `LineBlockParser` never gets the marker line — the list
    /// parser consumes it first and buffers the post-marker text — so the
    /// item read as a `PLAIN` of two literal `|` lines. Bridge that gap here,
    /// mirroring [`Self::maybe_open_fenced_code_in_new_list_item`].
    ///
    /// Runs *after* the marker-line table helpers: `- | a | b |` is a pipe
    /// table row, and a table start also satisfies `try_parse_line_block_start`.
    ///
    /// Returns the number of source lines consumed beyond the marker line.
    pub(super) fn maybe_open_line_block_in_new_list_item(&mut self) -> Option<usize> {
        if !self.config.extensions.line_blocks {
            return None;
        }
        let Some(Container::ListItem {
            content_col,
            buffer,
            ..
        }) = self.containers.stack.last()
        else {
            return None;
        };
        try_parse_line_block_start(buffer.sole_text_segment()?)?;
        let content_col = *content_col;
        let bq_depth = self.current_blockquote_depth();

        let prefix = ContainerPrefix::from_scalars(
            bq_depth,
            content_col,
            bq_depth > 0,
            0,
            true,
            self.config.dialect,
        );
        let window = StrippedLines::new(&self.lines, self.pos, &prefix);

        if self.config.extensions.pipe_tables {
            let mut probe = GreenNodeBuilder::new();
            if tables::try_parse_pipe_table(&window, &mut probe, self.config).is_some() {
                return None;
            }
        }

        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut() {
            buffer.clear();
        }
        let new_pos = parse_line_block(&window, &mut self.builder, self.config);
        Some(new_pos.saturating_sub(self.pos).saturating_sub(1))
    }

    /// When a new list item's marker-line content is a table caption that a
    /// table follows (`- Table: cap\n\n  | a | b |\n  …`), emit the whole
    /// caption-led table as the item's content instead of leaving the caption
    /// line buffered as a paragraph.
    ///
    /// Without this, the caption line is buffered and emitted as a `PLAIN`, and
    /// the table — dispatched later at its grid line — re-claims the same line
    /// via its backward caption scan (`find_caption_before_table`), duplicating
    /// the caption and breaking losslessness. The dispatcher's `TableParser`
    /// never fires on the marker line because the list parser consumes it before
    /// block dispatch runs, so we bridge that gap here, mirroring
    /// `maybe_open_fenced_code_in_new_list_item`. Returns the number of source
    /// lines consumed beyond the list-marker line.
    pub(super) fn maybe_open_caption_table_in_new_list_item(&mut self) -> Option<usize> {
        if !self.config.extensions.table_captions {
            return None;
        }
        if !(self.config.extensions.simple_tables
            || self.config.extensions.multiline_tables
            || self.config.extensions.grid_tables
            || self.config.extensions.pipe_tables)
        {
            return None;
        }

        let Some(Container::ListItem {
            content_col,
            buffer,
            ..
        }) = self.containers.stack.last()
        else {
            return None;
        };
        buffer.sole_text_segment()?;
        let content_col = *content_col;

        let prefix = ContainerPrefix::from_stack(&self.containers.stack, true, self.config);
        debug_assert_eq!(prefix.list_content_col(), content_col);
        let window = StrippedLines::new(&self.lines, self.pos, &prefix);
        if !tables::is_caption_followed_by_table(&window, self.pos) {
            return None;
        }

        let mut consumed = None;
        if self.config.extensions.grid_tables {
            consumed = tables::try_parse_grid_table(&window, &mut self.builder, self.config);
        }
        if consumed.is_none() && self.config.extensions.multiline_tables {
            consumed = tables::try_parse_multiline_table(&window, &mut self.builder, self.config);
        }
        if consumed.is_none() && self.config.extensions.pipe_tables {
            consumed = tables::try_parse_pipe_table(&window, &mut self.builder, self.config);
        }
        if consumed.is_none() && self.config.extensions.simple_tables {
            consumed = tables::try_parse_simple_table(&window, &mut self.builder, self.config);
        }
        let consumed = consumed?;

        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut() {
            buffer.clear();
        }
        Some(consumed.saturating_sub(1))
    }

    /// When a new list item's marker line *begins a table* that is followed by a
    /// trailing caption (`- | a | b |\n  | - | - |\n\n  : cap` or the
    /// `Table:`/`table:` keyword form), parse the whole table-with-caption as the
    /// item's content here, at the marker line.
    ///
    /// A marker-line table is normally buffered and recognized only at item
    /// close via [`crate::parser::utils::list_item_buffer::ListItemBuffer`]'s
    /// structural lift. That lift cannot see a *trailing* caption: the blank line
    /// after the table flushes the buffer (`Table:` form), and a bare `: cap`
    /// line is additionally claimed by the definition-list parser as a term/
    /// definition (`: cap` form) — both before the caption ever reaches the
    /// buffer. Parsing the table at the marker line instead lets the table
    /// parser's own trailing-caption scan (`find_caption_after_table`) absorb the
    /// caption, matching pandoc, which always treats `: cap` after a table as the
    /// table's `Caption`, never a definition list.
    ///
    /// Gated on a caption actually being present so the *no-caption* marker-line
    /// table keeps its existing buffer-lift CST untouched. Returns the number of
    /// source lines consumed beyond the list-marker line.
    pub(super) fn maybe_open_table_with_trailing_caption_in_new_list_item(
        &mut self,
    ) -> Option<usize> {
        if !self.config.extensions.table_captions {
            return None;
        }
        if !(self.config.extensions.simple_tables
            || self.config.extensions.multiline_tables
            || self.config.extensions.grid_tables
            || self.config.extensions.pipe_tables)
        {
            return None;
        }

        let Some(Container::ListItem {
            content_col,
            buffer,
            ..
        }) = self.containers.stack.last()
        else {
            return None;
        };
        let first = buffer.sole_text_segment()?;
        if !matches!(
            first.trim_start().as_bytes().first(),
            Some(b'|') | Some(b'+')
        ) {
            return None;
        }
        let content_col = *content_col;

        let prefix = ContainerPrefix::from_stack(&self.containers.stack, true, self.config);
        debug_assert_eq!(prefix.list_content_col(), content_col);
        let window = StrippedLines::new(&self.lines, self.pos, &prefix);

        if tables::is_caption_followed_by_table(&window, self.pos) {
            return None;
        }

        let mut probe = GreenNodeBuilder::new();
        let _ = try_parse_any_table_kind(&window, &mut probe, self.config)?;
        let probe_root = SyntaxNode::new_root(probe.finish());
        let has_caption = probe_root
            .children()
            .any(|c| c.kind() == SyntaxKind::TABLE_CAPTION);
        if !has_caption {
            return None;
        }

        let consumed = try_parse_any_table_kind(&window, &mut self.builder, self.config)?;
        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut() {
            buffer.clear();
        }
        Some(consumed.saturating_sub(1))
    }

    /// CommonMark §5.2 rule #2: when a list marker is followed by ≥ 5 columns
    /// of whitespace and non-empty content, the content begins as an indented
    /// code block on the marker line. The marker parser collapses the post-
    /// marker whitespace to "marker + 1 (possibly virtual) space" and leaves
    /// the surplus in the post-marker text. This helper detects such a single-
    /// line indented-code first-line and converts the buffered text into a
    /// CODE_BLOCK > CODE_CONTENT inside the LIST_ITEM.
    ///
    /// Multi-line accumulation (subsequent indented-code lines on continuation
    /// lines) is handled by the regular block-detection path.
    pub(super) fn maybe_open_indented_code_in_new_list_item(&mut self) {
        let Some(Container::ListItem {
            content_col,
            buffer,
            marker_only,
            virtual_marker_space,
        }) = self.containers.stack.last()
        else {
            return;
        };
        if *marker_only {
            return;
        }
        let Some(text) = buffer.sole_text_segment() else {
            return;
        };
        let content_col = *content_col;
        let virtual_marker_space = *virtual_marker_space;
        let text_owned = text.to_string();

        let mut iter = text_owned.split_inclusive('\n');
        let line_with_nl = iter.next().unwrap_or("").to_string();
        if iter.next().is_some() {
            return;
        }

        let line_no_nl = line_with_nl
            .strip_suffix("\r\n")
            .or_else(|| line_with_nl.strip_suffix('\n'))
            .unwrap_or(&line_with_nl);
        let nl_suffix = &line_with_nl[line_no_nl.len()..];

        let buffer_start_col = if virtual_marker_space {
            content_col.saturating_sub(1)
        } else {
            content_col
        };

        let target = content_col + 4;
        let (cols_walked, ws_bytes) = crate::parser::utils::container_stack::leading_indent_from(
            line_no_nl,
            buffer_start_col,
        );

        if buffer_start_col + cols_walked < target {
            return;
        }
        if ws_bytes >= line_no_nl.len() {
            return;
        }

        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut() {
            buffer.clear();
        }

        self.builder.start_node(SyntaxKind::CODE_BLOCK.into());
        self.builder.start_node(SyntaxKind::CODE_CONTENT.into());
        if ws_bytes > 0 {
            self.builder
                .token(SyntaxKind::WHITESPACE.into(), &line_no_nl[..ws_bytes]);
        }
        let rest = &line_no_nl[ws_bytes..];
        if !rest.is_empty() {
            self.builder.token(SyntaxKind::TEXT.into(), rest);
        }
        if !nl_suffix.is_empty() {
            self.builder.token(SyntaxKind::NEWLINE.into(), nl_suffix);
        }
        self.builder.finish_node();
        self.builder.finish_node();
    }

    pub(super) fn has_matching_fence_closer(
        &self,
        fence: &code_blocks::FenceInfo,
        bq_depth: usize,
        content_col: usize,
    ) -> bool {
        let mut container_scan = code_blocks::ContainerExitScan::new(content_col);
        for raw_line in self.lines.iter().skip(self.pos + 1) {
            let (line_bq_depth, inner) = count_blockquote_markers(raw_line);
            if line_bq_depth < bq_depth {
                break;
            }
            if container_scan.exits(inner) {
                break;
            }
            let candidate = if content_col > 0 && !inner.is_empty() {
                let idx = byte_index_at_column(inner, content_col);
                if idx <= inner.len() {
                    &inner[idx..]
                } else {
                    inner
                }
            } else {
                inner
            };
            if code_blocks::is_closing_fence(candidate, fence) {
                return true;
            }
        }
        false
    }

    /// Whether a fence opened on a *lazy* blockquote line has a matching
    /// closer inside the quote's raw content.
    ///
    /// A lazy twin of [`Self::has_matching_fence_closer`], which breaks on the
    /// first line with fewer `>` markers and so never sees past the opener
    /// here. This one mirrors `FencedCodeBlockParser::detect_prepared`'s scan
    /// instead: a marker-less line keeps the scan alive because the gobble
    /// folds it back into the quote, while a blank line ends the quote and so
    /// ends the scan. Candidates are fully de-indented to match
    /// `lazy_gobble_trim`; `is_closing_fence` tolerates only three spaces of
    /// its own.
    pub(super) fn lazy_fence_has_matching_closer(&self, fence: &code_blocks::FenceInfo) -> bool {
        self.lines
            .iter()
            .skip(self.pos + 1)
            .take_while(|raw_line| !raw_line.trim().is_empty())
            .any(|raw_line| {
                let (_, inner) = count_blockquote_markers(raw_line);
                code_blocks::is_closing_fence(inner.trim_start_matches([' ', '\t']), fence)
            })
    }

    /// Whether de-indented lazy content opens a fenced code block that would
    /// actually form.
    ///
    /// `rest` must already be gobble-trimmed: `try_parse_fence_open` tolerates
    /// three spaces and no tabs, while the gobble drops every leading byte.
    /// Extension-gated the way [`Self::maybe_open_fenced_code_in_new_list_item`]
    /// is, and closer-gated because pandoc's `codeBlockFenced` fails without
    /// one — `> - a` / `   ``` ` / `   c` stays a single `Plain`. An info
    /// string is *not* an alternative to the closer here: `- a` / ```` ```rust ````
    /// / `c` is lazy text under pandoc.
    pub(super) fn lazy_content_opens_fence(&self, rest: &str) -> Option<code_blocks::FenceInfo> {
        let fence = code_blocks::try_parse_fence_open(rest, self.config.dialect)?;
        let enabled = match fence.fence_char {
            '`' => self.config.extensions.backtick_code_blocks,
            '~' => self.config.extensions.fenced_code_blocks,
            _ => true,
        };
        if !enabled || !self.lazy_fence_has_matching_closer(&fence) {
            return None;
        }
        Some(fence)
    }

    pub(super) fn is_paragraph_open(&self) -> bool {
        matches!(self.containers.last(), Some(Container::Paragraph { .. }))
    }

    /// Whether the innermost container is a list item whose content is still
    /// buffered.
    ///
    /// A `ListItemBuffer` holds bytes that have *not* been written to the
    /// green builder yet, so it is the analogue of an open paragraph: any
    /// block emitted while it is non-empty lands before the buffered text and
    /// reorders the document. Paragraph-interrupt rules must consult this as
    /// well as [`Self::is_paragraph_open`].
    pub(super) fn is_list_item_content_open(&self) -> bool {
        matches!(
            self.containers.last(),
            Some(Container::ListItem { buffer, .. }) if !buffer.is_empty()
        )
    }

    /// Whether `content`'s ordered list marker is one pandoc 3.10 refuses to
    /// read as a sublist: a start number other than 1 on a list that would be
    /// newly opened inside an enclosing list item or definition body.
    ///
    /// The "newly opened" half is what keeps `1. a` / `2. b` a two-item list —
    /// there `2.` matches the open list on the stack and continues it as a
    /// sibling item, which pandoc still accepts at any number. Only a marker
    /// with no matching open list would push a fresh `Container::List`.
    /// Returns the marker's indent (columns) when restricted, so callers that
    /// care about *where* the marker sits can apply their own column rules.
    ///
    /// CommonMark is unaffected: `pandoc -f commonmark -t native` still nests
    /// `- item` / `2. sub` as an `OrderedList (2, ...)`. The 3.10 change is a
    /// pandoc-markdown reader rule, so it branches on dialect, not just on the
    /// compat target (which every flavor shares).
    pub(super) fn restricted_ordered_sublist_indent(&self, content: &str) -> Option<usize> {
        if self.config.dialect == crate::options::Dialect::CommonMark {
            return None;
        }
        if !self
            .config
            .effective_pandoc_compat()
            .restricts_ordered_sublist_start()
        {
            return None;
        }
        if !lists::in_list_item_or_definition_body(&self.containers) {
            return None;
        }
        let indent_cols = leading_indent(content).0;
        let marker_match = lists::try_parse_list_marker(
            content,
            self.config,
            lists::open_list_hint_at_indent(&self.containers, indent_cols),
        )?;
        if lists::marker_start_number(&marker_match.marker).is_none_or(|start| start == 1) {
            return None;
        }
        let opens_new_list = match lists::find_matching_list_level(
            &self.containers,
            &marker_match.marker,
            indent_cols,
            self.config.dialect,
        ) {
            None => true,
            Some(level) => lists::open_item_content_col_in_list(&self.containers, level)
                .is_some_and(|col| indent_cols >= col),
        };
        opens_new_list.then_some(indent_cols)
    }

    pub(super) fn restricted_ordered_sublist(&self, content: &str) -> bool {
        self.restricted_ordered_sublist_indent(content).is_some()
    }

    /// Whether a restricted marker on this line still ends the block above it.
    ///
    /// It does everywhere a block could start, but 4+ columns past the
    /// enclosing content column no block can start at all — that is indented
    /// code territory, and indented code cannot interrupt a paragraph — so
    /// pandoc folds the line in as a soft break instead. `A.`/`I.`/`(6)`/`c)`
    /// (corpus case 0116) is the shape that depends on this.
    /// A marker indented at least four columns but short of the item's content
    /// column is also lazy text: it cannot open either an outer or inner list.
    pub(super) fn restricted_sublist_interrupts(&self, content: &str) -> bool {
        let Some(indent_cols) = self.restricted_ordered_sublist_indent(content) else {
            return false;
        };
        lists::innermost_content_col(&self.containers)
            .is_none_or(|col| (indent_cols < 4 || indent_cols >= col) && indent_cols < col + 4)
    }

    /// Append `line` to whichever open text buffer is holding the current
    /// block's content — the paragraph's, or the list item's.
    ///
    /// Used by the paragraph-interrupt guards in the no-blank-before
    /// dispatch arm, which must fold the line into the open block instead of
    /// letting a `Yes` detection emit a sibling.
    pub(super) fn append_lazy_continuation_line(&mut self, line: &str) {
        if self.is_paragraph_open() {
            paragraphs::append_paragraph_line(
                &mut self.containers,
                &mut self.builder,
                line,
                self.config,
            );
        } else if let Some(Container::ListItem {
            buffer,
            marker_only,
            ..
        }) = self.containers.stack.last_mut()
        {
            buffer.push_text(line, self.config);
            if !line.trim().is_empty() {
                *marker_only = false;
            }
        }
    }

    /// Fold an open paragraph's buffered content into a setext heading and emit it.
    ///
    /// Used for CommonMark multi-line setext: when a setext underline is matched
    /// and a paragraph is already open with buffered text, the entire paragraph
    /// (buffer + current text line) becomes the heading content. The HEADING node
    /// is wrapped retroactively from the paragraph's start checkpoint so the
    /// emitted bytes appear in source order.
    pub(super) fn emit_setext_heading_folding_paragraph(
        &mut self,
        text_line: &str,
        underline_line: &str,
        level: usize,
    ) {
        let (buffered_text, checkpoint) = match self.containers.stack.last() {
            Some(Container::Paragraph {
                buffer,
                start_checkpoint,
                ..
            }) => (buffer.get_text_for_parsing(), Some(*start_checkpoint)),
            _ => (String::new(), None),
        };

        if checkpoint.is_some() {
            self.containers.stack.pop();
        }

        let combined_text = if buffered_text.is_empty() {
            text_line.to_string()
        } else {
            format!("{}{}", buffered_text, text_line)
        };

        let cp = checkpoint.expect(
            "emit_setext_heading_folding_paragraph requires an open paragraph; \
             single-line setext should go through the regular dispatcher path",
        );
        self.builder.start_node_at(cp, SyntaxKind::HEADING.into());
        emit_setext_heading_body(
            &mut self.builder,
            &combined_text,
            underline_line,
            level,
            self.config,
        );
        self.builder.finish_node();
    }

    /// Try to fold a list item's buffered first-line text and the current line
    /// into a setext HEADING node, returning true on success.
    ///
    /// CommonMark §4.3 / Pandoc-markdown both treat the marker line of a list
    /// item as a fresh start for setext detection — i.e. `- Bar\n  ---\n` is a
    /// setext h2 inside the list item. The dispatcher path can't see this
    /// because the list parser consumes the marker line and buffers the
    /// post-marker text; by the time `  ---` reaches the dispatcher, the
    /// candidate text line is already inside the buffer rather than the line
    /// stream. This helper bridges that gap: when the innermost container is a
    /// `ListItem` with a single buffered text segment and the current
    /// (list-item-content-stripped) line is a setext underline, emit the
    /// folded heading directly and clear the buffer.
    ///
    /// Multi-line setext (multiple buffered text segments) is *not* handled
    /// here because Pandoc-markdown disagrees with CommonMark on whether
    /// `- Foo\n  Bar\n  ---\n` forms a setext heading.
    pub(super) fn try_fold_list_item_buffer_into_setext(
        &mut self,
        content: &str,
    ) -> Option<LineDispatch> {
        let Some(Container::ListItem {
            buffer,
            content_col,
            ..
        }) = self.containers.stack.last()
        else {
            return None;
        };
        let text_line = buffer.sole_text_segment()?;
        if buffer.buffered_line_count() != 1 {
            return None;
        }

        let content_col = *content_col;
        let (underline_indent_cols, _) = leading_indent(content);
        if underline_indent_cols < content_col {
            return None;
        }

        let lines = [text_line, content];
        try_parse_setext_heading(&lines, 0)?;

        let (text_no_newline, _) = strip_newline(text_line);
        if text_no_newline.trim().is_empty() {
            return None;
        }
        if try_parse_horizontal_rule(text_no_newline).is_some() {
            return None;
        }

        let text_owned = text_line.to_string();
        let markers = buffer.trailing_blockquote_markers();
        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut() {
            buffer.clear();
        }
        self.builder.start_node(SyntaxKind::HEADING.into());
        emit_setext_heading_text(&mut self.builder, &text_owned, self.config);
        for (leading_spaces, has_trailing_space) in markers {
            blockquotes::emit_one_line_prefix_marker(
                &mut self.builder,
                leading_spaces,
                has_trailing_space,
            );
        }
        emit_setext_underline(&mut self.builder, content);
        self.builder.finish_node();
        Some(LineDispatch::consumed(1))
    }

    /// CommonMark spec example #312: handle a detected list marker that's
    /// actually lazy continuation rather than a new list item. Returns true
    /// when the line was consumed as continuation (caller should advance pos
    /// without calling `handle_list_open_effect`).
    ///
    /// A marker line whose leading indent is ≥ 4 columns isn't a real list
    /// marker when (a) the indent doesn't reach the deepest open list item's
    /// content column (so it can't open a child list), and (b) no open list
    /// level matches the indent (so it can't be a sibling). In that case the
    /// content is just text that lazily extends the deepest open paragraph
    /// or list item.
    pub(super) fn try_lazy_list_continuation(
        &mut self,
        block_match: &crate::parser::block_dispatcher::PreparedBlockMatch,
        content: &str,
    ) -> bool {
        use crate::parser::block_dispatcher::ListPrepared;

        let Some(prepared) = block_match
            .payload
            .as_ref()
            .and_then(|p| p.downcast_ref::<ListPrepared>())
        else {
            return false;
        };

        if prepared.indent_cols < 4 || !lists::in_list(&self.containers) {
            return false;
        }

        let current_content_col = paragraphs::current_content_col(&self.containers);
        if prepared.indent_cols >= current_content_col
            && prepared.indent_cols < current_content_col + 4
        {
            return false;
        }

        if lists::find_matching_list_level(
            &self.containers,
            &prepared.marker,
            prepared.indent_cols,
            self.config.dialect,
        )
        .is_some()
        {
            return false;
        }

        if lists::band_fence_level(
            &self.containers,
            &prepared.marker,
            prepared.indent_cols,
            self.config.dialect,
        )
        .is_some()
        {
            return false;
        }

        match self.containers.last() {
            Some(Container::Paragraph { .. }) => {
                paragraphs::append_paragraph_line(
                    &mut self.containers,
                    &mut self.builder,
                    content,
                    self.config,
                );
                true
            }
            Some(Container::ListItem { .. }) => {
                if let Some(Container::ListItem {
                    buffer,
                    marker_only,
                    ..
                }) = self.containers.stack.last_mut()
                {
                    buffer.push_text(content, self.config);
                    if !content.trim().is_empty() {
                        *marker_only = false;
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// A list marker line that is really the delimiter row of a table the
    /// item's *marker line* opened. Returns true when the line was buffered
    /// as item content (caller should advance pos without opening a list).
    ///
    /// Pandoc collects a list item's lines and reparses them as a document,
    /// so `- a | b` / `  - | -` reparses `a | b\n- | -`: the first line is
    /// not a marker, `pipeTable` claims both lines, and `bulletList` never
    /// sees the second. Panache parses item content as it goes, so without
    /// this the `- ` would open a nested bullet whose content is a line
    /// block (`| -`). Buffering the line instead leaves the table to the
    /// buffer's structural lift at item close, which is where every other
    /// marker-line table is built.
    ///
    /// Only the marker line may be buffered so far: pandoc's reparse gives
    /// the table no way to interrupt an open paragraph, so `- x` / `  a | b`
    /// / `  - | -` really is a nested list in both. Pandoc-dialect only —
    /// `cmark-gfm` (via `pandoc -f gfm`) opens the nested list here, since
    /// its table extension grows out of a paragraph rather than a reparse.
    pub(super) fn try_buffer_marker_line_table_delimiter(&mut self, content: &str) -> bool {
        if self.config.dialect != crate::options::Dialect::Pandoc
            || !self.config.extensions.pipe_tables
            || self.pos == 0
        {
            return false;
        }

        let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last() else {
            return false;
        };
        let Some(first) = buffer.sole_text_segment() else {
            return false;
        };
        if first.trim_end_matches(['\n', '\r']).contains('\n') || !first.contains('|') {
            return false;
        }

        let prefix = ContainerPrefix::from_stack(&self.containers.stack, true, self.config);
        let window = StrippedLines::new(&self.lines, self.pos - 1, &prefix);
        if tables::opens_multiline_pipe_table(&window, self.config).is_none() {
            return false;
        }

        if let Some(Container::ListItem {
            buffer,
            marker_only,
            ..
        }) = self.containers.stack.last_mut()
        {
            buffer.push_text(content, self.config);
            if !content.trim().is_empty() {
                *marker_only = false;
            }
        }
        true
    }

    pub(super) fn handle_list_open_effect(
        &mut self,
        block_match: &crate::parser::block_dispatcher::PreparedBlockMatch,
        content: &str,
        indent_to_emit: Option<&str>,
    ) -> usize {
        use crate::parser::block_dispatcher::ListPrepared;

        let prepared = block_match
            .payload
            .as_ref()
            .and_then(|p| p.downcast_ref::<ListPrepared>());
        let Some(prepared) = prepared else {
            return 0;
        };

        if prepared.indent_cols >= 4 && !lists::in_list(&self.containers) {
            paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
            paragraphs::append_paragraph_line(
                &mut self.containers,
                &mut self.builder,
                content,
                self.config,
            );
            return 0;
        }

        if self.is_paragraph_open() {
            if !block_match.detection.eq(&BlockDetectionResult::Yes) {
                paragraphs::append_paragraph_line(
                    &mut self.containers,
                    &mut self.builder,
                    content,
                    self.config,
                );
                return 0;
            }
            self.close_containers_to(self.containers.depth() - 1);
        }

        if matches!(
            self.containers.last(),
            Some(Container::Definition {
                plain_open: true,
                ..
            })
        ) {
            self.emit_buffered_plain_if_needed();
        }

        let matched_level = lists::find_matching_list_level(
            &self.containers,
            &prepared.marker,
            prepared.indent_cols,
            self.config.dialect,
        );
        let band = lists::band_fence_level(
            &self.containers,
            &prepared.marker,
            prepared.indent_cols,
            self.config.dialect,
        );
        let matched_level = match &band {
            Some(b) if b.marker_matches => Some(b.level),
            Some(_) => None,
            None => matched_level,
        };
        let list_item = ListItemEmissionInput {
            content,
            marker_len: prepared.marker_len,
            spaces_after_cols: prepared.spaces_after_cols,
            spaces_after_bytes: prepared.spaces_after,
            indent_cols: prepared.indent_cols,
            indent_bytes: prepared.indent_bytes,
            virtual_marker_space: prepared.virtual_marker_space,
        };
        let current_content_col = paragraphs::current_content_col(&self.containers);
        let deep_ordered_matched_level = matched_level
            .and_then(|level| self.containers.stack.get(level).map(|c| (level, c)))
            .and_then(|(level, container)| match container {
                Container::List {
                    marker: list_marker,
                    base_indent_cols,
                    ..
                } if matches!(
                    (&prepared.marker, list_marker),
                    (ListMarker::Ordered(_), ListMarker::Ordered(_))
                ) && prepared.indent_cols >= 4
                    && *base_indent_cols >= 4
                    && prepared.indent_cols.abs_diff(*base_indent_cols) <= 3 =>
                {
                    Some(level)
                }
                _ => None,
            });

        let matched_list_awaits_item = matched_level.is_some_and(|level| {
            matches!(
                self.containers.stack.get(level),
                Some(Container::List { .. })
            ) && !self.containers.stack[level + 1..]
                .iter()
                .any(|c| matches!(c, Container::ListItem { .. }))
        });

        if deep_ordered_matched_level.is_none()
            && !matched_list_awaits_item
            && current_content_col > 0
            && prepared.indent_cols >= current_content_col
        {
            if let Some(level) = matched_level
                && let Some(Container::List {
                    base_indent_cols, ..
                }) = self.containers.stack.get(level)
                && prepared.indent_cols == *base_indent_cols
            {
                let num_parent_lists = self.containers.stack[..level]
                    .iter()
                    .filter(|c| matches!(c, Container::List { .. }))
                    .count();

                if num_parent_lists > 0 {
                    self.close_containers_to(level + 1);

                    if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                        self.close_containers_to(self.containers.depth() - 1);
                    }
                    if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
                        self.close_containers_to(self.containers.depth() - 1);
                    }

                    if let Some(indent_str) = indent_to_emit {
                        self.builder
                            .token(SyntaxKind::WHITESPACE.into(), indent_str);
                    }

                    let finish = if let Some(nested_marker) = prepared.nested_marker {
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
                    if let Some(extras) = self.maybe_open_fenced_code_in_new_list_item() {
                        return extras;
                    }
                    if let Some(extras) = self.maybe_open_caption_table_in_new_list_item() {
                        return extras;
                    }
                    if let Some(extras) =
                        self.maybe_open_table_with_trailing_caption_in_new_list_item()
                    {
                        return extras;
                    }
                    if let Some(extras) = self.maybe_open_line_block_in_new_list_item() {
                        return extras;
                    }
                    self.maybe_open_indented_code_in_new_list_item();
                    if let Some(extras) = self.maybe_open_definition_term_in_new_list_item() {
                        return extras;
                    }
                    return self.dispatch_bq_after_list_item(finish);
                }
            }

            self.emit_list_item_buffer_if_needed();

            let finish = start_nested_list(
                &mut self.containers,
                &mut self.builder,
                &prepared.marker,
                &list_item,
                indent_to_emit,
                self.config,
            );
            if let Some(extras) = self.maybe_open_fenced_code_in_new_list_item() {
                return extras;
            }
            if let Some(extras) = self.maybe_open_caption_table_in_new_list_item() {
                return extras;
            }
            if let Some(extras) = self.maybe_open_table_with_trailing_caption_in_new_list_item() {
                return extras;
            }
            if let Some(extras) = self.maybe_open_line_block_in_new_list_item() {
                return extras;
            }
            self.maybe_open_indented_code_in_new_list_item();
            if let Some(extras) = self.maybe_open_definition_term_in_new_list_item() {
                return extras;
            }
            return self.dispatch_bq_after_list_item(finish);
        }

        if let Some(level) = matched_level {
            self.close_containers_to(level + 1);

            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
            }
            if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
            }

            if let Some(indent_str) = indent_to_emit {
                self.builder
                    .token(SyntaxKind::WHITESPACE.into(), indent_str);
            }

            let finish = if let Some(nested_marker) = prepared.nested_marker {
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
            if let Some(extras) = self.maybe_open_fenced_code_in_new_list_item() {
                return extras;
            }
            if let Some(extras) = self.maybe_open_caption_table_in_new_list_item() {
                return extras;
            }
            if let Some(extras) = self.maybe_open_table_with_trailing_caption_in_new_list_item() {
                return extras;
            }
            if let Some(extras) = self.maybe_open_line_block_in_new_list_item() {
                return extras;
            }
            self.maybe_open_indented_code_in_new_list_item();
            if let Some(extras) = self.maybe_open_definition_term_in_new_list_item() {
                return extras;
            }
            return self.dispatch_bq_after_list_item(finish);
        }

        if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
            self.close_containers_to(self.containers.depth() - 1);
        }
        if let Some(b) = &band {
            self.close_containers_to(b.level);
        } else {
            while matches!(
                self.containers.last(),
                Some(Container::ListItem { .. } | Container::List { .. })
            ) {
                self.close_containers_to(self.containers.depth() - 1);
            }
        }

        self.builder.start_node(SyntaxKind::LIST.into());
        if let Some(indent_str) = indent_to_emit {
            self.builder
                .token(SyntaxKind::WHITESPACE.into(), indent_str);
        }
        self.containers.push(Container::List {
            marker: prepared.marker.clone(),
            base_indent_cols: prepared.indent_cols,
            has_blank_between_items: false,
        });

        let finish = if let Some(nested_marker) = prepared.nested_marker {
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
        if let Some(extras) = self.maybe_open_fenced_code_in_new_list_item() {
            return extras;
        }
        if let Some(extras) = self.maybe_open_caption_table_in_new_list_item() {
            return extras;
        }
        if let Some(extras) = self.maybe_open_table_with_trailing_caption_in_new_list_item() {
            return extras;
        }
        if let Some(extras) = self.maybe_open_line_block_in_new_list_item() {
            return extras;
        }
        self.maybe_open_indented_code_in_new_list_item();
        if let Some(extras) = self.maybe_open_definition_term_in_new_list_item() {
            return extras;
        }
        self.dispatch_bq_after_list_item(finish)
    }
}

/// Try each enabled table kind in dispatcher order.
fn try_parse_any_table_kind(
    window: &StrippedLines,
    builder: &mut GreenNodeBuilder<'static>,
    config: &ParserOptions,
) -> Option<usize> {
    let mut consumed = None;
    if config.extensions.grid_tables {
        consumed = tables::try_parse_grid_table(window, builder, config);
    }
    if consumed.is_none() && config.extensions.multiline_tables {
        consumed = tables::try_parse_multiline_table(window, builder, config);
    }
    if consumed.is_none() && config.extensions.pipe_tables {
        consumed = tables::try_parse_pipe_table(window, builder, config);
    }
    if consumed.is_none() && config.extensions.simple_tables {
        consumed = tables::try_parse_simple_table(window, builder, config);
    }
    consumed
}
