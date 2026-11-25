//! Plan mixed container transitions before writing any CST nodes.

use std::ops::Range;

use rowan::GreenNodeBuilder;

use super::super::blocks::blockquotes::{emit_one_blockquote_marker, emit_one_line_prefix_marker};
use super::super::blocks::code_blocks::{
    FenceInfo, is_closing_fence, is_gfm_math_fence, parse_fenced_code_block, try_parse_fence_open,
};
use super::super::blocks::container_prefix::{ContainerPrefix, StrippedLines};
use super::super::blocks::headings::{emit_atx_heading, try_parse_atx_heading};
use super::super::blocks::horizontal_rules::{emit_horizontal_rule, try_parse_horizontal_rule};
use super::super::blocks::indented_code::parse_indented_code_block;
use super::super::blocks::lists::{
    ListItemEmissionInput, ListMarker, ListMarkerMatch, OpenListHint, emit_list_item,
    list_item_content_col, marker_start_number, markers_match, try_parse_list_marker,
};
use super::super::diagnostics::Diagnostics;
use super::super::utils::container_stack::{byte_index_at_column, leading_indent};
use super::super::utils::helpers::is_blank_line;
use super::super::utils::marker_utils::try_parse_blockquote_marker;
use super::super::utils::text_buffer::ParagraphBuffer;
use super::{emit_blank, fence_enabled, is_quote_prose};
use crate::options::{Dialect, ParserOptions};
use crate::syntax::SyntaxKind as K;

type UnsupportedLine<'a> = (&'a str, &'static str);

