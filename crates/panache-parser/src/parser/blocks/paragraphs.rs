//! Paragraph handling utilities.
//!
//! Note: Most paragraph logic is in the main Parser since paragraphs
//! are tightly integrated with container handling.

use crate::options::ParserOptions;
use rowan::GreenNodeBuilder;

use crate::parser::blocks::container_prefix::{ContainerPrefix, StripOp};
use crate::parser::blocks::raw_blocks::{extract_environment_name, is_inline_math_environment};
use crate::parser::utils::container_stack::{Container, ContainerStack, OpenDisplayMath};
use crate::parser::utils::helpers::{is_blank_line, trim_end_newlines};
use crate::parser::utils::text_buffer::ParagraphBuffer;

fn dollar_run(trimmed: &str) -> Option<(usize, &str)> {
    let run_len = trimmed.bytes().take_while(|b| *b == b'$').count();
    if run_len < 2 {
        return None;
    }
    Some((run_len, &trimmed[run_len..]))
}

/// Scan a paragraph line for bracket math delimiters and return the open state
/// after the line.
///
/// Mirrors the inline parsers (`try_parse_single_backslash_display_math` /
/// `try_parse_double_backslash_display_math`): delimiters may share a line
/// with content, the double form wins at a position when both extensions are
/// enabled, and there is no escape handling inside an open region.
fn scan_bracket_delimiters(
    line: &str,
    mut state: Option<OpenDisplayMath>,
    config: &ParserOptions,
) -> Option<OpenDisplayMath> {
    let single = config.extensions.tex_math_single_backslash;
    let double = config.extensions.tex_math_double_backslash;
    if !single && !double {
        return state;
    }

    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match state {
            None => {
                if double && bytes[i..].starts_with(br"\\[") {
                    state = Some(OpenDisplayMath::DoubleBrackets);
                    i += 3;
                } else if single && bytes[i..].starts_with(br"\[") {
                    state = Some(OpenDisplayMath::SingleBrackets);
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Some(OpenDisplayMath::SingleBrackets) => {
                if bytes[i..].starts_with(br"\]") {
                    state = None;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Some(OpenDisplayMath::DoubleBrackets) => {
                if bytes[i..].starts_with(br"\\]") {
                    state = None;
                    i += 3;
                } else {
                    i += 1;
                }
            }
            Some(OpenDisplayMath::Dollars(_) | OpenDisplayMath::Jupyter) => return state,
        }
    }
    state
}

/// Track multi-line math regions (`$$ ... $$`, `\[ ... \]`, `\\[ ... \\]`)
/// across buffered lines, or note a possible Jupyter math opener.
///
/// While a region of one delimiter kind is open, delimiters of the other
/// kinds are content and are ignored; only the matching closer changes state.
/// Jupyter instead verifies pending candidates through its inline matcher.
///
/// Shared with `ListItemBuffer`, which tracks the same regions across
/// buffered list-item lines.
pub(in crate::parser) fn update_display_math_state(
    line_no_newline: &str,
    open_display_math: &mut Option<OpenDisplayMath>,
    config: &ParserOptions,
) {
    if config.extensions.tex_math_jupyter {
        if line_no_newline.trim().is_empty() {
            *open_display_math = None;
        } else if crate::parser::inlines::math::pending_jupyter_math(line_no_newline) {
            // Do not approximate Jupyter's brace depth, code spans, or bare
            // environments here. The continuation hook uses the same matcher
            // as inline emission before holding any following line.
            *open_display_math = Some(OpenDisplayMath::Jupyter);
        }
        return;
    }
    match *open_display_math {
        Some(OpenDisplayMath::Dollars(open_len)) => {
            let trimmed = line_no_newline.trim();
            if let Some((run_len, rest)) = dollar_run(trimmed) {
                let is_pure_dollars = rest.is_empty();
                let is_quarto_equation_attr_closer = rest.starts_with(char::is_whitespace) && {
                    let attr = rest.trim_start();
                    attr.starts_with('{') && attr.ends_with('}')
                };
                if (is_pure_dollars || is_quarto_equation_attr_closer) && run_len >= open_len {
                    *open_display_math = None;
                }
            }
        }
        Some(OpenDisplayMath::SingleBrackets | OpenDisplayMath::DoubleBrackets) => {
            *open_display_math =
                scan_bracket_delimiters(line_no_newline, *open_display_math, config);
        }
        None => {
            let trimmed = line_no_newline.trim();
            if let Some((run_len, rest)) = dollar_run(trimmed)
                && rest.is_empty()
            {
                *open_display_math = Some(OpenDisplayMath::Dollars(run_len));
            } else {
                *open_display_math = scan_bracket_delimiters(line_no_newline, None, config);
            }
        }
        Some(OpenDisplayMath::Jupyter) => unreachable!("Jupyter state requires its extension"),
    }
}

/// A paragraph's stripped lookahead, scanned monotonically across math spans.
/// The owning buffer drops this cache when it is cleared or split, so another
/// paragraph cannot inherit its inline context.
#[derive(Debug, Clone)]
pub(crate) struct JupyterMathLookahead {
    text: String,
    first_line: usize,
    offsets: Vec<usize>,
    cursor: usize,
    ops: Vec<StripOp>,
    lazy_blockquote_gobble: bool,
}

impl JupyterMathLookahead {
    pub(crate) fn new<'a>(
        mut text: String,
        first_line: usize,
        prefix: &ContainerPrefix,
        following: impl Iterator<Item = &'a str>,
    ) -> Self {
        let mut offsets = vec![text.len()];
        for line in following
            .map(|line| prefix.strip(line))
            .take_while(|line| !is_blank_line(line))
        {
            text.push_str(line);
            offsets.push(text.len());
        }
        Self {
            text,
            first_line,
            offsets,
            cursor: 0,
            ops: prefix.ops().to_vec(),
            lazy_blockquote_gobble: prefix.lazy_blockquote_gobble,
        }
    }

    pub(crate) fn contains(&self, line: usize, prefix: &ContainerPrefix) -> bool {
        line >= self.first_line
            && line - self.first_line < self.offsets.len()
            && self.ops == prefix.ops()
            && self.lazy_blockquote_gobble == prefix.lazy_blockquote_gobble
    }

    /// Find math crossing the start of `line`, retaining the verified cursor
    /// so neither earlier spans nor the accumulated buffer need to be rescanned.
    pub(crate) fn closing_line(&mut self, line: usize) -> Option<usize> {
        let boundary = *self.offsets.get(line.checked_sub(self.first_line)?)?;
        while self.cursor < boundary {
            let rest = &self.text[self.cursor..];
            if matches!(rest.as_bytes()[0], b'$' | b'\\')
                && let Some((len, ..)) = crate::parser::inlines::math::jupyter_math(rest)
            {
                self.cursor += len;
                if self.cursor > boundary {
                    return Some(
                        self.first_line + self.offsets.partition_point(|&at| at < self.cursor) - 1,
                    );
                }
                continue;
            }
            if rest.starts_with("$$") {
                self.cursor += 2;
                continue;
            }
            if rest.starts_with('`')
                && let Some((len, ..)) =
                    crate::parser::inlines::code_spans::try_parse_code_span(rest)
            {
                self.cursor += len;
                continue;
            }
            self.cursor += if let Some(after_escape) = rest.strip_prefix('\\') {
                1 + after_escape.chars().next().map_or(0, char::len_utf8)
            } else {
                rest.chars().next().expect("character boundary").len_utf8()
            };
        }
        None
    }
}

/// Find the closing line of matched Jupyter math crossing the end of `buffered`.
pub(in crate::parser) fn jupyter_math_closing_line<'a>(
    buffered: &str,
    following: impl Iterator<Item = &'a str>,
) -> Option<usize> {
    JupyterMathLookahead::new(
        buffered.to_owned(),
        0,
        &ContainerPrefix::default(),
        following,
    )
    .closing_line(0)
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
            open_display_math: None,
            start_checkpoint,
        });
    }
}

