use super::*;

impl<'a> Parser<'a> {
    /// Whether an HTML block about to interrupt an open paragraph should
    /// retag the paragraph wrapper as `PLAIN` (pandoc's
    /// `[Plain[foo], RawBlock<p>]` rule). Fires only under Pandoc dialect
    /// when the YesCanInterrupt match is an HTML `BlockTag` — by
    /// construction this is a strict-block (`PANDOC_BLOCK_TAGS`) or
    /// verbatim (`VERBATIM_TAGS`) tag, since inline-block / void block
    /// tags and Type7 / comments take the `cannot_interrupt` path and
    /// never reach this site.
    pub(super) fn html_block_demotes_paragraph_to_plain(
        &self,
        block_match: &PreparedBlockMatch,
    ) -> bool {
        if self.config.dialect != crate::options::Dialect::Pandoc {
            return false;
        }
        if self.block_registry.parser_name(block_match) != "html_block" {
            return false;
        }
        let html_block_type = block_match
            .payload
            .as_ref()
            .and_then(|p| p.downcast_ref::<crate::parser::blocks::html_blocks::HtmlBlockType>());
        matches!(
            html_block_type,
            Some(crate::parser::blocks::html_blocks::HtmlBlockType::BlockTag { .. })
        )
    }

    /// Dispatch a leading HTML block on a definition body's marker line
    /// (`:   <html>`).
    ///
    /// The first content line of a definition body otherwise flows into the
    /// buffered-plain path, which only special-cases ATX headings — a raw
    /// HTML block there would be parsed as inline text (`RawInline` inside a
    /// `Para`) instead of a structural block (`RawBlock` / `Div`), diverging
    /// from pandoc-native. This mirrors the blockquote / list / fenced-code
    /// arms of the definition first-content-line cascade. HTML that appears
    /// on a *later* definition-body line already dispatches correctly through
    /// the normal container path.
    ///
    /// Scope: only blocks that CLOSE on the marker line are lifted here. The
    /// extent is probed by parsing a synthetic line window (line 0 = the
    /// already-stripped post-marker bytes; continuation lines = the raw
    /// following lines with the outer container prefix stripped) into a
    /// throwaway builder and checking the block consumes exactly one line.
    /// Multi-line HTML bodies that open on the marker line fall through to
    /// the buffered-plain path (deferred — they need content-indent
    /// strip/re-inject for the continuation lines).
    ///
    /// Returns `Some(0)` when a marker-line HTML block was emitted (no lines
    /// beyond the marker are consumed), or `None` when no leading HTML block
    /// closes on the marker line.
    pub(super) fn try_dispatch_definition_html_block(
        &mut self,
        content_line: &str,
        content_col: usize,
    ) -> Option<usize> {
        let is_commonmark = self.config.dialect == crate::options::Dialect::CommonMark;
        let (content_no_nl, _) = strip_newline(content_line);
        let block_type = html_blocks::try_parse_html_block_start(content_no_nl, is_commonmark)?;

        let bq_depth = self.current_blockquote_depth();
        let content_prefix = ContainerPrefix::from_scalars(
            bq_depth,
            0,
            bq_depth > 0,
            content_col,
            false,
            self.config.dialect,
        );
        let probe_consumed = {
            let mut synthetic: Vec<&str> = Vec::with_capacity(self.lines.len() - self.pos);
            synthetic.push(content_line);
            for line in &self.lines[self.pos + 1..] {
                synthetic.push(content_prefix.strip(line));
            }
            let mut probe = GreenNodeBuilder::new();
            probe.start_node(SyntaxKind::DOCUMENT.into());
            let consumed = html_blocks::parse_html_block_with_wrapper(
                &mut probe,
                &synthetic,
                0,
                block_type.clone(),
                &ContainerPrefix::default(),
                SyntaxKind::HTML_BLOCK,
                html_blocks::SoftbreakFusion::None,
                self.config,
                None,
                None,
            );
            probe.finish_node();
            consumed
        };
        if probe_consumed == 0 {
            return None;
        }

        let wrapper_kind =
            marker_line_html_block_wrapper_kind(&block_type, content_no_nl, self.config);

        if probe_consumed == 1 {
            if self.config.dialect == crate::options::Dialect::Pandoc
                && bq_depth == 0
                && let Some(extras) = self.try_fuse_definition_comment_trailing(
                    content_line,
                    content_no_nl,
                    content_col,
                )
            {
                return Some(extras);
            }

            let single = [content_line];
            html_blocks::parse_html_block_with_wrapper(
                &mut self.builder,
                &single,
                0,
                block_type,
                &ContainerPrefix::default(),
                wrapper_kind,
                html_blocks::SoftbreakFusion::None,
                self.config,
                None,
                None,
            );
            return Some(0);
        }

        if self.config.dialect != crate::options::Dialect::Pandoc || bq_depth != 0 {
            return None;
        }
        let mut text = String::from(content_line);
        for line in &self.lines[self.pos + 1..self.pos + probe_consumed] {
            text.push_str(line);
        }
        let use_paragraph = self.pos > 0 && is_blank_line(self.lines[self.pos - 1]);
        let lifted = crate::parser::utils::list_item_buffer::try_emit_html_block_lift(
            &mut self.builder,
            &text,
            self.config,
            &[content_col],
            &[],
            use_paragraph,
            "",
            false,
        );
        if !lifted {
            return None;
        }
        Some(probe_consumed.saturating_sub(1))
    }

