//! Paragraph handling utilities.
//!
//! Note: Most paragraph logic is in the main Parser since paragraphs
//! are tightly integrated with container handling.

use crate::options::ParserOptions;
use rowan::GreenNodeBuilder;

use crate::parser::blocks::raw_blocks::{extract_environment_name, is_inline_math_environment};
use crate::parser::utils::container_stack::{Container, ContainerStack};
use crate::parser::utils::text_buffer::ParagraphBuffer;

fn update_display_math_dollar_state(
    line_no_newline: &str,
    open_display_math_dollar_count: &mut Option<usize>,
) {
    let trimmed = line_no_newline.trim();
    if trimmed.len() < 2 {
        return;
    }

    let run_len = trimmed.bytes().take_while(|b| *b == b'$').count();
    if run_len < 2 {
        return;
    }

    let rest = &trimmed[run_len..];
    let is_pure_dollars = rest.is_empty();
    let is_quarto_equation_attr_closer = rest.starts_with(char::is_whitespace) && {
        let attr = rest.trim_start();
        attr.starts_with('{') && attr.ends_with('}')
    };

    if let Some(open_len) = *open_display_math_dollar_count {
        if (is_pure_dollars || is_quarto_equation_attr_closer) && run_len >= open_len {
            *open_display_math_dollar_count = None;
        }
    } else if is_pure_dollars {
        *open_display_math_dollar_count = Some(run_len);
    }
}

fn extract_end_environment_name(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("\\end{") {
        return None;
    }
    let rest = &trimmed[5..];
    let close = rest.find('}')?;
    let name = &rest[..close];
    if name.is_empty() {
        return None;
    }
    Some(name)
}

/// Start a paragraph if not already in one.
///
/// Takes a checkpoint at the current builder position so the paragraph can be
/// retroactively wrapped as `PARAGRAPH` on close, or as `HEADING` for
/// multi-line setext heading promotion. Nothing is emitted into the builder
/// here; emission happens at close via `start_node_at(checkpoint, kind)`.
pub(in crate::parser) fn start_paragraph_if_needed(
    containers: &mut ContainerStack,
    builder: &mut GreenNodeBuilder<'static>,
) {
    if !matches!(containers.last(), Some(Container::Paragraph { .. })) {
        let start_checkpoint = builder.checkpoint();
        containers.push(Container::Paragraph {
            buffer: ParagraphBuffer::new(),
            open_inline_math_envs: Vec::new(),
            open_display_math_dollar_count: None,
            start_checkpoint,
        });
    }
}

/// Append a line to the current paragraph (preserving losslessness).
pub(in crate::parser) fn append_paragraph_line(
    containers: &mut ContainerStack,
    _builder: &mut GreenNodeBuilder<'static>,
    line: &str,
    _config: &ParserOptions,
) {
    // Buffer the line (with newline for losslessness)
    // Works for ALL paragraphs including those in blockquotes
    if let Some(Container::Paragraph {
        buffer,
        open_inline_math_envs,
        open_display_math_dollar_count,
        ..
    }) = containers.stack.last_mut()
    {
        buffer.push_text(line);

        let line_no_newline = line.trim_end_matches(&['\r', '\n'][..]);
        // Track standalone `$$` delimiter lines for all paragraphs so we keep
        // multi-line display math in a single paragraph parse context.
        // This prevents `$$` + `\begin{...}` forms from being split into
        // PARAGRAPH + TEX_BLOCK across parse passes.
        update_display_math_dollar_state(line_no_newline, open_display_math_dollar_count);
        if let Some(env_name) = extract_environment_name(line_no_newline)
            && is_inline_math_environment(&env_name)
        {
            open_inline_math_envs.push(env_name);
            return;
        }

        if let Some(end_name) = extract_end_environment_name(line_no_newline)
            && open_inline_math_envs
                .last()
                .is_some_and(|open| open == end_name)
        {
            open_inline_math_envs.pop();
        }
    }
}

/// Buffer a blockquote marker in the current paragraph.
///
/// Called when processing blockquote continuation lines while a paragraph is open
/// and using integrated inline parsing. The marker will be emitted at the correct
/// position when the paragraph is closed.
pub(in crate::parser) fn append_paragraph_marker(
    containers: &mut ContainerStack,
    leading_spaces: usize,
    has_trailing_space: bool,
) {
    if let Some(Container::Paragraph { buffer, .. }) = containers.stack.last_mut() {
        buffer.push_marker(leading_spaces, has_trailing_space);
    }
}

pub(in crate::parser) fn has_open_inline_math_environment(containers: &ContainerStack) -> bool {
    matches!(
        containers.last(),
        Some(Container::Paragraph {
            open_inline_math_envs,
            ..
        }) if !open_inline_math_envs.is_empty()
    )
}

pub(in crate::parser) fn has_open_display_math_dollars(containers: &ContainerStack) -> bool {
    matches!(
        containers.last(),
        Some(Container::Paragraph {
            open_display_math_dollar_count,
            ..
        }) if open_display_math_dollar_count.is_some()
    )
}

/// Get the current content column from the container stack.
pub(in crate::parser) fn current_content_col(containers: &ContainerStack) -> usize {
    containers
        .stack
        .iter()
        .rev()
        .find_map(|c| match c {
            Container::ListItem { content_col, .. } => Some(*content_col),
            Container::FootnoteDefinition { content_col, .. } => Some(*content_col),
            _ => None,
        })
        .unwrap_or(0)
}
