//! Continuation/blank-line handling policy.
//!
//! This module centralizes the parser's "should this line continue an existing container?"
//! logic (especially across blank lines). Keeping this logic in one place reduces the
//! risk of scattered ad-hoc heuristics diverging as blocks move into the dispatcher.

use crate::config::{Config, PandocCompat};

use crate::parser::block_dispatcher::{BlockContext, BlockParserRegistry};
use crate::parser::blocks::blockquotes::{count_blockquote_markers, strip_n_blockquote_markers};
use crate::parser::blocks::{definition_lists, html_blocks, lists, raw_blocks};
use crate::parser::utils::container_stack::{ContainerStack, leading_indent};

pub(crate) struct ContinuationPolicy<'a, 'cfg> {
    config: &'cfg Config,
    block_registry: &'a BlockParserRegistry,
}

impl<'a, 'cfg> ContinuationPolicy<'a, 'cfg> {
    pub(crate) fn new(config: &'cfg Config, block_registry: &'a BlockParserRegistry) -> Self {
        Self {
            config,
            block_registry,
        }
    }

    fn definition_min_block_indent(&self, content_col: usize) -> usize {
        if self.config.parser.effective_pandoc_compat() == PandocCompat::V3_7 {
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
        let next_marker = lists::try_parse_list_marker(next_inner, self.config);
        let next_is_definition_marker =
            definition_lists::try_parse_definition_marker(next_inner).is_some();
        let next_is_definition_term = !next_inner.trim().is_empty()
            && definition_lists::next_line_is_definition_marker(lines, next_line_pos).is_some();

        // `current_bq_depth` is used for proper indent calculation when the next line
        // increases blockquote nesting.

        let mut keep_level = 0;
        let mut content_indent_so_far = 0usize;

        // First, account for blockquotes
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
                crate::parser::utils::container_stack::Container::Definition {
                    content_col,
                    ..
                } => {
                    // A blank line does not necessarily end a definition, but the continuation
                    // indent must be measured relative to any outer content containers (e.g.
                    // footnotes). Otherwise a line indented only for the footnote would wrongly
                    // continue the definition.
                    let min_indent = self.definition_min_block_indent(*content_col);
                    let effective_indent = raw_indent_cols.saturating_sub(content_indent_so_far);
                    if effective_indent >= min_indent {
                        keep_level = i + 1;
                    }
                    content_indent_so_far += *content_col;
                }
                crate::parser::utils::container_stack::Container::DefinitionItem { .. } => {
                    if next_is_definition_marker {
                        keep_level = i + 1;
                    }
                }
                crate::parser::utils::container_stack::Container::DefinitionList { .. } => {
                    if next_is_definition_marker || next_is_definition_term {
                        keep_level = i + 1;
                    }
                }
                crate::parser::utils::container_stack::Container::List {
                    marker,
                    base_indent_cols,
                    ..
                } => {
                    let effective_indent = raw_indent_cols.saturating_sub(content_indent_so_far);
                    let continues_list = if let Some(ref marker_match) = next_marker {
                        lists::markers_match(marker, &marker_match.marker)
                            && effective_indent <= base_indent_cols + 3
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
                    content_col, ..
                } => {
                    let effective_indent = if next_bq_depth > current_bq_depth {
                        let after_current_bq =
                            strip_n_blockquote_markers(next_line, current_bq_depth);
                        let (spaces_before_next_marker, _) = leading_indent(after_current_bq);
                        spaces_before_next_marker.saturating_sub(content_indent_so_far)
                    } else {
                        raw_indent_cols.saturating_sub(content_indent_so_far)
                    };

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
    pub(crate) fn definition_plain_can_continue(
        &self,
        stripped_content: &str,
        raw_content: &str,
        content_indent: usize,
        block_ctx: &BlockContext,
        lines: &[&str],
        pos: usize,
    ) -> bool {
        let prev_line_blank = if pos > 0 {
            let prev_line = lines[pos - 1];
            let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
            prev_line.trim().is_empty() || (prev_bq_depth > 0 && prev_inner.trim().is_empty())
        } else {
            false
        };

        // A blank line that isn't indented to the definition content column ends the definition.
        let (indent_cols, _) = leading_indent(raw_content);
        if raw_content.trim().is_empty() && indent_cols < content_indent {
            return false;
        }
        let min_block_indent = self.definition_min_block_indent(content_indent);
        if prev_line_blank && indent_cols < min_block_indent {
            return false;
        }

        // If it's a block element marker, don't continue as plain.
        if definition_lists::try_parse_definition_marker(stripped_content).is_some()
            && leading_indent(raw_content).0 <= 3
            && !stripped_content.starts_with(':')
        {
            let is_next_definition = self
                .block_registry
                .detect_prepared(block_ctx, lines, pos)
                .map(|match_result| {
                    match_result.effect
                        == crate::parser::block_dispatcher::BlockEffect::OpenDefinitionList
                })
                .unwrap_or(false);
            if is_next_definition {
                return false;
            }
        }
        if lists::try_parse_list_marker(stripped_content, self.config).is_some() {
            if prev_line_blank {
                return false;
            }
            if block_ctx.in_list {
                return false;
            }
        }
        if count_blockquote_markers(stripped_content).0 > 0 {
            return false;
        }
        if self.config.extensions.raw_html
            && html_blocks::try_parse_html_block_start(stripped_content).is_some()
        {
            return false;
        }
        if self.config.extensions.raw_tex
            && raw_blocks::extract_environment_name(stripped_content).is_some()
        {
            return false;
        }

        if let Some(match_result) = self.block_registry.detect_prepared(block_ctx, lines, pos) {
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