    /// Dispatch an HTML block that opens on a *later* (non-marker) line of a
    /// content-container body (definition, footnote, admonition) whose lines
    /// carry a `content_col` indent. The general dispatcher's
    /// `parse_html_block_with_wrapper` ignores the `ContentIndent` prefix op,
    /// dropping the stripped indent (losslessness fail) and reparsing the body
    /// with its indent intact (an indented `CodeBlock` instead of markdown).
    /// This routes the block through the list-item lift, which strips
    /// `content_col`, reparses the dedented body as markdown, and re-injects
    /// the indent per line. The line-0 indent is injected *inside* the lifted
    /// block (as the open tag's leading `WHITESPACE`, via the lift's
    /// `line0_prefix` arg) rather than as a sibling — the formatter dumps HTML
    /// blocks verbatim and the `DEFINITION` formatter drops direct `WHITESPACE`
    /// children, so a sibling indent would vanish on format.
    ///
    /// `stripped_content` is the current line with its content indent already
    /// removed; `content_col` is that indent width; `indent_to_emit` is the
    /// stripped indent bytes for line 0. Returns the number of lines consumed
    /// on success. The caller has established Pandoc dialect, top-level
    /// blockquote depth 0, and an open content container.
    pub(super) fn try_dispatch_content_indent_html_block(
        &mut self,
        stripped_content: &str,
        content_col: usize,
        indent_to_emit: Option<&str>,
    ) -> Option<usize> {
        let (content_no_nl, _) = strip_newline(stripped_content);
        let block_type = html_blocks::try_parse_html_block_start(content_no_nl, false)?;

        let content_prefix =
            ContainerPrefix::from_scalars(0, 0, false, content_col, false, self.config.dialect);
        let probe_consumed = {
            let mut synthetic: Vec<&str> = Vec::with_capacity(self.lines.len() - self.pos);
            synthetic.push(stripped_content);
            for line in &self.lines[self.pos + 1..] {
                synthetic.push(content_prefix.strip(line));
            }
            let mut probe = GreenNodeBuilder::new();
            probe.start_node(SyntaxKind::DOCUMENT.into());
            let consumed = html_blocks::parse_html_block_with_wrapper(
                &mut probe,
                &synthetic,
                0,
                block_type.clone(),
                &ContainerPrefix::default(),
                SyntaxKind::HTML_BLOCK,
                html_blocks::SoftbreakFusion::None,
                self.config,
                None,
                None,
            );
            probe.finish_node();
            consumed
        };
        if probe_consumed == 0 {
            return None;
        }

        let mut text = String::from(stripped_content);
        for line in &self.lines[self.pos + 1..self.pos + probe_consumed] {
            text.push_str(line);
        }
        let use_paragraph = self.pos > 0 && is_blank_line(self.lines[self.pos - 1]);
        let line0_prefix = indent_to_emit.unwrap_or("");

        let lift_ok = {
            let mut probe = GreenNodeBuilder::new();
            probe.start_node(SyntaxKind::DOCUMENT.into());
            let ok = crate::parser::utils::list_item_buffer::try_emit_html_block_lift(
                &mut probe,
                &text,
                self.config,
                &[content_col],
                &[],
                use_paragraph,
                line0_prefix,
                true,
            );
            probe.finish_node();
            ok
        };
        if !lift_ok {
            return None;
        }

        self.emit_buffered_plain_if_needed();
        self.prepare_for_block_element();
        crate::parser::utils::list_item_buffer::try_emit_html_block_lift(
            &mut self.builder,
            &text,
            self.config,
            &[content_col],
            &[],
            use_paragraph,
            line0_prefix,
            true,
        );
        Some(probe_consumed)
    }

