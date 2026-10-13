use super::*;

pub(super) struct LazyFold<'l> {
    pub(super) line: &'l str,
    pub(super) inner_content: &'l str,
    pub(super) bq_depth: usize,
    pub(super) bq_marker_line: &'l str,
    pub(super) shifted_bq_prefix: &'l str,
    pub(super) used_shifted_bq: bool,
}

/// Fence recognition differs between Pandoc's paragraph and list readers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LazyFenceRule {
    BacktickOnly,
    AnyFence,
}

pub(super) struct LazyInterruptContext {
    fence_rule: LazyFenceRule,
    probe_div_closer: bool,
}

impl LazyInterruptContext {
    pub(super) fn for_paragraph() -> Self {
        Self {
            fence_rule: LazyFenceRule::BacktickOnly,
            probe_div_closer: true,
        }
    }

    pub(super) fn for_list_item() -> Self {
        Self {
            fence_rule: LazyFenceRule::AnyFence,
            probe_div_closer: false,
        }
    }
}

pub(super) struct LazyInterrupts {
    pub(super) hr: bool,
    pub(super) fence: bool,
    pub(super) heading: bool,
    pub(super) div_close: bool,
    pub(super) html: bool,
    pub(super) ends_gobble: bool,
}

impl LazyInterrupts {
    pub(super) fn any(&self) -> bool {
        self.hr || self.fence || self.heading || self.div_close || self.html || self.ends_gobble
    }
}

