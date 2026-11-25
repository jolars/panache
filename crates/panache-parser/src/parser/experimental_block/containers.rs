//! Plan mixed container transitions before writing any CST nodes.

use std::{borrow::Cow, ops::Range};

use rowan::GreenNodeBuilder;

use super::super::block_dispatcher::emit_reference_definition_spans;
use super::super::blocks::blockquotes::{emit_one_blockquote_marker, emit_one_line_prefix_marker};
use super::super::blocks::code_blocks::{
    FenceInfo, emit_planned_fenced_code_block, is_closing_fence, is_gfm_math_fence,
    parse_fenced_code_block, try_parse_fence_open,
};
use super::super::blocks::container_prefix::{ContainerPrefix, StripOp, StrippedLines};
use super::super::blocks::headings::{
    emit_atx_heading, emit_setext_underline, try_parse_atx_heading, try_parse_setext_heading,
};
use super::super::blocks::horizontal_rules::{emit_horizontal_rule, try_parse_horizontal_rule};
use super::super::blocks::indented_code::{
    emit_indented_code_line, parse_indented_code_block_with_first,
};
use super::super::blocks::lists::{
    ListItemEmissionInput, ListMarker, ListMarkerMatch, OpenListHint, emit_list_item,
    is_task_checkbox, list_item_content_col, marker_start_number, markers_match,
    try_parse_list_marker,
};
use super::super::diagnostics::Diagnostics;
use super::super::inlines::sink::{InjectedMarker, MarkerInjectingSink};
use super::super::utils::container_stack::{byte_index_at_column, leading_indent};
use super::super::utils::helpers::is_blank_line;
use super::super::utils::marker_utils::try_parse_blockquote_marker;
use super::super::utils::text_buffer::ParagraphBuffer;
use super::{
    HtmlBlockContext, HtmlBlockType, ReferenceSpans, SoftbreakFusion, emit_blank,
    emit_buffered_paragraph, ends_at_blank_line, fence_enabled, is_closing_marker, is_quote_prose,
    parse_html_block_with_wrapper, reference_definition_spans, try_parse_html_block_start,
};
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
    Fence(Range<usize>, FenceInfo, ContainerPrefix, Option<&'a str>),
    PlannedFence(Range<usize>, FenceInfo, ContainerPrefix, &'a str, bool),
    IndentedCode(Range<usize>, usize, usize, Option<&'a str>),
    PlannedIndentedCode(Vec<(&'a str, &'a str)>),
    Atx(&'a str, usize),
    Rule(&'a str),
    Setext(ParagraphBuffer, Vec<Prefix<'a>>, &'a str),
    Reference(Vec<Prefix<'a>>, PlannedReference<'a>),
    Html(Range<usize>, HtmlBlockType, ContainerPrefix, &'a str),
}

enum Frame {
    Quote,
    List { marker: ListMarker, indent: usize },
    Item { column: usize, empty_opener: bool },
}

pub(super) enum Prefix<'a> {
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

    fn interrupt_commonmark(
        &mut self,
        line: &'a str,
        remaining: &'a str,
        mut prefixes: Vec<Prefix<'a>>,
    ) -> &'a str {
        let content = restore_trailing_indent(line, remaining, &mut prefixes);
        if self.paragraph.is_empty() {
            self.structural_prefixes(prefixes);
        } else if self.paragraph_kind == K::PARAGRAPH {
            self.push_text("", prefixes);
            self.flush();
        } else {
            self.flush();
            for prefix in prefixes {
                match prefix {
                    Prefix::Quote(indent, space) => {
                        self.events.push(Event::QuoteLinePrefix(indent, space))
                    }
                    Prefix::Indent(indent) => self.events.push(Event::Whitespace(indent)),
                }
            }
        }
        content
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

fn list_interrupts_quote_paragraph(
    marker: &ListMarkerMatch,
    tail: &str,
    options: &ParserOptions,
) -> bool {
    if options.dialect == Dialect::CommonMark {
        marker_start_number(&marker.marker).is_none_or(|start| start == 1) && !is_blank_line(tail)
    } else {
        options.extensions.lists_without_preceding_blankline
    }
}

fn quote_tail(mut line: &str, depth: usize) -> Option<&str> {
    for _ in 0..depth {
        line = quote_prefix(line)?.1;
    }
    Some(line)
}

fn restore_trailing_indent<'a>(
    line: &'a str,
    remaining: &'a str,
    prefixes: &mut Vec<Prefix<'a>>,
) -> &'a str {
    let mut bytes = 0;
    while let Some(Prefix::Indent(indent)) = prefixes.last() {
        bytes += indent.len();
        prefixes.pop();
    }
    &line[line.len() - remaining.len() - bytes..]
}

fn absorb_quote_indent(prefix: &mut Prefix<'_>, prefixes: &mut Vec<Prefix<'_>>) {
    if let Prefix::Quote(indent, _) = prefix {
        while let Some(Prefix::Indent(prior)) = prefixes.last() {
            *indent += prior.len();
            prefixes.pop();
        }
    }
}

fn list_tail(line: &str, column: usize) -> Option<&str> {
    (is_blank_line(line) || leading_indent(line).0 >= column)
        .then(|| &line[byte_index_at_column(line, column)..])
}

fn quote_and_list_columns(frames: &[Frame]) -> Option<(usize, usize)> {
    let quotes = frames
        .iter()
        .take_while(|frame| matches!(frame, Frame::Quote))
        .count();
    let mut column = 0;
    for frame in &frames[quotes..] {
        match frame {
            Frame::List { .. } => {}
            Frame::Item { column: indent, .. } => column += indent,
            Frame::Quote => return None,
        }
    }
    Some((quotes, column))
}

fn container_ops(frames: &[Frame]) -> Vec<StripOp> {
    let mut ops = Vec::new();
    for frame in frames {
        match frame {
            Frame::Quote => ops.push(StripOp::BlockQuoteMarker),
            Frame::Item { column, .. } => {
                if let Some(StripOp::ListAdvance(previous)) = ops.last_mut() {
                    *previous += *column as u32;
                } else {
                    ops.push(StripOp::ListAdvance(*column as u32));
                }
            }
            Frame::List { .. } => {}
        }
    }
    ops
}

fn opaque_block_columns(frames: &[Frame], allow_list_first: bool) -> Option<(usize, usize, bool)> {
    if let Some((quotes, column)) = quote_and_list_columns(frames) {
        return Some((quotes, column, quotes > 0));
    }
    if !allow_list_first {
        return None;
    }
    let first_quote = frames
        .iter()
        .position(|frame| matches!(frame, Frame::Quote))?;
    let (_, column) = quote_and_list_columns(&frames[..first_quote])?;
    let (quotes, trailing_column) = quote_and_list_columns(&frames[first_quote..])?;
    (trailing_column == 0).then_some((quotes, column, false))
}

pub(super) struct PlannedReference<'a> {
    pub(super) source: Cow<'a, str>,
    pub(super) spans: ReferenceSpans,
    markers: Vec<(usize, InjectedMarker<'a>)>,
    consumed_lines: usize,
}

fn plan_reference<'a>(
    first: &'a str,
    following: &[&'a str],
    frames: &[Frame],
    dialect: Dialect,
) -> Option<PlannedReference<'a>> {
    let ops = container_ops(frames);
    let quoted = ops.contains(&StripOp::BlockQuoteMarker);
    let continuation = |raw: &'a str| -> Option<&'a str> {
        let tail = ContainerPrefix::strip_required_ops(raw, &ops)?;
        (!is_blank_line(tail)).then_some(if quoted { tail } else { raw })
    };

    // A complete first line can extend only with a title on the next line.
    if let Some(spans) = reference_definition_spans(first, true, dialect, false)
        && (spans.title.is_some()
            || following
                .first()
                .and_then(|raw| continuation(raw))
                .is_none_or(|tail| {
                    !tail
                        .trim_start_matches([' ', '\t'])
                        .starts_with(['"', '\'', '('])
                }))
    {
        return Some(PlannedReference {
            source: Cow::Borrowed(first),
            spans,
            markers: Vec::new(),
            consumed_lines: 1,
        });
    }

    let mut source = first.to_owned();
    let mut markers = Vec::new();
    let mut starts = vec![0];
    for &raw in following {
        let Some(tail) = continuation(raw) else { break };
        starts.push(source.len());
        let prefix = &raw[..raw.len() - tail.len()];
        if !prefix.is_empty() {
            markers.push((source.len(), InjectedMarker::Indent(prefix)));
        }
        source.push_str(tail);
    }
    let spans = reference_definition_spans(&source, true, dialect, false)?;
    let consumed_lines = starts.partition_point(|&start| start < spans.consumed);
    let end = starts.get(consumed_lines).copied().unwrap_or(source.len());
    source.truncate(end);
    markers.retain(|&(offset, _)| offset < end);
    Some(PlannedReference {
        source: Cow::Owned(source),
        spans,
        markers,
        consumed_lines,
    })
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
    let mut next_nonblank = 0;
    while let Some(&line) = lines.get(pos) {
        let mut remaining = line;
        let mut prefixes = Vec::new();
        let mut matched = 0;
        let mut sibling = None;
        let mut list_columns = 0;
        while let Some(frame) = planner.frames.get(matched) {
            let indent = if commonmark
                && list_columns > 0
                && prefixes.iter().all(|p| matches!(p, Prefix::Indent(_)))
            {
                leading_indent(line).0.saturating_sub(list_columns)
            } else {
                leading_indent(remaining).0
            };
            match frame {
                Frame::Quote => {
                    let Some((mut prefix, tail)) = quote_prefix(remaining) else {
                        break;
                    };
                    if commonmark {
                        absorb_quote_indent(&mut prefix, &mut prefixes);
                    }
                    prefixes.push(prefix);
                    remaining = tail;
                }
                Frame::List {
                    marker: first,
                    indent: list_indent,
                } => {
                    sibling = marker(remaining, options).filter(|next| {
                        (if commonmark {
                            let item_column = match planner.frames.get(matched + 1) {
                                Some(Frame::Item { column, .. }) => *column,
                                _ => 4,
                            };
                            indent < item_column
                        } else {
                            indent == *list_indent
                        }) && markers_match(first, &next.marker, options.dialect)
                    });
                    if sibling.is_some() {
                        matched += 1;
                        break;
                    }
                }
                Frame::Item { column, .. } => {
                    if indent < *column {
                        break;
                    }
                    let bytes = byte_index_at_column(remaining, *column);
                    let (indent, tail) = remaining.split_at(bytes);
                    if !commonmark {
                        // Pandoc's tabbed continuations are outside this slice.
                        let (_, tail_indent_bytes) = leading_indent(tail);
                        if tail[..tail_indent_bytes].contains('\t') {
                            return Err((remaining, "tabbed list continuation"));
                        }
                    }
                    list_columns += column;
                    prefixes.push(Prefix::Indent(indent));
                    remaining = tail;
                }
            }
            matched += 1;
        }

        if commonmark && is_blank_line(line) {
            // A blank belongs to the deepest container that can continue
            // on the next nonblank line.
            if next_nonblank <= pos {
                next_nonblank = pos + 1;
                while lines
                    .get(next_nonblank)
                    .is_some_and(|line| is_blank_line(line))
                {
                    next_nonblank += 1;
                }
            }
            let mut tail = lines.get(next_nonblank).copied();
            let mut depth = 0;
            for frame in &planner.frames {
                let Some(next) = tail else { break };
                match frame {
                    Frame::Quote => break,
                    Frame::List { marker: first, .. } => {
                        let Some(Frame::Item {
                            column,
                            empty_opener,
                        }) = planner.frames.get(depth + 1)
                        else {
                            if marker(next, options).is_some_and(|next| {
                                markers_match(first, &next.marker, options.dialect)
                            }) {
                                depth += 1;
                            }
                            break;
                        };
                        if leading_indent(next).0 < *column || *empty_opener {
                            if marker(next, options).is_some_and(|next| {
                                markers_match(first, &next.marker, options.dialect)
                            }) && leading_indent(next).0 < *column
                            {
                                depth += 1;
                            }
                            break;
                        }
                    }
                    Frame::Item { column, .. } => {
                        tail = Some(&next[byte_index_at_column(next, *column)..]);
                    }
                }
                depth += 1;
            }
            if depth == 0 {
                break;
            }
            planner.close_to(depth);
            planner.events.push(Event::Blank(line));
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
                if let Some(Frame::Item { column, .. }) = planner.frames.get(matched)
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
                    || (commonmark && leading_indent(remaining).0 >= 4)
                    || (matches!(planner.frames.get(matched), Some(Frame::Quote))
                        && marker(remaining, options).is_some_and(|next| {
                            let (_, indent) = leading_indent(remaining);
                            let tail = &remaining[indent + next.marker_len..];
                            !list_interrupts_quote_paragraph(&next, tail, options)
                        })))
            {
                // Missing quote markers do not erase the indent of an
                // enclosing list item on the same lazy continuation.
                for frame in &planner.frames[matched..] {
                    if let Frame::Item { column, .. } = frame
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
        let mut task_body = false;
        let mut opener_as_text = false;
        loop {
            if let Some((mut prefix, tail)) = quote_prefix(remaining) {
                if options.extensions.blank_before_blockquote && !planner.paragraph.is_empty() {
                    opener_as_text = true;
                    break;
                }
                planner.flush();
                if commonmark {
                    absorb_quote_indent(&mut prefix, &mut prefixes);
                }
                planner.events.push(Event::Open(K::BLOCK_QUOTE));
                planner.structural_prefixes(std::mem::take(&mut prefixes));
                planner.structural_prefixes(vec![prefix]);
                planner.frames.push(Frame::Quote);
                saw_quote = true;
                list_columns = 0;
                remaining = tail;
                continue;
            }
            let (mut indent, bytes) = leading_indent(remaining);
            if commonmark
                && list_columns > 0
                && prefixes
                    .iter()
                    .all(|prefix| matches!(prefix, Prefix::Indent(_)))
            {
                indent = leading_indent(line).0.saturating_sub(list_columns);
            }
            if let Some(next) = sibling.take().or_else(|| {
                (indent < 4)
                    .then(|| try_parse_list_marker(remaining, options, OpenListHint::None))
                    .flatten()
            }) {
                let tail = &remaining[bytes + next.marker_len + next.spaces_after_bytes..];
                if !commonmark && next.virtual_marker_space {
                    return Err((remaining, "list marker with virtual padding"));
                }
                if !planner.paragraph.is_empty()
                    && (commonmark || matches!(planner.frames.last(), Some(Frame::Quote)))
                    && !list_interrupts_quote_paragraph(&next, tail, options)
                {
                    opener_as_text = true;
                    break;
                }
                task_body = is_task_checkbox(tail);
                if task_body && !commonmark {
                    return Err((remaining, "nested task list"));
                }
                let nested_start_restricted = planner.frames.last().is_some_and(|f| {
                    matches!(f, Frame::Item { .. })
                        && !commonmark
                        && options.pandoc_compat.restricts_ordered_sublist_start()
                        && marker_start_number(&next.marker).is_some_and(|start| start != 1)
                });
                if nested_start_restricted {
                    opener_as_text = true;
                    planner.flush();
                    break;
                }
                if !commonmark && leading_indent(tail).0 >= 4 && !is_blank_line(tail) {
                    return Err((remaining, "list item opening with indented code"));
                }
                if commonmark
                    && planner.paragraph_kind == K::PARAGRAPH
                    && !planner.paragraph.is_empty()
                    && matches!(planner.frames.last(), Some(Frame::Quote))
                {
                    planner.push_text("", std::mem::take(&mut prefixes));
                }
                planner.flush();
                let mut item_line = remaining;
                let item_owns_indent = match planner.frames.last() {
                    Some(Frame::Item { .. }) => {
                        commonmark || matches!(prefixes.first(), Some(Prefix::Quote(..)))
                    }
                    Some(Frame::List { .. }) => commonmark,
                    _ => false,
                };
                if item_owns_indent
                    && matches!(prefixes.last(), Some(Prefix::Indent(_)))
                    && let Some(Prefix::Indent(indent)) = prefixes.pop()
                {
                    let mut bytes = indent.len();
                    if commonmark {
                        while let Some(Prefix::Indent(indent)) = prefixes.last() {
                            bytes += indent.len();
                            prefixes.pop();
                        }
                    }
                    item_line = &line[line.len() - remaining.len() - bytes..];
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
                planner.frames.push(Frame::Item {
                    column,
                    empty_opener: is_blank_line(tail),
                });
                remaining = tail;
                list_columns = 0;
                opened_item = true;
                if task_body {
                    break;
                }
                continue;
            }
            break;
        }

        if planner.frames.is_empty() {
            return Err((remaining, "container opener"));
        }
        if task_body {
            planner.push_text(&remaining[3..], prefixes);
            pos += 1;
            continue;
        }
        let quote_inside_list = commonmark
            && matches!(planner.frames.last(), Some(Frame::Quote))
            && planner
                .frames
                .iter()
                .any(|frame| matches!(frame, Frame::Item { .. }));
        if is_blank_line(remaining) {
            planner.flush();
            if !opened_item || (commonmark && matches!(planner.frames.last(), Some(Frame::Quote))) {
                planner.structural_prefixes(prefixes);
                planner.events.push(Event::Blank(remaining));
            } else if !remaining.is_empty() {
                planner.push_text(remaining, prefixes);
                let continuation = if commonmark {
                    lines.get(pos + 1).and_then(|line| {
                        ContainerPrefix::strip_required_ops(line, &container_ops(&planner.frames))
                    })
                } else {
                    None
                };
                if !continuation
                    .is_some_and(|line| !is_blank_line(line) && is_quote_prose(line, options))
                {
                    planner.flush();
                }
            }
        } else if planner.paragraph.is_empty()
            && leading_indent(remaining).0 >= 4
            && (commonmark
                || !opened_item
                    && matches!(
                        planner.frames.as_slice(),
                        [Frame::List { .. }, Frame::Item { .. }]
                    ))
        {
            let Some((quote_depth, column, _)) =
                opaque_block_columns(&planner.frames, commonmark).filter(|_| !quote_inside_list)
            else {
                let ops = container_ops(&planner.frames);
                let prefix = ContainerPrefix::from_ops(&ops, false);
                let first = restore_trailing_indent(line, remaining, &mut prefixes);
                planner.structural_prefixes(prefixes);
                let mut code = vec![("", first)];
                let mut end = pos + 1;
                for raw in &lines[end..] {
                    let Some(tail) = ContainerPrefix::strip_required_ops(raw, &ops) else {
                        break;
                    };
                    let body = prefix
                        .strip_through_last_quote(raw)
                        .expect("matched quote prefix");
                    if !is_blank_line(tail) && leading_indent(tail).0 < 4 {
                        break;
                    }
                    code.push((&raw[..raw.len() - body.len()], body));
                    if !is_blank_line(tail) {
                        end = pos + code.len();
                    }
                }
                code.truncate(end - pos);
                planner.events.push(Event::PlannedIndentedCode(code));
                pos = end;
                continue;
            };
            let mut end = pos + 1;
            for (index, raw) in lines.iter().enumerate().skip(end) {
                let Some(tail) = quote_tail(raw, quote_depth) else {
                    break;
                };
                if !is_blank_line(tail) {
                    if leading_indent(tail).0 < column + 4 {
                        break;
                    }
                    end = index + 1;
                }
            }
            if !opened_item {
                for prefix in prefixes {
                    if let Prefix::Quote(indent, space) = prefix {
                        planner.events.push(Event::QuoteMarker(indent, space));
                    }
                }
            }
            planner.events.push(Event::IndentedCode(
                pos..end,
                quote_depth,
                column,
                opened_item.then_some(remaining),
            ));
            pos = end;
            continue;
        } else if (commonmark || !opened_item)
            && let Some(fence) = try_parse_fence_open(remaining, options.dialect)
            && fence_enabled(&fence, options)
            && !(options.extensions.tex_math_gfm && is_gfm_math_fence(&fence))
        {
            if !commonmark && fence.fence_char == '~' && !planner.paragraph.is_empty() {
                planner.push_text(remaining, prefixes);
                pos += 1;
                continue;
            }
            if commonmark && opaque_block_columns(&planner.frames, true).is_none() {
                let mut ops = container_ops(&planner.frames);
                let mut end = pos + 1;
                let mut closed = false;
                for raw in &lines[end..] {
                    let Some(tail) = ContainerPrefix::strip_required_ops(raw, &ops) else {
                        break;
                    };
                    end += 1;
                    if is_closing_fence(tail, &fence) {
                        closed = true;
                        break;
                    }
                }
                // The existing CST owns a short initial indent as part of
                // the first quote marker on fenced-code body lines.
                if matches!(
                    ops.as_slice(),
                    [StripOp::ListAdvance(_), StripOp::BlockQuoteMarker, ..]
                ) && lines[pos + 1..end]
                    .iter()
                    .all(|raw| quote_prefix(raw).is_some())
                {
                    ops.remove(0);
                }
                let prefix = ContainerPrefix::from_ops(&ops, true);
                let first = planner.interrupt_commonmark(line, remaining, prefixes);
                planner
                    .events
                    .push(Event::PlannedFence(pos..end, fence, prefix, first, closed));
                pos = end;
                continue;
            }
            let (quote_depth, column, quotes_first) =
                opaque_block_columns(&planner.frames, commonmark)
                    .ok_or((remaining, "nested fenced code"))?;
            let prefix = ContainerPrefix::from_scalars(
                quote_depth,
                column,
                quotes_first,
                0,
                opened_item,
                options.dialect,
            );
            let mut end = pos + 1;
            let mut closed = false;
            for &raw in &lines[end..] {
                let tail = if quotes_first {
                    quote_tail(raw, quote_depth).and_then(|line| list_tail(line, column))
                } else {
                    list_tail(raw, column).and_then(|line| quote_tail(line, quote_depth))
                };
                let Some(tail) = tail else { break };
                end += 1;
                if is_closing_fence(tail, &fence) {
                    closed = true;
                    break;
                }
            }
            if !closed && !commonmark {
                return Err((remaining, "nested fenced code"));
            }
            if commonmark && quote_depth > 0 && !quotes_first {
                let first = planner.interrupt_commonmark(line, remaining, prefixes);
                let mut prefix = prefix;
                prefix.list_marker_consumed_on_line_0 = true;
                planner
                    .events
                    .push(Event::PlannedFence(pos..end, fence, prefix, first, closed));
                pos = end;
                continue;
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
            planner.events.push(Event::Fence(
                pos..end,
                fence,
                prefix,
                opened_item.then_some(remaining),
            ));
            pos = end;
            continue;
        } else if let Some(level) = try_parse_atx_heading(remaining) {
            if !planner.paragraph.is_empty() && options.extensions.blank_before_header {
                planner.push_text(remaining, prefixes);
            } else {
                let heading = if commonmark {
                    planner.interrupt_commonmark(line, remaining, prefixes)
                } else {
                    planner.flush();
                    planner.structural_prefixes(prefixes);
                    remaining
                };
                planner.events.push(Event::Atx(heading, level));
            }
        } else if commonmark
            && !planner.paragraph.is_empty()
            && try_parse_setext_heading(&["text\n", remaining], 0).is_some()
            && !planner.paragraph.get_text_for_parsing().trim().is_empty()
        {
            let underline = restore_trailing_indent(line, remaining, &mut prefixes);
            planner.events.push(Event::Setext(
                std::mem::take(&mut planner.paragraph),
                prefixes,
                underline,
            ));
        } else if try_parse_horizontal_rule(remaining).is_some() {
            let rule = if commonmark {
                planner.interrupt_commonmark(line, remaining, prefixes)
            } else {
                planner.flush();
                planner.structural_prefixes(prefixes);
                remaining
            };
            planner.events.push(Event::Rule(rule));
        } else if commonmark
            && options.extensions.reference_links
            && planner.paragraph.is_empty()
            && remaining.trim_start().starts_with('[')
            && let Some(reference) = plan_reference(
                remaining,
                &lines[pos + 1..],
                &planner.frames,
                options.dialect,
            )
        {
            let indent_start = prefixes
                .iter()
                .rposition(|prefix| matches!(prefix, Prefix::Quote(..)))
                .map_or(0, |i| i + 1);
            let indent = prefixes.split_off(indent_start);
            planner.structural_prefixes(prefixes);
            pos += reference.consumed_lines;
            planner.events.push(Event::Reference(indent, reference));
            continue;
        } else if commonmark
            && options.extensions.raw_html
            && let Some(kind) = try_parse_html_block_start(remaining, true)
            && (planner.paragraph.is_empty() || !matches!(kind, HtmlBlockType::Type7))
        {
            let ops = container_ops(&planner.frames);
            let prefix = ContainerPrefix::from_ops(&ops, false);
            let mut end = pos + 1;
            if !is_closing_marker(remaining, &kind) {
                for raw in &lines[end..] {
                    let Some(tail) = ContainerPrefix::strip_required_ops(raw, &ops) else {
                        break;
                    };
                    if ends_at_blank_line(&kind) && is_blank_line(tail) {
                        break;
                    }
                    end += 1;
                    if is_closing_marker(tail, &kind) {
                        break;
                    }
                }
            }
            let first = planner.interrupt_commonmark(line, remaining, prefixes);
            planner
                .events
                .push(Event::Html(pos..end, kind, prefix, first));
            pos = end;
            continue;
        } else if opener_as_text
            || is_quote_prose(remaining, options)
            || (commonmark
                && !planner.paragraph.is_empty()
                && remaining.trim_start().starts_with('[')
                && remaining.contains("]:"))
            || (commonmark && leading_indent(remaining).0 >= 4 && !planner.paragraph.is_empty())
        {
            // Within a quoted list the incumbent retains list indentation in
            // the text after the quote marker.
            if commonmark
                && planner.paragraph.is_empty()
                && prefixes
                    .iter()
                    .all(|prefix| matches!(prefix, Prefix::Indent(_)))
            {
                let bytes = prefixes
                    .iter()
                    .map(|prefix| match prefix {
                        Prefix::Indent(indent) => indent.len(),
                        Prefix::Quote(..) => 0,
                    })
                    .sum::<usize>();
                remaining = &line[line.len() - remaining.len() - bytes..];
                prefixes.clear();
            } else if matches!(planner.frames.last(), Some(Frame::Item { .. }))
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
    if !commonmark && !saw_quote {
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
                emit_buffered_paragraph(builder, kind, &buffer, options);
            }
            Event::Blank(line) => emit_blank(builder, line),
            Event::Whitespace(indent) => builder.token(K::WHITESPACE.into(), indent),
            Event::Fence(range, fence, prefix, first) => {
                let window =
                    StrippedLines::new(&lines[..start + range.end], start + range.start, &prefix);
                parse_fenced_code_block(
                    builder,
                    &window,
                    fence,
                    first,
                    diagnostics,
                    options.flavor,
                    None,
                );
            }
            Event::PlannedFence(range, fence, prefix, first, closed) => {
                let window =
                    StrippedLines::new(&lines[..start + range.end], start + range.start, &prefix);
                emit_planned_fenced_code_block(
                    builder,
                    &window,
                    fence,
                    first,
                    diagnostics,
                    options.flavor,
                    closed,
                );
            }
            Event::IndentedCode(range, quotes, column, first) => {
                parse_indented_code_block_with_first(
                    builder,
                    &lines[..start + range.end],
                    start + range.start,
                    quotes,
                    column,
                    first,
                );
            }
            Event::PlannedIndentedCode(code) => {
                builder.start_node(K::CODE_BLOCK.into());
                builder.start_node(K::CODE_CONTENT.into());
                for (prefix, body) in code {
                    if !prefix.is_empty() {
                        builder.token(K::LINE_PREFIX.into(), prefix);
                    }
                    emit_indented_code_line(builder, body);
                }
                builder.finish_node();
                builder.finish_node();
            }
            Event::Atx(line, level) => emit_atx_heading(builder, line, level, options),
            Event::Rule(line) => emit_horizontal_rule(builder, line),
            Event::Setext(buffer, prefixes, underline) => {
                builder.start_node(K::HEADING.into());
                buffer.emit_setext_heading_text(builder, options);
                for prefix in prefixes {
                    match prefix {
                        Prefix::Quote(indent, space) => {
                            emit_one_line_prefix_marker(builder, indent, space)
                        }
                        Prefix::Indent(indent) => builder.token(K::LINE_PREFIX.into(), indent),
                    }
                }
                emit_setext_underline(builder, underline);
                builder.finish_node();
            }
            Event::Reference(prefixes, reference) => {
                builder.start_node(K::REFERENCE_DEFINITION.into());
                for prefix in prefixes {
                    if let Prefix::Indent(indent) = prefix {
                        builder.token(K::WHITESPACE.into(), indent);
                    }
                }
                let mut sink = MarkerInjectingSink::new(builder, &reference.markers);
                emit_reference_definition_spans(&mut sink, &reference.source, reference.spans);
                sink.finish();
                builder.finish_node();
            }
            Event::Html(range, kind, prefix, first) => {
                let context = HtmlBlockContext {
                    prefix: &prefix,
                    wrapper_kind: K::HTML_BLOCK,
                    fusion: SoftbreakFusion::None,
                    config: options,
                };
                parse_html_block_with_wrapper(
                    builder,
                    &lines[..start + range.end],
                    start + range.start,
                    kind,
                    &context,
                    Some(first),
                    None,
                );
            }
        }
    }
}