    /// Blockquote-nested variant of
    /// [`Self::try_dispatch_content_indent_html_block`]. When the
    /// content-container body sits inside one or more blockquotes
    /// (`> :   text\n>\n>     <div>\n>     x\n>     </div>`), the later-line
    /// HTML block's continuation lines carry both the `> ` markers and the
    /// content indent. The `bq_depth == 0` path can't handle them (its lift
    /// strips only spaces), so the general dispatcher used to fall through
    /// and silently drop the line-0 content indent (a losslessness
    /// violation) while reparsing the body as an indented `CodeBlock`.
    ///
    /// Here we pre-strip every continuation line with the full container
    /// prefix (bq markers + content indent), reparse the dedented body, and
    /// re-inject the captured `>     ` prefix bytes per line during graft so
    /// the CST stays byte-equal to source and the body lifts to `Div [Para
    /// x]`, matching pandoc's block structure. Line 0's outer `> ` marker was
    /// already emitted upstream by the blockquote container, so only its
    /// content indent is re-injected inside the lifted block.
    ///
    /// Gated to Pandoc. Returns the number of lines consumed on success.
    pub(super) fn try_dispatch_bq_content_indent_html_block(
        &mut self,
        stripped_content: &str,
        content_col: usize,
        indent_to_emit: Option<&str>,
    ) -> Option<usize> {
        use crate::parser::blocks::container_prefix::ContainerPrefixLine;

        if self.config.dialect != crate::options::Dialect::Pandoc {
            return None;
        }
        let bq_depth = self.current_blockquote_depth();
        if bq_depth == 0 {
            return None;
        }

        let (content_no_nl, _) = strip_newline(stripped_content);
        let block_type = html_blocks::try_parse_html_block_start(content_no_nl, false)?;

        let content_prefix = ContainerPrefix::from_scalars(
            bq_depth,
            0,
            true,
            content_col,
            false,
            self.config.dialect,
        );

        let probe_consumed = {
            let mut synthetic: Vec<&str> = Vec::with_capacity(self.lines.len() - self.pos);
            synthetic.push(stripped_content);
            for line in &self.lines[self.pos + 1..] {
                synthetic.push(content_prefix.strip(line));
            }
            let mut probe = GreenNodeBuilder::new();
            probe.start_node(SyntaxKind::DOCUMENT.into());
            let consumed = html_blocks::parse_html_block_with_wrapper(
                &mut probe,
                &synthetic,
                0,
                block_type,
                &ContainerPrefix::default(),
                SyntaxKind::HTML_BLOCK,
                html_blocks::SoftbreakFusion::None,
                self.config,
                None,
                None,
            );
            probe.finish_node();
            consumed
        };
        if probe_consumed == 0 {
            return None;
        }

        let mut parse_text = String::from(stripped_content);
        let mut prefix_lines: Vec<ContainerPrefixLine> = vec![ContainerPrefixLine::list_only(
            indent_to_emit.unwrap_or("").to_string(),
        )];
        for line in &self.lines[self.pos + 1..self.pos + probe_consumed] {
            let stripped = content_prefix.strip(line);
            let captured = &line[..line.len() - stripped.len()];
            parse_text.push_str(stripped);
            prefix_lines.push(ContainerPrefixLine::bq_only(captured.to_string()));
        }
        let use_paragraph = self.pos > 0 && is_blank_line(self.lines[self.pos - 1]);

        let lift_ok = {
            let mut probe = GreenNodeBuilder::new();
            probe.start_node(SyntaxKind::DOCUMENT.into());
            let ok = crate::parser::utils::list_item_buffer::emit_html_block_lift_from_stripped(
                &mut probe,
                &parse_text,
                self.config,
                prefix_lines.clone(),
                use_paragraph,
                true,
            );
            probe.finish_node();
            ok
        };
        if !lift_ok {
            return None;
        }

        self.emit_buffered_plain_if_needed();
        self.prepare_for_block_element();
        crate::parser::utils::list_item_buffer::emit_html_block_lift_from_stripped(
            &mut self.builder,
            &parse_text,
            self.config,
            prefix_lines,
            use_paragraph,
            true,
        );
        Some(probe_consumed)
    }

