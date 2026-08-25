use crate::config::WrapMode;
use crate::formatter::indent_utils::{calculate_list_item_indent, is_alignable_marker};
use crate::formatter::inline_layout::{self, WrapStrategy};
use crate::formatter::tables;
use crate::syntax::{AstNode, DefinitionItem, FencedDiv, SyntaxKind, SyntaxNode};
use panache_parser::parser::blocks::definition_lists::try_parse_definition_marker;
use panache_parser::parser::blocks::headings::try_parse_atx_heading;
use rowan::NodeOrToken;

use super::Formatter;
use super::preserve::{self, preserve_lines};
use super::utils::is_block_element;

impl Formatter {
    pub(super) fn format_definition_list(&mut self, node: &SyntaxNode, indent: usize) {
        if self.is_grid_table_caption_definition_list(node) {
            self.output.push_str(&node.text().to_string());
            if !self.output.ends_with('\n') {
                self.output.push('\n');
            }
            return;
        }
        if indent == 0 && !self.output.is_empty() && !self.output.ends_with("\n\n") {
            self.output.push('\n');
        }
        let mut saw_item = false;
        for child in node.children() {
            if child.kind() == SyntaxKind::BLANK_LINE {
                continue;
            }
            if child.kind() == SyntaxKind::DEFINITION_ITEM {
                if saw_item && !self.output.ends_with("\n\n") {
                    self.output.push('\n');
                }
                saw_item = true;
            }
            self.format_node_sync(&child, indent);
        }
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
    }

    pub(super) fn format_definition_item(&mut self, node: &SyntaxNode, indent: usize) {
        let is_compact_by_structure = DefinitionItem::cast(node.clone())
            .map(|item| item.is_compact())
            .unwrap_or(true);
        let mut has_blank_between_term_and_first_definition = false;
        let mut seen_term = false;
        let mut seen_definition = false;

        for child in node.children() {
            match child.kind() {
                SyntaxKind::TERM => {
                    seen_term = true;
                }
                SyntaxKind::BLANK_LINE => {
                    if seen_term && !seen_definition {
                        has_blank_between_term_and_first_definition = true;
                    }
                }
                SyntaxKind::DEFINITION => {
                    seen_definition = true;
                }
                _ => {}
            }
        }

        let is_compact = is_compact_by_structure && !has_blank_between_term_and_first_definition;
        let mut saw_term = false;

        for child in node.children() {
            match child.kind() {
                SyntaxKind::BLANK_LINE => {}
                SyntaxKind::TERM => {
                    self.format_node_sync(&child, indent);
                    saw_term = true;
                }
                SyntaxKind::DEFINITION => {
                    if saw_term {
                        if is_compact {
                            if !self.output.ends_with('\n') {
                                self.output.push('\n');
                            }
                        } else if !self.output.ends_with("\n\n") {
                            self.output.push('\n');
                        }
                    } else if !self.output.is_empty() && !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    self.format_node_sync(&child, indent);
                }
                _ => self.format_node_sync(&child, indent),
            }
        }
    }

    pub(super) fn format_term(&mut self, node: &SyntaxNode, indent: usize) {
        if indent > 0 && (self.output.is_empty() || self.output.ends_with('\n')) {
            self.output.push_str(&" ".repeat(indent));
        }
        for child in node.children_with_tokens() {
            match child {
                NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::TEXT => {
                    self.output.push_str(tok.text());
                }
                NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::NEWLINE => {
                    self.output.push('\n');
                }
                NodeOrToken::Node(n) => {
                    self.format_node_sync(&n, indent);
                }
                _ => {}
            }
        }
    }

    pub(super) fn format_definition(&mut self, node: &SyntaxNode, indent: usize) {
        let def_indent = indent + 4;
        let saved_wrap = Self::reflow_would_promote_a_definition_term(node)
            .then(|| self.config.wrap.replace(WrapMode::Preserve));
        let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);

        if indent > 0 {
            self.output.push_str(&" ".repeat(indent));
        }
        self.output.push_str(":   ");

        let children: Vec<_> = node.children_with_tokens().collect();
        let mut first_para_idx = None;

        let mut text_idx = None;
        for (i, child) in children.iter().enumerate() {
            if let NodeOrToken::Token(tok) = child
                && tok.kind() == SyntaxKind::TEXT
            {
                text_idx = Some(i);
            }
        }