pub(super) enum Event<'a> {
    Open(K),
    Close,
    QuoteMarker(usize, bool),
    QuoteLinePrefix(usize, bool),
    Item(&'a str, ListMarkerMatch),
    Paragraph(K, ParagraphBuffer),
    Blank(&'a str),
    Whitespace(&'a str),
    Fence(Range<usize>, FenceInfo, ContainerPrefix),
    IndentedCode(Range<usize>, usize),
    Atx(&'a str, usize),
    Rule(&'a str),
}

enum Frame {
    Quote,
    List { marker: ListMarker, indent: usize },
    Item { column: usize },
}

enum Prefix<'a> {
    Quote(usize, bool),
    Indent(&'a str),
}

struct Planner<'a> {
    frames: Vec<Frame>,
    events: Vec<Event<'a>>,
    paragraph: ParagraphBuffer,
    paragraph_kind: K,
}

impl<'a> Planner<'a> {
    fn flush(&mut self) {
        if !self.paragraph.is_empty() {
            self.events.push(Event::Paragraph(
                self.paragraph_kind,
                std::mem::take(&mut self.paragraph),
            ));
        }
    }

    fn close_to(&mut self, depth: usize) {
        self.flush();
        while self.frames.len() > depth {
            self.frames.pop();
            self.events.push(Event::Close);
        }
    }

    fn structural_prefixes(&mut self, prefixes: Vec<Prefix<'a>>) {
        for prefix in prefixes {
            match prefix {
                Prefix::Quote(indent, space) => self.events.push(Event::QuoteMarker(indent, space)),
                Prefix::Indent(indent) => self.events.push(Event::Whitespace(indent)),
            }
        }
    }

    fn push_text(&mut self, remaining: &'a str, prefixes: Vec<Prefix<'a>>) {
        if self.paragraph.is_empty() {
            self.paragraph_kind = if matches!(self.frames.last(), Some(Frame::Item { .. })) {
                K::PLAIN
            } else {
                K::PARAGRAPH
            };
        }
        for prefix in prefixes {
            match prefix {
                Prefix::Quote(indent, space) => self.paragraph.push_marker(indent, space),
                Prefix::Indent(indent) => self.paragraph.push_indent(indent),
            }
        }
        self.paragraph.push_text(remaining);
    }
}

fn quote_prefix(line: &str) -> Option<(Prefix<'_>, &str)> {
    let (marker_end, tail) = try_parse_blockquote_marker(line)?;
    Some((
        Prefix::Quote(marker_end - 1, tail > marker_end),
        &line[tail..],
    ))
}

fn marker(line: &str, options: &ParserOptions) -> Option<ListMarkerMatch> {
    (leading_indent(line).0 < 4)
        .then(|| try_parse_list_marker(line, options, OpenListHint::None))
        .flatten()
}

pub(super) fn plan<'a>(
    lines: &[&'a str],
    options: &ParserOptions,
) -> Result<(usize, Vec<Event<'a>>), UnsupportedLine<'a>> {
    if lines.is_empty() {
        return Ok((0, Vec::new()));
    }
    let mut planner = Planner {
        frames: Vec::new(),
        events: Vec::new(),
        paragraph: ParagraphBuffer::new(),
        paragraph_kind: K::PARAGRAPH,
    };
    let commonmark = options.dialect == Dialect::CommonMark;
    let mut pos = 0;
    let mut saw_quote = false;
    while let Some(&line) = lines.get(pos) {
        let mut remaining = line;
        let mut prefixes = Vec::new();
        let mut matched = 0;
        let mut sibling = None;
        while let Some(frame) = planner.frames.get(matched) {
            match frame {
                Frame::Quote => {
                    let Some((prefix, tail)) = quote_prefix(remaining) else {
                        break;
                    };
                    prefixes.push(prefix);
                    remaining = tail;
                }
                Frame::List {
                    marker: first,
                    indent,
                } => {
                    sibling = marker(remaining, options).filter(|next| {
                        leading_indent(remaining).0 == *indent
                            && markers_match(first, &next.marker, options.dialect)
                    });
                    if sibling.is_some() {
                        matched += 1;
                        break;
                    }
                }
                Frame::Item { column } => {
                    if leading_indent(remaining).0 < *column {
                        break;
                    }
                    let bytes = byte_index_at_column(remaining, *column);
                    let (indent, tail) = remaining.split_at(bytes);
                    prefixes.push(Prefix::Indent(indent));
                    remaining = tail;
                }
            }
            matched += 1;
        }

        if commonmark
            && sibling.is_some()
            && planner.paragraph_kind == K::PARAGRAPH
            && !planner.paragraph.is_empty()
            && matches!(
                planner.frames.get(matched..),
                Some([Frame::Item { .. }, Frame::Quote])
            )
        {
            planner.push_text(remaining, prefixes);
            pos += 1;
            continue;
        }

        if sibling.is_none() && matched < planner.frames.len() {
            if is_blank_line(remaining) {
                let next = lines[pos + 1..].iter().find(|line| !is_blank_line(line));
                let mut tail = next.copied();
                for frame in &planner.frames[..matched] {
                    if matches!(frame, Frame::Quote) {
                        tail = tail.and_then(|line| quote_prefix(line).map(|(_, tail)| tail));
                    }
                }
                if let Some(Frame::Item { column }) = planner.frames.get(matched)
                    && tail.is_some_and(|line| leading_indent(line).0 >= *column)
                {
                    planner.flush();
                    planner.structural_prefixes(prefixes);
                    planner.events.push(Event::Blank(remaining));
                    pos += 1;
                    continue;
                }
                if let Some(Frame::List {
                    marker: first,
                    indent,
                }) = planner.frames.get(matched.wrapping_sub(1))
                    && tail.is_some_and(|line| {
                        leading_indent(line).0 == *indent
                            && marker(line, options).is_some_and(|next| {
                                markers_match(first, &next.marker, options.dialect)
                            })
                    })
                {
                    planner.close_to(matched);
                    planner.structural_prefixes(prefixes);
                    planner.events.push(Event::Blank(remaining));
                    pos += 1;
                    continue;
                }
                if is_blank_line(line) {
                    if let [
                        Frame::List {
                            marker: first,
                            indent,
                        },
                        Frame::Item { .. },
                        ..,
                    ] = planner.frames.as_slice()
                        && let Some(next) = next
                        && leading_indent(next).0 == *indent
                        && marker(next, options)
                            .is_some_and(|next| markers_match(first, &next.marker, options.dialect))
                    {
                        planner.close_to(1);
                        planner.events.push(Event::Blank(line));
                        pos += 1;
                        continue;
                    }
                    break;
                }
            }
            if !planner.paragraph.is_empty()
                && !is_blank_line(remaining)
                && (is_quote_prose(remaining, options)
                    || (commonmark && leading_indent(remaining).0 >= 4))
            {
                // Missing quote markers do not erase the indent of an
                // enclosing list item on the same lazy continuation.
                for frame in &planner.frames[matched..] {
                    if let Frame::Item { column } = frame
                        && leading_indent(remaining).0 >= *column
                    {
                        let bytes = byte_index_at_column(remaining, *column);
                        let (indent, tail) = remaining.split_at(bytes);
                        prefixes.push(Prefix::Indent(indent));
                        remaining = tail;
                    }
                }
                planner.push_text(remaining, prefixes);
                pos += 1;
                continue;
            }
            if matches!(
                planner.frames.get(matched.wrapping_sub(1)),
                Some(Frame::List { .. })
            ) {
                matched -= 1;
            }
        }

        if sibling.is_some() || matched < planner.frames.len() {
            planner.close_to(matched);
            if planner.frames.is_empty() {
                break;
            }
        }

        let mut opened_item = false;
        let mut ordered_marker_as_text = false;
        loop {
            if let Some((prefix, tail)) = quote_prefix(remaining) {
                planner.flush();
                planner.events.push(Event::Open(K::BLOCK_QUOTE));
                planner.structural_prefixes(std::mem::take(&mut prefixes));
                planner.structural_prefixes(vec![prefix]);
                planner.frames.push(Frame::Quote);
                saw_quote = true;
                remaining = tail;
                continue;
            }
            if let Some(next) = sibling.take().or_else(|| marker(remaining, options)) {
                let (indent, bytes) = leading_indent(remaining);
                let tail = &remaining[bytes + next.marker_len + next.spaces_after_bytes..];
                if next.virtual_marker_space {
                    return Err((remaining, "list marker with virtual padding"));
                }
                if ["[x]", "[X]", "[ ]"]
                    .iter()
                    .any(|task| tail.starts_with(task))
                {
                    return Err((remaining, "nested task list"));
                }
                let nested_start_restricted = planner.frames.last().is_some_and(|f| {
                    matches!(f, Frame::Item { .. })
                        && !commonmark
                        && options.pandoc_compat.restricts_ordered_sublist_start()
                        && marker_start_number(&next.marker).is_some_and(|start| start != 1)
                });
                let commonmark_lazy_ordered = commonmark
                    && !planner.paragraph.is_empty()
                    && matches!(planner.frames.last(), Some(Frame::Item { .. }))
                    && marker_start_number(&next.marker).is_some_and(|start| start != 1);
                if nested_start_restricted || commonmark_lazy_ordered {
                    ordered_marker_as_text = true;
                    if nested_start_restricted {
                        planner.flush();
                    }
                    break;
                }
                planner.flush();
                let mut item_line = remaining;
                if matches!(
                    prefixes.as_slice(),
                    [Prefix::Quote(..), .., Prefix::Indent(_)]
                ) && matches!(planner.frames.last(), Some(Frame::Item { .. }))
                    && let Some(Prefix::Indent(indent)) = prefixes.pop()
                {
                    item_line = &line[line.len() - remaining.len() - indent.len()..];
                    for prefix in prefixes.drain(..) {
                        match prefix {
                            Prefix::Quote(indent, space) => {
                                planner.events.push(Event::QuoteLinePrefix(indent, space))
                            }
                            Prefix::Indent(indent) => {
                                planner.events.push(Event::Whitespace(indent))
                            }
                        }
                    }
                }
                planner.structural_prefixes(std::mem::take(&mut prefixes));
                if !matches!(planner.frames.last(), Some(Frame::List { .. })) {
                    planner.events.push(Event::Open(K::LIST));
                    planner.frames.push(Frame::List {
                        marker: next.marker.clone(),
                        indent,
                    });
                }
                let padding = if commonmark && is_blank_line(tail) {
                    1
                } else {
                    next.spaces_after_cols
                };
                let column = list_item_content_col(indent, next.marker_len, padding, options);
                planner.events.push(Event::Item(item_line, next));
                planner.frames.push(Frame::Item { column });
                remaining = tail;
                opened_item = true;
                continue;
            }
            break;
        }

        if planner.frames.is_empty() {
            return Err((remaining, "container opener"));
        }
        if is_blank_line(remaining) {
            planner.flush();
            if !opened_item {
                planner.structural_prefixes(prefixes);
                planner.events.push(Event::Blank(remaining));
            } else if !remaining.is_empty() {
                planner.push_text(remaining, prefixes);
                planner.flush();
            }
        } else if planner.paragraph.is_empty()
            && !opened_item
            && leading_indent(remaining).0 >= 4
            && let [Frame::List { .. }, Frame::Item { column }] = planner.frames.as_slice()
        {
            let mut end = pos + 1;
            for (index, raw) in lines.iter().enumerate().skip(end) {
                if !is_blank_line(raw) {
                    if leading_indent(raw).0 < *column + 4 {
                        break;
                    }
                    end = index + 1;
                }
            }
            planner.events.push(Event::IndentedCode(pos..end, *column));
            pos = end;
            continue;
        } else if !opened_item
            && let Some(fence) = try_parse_fence_open(remaining, options.dialect)
            && fence_enabled(&fence, options)
            && !(options.extensions.tex_math_gfm && is_gfm_math_fence(&fence))
        {
            let quote_depth = planner
                .frames
                .iter()
                .take_while(|frame| matches!(frame, Frame::Quote))
                .count();
            let column = match &planner.frames[quote_depth..] {
                [] => 0,
                [Frame::List { .. }, Frame::Item { column }] => *column,
                _ => return Err((remaining, "nested fenced code")),
            };
            let prefix = ContainerPrefix::from_scalars(
                quote_depth,
                column,
                quote_depth > 0,
                0,
                false,
                options.dialect,
            );
            let mut end = pos + 1;
            let mut closed = false;
            for &raw in &lines[end..] {
                let mut tail = raw;
                for _ in 0..quote_depth {
                    let Some((_, rest)) = quote_prefix(tail) else {
                        break;
                    };
                    tail = rest;
                }
                if quote_depth > 0
                    && raw[..raw.len() - tail.len()]
                        .bytes()
                        .filter(|&byte| byte == b'>')
                        .count()
                        != quote_depth
                {
                    break;
                }
                if !is_blank_line(tail) && leading_indent(tail).0 < column {
                    break;
                }
                tail = &tail[byte_index_at_column(tail, column)..];
                end += 1;
                if is_closing_fence(tail, &fence) {
                    closed = true;
                    break;
                }
            }
            if !closed && !commonmark {
                return Err((remaining, "nested fenced code"));
            }
            if column == 0 {
                if planner.paragraph.is_empty() {
                    planner.structural_prefixes(prefixes);
                } else {
                    for part in prefixes {
                        if let Prefix::Quote(indent, space) = part {
                            planner.paragraph.push_marker(indent, space);
                        }
                    }
                }
                planner.flush();
            } else {
                planner.flush();
                for part in prefixes {
                    if let Prefix::Quote(indent, space) = part {
                        planner.events.push(Event::QuoteLinePrefix(indent, space));
                    }
                }
            }
            planner.events.push(Event::Fence(pos..end, fence, prefix));
            pos = end;
            continue;
        } else if let Some(level) = try_parse_atx_heading(remaining) {
            planner.flush();
            planner.structural_prefixes(prefixes);
            planner.events.push(Event::Atx(remaining, level));
        } else if try_parse_horizontal_rule(remaining).is_some() {
            planner.flush();
            planner.structural_prefixes(prefixes);
            planner.events.push(Event::Rule(remaining));
        } else if ordered_marker_as_text || is_quote_prose(remaining, options) {
            // Within a quoted list the incumbent retains list indentation in
            // the text after the quote marker.
            if matches!(planner.frames.last(), Some(Frame::Item { .. }))
                && matches!(prefixes.last(), Some(Prefix::Indent(_)))
                && (planner.paragraph.is_empty()
                    || prefixes.iter().any(|p| matches!(p, Prefix::Quote(..))))
                && let Some(Prefix::Indent(indent)) = prefixes.pop()
            {
                remaining = &line[line.len() - remaining.len() - indent.len()..];
            }
            if planner.paragraph.is_empty() && matches!(planner.frames.last(), Some(Frame::Quote)) {
                planner.structural_prefixes(std::mem::take(&mut prefixes));
            }
            planner.push_text(remaining, prefixes);
        } else {
            return Err((remaining, "nested block"));
        }
        pos += 1;
    }
    if !saw_quote {
        return Err((lines[0], "list continuation or inner block"));
    }
    planner.close_to(0);
    Ok((pos, planner.events))
}

pub(super) fn emit(
    builder: &mut GreenNodeBuilder<'static>,
    events: Vec<Event<'_>>,
    lines: &[&str],
    start: usize,
    diagnostics: &Diagnostics,
    options: &ParserOptions,
) {
    for event in events {
        match event {
            Event::Open(kind) => builder.start_node(kind.into()),
            Event::Close => builder.finish_node(),
            Event::QuoteMarker(indent, space) => emit_one_blockquote_marker(builder, indent, space),
            Event::QuoteLinePrefix(indent, space) => {
                emit_one_line_prefix_marker(builder, indent, space)
            }
            Event::Item(line, marker) => {
                let (indent_cols, indent_bytes) = leading_indent(line);
                emit_list_item(
                    builder,
                    &ListItemEmissionInput {
                        content: line,
                        marker_len: marker.marker_len,
                        spaces_after_cols: marker.spaces_after_cols,
                        spaces_after_bytes: marker.spaces_after_bytes,
                        indent_cols,
                        indent_bytes,
                        virtual_marker_space: marker.virtual_marker_space,
                    },
                    options,
                );
            }
            Event::Paragraph(kind, buffer) => {
                builder.start_node(kind.into());
                buffer.emit_with_inlines(builder, options, false);
                builder.finish_node();
            }
            Event::Blank(line) => emit_blank(builder, line),
            Event::Whitespace(indent) => builder.token(K::WHITESPACE.into(), indent),
            Event::Fence(range, fence, prefix) => {
                let window =
                    StrippedLines::new(&lines[..start + range.end], start + range.start, &prefix);
                parse_fenced_code_block(
                    builder,
                    &window,
                    fence,
                    None,
                    diagnostics,
                    options.flavor,
                    None,
                );
            }
            Event::IndentedCode(range, column) => {
                parse_indented_code_block(
                    builder,
                    &lines[..start + range.end],
                    start + range.start,
                    0,
                    column,
                );
            }
            Event::Atx(line, level) => emit_atx_heading(builder, line, level, options),
            Event::Rule(line) => emit_horizontal_rule(builder, line),
        }
    }
}
