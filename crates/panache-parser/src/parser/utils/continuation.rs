//! Continuation/blank-line handling policy.
//!
//! This module centralizes the parser's "should this line continue an existing container?"
//! logic (especially across blank lines). Keeping this logic in one place reduces the
//! risk of scattered ad-hoc heuristics diverging as blocks move into the dispatcher.

use crate::options::{PandocCompat, ParserOptions};

use crate::parser::block_dispatcher::{BlockContext, BlockParserRegistry};
use crate::parser::blocks::blockquotes::{
    self, count_blockquote_markers, strip_n_blockquote_markers,
};
use crate::parser::blocks::container_prefix::{
    ContainerPrefix, StrippedLines, resolve_content_indent,
};
use crate::parser::blocks::{definition_lists, html_blocks, lists, raw_blocks};
use crate::parser::utils::container_stack::{ContainerStack, leading_indent};
use crate::parser::utils::helpers::is_blank_line;

pub(crate) struct ContinuationPolicy<'a, 'cfg> {
    config: &'cfg ParserOptions,
    block_registry: &'a BlockParserRegistry,
}

impl<'a, 'cfg> ContinuationPolicy<'a, 'cfg> {
    pub(crate) fn new(
        config: &'cfg ParserOptions,
        block_registry: &'a BlockParserRegistry,
    ) -> Self {
        Self {
            config,
            block_registry,
        }
    }

    fn definition_min_block_indent(&self, content_col: usize) -> usize {
        if self.config.effective_pandoc_compat() == PandocCompat::V3_7 {
            content_col.max(4)
        } else {
            content_col
        }
    }