impl<'a> Parser<'a> {
    /// Close blockquotes down to a target depth.
    ///
    /// Must use `Parser::close_containers_to` (not `ContainerStack::close_to`) so list/paragraph
    /// buffers are emitted for losslessness.
    pub(super) fn close_blockquotes_to_depth(&mut self, target_depth: usize) {
        let mut current = self.current_blockquote_depth();
        while current > target_depth {
            while !matches!(self.containers.last(), Some(Container::BlockQuote { .. })) {
                if self.containers.depth() == 0 {
                    break;
                }
                self.close_containers_to(self.containers.depth() - 1);
            }
            if matches!(self.containers.last(), Some(Container::BlockQuote { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
                current -= 1;
            } else {
                break;
            }
        }
    }

    pub(super) fn active_alert_blockquote_depth(&self) -> Option<usize> {
        self.containers.stack.iter().rev().find_map(|c| match c {
            Container::Alert { blockquote_depth } => Some(*blockquote_depth),
            _ => None,
        })
    }

    pub(super) fn in_active_alert(&self) -> bool {
        self.active_alert_blockquote_depth().is_some()
    }

    pub(super) fn alert_marker_from_content(content: &str) -> Option<&'static str> {
        let (without_newline, _) = strip_newline(content);
        let trimmed = without_newline.trim();
        GITHUB_ALERT_MARKERS
            .into_iter()
            .find(|marker| *marker == trimmed)
    }

    pub(super) fn blockquote_marker_info(
        &self,
        payload: Option<&BlockQuotePrepared>,
        line: &str,
    ) -> Vec<marker_utils::BlockQuoteMarkerInfo> {
        payload
            .map(|payload| payload.marker_info.clone())
            .unwrap_or_else(|| parse_blockquote_marker_info(line))
    }

    /// Build blockquote marker metadata for the current source line.
    ///
    /// When a blockquote marker is detected at a shifted list content column
    /// (e.g. `    > ...` inside a list item), the prefix indentation must be
    /// folded into the first marker's leading spaces for lossless emission.
    pub(super) fn marker_info_for_line(
        &self,
        payload: Option<&BlockQuotePrepared>,
        raw_line: &str,
        marker_line: &str,
        shifted_prefix: &str,
        used_shifted: bool,
    ) -> Vec<marker_utils::BlockQuoteMarkerInfo> {
        let mut marker_info = if used_shifted {
            parse_blockquote_marker_info(marker_line)
        } else {
            self.blockquote_marker_info(payload, raw_line)
        };
        if used_shifted && !shifted_prefix.is_empty() {
            let (prefix_cols, _) = leading_indent(shifted_prefix);
            if let Some(first) = marker_info.first_mut() {
                first.leading_spaces += prefix_cols;
            }
        }
        marker_info
    }

    /// Build a `BlockContext` describing the current line *as if* the
    /// container stack already carried `bq_depth` blockquotes.
    ///
    /// Field-for-field mirror of the context `parse_inner_content` builds
    /// (see the `BlockContext { .. }` literal and the blank/doc-start
    /// fill-in that follows it) — the two must stay in sync, because a
    /// verdict this probe reaches has to survive re-detection there. The
    /// deliberate differences:
    ///
    /// - `blockquote_depth` is the hypothetical `bq_depth`, not the stack's.
    /// - `next_line` is stripped of *every* marker, matching the inner
    ///   context. `parse_line`'s own context passes the raw next line
    ///   instead, which would make `SetextHeadingParser`'s leading-byte
    ///   gate reject any underline still carrying a `>`.
    /// - `has_blank_before`'s blockquote clause also fires when this probe
    ///   would open a level, since that is what the stack looks like at
    ///   re-detection time.
    /// - `after_metadata_block` is read, never taken: a probe must not
    ///   consume parser state.
    pub(super) fn probe_block_context(
        &self,
        bq_depth: usize,
        current_bq_depth: usize,
        content: &'a str,
    ) -> BlockContext<'a> {
        let has_blank_before = if self.pos == 0 || self.after_metadata_block {
            true
        } else {
            let prev_line = self.lines[self.pos - 1];
            let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
            let (prev_inner_no_nl, _) = strip_newline(prev_inner);
            let prev_is_fenced_div_open = self.config.extensions.fenced_divs
                && fenced_divs::try_parse_div_fence_open(
                    strip_n_blockquote_markers(prev_inner_no_nl, prev_bq_depth).trim_start(),
                )
                .is_some();

            is_blank_line(prev_line)
                || prev_is_fenced_div_open
                || bq_depth > current_bq_depth
                || matches!(self.containers.last(), Some(Container::BlockQuote { .. }))
                || !self.previous_block_requires_blank_before_new_block()
        };

        let at_line_zero = self.pos == 0 && bq_depth == 0;
        let at_document_start = self.origin_allows_document_start() && at_line_zero;
        let prev_line_blank = self.pos > 0 && {
            let prev_line = self.lines[self.pos - 1];
            let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
            is_blank_line(prev_line) || (prev_bq_depth > 0 && is_blank_line(prev_inner))
        };

        BlockContext {
            has_blank_before,
            has_blank_before_strict: at_line_zero || prev_line_blank,
            at_document_start,
            in_fenced_div: self.in_fenced_div(),
            fenced_div_open_indent: self.innermost_fenced_div_open_indent(),
            fenced_div_wraps_list: self.fenced_div_wraps_innermost_list(),
            myst_directive_closer: self.innermost_myst_directive_closer(),
            blockquote_depth: bq_depth,
            config: self.config,
            diags: self.diagnostics.clone(),
            content_indent: 0,
            indent_to_emit: None,
            list_indent_info: if lists::in_list(&self.containers) {
                let content_col = paragraphs::current_content_col(&self.containers);
                (content_col > 0)
                    .then_some(crate::parser::block_dispatcher::ListIndentInfo { content_col })
            } else {
                None
            },
            in_list: lists::in_list(&self.containers),
            in_definition_list: definition_lists::in_definition_list(&self.containers),
            in_marker_only_list_item: matches!(
                self.containers.last(),
                Some(Container::ListItem {
                    marker_only: true,
                    ..
                })
            ),
            list_item_unclosed_html_block_tag: self.list_item_unclosed_html_block_tag(),
            open_code_span_openers: self.open_code_span_openers(),
            paragraph_open: self.is_paragraph_open(),
            list_item_content_open: self.is_list_item_content_open(),
            next_line: (self.pos + 1 < self.lines.len())
                .then(|| count_blockquote_markers(self.lines[self.pos + 1]).1),
            open_alpha_hint: lists::open_list_hint_at_indent(
                &self.containers,
                leading_indent(content).0,
            ),
            restricted_ordered_sublist: self.restricted_ordered_sublist(content),
        }
    }

