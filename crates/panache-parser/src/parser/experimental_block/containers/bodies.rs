//! Recognition and continuation of definition lists and footnote bodies.

use super::{
    ContainerPrefix, FOOTNOTE_INDENT_COLUMNS, Frame, ParserOptions, StrippedLines,
    byte_index_at_column, container_ops, leading_indent, list_tail, pandoc_list_continues,
    quote_prefix, try_parse_definition_marker,
};
use crate::parser::blocks::definition_lists::next_line_is_definition_marker;
use crate::parser::blocks::reference_links::try_parse_footnote_marker;
use crate::parser::blocks::tables::is_caption_followed_by_table;

pub(in crate::parser::experimental_block) fn footnote_marker(
    line: &str,
    options: &ParserOptions,
) -> Option<(usize, String, usize)> {
    if !options.extensions.footnotes || !line.trim_start_matches(' ').starts_with("[^") {
        return None;
    }
    let indent = line.bytes().take_while(|&byte| byte == b' ').count();
    if indent > 3 {
        return None;
    }
    let (id, end) = try_parse_footnote_marker(&line[indent..])?;
    Some((indent, id, indent + end))
}

pub(in crate::parser::experimental_block) fn root_definition_term(
    lines: &[&str],
    pos: usize,
    options: &ParserOptions,
) -> bool {
    definition_term(lines, pos, &[], options).is_some()
}

pub(super) fn definition_term(
    lines: &[&str],
    pos: usize,
    frames: &[Frame],
    options: &ParserOptions,
) -> Option<usize> {
    if !options.extensions.definition_lists {
        return None;
    }
    let next = *lines.get(pos + 1)?;
    let candidate = next.trim_start_matches([' ', '\t', '>', '\r', '\n']);
    if !candidate.starts_with([':', '~']) && !candidate.is_empty() {
        return None;
    }
    let prefix = ContainerPrefix::from_ops(&container_ops(frames), true);
    let blanks = next_line_is_definition_marker(
        &StrippedLines::new(lines, pos, &prefix),
        pos,
        options.extensions.table_captions,
    )?;
    if frames.iter().any(|frame| body_column(frame).is_some())
        && !prefix.resolve(lines[pos + 1 + blanks]).reaches_frame()
    {
        return None;
    }
    Some(blanks)
}

pub(super) fn definition_marker(
    line: &str,
    lines: &[&str],
    pos: usize,
    frames: &[Frame],
    options: &ParserOptions,
) -> Option<(char, usize, usize, usize)> {
    let marker = try_parse_definition_marker(line)?;
    if marker.0 == ':' && options.extensions.table_captions {
        let prefix = ContainerPrefix::from_ops(&container_ops(frames), true);
        let window = StrippedLines::new(lines, pos, &prefix);
        if is_caption_followed_by_table(&window, pos) {
            return None;
        }
    }
    Some(marker)
}

pub(super) fn body_column(frame: &Frame) -> Option<usize> {
    match frame {
        Frame::Definition { column } => Some(*column),
        Frame::Footnote => Some(FOOTNOTE_INDENT_COLUMNS),
        _ => None,
    }
}

pub(super) fn body_blank_depth(
    frames: &[Frame],
    lines: &[&str],
    next: usize,
    options: &ParserOptions,
) -> usize {
    let Some(mut tail) = lines.get(next).copied() else {
        return 0;
    };
    let mut keep = 0;
    for (index, frame) in frames.iter().enumerate() {
        match frame {
            Frame::Quote => {
                let Some((_, content)) = quote_prefix(tail) else {
                    break;
                };
                tail = content;
                keep = index + 1;
            }
            Frame::Item { column, .. } => {
                let Some(content) = list_tail(tail, *column) else {
                    break;
                };
                tail = content;
                keep = index + 1;
            }
            Frame::List { marker, indent } => {
                if pandoc_list_continues(tail, marker, *indent, options) {
                    keep = index + 1;
                }
            }
            Frame::DefinitionList => {
                if try_parse_definition_marker(tail).is_some()
                    || definition_term(lines, next, &frames[..index], options).is_some()
                {
                    keep = index + 1;
                }
            }
            Frame::DefinitionItem => {
                if try_parse_definition_marker(tail).is_some() {
                    keep = index + 1;
                }
            }
            Frame::Div { .. } => keep = index + 1,
            Frame::Alert => {}
            _ => {
                let column = body_column(frame).unwrap();
                let minimum = if matches!(frame, Frame::Definition { .. })
                    && options.effective_pandoc_compat() == crate::options::PandocCompat::V3_7
                {
                    column.max(4)
                } else {
                    column
                };
                if leading_indent(tail).0 < minimum {
                    break;
                }
                tail = &tail[byte_index_at_column(tail, column)..];
                keep = index + 1;
            }
        }
    }
    keep
}