    pub(crate) fn compute_levels_to_keep(
        &self,
        current_bq_depth: usize,
        containers: &ContainerStack,
        lines: &[&str],
        next_line_pos: usize,
        next_line: &str,
    ) -> usize {
        let (next_bq_depth, next_inner) = count_blockquote_markers(next_line);
        let (raw_indent_cols, _) = leading_indent(next_inner);
        // A newly opened quote belongs to a list only if its marker reaches
        // the item's content column. Its contents cannot continue that list.
        let next_list_content = if next_bq_depth > current_bq_depth {
            strip_n_blockquote_markers(next_line, current_bq_depth)
        } else {
            next_inner
        };
        let (list_indent_cols, _) = leading_indent(next_list_content);
        let next_marker = lists::try_parse_list_marker(
            next_list_content,
            self.config,
            lists::open_list_hint_at_indent(containers, list_indent_cols),
        );
        let next_is_definition_marker =
            definition_lists::try_parse_definition_marker(next_inner).is_some();
        let next_line_opens_definition = !is_blank_line(next_inner) && {
            let prefix = ContainerPrefix::from_stack(&containers.stack, false, self.config);
            let window = StrippedLines::new(lines, next_line_pos, &prefix);
            definition_lists::next_line_is_definition_marker(
                &window,
                next_line_pos,
                self.config.extensions.table_captions,
            )
            .is_some()
        };
        let next_is_definition_term_below = |level: usize| -> bool {
            next_line_opens_definition
                && raw_indent_cols
                    >= crate::parser::utils::container_stack::content_container_indent(
                        &containers.stack[..level],
                    ) + containers.stack[..level]
                        .iter()
                        .rev()
                        .find_map(|c| match c {
                            crate::parser::utils::container_stack::Container::ListItem {
                                content_col,
                                ..
                            } => Some(*content_col),
                            _ => None,
                        })
                        .unwrap_or(0)
        };

        let stripped_is_definition_marker = |content_indent_so_far: usize| -> bool {
            if content_indent_so_far == 0
                || !resolve_content_indent(next_inner, content_indent_so_far).reaches_frame()
            {
                return false;
            }
            let strip_bytes = crate::parser::utils::container_stack::byte_index_at_column(
                next_inner,
                content_indent_so_far,
            );
            if strip_bytes > next_inner.len() {
                return false;
            }
            definition_lists::try_parse_definition_marker(&next_inner[strip_bytes..]).is_some()
        };

        let mut keep_level = 0;
        let mut content_indent_so_far = 0usize;

        for (i, c) in containers.stack.iter().enumerate() {
            match c {
                crate::parser::utils::container_stack::Container::BlockQuote { .. } => {
                    let bq_count = containers.stack[..=i]
                        .iter()
                        .filter(|x| {
                            matches!(
                                x,
                                crate::parser::utils::container_stack::Container::BlockQuote { .. }
                            )
                        })
                        .count();
                    if bq_count <= next_bq_depth {
                        keep_level = i + 1;
                    }
                }
                crate::parser::utils::container_stack::Container::FootnoteDefinition {
                    content_col,
                    ..
                } => {
                    content_indent_so_far += *content_col;
                    let min_indent = (*content_col).max(4);
                    if raw_indent_cols >= min_indent {
                        keep_level = i + 1;
                    }
                }
                crate::parser::utils::container_stack::Container::Admonition { content_col } => {
                    content_indent_so_far += *content_col;
                    if raw_indent_cols >= *content_col {
                        keep_level = i + 1;
                    }
                }
                crate::parser::utils::container_stack::Container::Definition {
                    content_col,
                    ..
                } => {
                    let min_indent = self.definition_min_block_indent(*content_col);
                    let effective_indent = raw_indent_cols.saturating_sub(content_indent_so_far);
                    if effective_indent >= min_indent {
                        keep_level = i + 1;
                    }
                    content_indent_so_far += *content_col;
                }
                crate::parser::utils::container_stack::Container::DefinitionItem { .. }
                    if next_is_definition_marker
                        || stripped_is_definition_marker(content_indent_so_far) =>
                {
                    keep_level = i + 1;
                }
                crate::parser::utils::container_stack::Container::DefinitionList { .. }
                    if next_is_definition_marker
                        || next_is_definition_term_below(i)
                        || stripped_is_definition_marker(content_indent_so_far) =>
                {
                    keep_level = i + 1;
                }
                crate::parser::utils::container_stack::Container::List {
                    marker,
                    base_indent_cols,
                    ..
                } => {
                    let definition_ancestor_kept = containers.stack[..i]
                        .iter()
                        .enumerate()
                        .rev()
                        .find_map(|(idx, container)| {
                            matches!(
                                container,
                                crate::parser::utils::container_stack::Container::Definition { .. }
                            )
                            .then_some(keep_level > idx)
                        })
                        .unwrap_or(true);
                    if !definition_ancestor_kept {
                        continue;
                    }

                    let effective_indent = list_indent_cols.saturating_sub(content_indent_so_far);
                    let continues_list = if let Some(ref marker_match) = next_marker {
                        let indent_in_range = match marker {
                            lists::ListMarker::Ordered(_) => {
                                effective_indent.abs_diff(*base_indent_cols) <= 3
                            }
                            lists::ListMarker::Bullet(_) => {
                                let jumps_out_of_shallow_list =
                                    effective_indent >= 4 && *base_indent_cols < 4;
                                if jumps_out_of_shallow_list {
                                    false
                                } else if effective_indent >= *base_indent_cols {
                                    effective_indent <= base_indent_cols + 3
                                } else {
                                    let has_outer_match =
                                        containers.stack[..i].iter().any(|outer| {
                                            matches!(
                                                outer,
                                                crate::parser::utils::container_stack::Container::List {
                                                    marker: outer_marker,
                                                    base_indent_cols: outer_base,
                                                    ..
                                                } if matches!(
                                                    outer_marker,
                                                    lists::ListMarker::Bullet(_)
                                                ) && lists::markers_match(
                                                    outer_marker,
                                                    &marker_match.marker,
                                                    self.config.dialect,
                                                ) && *outer_base <= effective_indent
                                            )
                                        });
                                    !has_outer_match
                                        && base_indent_cols.saturating_sub(effective_indent) <= 3
                                }
                            }
                        };
                        lists::markers_match(marker, &marker_match.marker, self.config.dialect)
                            && indent_in_range
                    } else {
                        let item_content_col = containers
                            .stack
                            .get(i + 1)
                            .and_then(|c| match c {
                                crate::parser::utils::container_stack::Container::ListItem {
                                    content_col,
                                    ..
                                } => Some(*content_col),
                                _ => None,
                            })
                            .unwrap_or(1);
                        effective_indent >= item_content_col
                    };
                    if continues_list {
                        keep_level = i + 1;
                    }
                }
                crate::parser::utils::container_stack::Container::ListItem {
                    content_col,
                    marker_only,
                    ..
                } => {
                    let definition_ancestor_kept = containers.stack[..i]
                        .iter()
                        .enumerate()
                        .rev()
                        .find_map(|(idx, container)| {
                            matches!(
                                container,
                                crate::parser::utils::container_stack::Container::Definition { .. }
                            )
                            .then_some(keep_level > idx)
                        })
                        .unwrap_or(true);
                    if !definition_ancestor_kept {
                        continue;
                    }

                    if *marker_only && self.config.dialect == crate::options::Dialect::CommonMark {
                        if next_marker.is_none() && i > 0 && keep_level == i {
                            keep_level = i - 1;
                        }
                        continue;
                    }

                    let effective_indent = list_indent_cols.saturating_sub(content_indent_so_far);

                    let is_new_item_at_outer_level = if next_marker.is_some() {
                        effective_indent < *content_col
                    } else {
                        false
                    };

                    if !is_new_item_at_outer_level && effective_indent >= *content_col {
                        keep_level = i + 1;
                    }
                }
                _ => {}
            }
        }

        keep_level
    }