/// Append a line to the current paragraph (preserving losslessness).
pub(in crate::parser) fn append_paragraph_line(
    containers: &mut ContainerStack,
    builder: &mut GreenNodeBuilder<'static>,
    line: &str,
    config: &ParserOptions,
) {
    append_paragraph_line_gobbling(containers, builder, line, 0, config);
}

/// Append a line to the current paragraph, holding its leading `held` bytes
/// out of the text the inline parser sees.
///
/// Those bytes are a content container's continuation indent — the columns
/// pandoc strips off the line before the body is parsed (`noteBlock`'s
/// `indentSpaces` for a footnote, the same rule `listLine` applies in a list
/// item). They are buffered as an `Indent` segment and spliced back as
/// `WHITESPACE` at emission, so the parse stays byte-lossless while an inline
/// construct that preserves interior whitespace measures from the content
/// column instead of from column 0.
///
/// `held` must be a byte offset into `line` at a character boundary; pass 0 to
/// buffer the line verbatim.
pub(in crate::parser) fn append_paragraph_line_gobbling(
    containers: &mut ContainerStack,
    _builder: &mut GreenNodeBuilder<'static>,
    line: &str,
    held: usize,
    config: &ParserOptions,
) {
    if let Some(Container::Paragraph {
        buffer,
        open_inline_math_envs,
        open_display_math,
        ..
    }) = containers.stack.last_mut()
    {
        if held > 0 {
            buffer.push_indent(&line[..held]);
            buffer.push_text(&line[held..]);
        } else {
            buffer.push_text(line);
        }

        let line_no_newline = trim_end_newlines(line);
        update_display_math_state(line_no_newline, open_display_math, config);
        if !config.extensions.tex_math_jupyter
            && let Some(env_name) = extract_environment_name(line_no_newline)
            && is_inline_math_environment(env_name)
        {
            open_inline_math_envs.push(env_name.to_string());
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

pub(in crate::parser) fn has_open_display_math(containers: &ContainerStack) -> bool {
    matches!(
        containers.last(),
        Some(Container::Paragraph {
            open_display_math: Some(_),
            ..
        })
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