    /// How many blockquote levels this line may open, when fewer than its
    /// `>` count.
    ///
    /// Pandoc's `blockQuote` strips exactly *one* `>` per line of a quoted
    /// run and recursively re-parses the remainder, so every parser ahead
    /// of `blockQuote` in the reader order gets a shot at content that
    /// still begins with `>`. `setextHeader` is one of those, which is why
    /// `pandoc -f markdown -t native` reads `> > a\n> ---\n` as
    /// `BlockQuote [Header 2 [Str ">", Space, Str "a"]]` — the underline's
    /// single marker caps the quote at depth 1 and the surplus `>` becomes
    /// literal heading text.
    ///
    /// Counting markers per line, as this parser does, would open both
    /// quotes. So probe the registry at each depth the line would pass
    /// through and stop at the first one whose winner outranks
    /// `BlockQuoteParser`; that depth is the cap. `k == bq_depth` is
    /// excluded, so "nothing claims" is bit-identical to counting.
    ///
    /// Pandoc-dialect only, and this one *is* a dialect difference rather
    /// than a parser's own gate. Capping presumes the surplus markers are
    /// literal text, which is true only under Pandoc. CommonMark reads
    /// them as real containers, and `SetextHeadingParser` says so by
    /// folding the content's own markers into its same-container
    /// comparison — so at probe depth `k` it answers about depth
    /// `k + own_markers`, and its "yes" cannot be read as a verdict for
    /// `k`. Without this gate `> > a\n> > ---\n` collapses to a single
    /// quote under CommonMark, where `cmark` nests two.
    pub(super) fn blockquote_depth_cap(
        &self,
        current_bq_depth: usize,
        bq_depth: usize,
    ) -> Option<usize> {
        if self.config.dialect != crate::options::Dialect::Pandoc {
            return None;
        }

        if self.content_container_indent_to_strip() != 0 {
            return None;
        }

        let base = ContainerPrefix::from_stack(
            &self.containers.stack,
            self.dispatch_list_marker_consumed,
            self.config,
        );
        for k in current_bq_depth.max(1)..bq_depth {
            let prefix = base.with_extra_blockquotes(k - current_bq_depth);
            let stripped = StrippedLines::new(&self.lines, self.pos, &prefix)
                .with_hugo_lookahead(&self.hugo_lookahead);
            let ctx = self.probe_block_context(k, current_bq_depth, stripped.first());
            if let Some(block_match) = self.block_registry.detect_prepared(&ctx, &stripped)
                && self.block_registry.outranks_blockquote(&block_match)
            {
                return Some(k);
            }
        }
        None
    }