    /// Fuse a definition-body comment/PI close-line trailing text with its
    /// following non-blank continuation lines into one paragraph, matching
    /// pandoc (`:   <!-- --> t\n    more` -> `RawBlock, Para/Plain [t,
    /// SoftBreak, more]`). Returns the number of continuation lines consumed
    /// on success (grafted as siblings of the definition), or `None` when
    /// there is nothing to fuse (no trailing text, no continuation, or the
    /// window doesn't reparse to a clean `RawBlock` + single-paragraph split).
    /// The caller has already established Pandoc dialect + `bq_depth == 0`.
    pub(super) fn try_fuse_definition_comment_trailing(
        &mut self,
        content_line: &str,
        content_no_nl: &str,
        content_col: usize,
    ) -> Option<usize> {
        let trimmed = content_no_nl.trim_start();
        let marker = if trimmed.starts_with("<!--") {
            "-->"
        } else if trimmed.starts_with("<?") {
            "?>"
        } else {
            return None;
        };
        let close = content_no_nl.find(marker)?;
        let trailing = &content_no_nl[close + marker.len()..];
        if trailing.trim().is_empty() {
            return None;
        }

        let mut fuse_count = 0usize;
        while self.pos + 1 + fuse_count < self.lines.len() {
            let line = self.lines[self.pos + 1 + fuse_count];
            if is_blank_line(line) {
                break;
            }
            let stripped = strip_leading_spaces_n(line, content_col);
            if lists::try_parse_list_marker(stripped, self.config, lists::OpenListHint::None)
                .is_some()
            {
                break;
            }
            fuse_count += 1;
        }
        if fuse_count == 0 {
            return None;
        }

        let mut text = String::from(content_line);
        for line in &self.lines[self.pos + 1..self.pos + 1 + fuse_count] {
            text.push_str(line);
        }
        let use_paragraph = self.pos > 0 && is_blank_line(self.lines[self.pos - 1]);
        let lifted = crate::parser::utils::list_item_buffer::try_emit_html_block_lift(
            &mut self.builder,
            &text,
            self.config,
            &[content_col],
            &[],
            use_paragraph,
            "",
            false,
        );
        if !lifted {
            return None;
        }
        Some(fuse_count)
    }

    /// Dispatch an HTML block that opens AND closes on a footnote body's first
    /// content line (`[^1]: <div>x</div>`). Mirrors
    /// `try_dispatch_definition_html_block`, but only tags that can interrupt a
    /// paragraph lift: pandoc keeps comments, PIs, `<span>`, and void
    /// inline-block tags (`<embed>`) inline inside footnote bodies (unlike
    /// definition bodies, where a leading comment lifts to a `RawBlock`). Gated
    /// on Pandoc dialect so GFM/CommonMark footnotes stay byte-identical.
    /// Returns `Some(0)` when the block was emitted (no extra lines consumed).
    ///
    /// Not unified with [`Parser::lazy_interrupts`]: this answers a different
    /// question (does the block *close on the marker line*?) via a synthetic
    /// re-parse with emission side effects; the shared atom is
    /// `html_block_cannot_interrupt`.
    pub(super) fn try_dispatch_footnote_html_block(
        &mut self,
        first_line_content: &str,
        content_col: usize,
    ) -> Option<usize> {
        if self.config.dialect != crate::options::Dialect::Pandoc {
            return None;
        }
        let (content_no_nl, _) = strip_newline(first_line_content);
        let block_type = html_blocks::try_parse_html_block_start(content_no_nl, false)?;
        if crate::parser::block_dispatcher::html_block_cannot_interrupt(
            &block_type,
            content_no_nl,
            true,
        ) {
            return None;
        }

        let closes_on_marker_line = {
            let bq_depth = self.current_blockquote_depth();
            let prefix = ContainerPrefix::from_scalars(
                bq_depth,
                0,
                bq_depth > 0,
                content_col,
                false,
                self.config.dialect,
            );
            let mut synthetic: Vec<&str> = Vec::with_capacity(self.lines.len() - self.pos);
            synthetic.push(first_line_content);
            for line in &self.lines[self.pos + 1..] {
                synthetic.push(prefix.strip(line));
            }
            let mut probe = GreenNodeBuilder::new();
            probe.start_node(SyntaxKind::DOCUMENT.into());
            let consumed = html_blocks::parse_html_block_with_wrapper(
                &mut probe,
                &synthetic,
                0,
                block_type.clone(),
                &ContainerPrefix::default(),
                SyntaxKind::HTML_BLOCK,
                html_blocks::SoftbreakFusion::None,
                self.config,
                None,
                None,
            );
            probe.finish_node();
            consumed == 1
        };
        if !closes_on_marker_line {
            return None;
        }

        let wrapper_kind =
            marker_line_html_block_wrapper_kind(&block_type, content_no_nl, self.config);
        let single = [first_line_content];
        html_blocks::parse_html_block_with_wrapper(
            &mut self.builder,
            &single,
            0,
            block_type,
            &ContainerPrefix::default(),
            wrapper_kind,
            html_blocks::SoftbreakFusion::None,
            self.config,
            None,
            None,
        );
        Some(0)
    }