    /// Checks whether a line inside a definition should be treated as a plain continuation
    /// (and buffered into the definition PLAIN), rather than parsed as a new block.
    ///
    /// Not unified with `Parser::lazy_interrupts` (the blockquote gates'
    /// probe list): the polarity is inverted and the probe set differs
    /// (marker and indent policy, raw TeX, a catch-all `detect_prepared`).
    /// The HTML probe does share `html_block_cannot_interrupt`, so a tag
    /// that stays lazy text in a quote also stays lazy text in an open
    /// definition PLAIN.
    pub(crate) fn definition_plain_can_continue(
        &self,
        stripped_content: &str,
        raw_content: &str,
        block_ctx: &BlockContext,
        lines: &[&str],
        pos: usize,
        plain_open: bool,
    ) -> bool {
        let content_indent = block_ctx.content_indent;
        let prev_line_blank = if pos > 0 {
            let prev_line = lines[pos - 1];
            let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
            is_blank_line(prev_line) || (prev_bq_depth > 0 && is_blank_line(prev_inner))
        } else {
            false
        };

        let (indent_cols, _) = leading_indent(raw_content);
        if is_blank_line(raw_content) && indent_cols < content_indent {
            return false;
        }
        let min_block_indent = self.definition_min_block_indent(content_indent);
        if prev_line_blank && indent_cols < min_block_indent {
            return false;
        }

        if definition_lists::try_parse_definition_marker(stripped_content).is_some()
            && leading_indent(raw_content).0 <= 3
            && !stripped_content.starts_with(':')
        {
            let is_next_definition = {
                let prefix = ContainerPrefix::from_ctx(block_ctx);
                let stripped = StrippedLines::new(lines, pos, &prefix);
                self.block_registry
                    .detect_prepared(block_ctx, &stripped)
                    .map(|match_result| {
                        match_result.effect
                            == crate::parser::block_dispatcher::BlockEffect::OpenDefinitionList
                    })
                    .unwrap_or(false)
            };
            if is_next_definition {
                return false;
            }
        }
        if lists::try_parse_list_marker(stripped_content, self.config, block_ctx.open_alpha_hint)
            .is_some()
        {
            if prev_line_blank {
                return false;
            }
            if block_ctx.in_list {
                return false;
            }
            let (raw_indent_cols, _) = leading_indent(raw_content);
            if content_indent > 0 && raw_indent_cols >= content_indent {
                return false;
            }
        }
        if count_blockquote_markers(stripped_content).0 > 0 {
            if self.config.extensions.blank_before_blockquote
                && plain_open
                && !blockquotes::can_start_blockquote(
                    pos,
                    lines,
                    self.config.extensions.fenced_divs,
                )
            {
                return true;
            }
            return false;
        }
        if self.config.extensions.raw_html {
            let is_commonmark = self.config.dialect == crate::options::Dialect::CommonMark;
            let (probe, _) = crate::parser::utils::helpers::strip_newline(stripped_content);
            if let Some(block_type) = html_blocks::try_parse_html_block_start(probe, is_commonmark)
            {
                if plain_open
                    && crate::parser::block_dispatcher::html_block_cannot_interrupt(
                        &block_type,
                        probe,
                        !is_commonmark,
                    )
                {
                    return true;
                }
                return false;
            }
        }
        if self.config.extensions.raw_tex
            && raw_blocks::extract_environment_name(stripped_content).is_some()
        {
            return false;
        }

        let prefix = ContainerPrefix::from_ctx(block_ctx);
        let stripped = StrippedLines::new(lines, pos, &prefix);
        if let Some(match_result) = self.block_registry.detect_prepared(block_ctx, &stripped) {
            if match_result.effect == crate::parser::block_dispatcher::BlockEffect::OpenList
                && !prev_line_blank
            {
                return true;
            }
            if match_result.effect
                == crate::parser::block_dispatcher::BlockEffect::OpenDefinitionList
                && match_result
                    .payload
                    .as_ref()
                    .and_then(|payload| {
                        payload
                            .downcast_ref::<crate::parser::block_dispatcher::DefinitionPrepared>()
                    })
                    .is_some_and(|prepared| {
                        matches!(
                            prepared,
                            crate::parser::block_dispatcher::DefinitionPrepared::Term { .. }
                        )
                    })
            {
                return true;
            }
            return false;
        }

        true
    }
}