    pub(super) fn shifted_blockquote_from_list<'b>(
        &self,
        line: &'b str,
    ) -> Option<(usize, &'b str, &'b str, &'b str)> {
        let list_content_col = self
            .containers
            .stack
            .iter()
            .rev()
            .find_map(|c| match c {
                Container::ListItem { content_col, .. } => Some(*content_col),
                _ => None,
            })
            .unwrap_or(0);
        let content_container_indent = self.content_container_indent_to_strip();
        if list_content_col == 0 && self.current_blockquote_depth() == 0 {
            return None;
        }
        let marker_col = list_content_col.saturating_add(content_container_indent);
        if marker_col == 0 {
            return None;
        }

        let (indent_cols, _) = leading_indent(line);
        if indent_cols < marker_col {
            return None;
        }

        let idx = byte_index_at_column(line, marker_col);
        if idx > line.len() {
            return None;
        }

        let candidate = &line[idx..];
        let (candidate_depth, candidate_inner) = count_blockquote_markers(candidate);
        if candidate_depth == 0 {
            return None;
        }

        Some((candidate_depth, candidate_inner, candidate, &line[..idx]))
    }

    pub(super) fn emit_blockquote_markers(
        &mut self,
        marker_info: &[marker_utils::BlockQuoteMarkerInfo],
        depth: usize,
    ) {
        for i in 0..depth {
            if let Some(info) = marker_info.get(i) {
                blockquotes::emit_one_blockquote_marker(
                    &mut self.builder,
                    info.leading_spaces,
                    info.has_trailing_space,
                );
            }
        }
    }

    pub(super) fn current_blockquote_depth(&self) -> usize {
        blockquotes::current_blockquote_depth(&self.containers)
    }

    /// Whether pandoc's blockquote gobble stops at this line instead of
    /// folding it into the quote.
    ///
    /// `emailBlockQuote` keeps eating lines through the reader's `endline`
    /// guards, and those guards run against the raw next line — *before* the
    /// leading whitespace the fold otherwise skips. So each guard here is
    /// anchored at byte 0 even where the construct itself tolerates up to
    /// three spaces of indent, which is why `> para` / `# head` ends the
    /// quote under `-blank_before_header` while `> para` / ` # head` does
    /// not.
    ///
    /// `notFollowedBy emailBlockQuoteStart` (the `-blank_before_blockquote`
    /// guard) has no counterpart below: a line whose markers this branch has
    /// not already consumed cannot start with `>`.
    pub(super) fn blockquote_gobble_ends_at(&self, line: &str, inner_content: &str) -> bool {
        let quote_is_in_list_item = self
            .containers
            .stack
            .iter()
            .rposition(|c| matches!(c, Container::BlockQuote { .. }))
            .is_some_and(|quote| {
                self.containers.stack[..quote]
                    .iter()
                    .any(|c| matches!(c, Container::ListItem { .. }))
            });
        if (self.config.extensions.lists_without_preceding_blankline || quote_is_in_list_item)
            && try_parse_list_marker(
                inner_content,
                self.config,
                lists::open_list_hint_at_indent(&self.containers, leading_indent(inner_content).0),
            )
            .is_some()
        {
            return true;
        }

        if !self.config.extensions.blank_before_header && inner_content.starts_with('#') {
            return true;
        }

        if self.config.extensions.backtick_code_blocks
            && inner_content.starts_with('`')
            && code_blocks::try_parse_fence_open(inner_content, self.config.dialect).is_some()
        {
            return true;
        }

        self.div_closer_ends_blockquote(line) || self.html_closer_ends_blockquote(line)
    }

    /// The shared lazy-interrupt probe list: does this reduced-marker line
    /// interrupt the open paragraph (or list-item buffer) instead of folding
    /// in as lazy continuation text?
    ///
    /// One predicate, two gates (paragraph and list item), differing only via
    /// [`LazyInterruptContext`]. Kept as a probe list rather than delegating
    /// to `detect_prepared`: the dispatcher context can't express the
    /// `endline`-guard quirks (byte-0 anchoring in
    /// `blockquote_gobble_ends_at`, the `blank_before_header` trim split
    /// below), and hypothetical dispatch has payload side effects. A missing
    /// probe fails *silently* --- the line becomes lazy inline text, caught
    /// only by pandoc-diffing --- so each probe family carries a pandoc
    /// corpus pin (0077, 0512/0513, 0525-0530).
    ///
    /// The probes run on `inner_content` (markers stripped; identical to
    /// `line` for zero-marker lines): a reduced-marker line like `> # head`
    /// under a depth-2 quote is not lazy at its own level, so the stripped
    /// content decides the interruption (issue #429). The two exceptions
    /// that read the raw `line` are called out inline.
    pub(super) fn lazy_interrupts(
        &self,
        line: &str,
        inner_content: &str,
        ctx: &LazyInterruptContext,
    ) -> LazyInterrupts {
        let is_commonmark = self.config.dialect == crate::options::Dialect::CommonMark;
        let hr = is_commonmark && try_parse_horizontal_rule(inner_content).is_some();
        let fence = if is_commonmark {
            code_blocks::try_parse_fence_open(inner_content, self.config.dialect).is_some()
        } else {
            self.lazy_content_opens_fence(inner_content.trim_start_matches([' ', '\t']))
                .is_some_and(|fence| match ctx.fence_rule {
                    LazyFenceRule::BacktickOnly => fence.fence_char == '`',
                    LazyFenceRule::AnyFence => true,
                })
        };
        let heading_can_interrupt = is_commonmark || !self.config.extensions.blank_before_header;
        let heading_probe = if is_commonmark {
            inner_content
        } else {
            inner_content.trim_start_matches([' ', '\t'])
        };
        let heading = heading_can_interrupt && try_parse_atx_heading(heading_probe).is_some();
        let div_close = ctx.probe_div_closer
            && self.config.extensions.fenced_divs
            && self.in_fenced_div()
            && (fenced_divs::is_div_closing_fence(line)
                || (fenced_divs::is_div_closing_fence(inner_content)
                    && self.quoted_div_closes_at(count_blockquote_markers(line).0)));
        let html = self.lazy_html_block_interrupts(inner_content);
        let ends_gobble = !is_commonmark && self.blockquote_gobble_ends_at(line, inner_content);
        LazyInterrupts {
            hr,
            fence,
            heading,
            div_close,
            html,
            ends_gobble,
        }
    }

    /// Whether a `:::` line carrying `markers` blockquote markers closes the
    /// innermost open div, given that it is lazy at the current depth.
    ///
    /// Only when the div was opened inside a quote the line still carries:
    /// pandoc extracts the quote's raw content, and there the closer sits
    /// flush against the div it opened. `> ::: d` / `> > a` / `> :::` reads
    /// as `Div [BlockQuote [Para "a"]]`, and so does the same shape one
    /// quote deeper. A div opened *outside* the quote is the other shape --
    /// pandoc leaves such a line as a `Para [Str ":::"]` and reports the div
    /// unclosed -- so it stays out here; see
    /// [`Parser::div_closer_ends_blockquote`] for that side.
    pub(super) fn quoted_div_closes_at(&self, markers: usize) -> bool {
        let stack = &self.containers.stack;
        let Some(div) = stack
            .iter()
            .rposition(|c| matches!(c, Container::FencedDiv { .. }))
        else {
            return false;
        };
        let quotes_before_div = stack[..div]
            .iter()
            .filter(|c| matches!(c, Container::BlockQuote { .. }))
            .count();
        quotes_before_div >= 1 && quotes_before_div <= markers
    }

    /// Whether a fenced-div closing fence ends the open blockquote rather
    /// than closing a div inside it.
    ///
    /// Pandoc extracts a quote's raw content from wherever the quote starts,
    /// so a `:::` line is quote content only when the div it closes was
    /// opened inside the quote. A div opened *outside* it is still open at
    /// extraction time and its closer ends the quote: `::: a` / `> text` /
    /// `:::` closes the div at the top level and leaves the quote holding
    /// just the paragraph (issue #310). `> ::: a` / `> x` / `:::` is the
    /// other way round — the div lives in the quote, so its closer does too.
    pub(super) fn div_closer_ends_blockquote(&self, line: &str) -> bool {
        if !self.config.extensions.fenced_divs || !fenced_divs::is_div_closing_fence(line) {
            return false;
        }
        let innermost_div = self
            .containers
            .stack
            .iter()
            .rposition(|c| matches!(c, Container::FencedDiv { .. }));
        let innermost_quote = self
            .containers
            .stack
            .iter()
            .rposition(|c| matches!(c, Container::BlockQuote { .. }));
        match (innermost_div, innermost_quote) {
            (Some(div), Some(quote)) => div < quote,
            (Some(_), None) => true,
            (None, _) => false,
        }
    }

    /// Fold a lazy line back into the open blockquote, pandoc-style.
    ///
    /// Emits the `>` markers the line carries, then the leading whitespace
    /// pandoc's gobble skips as a bare `WHITESPACE` token, and parses what is
    /// left one level down. Dropping the indent before the inner parse is the
    /// point: the quote's raw content never sees it, so it can neither
    /// continue a line block nor open an indented code block.
    pub(super) fn fold_lazy_line_into_blockquote(
        &mut self,
        fold: LazyFold<'a>,
        blockquote_payload: Option<&BlockQuotePrepared>,
    ) -> LineDispatch {
        if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
            self.close_containers_to(self.containers.depth() - 1);
        }

        if fold.bq_depth > 0 {
            let marker_info = self.marker_info_for_line(
                blockquote_payload,
                fold.line,
                fold.bq_marker_line,
                fold.shifted_bq_prefix,
                fold.used_shifted_bq,
            );
            for i in 0..fold.bq_depth {
                if let Some(info) = marker_info.get(i) {
                    self.emit_or_buffer_blockquote_marker(
                        info.leading_spaces,
                        info.has_trailing_space,
                    );
                }
            }
        }

        let rest = fold.inner_content.trim_start_matches([' ', '\t']);
        let indent = &fold.inner_content[..fold.inner_content.len() - rest.len()];
        if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
            self.emit_list_item_buffer_if_needed();
            if self.lazy_content_opens_fence(rest).is_some() {
                self.close_lists_above_indent(0);
            }
        }
        if !indent.is_empty() {
            self.builder.token(SyntaxKind::WHITESPACE.into(), indent);
        }

        self.lines[self.pos] = rest;

        self.parse_inner_content(rest, Some(rest))
    }

    /// Emit or buffer a blockquote marker depending on parser state.
    ///
    /// If a paragraph is open and we're using integrated parsing, buffer the marker.
    /// Otherwise emit it directly to the builder.
    pub(super) fn emit_or_buffer_blockquote_marker(
        &mut self,
        leading_spaces: usize,
        has_trailing_space: bool,
    ) {
        if let Some(Container::ListItem {
            buffer,
            marker_only,
            ..
        }) = self.containers.stack.last_mut()
        {
            buffer.push_blockquote_marker(leading_spaces, has_trailing_space);
            *marker_only = false;
            return;
        }

        if let Some(Container::Definition {
            plain_open: true,
            plain_buffer,
            ..
        }) = self.containers.stack.last_mut()
        {
            plain_buffer.push_marker(leading_spaces, has_trailing_space);
            return;
        }

        if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
            paragraphs::append_paragraph_marker(
                &mut self.containers,
                leading_spaces,
                has_trailing_space,
            );
        } else {
            blockquotes::emit_one_blockquote_marker(
                &mut self.builder,
                leading_spaces,
                has_trailing_space,
            );
        }
    }
}