    /// Whether a lazy line opens an HTML block that interrupts the open
    /// paragraph (or a list item's buffered text) inside a blockquote.
    ///
    /// Mirrors the dispatcher's HTML detection for the lazy path, which
    /// never reaches it: under Pandoc the quote's content parse stops a
    /// paragraph at a strict-block tag (`isBlockTag` minus the
    /// `isInlineTag` special cases — `html_block_cannot_interrupt`), so
    /// `> a` / `<hr>` is `BlockQuote [Plain "a", RawBlock "<hr>"]`, not
    /// lazy inline text. Declarations and CDATA are not raw HTML to
    /// pandoc-markdown, and an open tag with no unquoted `>` anywhere
    /// ahead never matches `htmlTag`; both stay paragraph text. Under
    /// CommonMark a type 1-6 start is not paragraph-continuation text
    /// (spec §5.1), so laziness does not fire and the quote closes.
    ///
    /// `inner_content` is the line with its `>` markers stripped, same
    /// as the other interrupt probes (issues #428, #429).
    pub(super) fn lazy_html_block_interrupts(&self, inner_content: &str) -> bool {
        if !self.config.extensions.raw_html {
            return false;
        }
        if self.config.dialect == crate::options::Dialect::CommonMark {
            let bytes = inner_content.as_bytes();
            let mut i = 0;
            while i < bytes.len() && i < 3 && bytes[i] == b' ' {
                i += 1;
            }
            if bytes.get(i) != Some(&b'<') {
                return false;
            }
            return html_blocks::try_parse_html_block_start(inner_content, true).is_some_and(
                |block_type| {
                    !crate::parser::block_dispatcher::html_block_cannot_interrupt(
                        &block_type,
                        inner_content,
                        false,
                    )
                },
            );
        }
        let probe = inner_content.trim_start_matches([' ', '\t']);
        if !probe.starts_with('<') {
            return false;
        }
        let Some(block_type) = html_blocks::try_parse_html_block_start(probe, false) else {
            return false;
        };
        if !matches!(block_type, html_blocks::HtmlBlockType::BlockTag { .. }) {
            return false;
        }
        if crate::parser::block_dispatcher::html_block_cannot_interrupt(&block_type, probe, true) {
            return false;
        }
        let prefix = ContainerPrefix::from_stack(&self.containers.stack, false, self.config)
            .without_innermost_list_advance();
        html_blocks::pandoc_html_block_start_is_valid(&self.lines, self.pos, &prefix, &block_type)
    }

    /// Pandoc's `notFollowedByHtmlCloser` on the quote gobble: inside
    /// markdown-in-html, a line opening with the close form of the open
    /// tag ends the quote instead of folding in as lazy content. The
    /// open-tag signal lives in a list item's buffer below the quote
    /// (markdown-in-html is not a stack transition here), so the
    /// ordering gate and the line test are the prefix's — the same seam
    /// the table end scans consult (`tables::html_closer_ends_lines`).
    pub(super) fn html_closer_ends_blockquote(&self, line: &str) -> bool {
        let prefix = ContainerPrefix::from_stack(&self.containers.stack, false, self.config);
        tables::html_closer_ends_lines(&prefix, line)
    }

    /// Look up the immediate enclosing `Container::ListItem`'s buffer for an
    /// unclosed Pandoc matched-pair HTML open tag. See
    /// [`crate::parser::utils::list_item_buffer::ListItemBuffer::unclosed_pandoc_matched_pair_tag`]
    /// for the gate; used to populate
    /// `BlockContext::list_item_unclosed_html_block_tag` so the dispatcher
    /// can suppress the close-form match that would otherwise interrupt
    /// `- <div>\n  body\n  </div>` and friends.
    pub(super) fn list_item_unclosed_html_block_tag(&self) -> Option<String> {
        let Container::ListItem { buffer, .. } = self.containers.stack.last()? else {
            return None;
        };
        buffer.unclosed_pandoc_matched_pair_tag(self.config)
    }
}

pub(super) fn marker_line_html_block_wrapper_kind(
    block_type: &html_blocks::HtmlBlockType,
    content_no_nl: &str,
    config: &ParserOptions,
) -> SyntaxKind {
    match block_type {
        html_blocks::HtmlBlockType::BlockTag {
            tag_name,
            is_closing: false,
            ..
        } if tag_name == "div"
            && config.dialect == crate::options::Dialect::Pandoc
            && config.extensions.native_divs
            && html_blocks::probe_open_tag_line_has_close_gt(content_no_nl, "div") =>
        {
            SyntaxKind::HTML_BLOCK_DIV
        }
        _ => SyntaxKind::HTML_BLOCK,
    }
}