        if let Some(tidx) = text_idx {
            for (i, child) in children.iter().enumerate().skip(tidx + 1) {
                if let NodeOrToken::Node(n) = child {
                    match n.kind() {
                        SyntaxKind::PARAGRAPH => {
                            first_para_idx = Some(i);
                            break;
                        }
                        SyntaxKind::BLANK_LINE => {
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        for (i, child) in children.iter().enumerate() {
            match child {
                NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::TEXT => {
                    self.output.push_str(tok.text());
                }
                NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::NEWLINE => {
                    let bare_marker_pull_up = self.output.ends_with(":   ")
                        && children.get(i + 1).is_some_and(|next| match next {
                            NodeOrToken::Node(n) if n.kind() == SyntaxKind::PLAIN => {
                                let first_line = n
                                    .text()
                                    .to_string()
                                    .lines()
                                    .next()
                                    .unwrap_or_default()
                                    .trim_start_matches([' ', '\t'])
                                    .to_string();
                                try_parse_atx_heading(&first_line).is_none()
                            }
                            NodeOrToken::Node(n) => is_block_element(n.kind()),
                            _ => false,
                        });
                    if first_para_idx.is_some_and(|idx| i + 1 == idx) {
                        self.output.push(' ');
                    } else if !bare_marker_pull_up {
                        if self.output.ends_with(":   ") {
                            self.output.truncate(self.output.len() - 3);
                        }
                        self.output.push('\n');
                    }
                }
                NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::DEFINITION_MARKER => {}
                NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::WHITESPACE => {}
                NodeOrToken::Node(n) => match n.kind() {
                    SyntaxKind::CODE_BLOCK => {
                        if self.output.ends_with(":   ") {
                            self.format_container_code_block(
                                n,
                                "",
                                def_indent,
                                true,
                                Some(Self::container_content_offset(n)),
                                false,
                            );
                        } else {
                            if !self.output.ends_with("\n\n") {
                                self.output.push('\n');
                            }
                            self.format_indented_code_block(n, def_indent);
                        }
                    }
                    SyntaxKind::HEADING => {
                        self.output.push_str(&self.format_heading(n));
                        self.output.push('\n');

                        let has_following_blocks =
                            children.iter().skip(i + 1).any(|sib| match sib {
                                NodeOrToken::Node(sn) => sn.kind() != SyntaxKind::BLANK_LINE,
                                _ => false,
                            });
                        let next_is_blank_line = children.get(i + 1).is_some_and(|sib| {
                            matches!(
                                sib,
                                NodeOrToken::Node(sn) if sn.kind() == SyntaxKind::BLANK_LINE
                            )
                        });
                        if has_following_blocks && !next_is_blank_line {
                            self.output.push('\n');
                        }
                    }
                    SyntaxKind::PLAIN => {
                        if let Some((heading_line, remainder)) =
                            self.leading_atx_heading_with_remainder(n)
                        {
                            self.output.push_str(&heading_line);
                            self.output.push('\n');
                            self.output.push('\n');
                            for line in self.wrap_text_for_indent(&remainder, def_indent) {
                                self.output.push_str(&" ".repeat(def_indent));
                                self.output.push_str(line.trim_start());
                                self.output.push('\n');
                            }
                        } else {
                            self.format_node_sync(n, def_indent);
                        }
                    }
                    SyntaxKind::PARAGRAPH => {
                        if first_para_idx == Some(i) {
                            let marker_len = ":   ".len();
                            let first_line_space =
                                self.config.line_width.saturating_sub(indent + marker_len);
                            let available_width = self.config.line_width.saturating_sub(def_indent);
                            let widths = [first_line_space, available_width];

                            let lines = match wrap_mode {
                                WrapMode::Preserve => preserve_lines(
                                    n,
                                    self.config.formatter_extensions.escaped_line_breaks,
                                ),
                                WrapMode::Reflow => {
                                    self.wrapped_lines_for_paragraph_with_widths(n, &widths)
                                }
                                WrapMode::Sentence => self.sentence_lines_for_paragraph(n),
                                WrapMode::Semantic => self.semantic_lines_for_paragraph(n),
                            };

                            if !lines.is_empty() {
                                self.output.push_str(&lines[0]);
                                self.output.push('\n');
                                for line in lines.iter().skip(1) {
                                    self.output.push_str(&" ".repeat(def_indent));
                                    self.output.push_str(line.trim_start());
                                    self.output.push('\n');
                                }
                            }
                        } else {
                            if !self.output.ends_with("\n\n") {
                                self.output.push('\n');
                            }
                            self.format_list_continuation_paragraph(n, def_indent);
                        }
                    }
                    SyntaxKind::BLANK_LINE => {
                        let is_before_first_para = first_para_idx.is_some_and(|idx| i < idx);

                        if !is_before_first_para {
                            self.output.push('\n');
                        }
                    }
                    SyntaxKind::LIST => {
                        let start = self.output.len();
                        self.format_node_sync(n, def_indent);

                        if self.output[..start].ends_with(":   ")
                            && self.output[start..].starts_with(&" ".repeat(def_indent))
                        {
                            self.output.drain(start..start + def_indent);
                        }
                    }
                    SyntaxKind::BLOCK_QUOTE if self.output.ends_with(":   ") => {
                        let mut pieces: Vec<String> = Vec::new();
                        let block_text = n.text().to_string();
                        for line in block_text.lines() {
                            let trimmed = line.trim_start();
                            let content = if let Some(rest) = trimmed.strip_prefix('>') {
                                rest.trim_start()
                            } else {
                                trimmed
                            };
                            if !content.is_empty() {
                                pieces.push(content.to_string());
                            }
                        }

                        self.output.push_str("> ");
                        self.output.push_str(&pieces.join(" "));
                        self.output.push('\n');

                        if let Some(next_non_blank) = node
                            .children()
                            .skip(i + 1)
                            .find(|sibling| sibling.kind() != SyntaxKind::BLANK_LINE)
                            && is_block_element(next_non_blank.kind())
                            && !self.output.ends_with("\n\n")
                        {
                            self.output.push('\n');
                        }
                    }
                    SyntaxKind::BLOCK_QUOTE => self.format_node_sync(n, def_indent),
                    _ => {
                        self.format_node_sync(n, def_indent);
                    }
                },
                _ => {}
            }
        }
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        if let Some(saved) = saved_wrap {
            self.config.wrap = saved;
        }
    }
}

impl Formatter {
    fn is_marker_only_blockquote_continuation(node: &SyntaxNode) -> bool {
        if !matches!(node.kind(), SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH) {
            return false;
        }

        let mut has_blockquote_marker = false;
        let mut has_meaningful_content = false;

        for element in node.children_with_tokens() {
            match element {
                NodeOrToken::Token(token) => match token.kind() {
                    SyntaxKind::LINE_PREFIX => {
                        if token.text().contains('>') {
                            has_blockquote_marker = true;
                        }
                    }
                    SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => {}
                    _ => {
                        if !token.text().trim().is_empty() {
                            has_meaningful_content = true;
                        }
                    }
                },
                NodeOrToken::Node(child) => {
                    if child.kind() != SyntaxKind::WHITESPACE && child.kind() != SyntaxKind::NEWLINE
                    {
                        has_meaningful_content = true;
                    }
                }
            }
        }

        has_blockquote_marker && !has_meaningful_content
    }

    fn has_open_only_fenced_div(item: &SyntaxNode) -> bool {
        item.descendants().any(|node| {
            let Some(fenced_div) = FencedDiv::cast(node) else {
                return false;
            };

            !fenced_div.has_closing_fence()
                && !fenced_div
                    .body_blocks()
                    .any(|body_child| body_child.kind() != SyntaxKind::BLANK_LINE)
        })
    }

    fn has_continuation_eligible_predecessor(node: &SyntaxNode) -> bool {
        let mut prev = node.prev_sibling();
        while let Some(sibling) = prev {
            match sibling.kind() {
                SyntaxKind::BLANK_LINE => prev = sibling.prev_sibling(),
                SyntaxKind::LIST_ITEM | SyntaxKind::PARAGRAPH | SyntaxKind::CODE_BLOCK => {
                    return true;
                }
                _ => return false,
            }
        }
        false
    }

    fn normalize_task_checkbox(checkbox: &str) -> String {
        if checkbox == "[X]" {
            "[x]".to_string()
        } else {
            checkbox.to_string()
        }
    }

    /// Extract the marker text from a ListItem node
    /// Standardizes bullet list markers to "-" for consistency.
    ///
    /// This helper is used for *width* and *indent* calculations, where
    /// all three bullet characters (`-`, `+`, `*`) are interchangeable
    /// (single byte each), so normalizing here is harmless across dialects.
    /// The marker actually pushed to output goes through dialect-aware
    /// normalization (see `normalize_bullet_for_output`).
    pub(super) fn extract_list_marker(node: &SyntaxNode) -> Option<String> {
        for el in node.children_with_tokens() {
            if let NodeOrToken::Token(t) = el
                && t.kind() == SyntaxKind::LIST_MARKER
            {
                let marker = t.text().to_string();
                if marker.len() == 1 && matches!(marker.as_str(), "-" | "*" | "+") {
                    return Some("-".to_string());
                }
                return Some(marker);
            }
        }
        None
    }

    /// Decide whether to normalize a raw bullet character (`-`/`+`/`*`)
    /// when emitting it. Pandoc-markdown treats them as interchangeable, so
    /// we standardize for visual consistency. CommonMark §5.3 makes the
    /// bullet character semantically meaningful — a list whose marker
    /// changes from `-` to `+` becomes two separate lists (spec example
    /// #301) — so we preserve the source character to keep that grouping
    /// intent intact across re-formats.
    fn normalize_bullet_for_output(&self, raw: &str) -> String {
        let preserve = panache_parser::Dialect::for_flavor(self.config.flavor)
            == panache_parser::Dialect::CommonMark;
        if !preserve && raw.len() == 1 && matches!(raw, "-" | "+" | "*") {
            "-".to_string()
        } else {
            raw.to_string()
        }
    }

    /// Block kinds that carry a list item's inline content on the marker line.
    ///
    /// `FIGURE` sits alongside `PLAIN`/`PARAGRAPH` because pandoc's
    /// `implicit_figures` promotes an image-only item body to a figure, but it
    /// lays out exactly like the plain it replaces.
    fn is_item_content_block(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH | SyntaxKind::FIGURE
        )
    }

    /// Block-level kinds that participate in CMark looseness detection inside
    /// a list item. HTML_BLOCK is intentionally excluded — pandoc treats raw
    /// HTML comments inline, so panache's ignore-directive comments inside an
    /// otherwise-tight item must not flip the list to loose.
    fn is_loose_trigger_block(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::PLAIN
                | SyntaxKind::PARAGRAPH
                | SyntaxKind::FIGURE
                | SyntaxKind::HEADING
                | SyntaxKind::CODE_BLOCK
                | SyntaxKind::BLOCK_QUOTE
                | SyntaxKind::HORIZONTAL_RULE
                | SyntaxKind::LIST
        )
    }

    fn is_empty_nested_list(list_node: &SyntaxNode) -> bool {
        let items: Vec<_> = list_node
            .children()
            .filter(|c| c.kind() == SyntaxKind::LIST_ITEM)
            .collect();

        if items.len() != 1 {
            return false;
        }

        let item = &items[0];

        for child in item.children_with_tokens() {
            match child {
                NodeOrToken::Token(t) => {
                    if matches!(t.kind(), SyntaxKind::TEXT | SyntaxKind::ESCAPED_CHAR) {
                        return false;
                    }
                }
                NodeOrToken::Node(n) => {
                    if !matches!(
                        n.kind(),
                        SyntaxKind::LIST_MARKER | SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE
                    ) {
                        return false;
                    }
                }
            }
        }

        true
    }

    /// Calculate the maximum marker width for all direct ListItem children of a List
    /// Returns 0 if markers shouldn't be aligned
    pub(super) fn calculate_max_marker_width(list_node: &SyntaxNode) -> usize {
        let markers: Vec<String> = list_node
            .children()
            .filter(|child| child.kind() == SyntaxKind::LIST_ITEM)
            .filter_map(|item| Self::extract_list_marker(&item))
            .collect();

        if !markers.iter().any(|m| is_alignable_marker(m)) {
            return 0;
        }

        markers
            .iter()
            .filter(|m| is_alignable_marker(m))
            .map(|m| m.len())
            .max()
            .unwrap_or(0)
    }

    /// Calculate the content indentation offset for a list item (marker + padding + space)
    /// This is the column where the list item's content starts relative to the list's base indent
    pub(super) fn calculate_list_item_content_indent(
        item_node: &SyntaxNode,
        max_marker_width: usize,
        four_space_rule: bool,
        tab_width: usize,
    ) -> usize {
        let marker = Self::extract_list_marker(item_node).unwrap_or_default();

        let has_checkbox = item_node.children_with_tokens().any(|el| {
            if let NodeOrToken::Token(t) = el {
                t.kind() == SyntaxKind::TASK_CHECKBOX
            } else {
                false
            }
        });

        let indent = calculate_list_item_indent(
            &marker,
            max_marker_width,
            has_checkbox,
            four_space_rule,
            tab_width,
        );
        indent.continuation_offset()
    }

    /// Format a paragraph that is a continuation of a list item.
    /// Strips existing indentation from the text and applies the correct list item indentation.
    pub(super) fn format_list_continuation_paragraph(&mut self, node: &SyntaxNode, indent: usize) {
        let line_width = self.config.line_width.saturating_sub(indent);
        let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
        let para_start = self.output.len();

        match wrap_mode {
            WrapMode::Preserve => {
                let escaped = self.config.formatter_extensions.escaped_line_breaks;
                for line in preserve::preserve_lines(node, escaped) {
                    self.output.push_str(&" ".repeat(indent));
                    self.output.push_str(line.trim_start());
                    self.output.push('\n');
                }
            }
            WrapMode::Reflow => {
                let lines = self.wrapped_lines_for_paragraph(node, line_width);
                for line in lines {
                    self.output.push_str(&" ".repeat(indent));
                    self.output.push_str(&line);
                    self.output.push('\n');
                }
            }
            WrapMode::Sentence | WrapMode::Semantic => {
                let lines = if matches!(wrap_mode, WrapMode::Semantic) {
                    self.semantic_lines_for_paragraph(node)
                } else {
                    self.sentence_lines_for_paragraph(node)
                };
                for line in lines {
                    self.output.push_str(&" ".repeat(indent));
                    self.output.push_str(&line);
                    self.output.push('\n');
                }
            }
        }

        self.guard_definition_marker_start(para_start, indent);
    }

    /// Format a List node
    pub(super) fn format_list(&mut self, node: &SyntaxNode, indent: usize) {
        if indent == 0
            && self.fenced_div_depth == 0
            && !self.output.is_empty()
            && !self.output.ends_with("\n\n")
        {
            self.output.push('\n');
        }

        let max_marker_width = Self::calculate_max_marker_width(node);
        self.max_marker_widths.push(max_marker_width);

        let list_children: Vec<_> = node.children().collect();
        let has_blank_between_items = list_children.iter().enumerate().any(|(idx, child)| {
            if child.kind() != SyntaxKind::BLANK_LINE {
                return false;
            }
            let prev_is_item = idx > 0 && list_children[idx - 1].kind() == SyntaxKind::LIST_ITEM;
            let next_is_item = idx + 1 < list_children.len()
                && list_children[idx + 1].kind() == SyntaxKind::LIST_ITEM;
            prev_is_item && next_is_item
        });
        let has_nested_lists = list_children.iter().any(|child| {
            child.kind() == SyntaxKind::LIST_ITEM
                && child
                    .children()
                    .any(|item_child| item_child.kind() == SyntaxKind::LIST)
        });
        let has_blockquote_children = list_children.iter().any(|child| {
            child.kind() == SyntaxKind::LIST_ITEM
                && child
                    .children()
                    .any(|item_child| matches!(item_child.kind(), SyntaxKind::BLOCK_QUOTE))
        });
        let has_blank_within_item = list_children.iter().any(|child| {
            if child.kind() != SyntaxKind::LIST_ITEM {
                return false;
            }
            let mut saw_block = false;
            for item_child in child.children() {
                let kind = item_child.kind();
                if matches!(kind, SyntaxKind::BLANK_LINE) {
                    if saw_block
                        && item_child
                            .next_sibling()
                            .is_some_and(|s| Self::is_loose_trigger_block(s.kind()))
                    {
                        return true;
                    }
                } else if Self::is_loose_trigger_block(kind) {
                    saw_block = true;
                }
            }
            false
        });
        let has_structural_multi_block = list_children.iter().any(|child| {
            if child.kind() != SyntaxKind::LIST_ITEM {
                return false;
            }
            let block_children: Vec<_> = child
                .children()
                .filter(|c| Self::is_loose_trigger_block(c.kind()))
                .collect();
            if block_children.len() < 2 {
                return false;
            }
            block_children.iter().any(|c| {
                matches!(
                    c.kind(),
                    SyntaxKind::HEADING | SyntaxKind::CODE_BLOCK | SyntaxKind::HORIZONTAL_RULE
                )
            })
        });
        let has_trailing_blank_in_nested_list = list_children.iter().any(|child| {
            if child.kind() != SyntaxKind::LIST_ITEM {
                return false;
            }
            child.children().any(|item_child| {
                item_child.kind() == SyntaxKind::LIST
                    && item_child
                        .children()
                        .last()
                        .is_some_and(|c| c.kind() == SyntaxKind::BLANK_LINE)
            })
        });
        let is_loose = has_blank_between_items
            || has_blockquote_children
            || has_blank_within_item
            || has_structural_multi_block
            || has_trailing_blank_in_nested_list;
        let _ = has_nested_lists;

        log::trace!("Formatting list: is_loose={}", is_loose);

        let mut item_count = 0;
        let total_items = node
            .children()
            .filter(|c| c.kind() == SyntaxKind::LIST_ITEM)
            .count();

        let mut last_item_content_indent = 0;

        for child in node.children() {
            if child.kind() == SyntaxKind::LIST_ITEM {
                let prev_is_fenced_div = child
                    .prev_sibling()
                    .map(|n| n.kind() == SyntaxKind::FENCED_DIV)
                    .unwrap_or(false);
                if prev_is_fenced_div && self.output.ends_with("\n\n") {
                    self.output.pop();
                }
                item_count += 1;

                last_item_content_indent = indent
                    + Self::calculate_list_item_content_indent(
                        &child,
                        max_marker_width,
                        self.config.parser_extensions.four_space_rule,
                        self.config.tab_width,
                    );

                self.format_node_sync(&child, indent);

                if is_loose
                    && item_count < total_items
                    && !self.output.ends_with("\n\n")
                    && !Self::has_open_only_fenced_div(&child)
                {
                    let mut next = child.next_sibling();
                    while let Some(sibling) = next.clone() {
                        if sibling.kind() == SyntaxKind::BLANK_LINE {
                            next = sibling.next_sibling();
                        } else {
                            break;
                        }
                    }
                    let next_non_blank_is_list_item = next
                        .map(|n| n.kind() == SyntaxKind::LIST_ITEM)
                        .unwrap_or(false);
                    if next_non_blank_is_list_item {
                        self.output.push('\n');
                    }
                }
            } else if child.kind() == SyntaxKind::BLANK_LINE {
                let prev_is_item = child
                    .prev_sibling()
                    .map(|n| n.kind() == SyntaxKind::LIST_ITEM)
                    .unwrap_or(false);
                let next_is_item = child
                    .next_sibling()
                    .map(|n| n.kind() == SyntaxKind::LIST_ITEM)
                    .unwrap_or(false);
                let next_is_continuation_list = child
                    .next_sibling()
                    .map(|n| {
                        n.kind() == SyntaxKind::LIST
                            && Self::has_continuation_eligible_predecessor(&n)
                    })
                    .unwrap_or(false);
                if prev_is_item
                    && (next_is_item || next_is_continuation_list)
                    && !self.output.ends_with("\n\n")
                    && (!is_loose || next_is_continuation_list)
                {
                    self.output.push('\n');
                }
                continue;
            } else if child.kind() == SyntaxKind::PARAGRAPH {
                if Self::has_continuation_eligible_predecessor(&child) {
                    self.format_list_continuation_paragraph(&child, last_item_content_indent);
                } else {
                    self.format_node_sync(&child, indent);
                }
            } else if child.kind() == SyntaxKind::CODE_BLOCK {
                if Self::has_continuation_eligible_predecessor(&child) {
                    self.format_indented_code_block(&child, last_item_content_indent);
                } else {
                    self.format_node_sync(&child, indent);
                }
            } else if child.kind() == SyntaxKind::LIST {
                if Self::has_continuation_eligible_predecessor(&child) {
                    self.format_node_sync(&child, last_item_content_indent);
                } else {
                    self.format_node_sync(&child, indent);
                }
            } else {
                self.format_node_sync(&child, indent);
            }
        }

        self.max_marker_widths.pop();

        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
    }

    /// Find Plain or PARAGRAPH child in a ListItem node.
    /// These nodes wrap the text content in Pandoc-style AST.
    /// For nested lists, skip Plain nodes that appear before the ListMarker
    /// (these contain only indentation whitespace).
    fn find_content_node(node: &SyntaxNode) -> Option<SyntaxNode> {
        let mut seen_marker = false;
        let mut seen_leading_html_block = false;
        for el in node.children_with_tokens() {
            match el {
                rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::LIST_MARKER => {
                    seen_marker = true;
                }
                rowan::NodeOrToken::Node(n) if seen_marker => match n.kind() {
                    k if Self::is_item_content_block(k) => {
                        if seen_leading_html_block {
                            return None;
                        }
                        return Some(n);
                    }
                    SyntaxKind::HTML_BLOCK
                    | SyntaxKind::HTML_BLOCK_RAW
                    | SyntaxKind::HTML_BLOCK_DIV => {
                        seen_leading_html_block = true;
                    }
                    SyntaxKind::BLANK_LINE => {}
                    _ => return None,
                },
                _ => {}
            }
        }
        None
    }

    /// Emit a list item's remaining blocks after a leading block that was
    /// rendered abutting the marker (nested list, blockquote, fenced div, code
    /// block, line block), at `hanging` indent.
    ///
    /// A single blank-line separator is preserved between loose blocks. The
    /// blank is structural, not cosmetic: without it two sibling divs glue
    /// together (`:::` immediately followed by `:::  other`, issue #439), and a
    /// paragraph after a nested list or blockquote gets lazily folded back into
    /// that leading block on re-parse (`- - x\n\n  b` -> `- - x b`). Either way
    /// the output re-parses to a different shape and idempotency breaks.
    fn format_item_blocks_after_leading(
        &mut self,
        node: &SyntaxNode,
        leading: &SyntaxNode,
        hanging: usize,
    ) {
        let mut reached_leading = false;
        let mut pending_blank = false;
        for child in node.children() {
            if &child == leading {
                reached_leading = true;
                continue;
            }
            if !reached_leading {
                continue;
            }
            if child.kind() == SyntaxKind::BLANK_LINE {
                pending_blank = true;
                continue;
            }
            if pending_blank {
                if !self.output.ends_with("\n\n") {
                    self.output.push('\n');
                }
                pending_blank = false;
            }
            self.format_node_sync(&child, hanging);
        }
    }

    /// Whether reflowing this container's content would manufacture a
    /// definition term out of it.
    ///
    /// Two adjacent plain blocks where the second starts with a `:` or `~`
    /// marker only stay two blocks while the first one is longer than a line:
    /// a one-line block above a marker *is* a term, so wrapping `a\nb` down to
    /// `a b` turns `[Plain, Plain]` into a `DefinitionList` on reparse. A tight
    /// list item or definition body has no blank line to separate the two
    /// with, so the only rendering that survives a round-trip is the source's
    /// own line breaks.
    ///
    /// A definition list already nested in the body is the same hazard seen
    /// from the other side: its term was promoted out of a one-line block, so
    /// collapsing the block above it hands the nested marker *that* line
    /// instead and swallows the term as a definition.
    pub(super) fn reflow_would_promote_a_definition_term(item: &SyntaxNode) -> bool {
        item.children()
            .filter(|child| matches!(child.kind(), SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH))
            .filter_map(|child| child.next_sibling())
            .any(|next| match next.kind() {
                SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH => next
                    .text()
                    .to_string()
                    .lines()
                    .next()
                    .and_then(|line| try_parse_definition_marker(line.trim_start()))
                    .is_some(),
                SyntaxKind::DEFINITION_LIST => true,
                _ => false,
            })
    }

    /// Format a ListItem node
    pub(super) fn format_list_item(&mut self, node: &SyntaxNode, indent: usize) {
        if Self::reflow_would_promote_a_definition_term(node) {
            let saved = self.config.wrap.clone();
            self.config.wrap = Some(WrapMode::Preserve);
            self.format_list_item_inner(node, indent);
            self.config.wrap = saved;
        } else {
            self.format_list_item_inner(node, indent);
        }
    }

    fn format_list_item_inner(&mut self, node: &SyntaxNode, indent: usize) {
        for child in node.children() {
            if matches!(
                child.kind(),
                SyntaxKind::HTML_BLOCK | SyntaxKind::HTML_BLOCK_RAW | SyntaxKind::COMMENT
            ) && let Some(directive) = crate::directives::extract_directive_from_node(&child)
            {
                self.directive_tracker.process_directive(&directive);
            }
        }

        let mut marker = String::new();
        let mut checkbox = None;

        for el in node.children_with_tokens() {
            if let NodeOrToken::Token(t) = el {
                match t.kind() {
                    SyntaxKind::WHITESPACE => {}
                    SyntaxKind::LIST_MARKER => {
                        marker = self.normalize_bullet_for_output(t.text());
                    }
                    SyntaxKind::TASK_CHECKBOX => {
                        checkbox = Some(Self::normalize_task_checkbox(t.text()));
                    }
                    _ => {}
                }
            }
        }

        let max_marker_width = self.max_marker_widths.last().copied().unwrap_or(0);

        let list_indent = calculate_list_item_indent(
            &marker,
            max_marker_width,
            checkbox.is_some(),
            self.config.parser_extensions.four_space_rule,
            self.config.tab_width,
        );

        let total_indent = indent;
        let hanging = list_indent.hanging_indent(total_indent);
        let text_continuation = total_indent + list_indent.text_continuation_offset();
        let content_indent = total_indent + list_indent.content_offset();
        let available_width = self.config.line_width.saturating_sub(content_indent);

        let first_non_blank_child = node
            .children()
            .find(|child| child.kind() != SyntaxKind::BLANK_LINE);
        if let Some(leading_heading) = first_non_blank_child.as_ref()
            && leading_heading.kind() == SyntaxKind::HEADING
        {
            self.output.push_str(&" ".repeat(total_indent));
            self.output
                .push_str(&" ".repeat(list_indent.marker_padding));
            self.output.push_str(&marker);
            self.output.push_str(&" ".repeat(list_indent.spaces_after));
            if let Some(ref cb) = checkbox {
                self.output.push_str(cb);
                self.output.push(' ');
            }
            self.output.push_str(&self.format_heading(leading_heading));
            self.output.push('\n');

            let has_following_blocks = node
                .children()
                .any(|child| &child != leading_heading && child.kind() != SyntaxKind::BLANK_LINE);
            if has_following_blocks {
                self.output.push('\n');
            }

            for child in node.children() {
                if &child == leading_heading || child.kind() == SyntaxKind::BLANK_LINE {
                    continue;
                }

                match child.kind() {
                    k if Self::is_item_content_block(k) => {
                        self.format_list_continuation_paragraph(&child, hanging);
                    }
                    SyntaxKind::LIST => {
                        self.format_node_sync(&child, hanging);
                    }
                    SyntaxKind::CODE_BLOCK => {
                        self.format_indented_code_block(&child, hanging);
                    }
                    _ => {
                        self.format_node_sync(&child, hanging);
                    }
                }
            }
            return;
        }

        if let Some(leading_bq) = first_non_blank_child.as_ref()
            && leading_bq.kind() == SyntaxKind::BLOCK_QUOTE
            && Self::find_content_node(node).is_none()
        {
            self.output.push_str(&" ".repeat(total_indent));
            self.output
                .push_str(&" ".repeat(list_indent.marker_padding));
            self.output.push_str(&marker);
            self.output.push_str(&" ".repeat(list_indent.spaces_after));
            if let Some(ref cb) = checkbox {
                self.output.push_str(cb);
                self.output.push(' ');
            }
            let bq_start = self.output.len();
            self.format_node_sync(leading_bq, 0);
            if hanging > 0 {
                let bq_block = self.output.split_off(bq_start);
                let prefix = " ".repeat(hanging);
                let mut first = true;
                for line in bq_block.split_inclusive('\n') {
                    let is_blank = line.trim_end_matches('\n').is_empty();
                    if !first && !is_blank {
                        self.output.push_str(&prefix);
                    }
                    self.output.push_str(line);
                    first = false;
                }
            }

            self.format_item_blocks_after_leading(node, leading_bq, hanging);
            return;
        }

        if let Some(leading_block) = first_non_blank_child.as_ref()
            && matches!(
                leading_block.kind(),
                SyntaxKind::FENCED_DIV
                    | SyntaxKind::CODE_BLOCK
                    | SyntaxKind::LINE_BLOCK
                    | SyntaxKind::DEFINITION_LIST
            )
            && Self::find_content_node(node).is_none()
        {
            self.output.push_str(&" ".repeat(total_indent));
            self.output
                .push_str(&" ".repeat(list_indent.marker_padding));
            self.output.push_str(&marker);
            self.output.push_str(&" ".repeat(list_indent.spaces_after));
            if let Some(ref cb) = checkbox {
                self.output.push_str(cb);
                self.output.push(' ');
            }
            let block_start = self.output.len();
            if leading_block.kind() == SyntaxKind::CODE_BLOCK {
                let raw = self.format_code_block_to_string(leading_block);
                let raw = raw.strip_suffix('\n').unwrap_or(&raw);
                let block_lines: Vec<&str> = raw.split('\n').collect();
                let prefix = " ".repeat(hanging);
                let last = block_lines.len().saturating_sub(1);
                let content_indent = block_lines
                    .get(1..last)
                    .unwrap_or(&[])
                    .iter()
                    .filter(|l| !l.trim().is_empty())
                    .map(|l| l.len() - l.trim_start().len())
                    .min()
                    .unwrap_or(0);
                for (i, line) in block_lines.iter().enumerate() {
                    if i == 0 {
                        self.output.push_str(line.trim_start());
                    } else if line.trim().is_empty() {
                    } else if i == last {
                        self.output.push_str(&prefix);
                        self.output.push_str(line);
                    } else {
                        self.output.push_str(&prefix);
                        let cut = content_indent.min(line.len());
                        self.output.push_str(&line[cut..]);
                    }
                    self.output.push('\n');
                }
            } else {
                self.format_node_sync(leading_block, 0);
                if self.output.as_bytes().get(block_start) == Some(&b'\n') {
                    self.output.remove(block_start);
                }
                if hanging > 0 {
                    let block_text = self.output.split_off(block_start);
                    let prefix = " ".repeat(hanging);
                    let mut first = true;
                    for line in block_text.split_inclusive('\n') {
                        let is_blank = line.trim_end_matches('\n').is_empty();
                        if !first && !is_blank {
                            self.output.push_str(&prefix);
                        }
                        self.output.push_str(line);
                        first = false;
                    }
                }
            }

            self.format_item_blocks_after_leading(node, leading_block, hanging);
            return;
        }

        if let Some(leading_list) = first_non_blank_child.as_ref()
            && leading_list.kind() == SyntaxKind::LIST
            && !Self::is_empty_nested_list(leading_list)
            && Self::find_content_node(node).is_none()
        {
            self.output.push_str(&" ".repeat(total_indent));
            self.output
                .push_str(&" ".repeat(list_indent.marker_padding));
            self.output.push_str(&marker);
            self.output.push_str(&" ".repeat(list_indent.spaces_after));
            if let Some(ref cb) = checkbox {
                self.output.push_str(cb);
                self.output.push(' ');
            }
            let saved_len = self.output.len();
            self.format_node_sync(leading_list, 0);
            if self.output.as_bytes().get(saved_len) == Some(&b'\n') {
                self.output.remove(saved_len);
            }
            if hanging > 0 {
                let inner_block = self.output.split_off(saved_len);
                let prefix = " ".repeat(hanging);
                let mut first = true;
                for line in inner_block.split_inclusive('\n') {
                    let is_blank = line.trim_end_matches('\n').is_empty();
                    if !first && !is_blank {
                        self.output.push_str(&prefix);
                    }
                    self.output.push_str(line);
                    first = false;
                }
            }

            self.format_item_blocks_after_leading(node, leading_list, hanging);
            return;
        }

        let content_node = Self::find_content_node(node);

        let content_has_hard_breaks = content_node
            .as_ref()
            .map(|content| {
                content
                    .descendants_with_tokens()
                    .any(|el| el.kind() == SyntaxKind::HARD_LINE_BREAK)
            })
            .unwrap_or(false);

        let wrap_source = content_node.as_ref();

        let has_only_empty_nested_list = node
            .children()
            .any(|c| c.kind() == SyntaxKind::LIST && Self::is_empty_nested_list(&c))
            && wrap_source.is_none_or(|source| source.text().to_string().trim().is_empty());

        let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
        let content_starts_with_blockquote = content_node
            .as_ref()
            .map(|content| content.text().to_string().trim_start().starts_with('>'))
            .unwrap_or(false);
        let line_widths = [available_width];
        let lines = match wrap_mode {
            WrapMode::Preserve | WrapMode::Sentence | WrapMode::Semantic => Vec::new(),
            WrapMode::Reflow => wrap_source
                .map(|source| {
                    inline_layout::wrapped_lines_for_node(
                        &self.config,
                        source,
                        &line_widths,
                        &|n| self.format_inline_node(n),
                        WrapStrategy::ListReflow,
                    )
                })
                .unwrap_or_default(),
        };
        let content_has_format_directive = content_node
            .as_ref()
            .map(|content| {
                crate::directives::collect_inline_directives(content)
                    .iter()
                    .any(|d| match d {
                        crate::directives::Directive::Start(kind)
                        | crate::directives::Directive::End(kind) => kind.affects_formatting(),
                    })
            })
            .unwrap_or(false);

        let preserve_lines = match wrap_mode {
            WrapMode::Preserve => {
                let escaped = self.config.formatter_extensions.escaped_line_breaks;
                Some(
                    content_node
                        .as_ref()
                        .map(|content| preserve::preserve_lines(content, escaped))
                        .unwrap_or_default(),
                )
            }
            _ if content_has_format_directive => {
                let source = content_node
                    .as_ref()
                    .map(|content| content.text().to_string())
                    .unwrap_or_default();
                Some(source.lines().map(ToString::to_string).collect::<Vec<_>>())
            }
            _ => None,
        };
        let sentence_lines: Option<Vec<String>> = match wrap_mode {
            WrapMode::Sentence | WrapMode::Semantic => {
                let strategy = if matches!(wrap_mode, WrapMode::Semantic) {
                    WrapStrategy::ListSemantic
                } else {
                    WrapStrategy::ListSentence
                };
                Some(
                    wrap_source
                        .map(|source| {
                            inline_layout::wrapped_lines_for_node(
                                &self.config,
                                source,
                                &[],
                                &|n| self.format_inline_node(n),
                                strategy,
                            )
                        })
                        .unwrap_or_default(),
                )
            }
            _ => None,
        };

        let heading_with_remainder = content_node
            .as_ref()
            .and_then(|content| self.leading_atx_heading_with_remainder(content));

        log::trace!(
            "ListItem wrapping: {} lines, hanging indent={}",
            lines.len(),
            hanging
        );

        if let Some((heading_line, remainder)) = heading_with_remainder {
            self.output.push_str(&" ".repeat(total_indent));
            self.output
                .push_str(&" ".repeat(list_indent.marker_padding));
            self.output.push_str(&marker);
            self.output.push_str(&" ".repeat(list_indent.spaces_after));
            if let Some(ref cb) = checkbox {
                self.output.push_str(cb);
                self.output.push(' ');
            }
            self.output.push_str(&heading_line);
            self.output.push('\n');
            self.output.push('\n');

            for line in self.wrap_text_for_indent(&remainder, hanging) {
                self.output.push_str(&" ".repeat(hanging));
                self.output.push_str(line.trim_start());
                self.output.push('\n');
            }
        } else if let Some(preserve_lines) = &preserve_lines {
            for (i, line) in preserve_lines.iter().enumerate() {
                if i == 0 {
                    self.output.push_str(&" ".repeat(total_indent));
                    self.output
                        .push_str(&" ".repeat(list_indent.marker_padding));
                    self.output.push_str(&marker);
                    self.output.push_str(&" ".repeat(list_indent.spaces_after));
                    if let Some(ref cb) = checkbox {
                        self.output.push_str(cb);
                        self.output.push(' ');
                    }
                } else {
                    self.output.push_str(&" ".repeat(text_continuation));
                }
                self.output.push_str(line.trim_start());
                if !has_only_empty_nested_list {
                    self.output.push('\n');
                }
            }
        } else if let Some(sentence_lines) = &sentence_lines {
            for (i, text) in sentence_lines.iter().enumerate() {
                log::trace!("  Line {}: sentence line", i);
                if i == 0 {
                    self.output.push_str(&" ".repeat(total_indent));
                    self.output
                        .push_str(&" ".repeat(list_indent.marker_padding));
                    self.output.push_str(&marker);
                    self.output.push_str(&" ".repeat(list_indent.spaces_after));

                    if let Some(ref cb) = checkbox {
                        self.output.push_str(cb);
                        self.output.push(' ');
                    }
                } else {
                    self.output.push_str(&" ".repeat(text_continuation));
                }
                if i > 0 {
                    self.output.push_str(text.trim_start());
                } else {
                    let normalized = text
                        .replace("<summary>\n\t", "<summary>\n    ")
                        .replace("<summary>\n  ", "<summary>\n    ");
                    self.output.push_str(&normalized);
                }
                if !has_only_empty_nested_list {
                    self.output.push('\n');
                }
            }
        } else {
            for (i, line) in lines.iter().enumerate() {
                log::trace!("  Line {}: {} chars", i, line.len());
                if i == 0 {
                    self.output.push_str(&" ".repeat(total_indent));
                    self.output
                        .push_str(&" ".repeat(list_indent.marker_padding));
                    self.output.push_str(&marker);
                    self.output.push_str(&" ".repeat(list_indent.spaces_after));

                    if let Some(ref cb) = checkbox {
                        self.output.push_str(cb);
                        self.output.push(' ');
                    }
                } else {
                    self.output.push_str(&" ".repeat(text_continuation));
                }
                let mut rendered_line = if i > 0 {
                    line.trim_start().to_string()
                } else {
                    line.to_string()
                };
                rendered_line = rendered_line
                    .replace("<summary>\n\t", "<summary>\n    ")
                    .replace("<summary>\n  ", "<summary>\n    ");
                if rendered_line.contains('\n') {
                    for (idx, segment) in rendered_line.split('\n').enumerate() {
                        let segment = if content_has_hard_breaks {
                            segment
                        } else {
                            segment.trim_end()
                        };
                        if idx == 0 {
                            self.output.push_str(segment);
                        } else {
                            let trimmed = segment.trim_start();
                            if !trimmed.is_empty() {
                                self.output.push('\n');
                                self.output.push_str(&" ".repeat(text_continuation));
                                self.output.push_str(trimmed);
                            }
                        }
                    }
                } else {
                    self.output.push_str(&rendered_line);
                }
                if !has_only_empty_nested_list {
                    self.output.push('\n');
                }
            }
        }

        if lines.is_empty() && has_only_empty_nested_list {
            self.output.push_str(&" ".repeat(total_indent));
            self.output
                .push_str(&" ".repeat(list_indent.marker_padding));
            self.output.push_str(&marker);
            self.output.push(' '); // Space before nested marker
        }

        for child in node.children() {
            match child.kind() {
                k if Self::is_item_content_block(k) => {
                    if Self::is_marker_only_blockquote_continuation(&child) {
                        continue;
                    }

                    let is_content_node = content_node.as_ref() == Some(&child);
                    let in_ignore_region = self.directive_tracker.is_formatting_ignored();

                    if !is_content_node || in_ignore_region {
                        let content_indent = list_indent.hanging_indent(total_indent);
                        if in_ignore_region {
                            self.format_node_sync(&child, 0);
                        } else {
                            self.format_list_continuation_paragraph(&child, content_indent);
                        }
                    }
                }
                SyntaxKind::LIST => {
                    if Self::is_empty_nested_list(&child) {
                        let nested_marker = Self::extract_list_marker(
                            &child
                                .children()
                                .find(|c| c.kind() == SyntaxKind::LIST_ITEM)
                                .unwrap(),
                        )
                        .unwrap_or_else(|| "-".to_string());
                        self.output.push_str(&nested_marker);
                        self.output.push('\n');
                    } else {
                        self.format_node_sync(&child, list_indent.hanging_indent(total_indent));
                    }
                }
                SyntaxKind::CODE_BLOCK => {
                    let content_indent = list_indent.hanging_indent(total_indent);
                    self.format_indented_code_block(&child, content_indent);
                }
                SyntaxKind::BLOCK_QUOTE => {
                    let follows_primary_content = child
                        .prev_sibling()
                        .map(|prev| Self::is_item_content_block(prev.kind()))
                        .unwrap_or(false);

                    if content_starts_with_blockquote && follows_primary_content {
                        if self.output.ends_with('\n') {
                            self.output.pop();
                        }

                        let mut pieces: Vec<String> = Vec::new();
                        let child_text = child.text().to_string();
                        for line in child_text.lines() {
                            let trimmed = line.trim_start();
                            let content = if let Some(rest) = trimmed.strip_prefix('>') {
                                rest.trim_start()
                            } else {
                                trimmed
                            };
                            if !content.is_empty() {
                                pieces.push(content.to_string());
                            }
                        }

                        if !pieces.is_empty() {
                            self.output.push(' ');
                            self.output.push_str(&pieces.join(" "));
                        }
                        self.output.push('\n');
                    } else {
                        let content_indent = list_indent.hanging_indent(total_indent);
                        self.format_node_sync(&child, content_indent);
                    }
                }
                SyntaxKind::HORIZONTAL_RULE => {
                    let no_content_emitted = lines.is_empty()
                        && preserve_lines.is_none()
                        && sentence_lines.is_none()
                        && content_node.is_none()
                        && !has_only_empty_nested_list;
                    let prev_kind = child.prev_sibling().map(|s| s.kind());
                    let is_first_real_child = !matches!(
                        prev_kind,
                        Some(SyntaxKind::PLAIN)
                            | Some(SyntaxKind::PARAGRAPH)
                            | Some(SyntaxKind::FIGURE)
                            | Some(SyntaxKind::HEADING)
                            | Some(SyntaxKind::CODE_BLOCK)
                            | Some(SyntaxKind::BLOCK_QUOTE)
                            | Some(SyntaxKind::LIST)
                            | Some(SyntaxKind::HORIZONTAL_RULE)
                    );
                    if no_content_emitted && is_first_real_child {
                        self.output.push_str(&" ".repeat(total_indent));
                        self.output
                            .push_str(&" ".repeat(list_indent.marker_padding));
                        self.output.push_str(&marker);
                        self.output.push_str(&" ".repeat(list_indent.spaces_after));
                        if let Some(ref cb) = checkbox {
                            self.output.push_str(cb);
                            self.output.push(' ');
                        }
                        let hr_text: String = child
                            .children_with_tokens()
                            .filter_map(|el| el.into_token())
                            .filter(|t| t.kind() == SyntaxKind::HORIZONTAL_RULE)
                            .map(|t| t.text().to_string())
                            .collect();
                        self.output.push_str(hr_text.trim());
                        self.output.push('\n');
                    } else {
                        let content_indent = list_indent.hanging_indent(total_indent);
                        self.format_node_sync(&child, content_indent);
                    }
                }
                SyntaxKind::BLANK_LINE => {
                    if !self.output.ends_with("\n\n") {
                        self.output.push('\n');
                    }
                }
                SyntaxKind::HTML_BLOCK
                | SyntaxKind::HTML_BLOCK_RAW
                | SyntaxKind::HTML_BLOCK_DIV => {
                    let no_content_emitted = lines.is_empty()
                        && preserve_lines.is_none()
                        && sentence_lines.is_none()
                        && content_node.is_none()
                        && !has_only_empty_nested_list;
                    let prev_kind = child.prev_sibling().map(|s| s.kind());
                    let is_first_real_child = !matches!(
                        prev_kind,
                        Some(SyntaxKind::PLAIN)
                            | Some(SyntaxKind::PARAGRAPH)
                            | Some(SyntaxKind::FIGURE)
                            | Some(SyntaxKind::HEADING)
                            | Some(SyntaxKind::CODE_BLOCK)
                            | Some(SyntaxKind::BLOCK_QUOTE)
                            | Some(SyntaxKind::LIST)
                            | Some(SyntaxKind::HORIZONTAL_RULE)
                            | Some(SyntaxKind::HTML_BLOCK)
                            | Some(SyntaxKind::HTML_BLOCK_RAW)
                            | Some(SyntaxKind::HTML_BLOCK_DIV)
                    );
                    if no_content_emitted && is_first_real_child {
                        self.output.push_str(&" ".repeat(total_indent));
                        self.output
                            .push_str(&" ".repeat(list_indent.marker_padding));
                        self.output.push_str(&marker);
                        self.output.push_str(&" ".repeat(list_indent.spaces_after));
                        if let Some(ref cb) = checkbox {
                            self.output.push_str(cb);
                            self.output.push(' ');
                        }
                        let block_text = child.text().to_string();
                        let trimmed = block_text.trim_end_matches('\n');
                        self.output.push_str(trimmed);
                        self.output.push('\n');
                    } else {
                        let content_indent = list_indent.hanging_indent(total_indent);
                        self.format_node_sync(&child, content_indent);
                    }
                }
                SyntaxKind::PIPE_TABLE | SyntaxKind::GRID_TABLE => {
                    let no_content_emitted = lines.is_empty()
                        && preserve_lines.is_none()
                        && sentence_lines.is_none()
                        && content_node.is_none()
                        && !has_only_empty_nested_list;
                    let prev_kind = child.prev_sibling().map(|s| s.kind());
                    let is_first_real_child = !matches!(
                        prev_kind,
                        Some(SyntaxKind::PLAIN)
                            | Some(SyntaxKind::PARAGRAPH)
                            | Some(SyntaxKind::FIGURE)
                            | Some(SyntaxKind::HEADING)
                            | Some(SyntaxKind::CODE_BLOCK)
                            | Some(SyntaxKind::BLOCK_QUOTE)
                            | Some(SyntaxKind::LIST)
                            | Some(SyntaxKind::HORIZONTAL_RULE)
                            | Some(SyntaxKind::HTML_BLOCK)
                            | Some(SyntaxKind::HTML_BLOCK_RAW)
                            | Some(SyntaxKind::HTML_BLOCK_DIV)
                            | Some(SyntaxKind::PIPE_TABLE)
                            | Some(SyntaxKind::GRID_TABLE)
                    );
                    let content_indent = list_indent.hanging_indent(total_indent);
                    let prefix_width = total_indent
                        + list_indent.marker_padding
                        + marker.len()
                        + list_indent.spaces_after;
                    if no_content_emitted
                        && is_first_real_child
                        && checkbox.is_none()
                        && prefix_width == content_indent
                    {
                        let prefix = format!(
                            "{}{}{}{}",
                            " ".repeat(total_indent),
                            " ".repeat(list_indent.marker_padding),
                            marker,
                            " ".repeat(list_indent.spaces_after),
                        );
                        let table_str = match child.kind() {
                            SyntaxKind::PIPE_TABLE => {
                                tables::format_pipe_table(&child, &self.config, content_indent)
                            }
                            _ => tables::format_grid_table(&child, &self.config, content_indent),
                        };
                        let first_line_indent = " ".repeat(content_indent);
                        self.output.push_str(&prefix);
                        self.output.push_str(
                            table_str
                                .strip_prefix(&first_line_indent)
                                .unwrap_or(&table_str),
                        );
                    } else {
                        self.format_node_sync(&child, content_indent);
                    }
                }
                _ => {
                    let content_indent = list_indent.hanging_indent(total_indent);
                    self.format_node_sync(&child, content_indent);
                }
            }
        }
    }
}
