//! Plan mixed container transitions before writing any CST nodes.

mod bodies;

use bodies::{body_blank_depth, body_column, definition_marker, definition_term};
pub(super) use bodies::{footnote_marker, root_definition_term};

use std::{borrow::Cow, cell::RefCell, ops::Range};

use rowan::GreenNodeBuilder;

use super::super::block_dispatcher::{
    emit_div_fence_close_with_prefix, emit_div_fence_open, emit_reference_definition_spans,
};
use super::super::blocks::blockquotes::{emit_one_blockquote_marker, emit_one_line_prefix_marker};
use super::super::blocks::code_blocks::{
    FenceInfo, emit_planned_fenced_code_block, emit_planned_fenced_math_block, is_closing_fence,
    is_gfm_math_fence, parse_fenced_code_block, try_parse_fence_open,
};
use super::super::blocks::container_prefix::{
    ContainerPrefix, ContainerPrefixLine, ContainerPrefixState, StripOp, StrippedLines,
};
use super::super::blocks::definition_lists::{
    emit_definition_marker, emit_term, try_parse_definition_marker,
};
use super::super::blocks::fenced_divs::{
    DivFenceInfo, is_div_closing_fence, try_parse_div_fence_open,
};
use super::super::blocks::headings::{
    emit_atx_heading, emit_setext_underline, try_parse_atx_heading, try_parse_setext_heading,
};
use super::super::blocks::horizontal_rules::{emit_horizontal_rule, try_parse_horizontal_rule};
use super::super::blocks::indented_code::{
    emit_indented_code_line, parse_indented_code_block_with_first,
};
use super::super::blocks::jupyter::LookaheadCache;
use super::super::blocks::line_blocks::{PreparedLineBlock, try_parse_line_block_start};
use super::super::blocks::lists::{
    ListItemEmissionInput, ListMarker, ListMarkerMatch, OpenListHint, OrderedMarker,
    emit_list_item_with_checkbox, is_task_checkbox, list_item_content_col, marker_start_number,
    markers_match, try_parse_list_marker,
};
use super::super::blocks::raw_blocks::{self, emit_raw_tex_block};
use super::super::blocks::reference_links::{
    emit_footnote_marker, line_is_mmd_link_attribute_continuation,
};
use super::super::blocks::tables::{
    DashTableKind, PreparedDashTable, PreparedGridTable, PreparedPipeTable, is_table_caption_start,
    prepare_dash_table_at, prepare_grid_table, prepare_pipe_table, try_parse_pipe_separator,
};
use super::super::diagnostics::Diagnostics;
use super::super::inlines::sink::{InjectedMarker, MarkerInjectingSink};
use super::super::utils::container_stack::{
    FOOTNOTE_INDENT_COLUMNS, OpenDisplayMath, byte_index_at_column, gobbled_indent_prefix_len,
    leading_indent, leading_indent_from,
};
use super::super::utils::helpers::is_blank_line;
use super::super::utils::list_item_buffer::graft_node;
use super::super::utils::marker_utils::{count_blockquote_markers, try_parse_blockquote_marker};
use super::super::utils::text_buffer::ParagraphBuffer;
use super::{
    HtmlBlockContext, HtmlBlockType, ParagraphMath, ReferenceSpans, SoftbreakFusion, emit_blank,
    emit_buffered_paragraph, ends_at_blank_line, fence_enabled, html, is_closing_marker,
    is_quote_prose, parse_html_block_with_wrapper, reference_definition_spans,
    try_parse_html_block_start,
};
use crate::options::{Dialect, ParserOptions};
use crate::syntax::SyntaxKind as K;

type UnsupportedLine<'a> = (&'a str, &'static str);

pub(super) enum Event<'a> {
    Open(K),
    Close,
    QuoteMarker(usize, bool),
    QuoteLinePrefix(usize, bool),
    Item(&'a str, ListMarkerMatch, bool),
    Paragraph(K, ParagraphBuffer, bool),
    Term(&'a str),
    DefinitionMarker(&'a str, char, &'a str),
    FootnoteMarker(&'a str, String, &'a str),
    Blank(&'a str),
    Newline(&'a str),
    Whitespace(&'a str),
    LinePrefix(&'a str),
    Fence(
        Range<usize>,
        FenceInfo,
        ContainerPrefix,
        Option<&'a str>,
        bool,
    ),
    PlannedFence(
        Range<usize>,
        FenceInfo,
        ContainerPrefix,
        &'a str,
        bool,
        bool,
    ),
    BodyFence(
        Range<usize>,
        FenceInfo,
        ContainerPrefix,
        &'a str,
        &'a str,
        bool,
    ),
    IndentedCode(Range<usize>, usize, usize, Option<&'a str>),
    PlannedIndentedCode(Vec<(&'a str, &'a str)>),
    OpenDiv(&'a str, DivFenceInfo),
    CloseDiv(&'a str, &'a str),
    Atx(&'a str, usize),
    AtxWithPrefix(&'a str, usize, &'a str),
    Rule(&'a str),
    Setext(ParagraphBuffer, Vec<Prefix<'a>>, &'a str),
    GridTable(usize, ContainerPrefix, PreparedGridTable, &'a str, &'a str),
    DashTable(
        usize,
        ContainerPrefix,
        PreparedDashTable,
        Option<(&'a str, &'a str)>,
    ),
    PipeTable(usize, ContainerPrefix, PreparedPipeTable, bool),
    BodyPipeTable(
        Range<usize>,
        ContainerPrefix,
        PreparedPipeTable,
        &'a str,
        &'a str,
    ),
    Reference(Vec<Prefix<'a>>, PlannedReference<'a>),
    Html(Range<usize>, HtmlBlockType, ContainerPrefix, &'a str),
    PandocHtml(html::PreparedHtml),
    LineBlock(usize, ContainerPrefix, PreparedLineBlock, &'a str, &'a str),
    RawTex(
        Range<usize>,
        ContainerPrefix,
        Option<(&'a str, &'a str)>,
        bool,
    ),
}

enum Frame {
    DefinitionList,
    DefinitionItem,
    Definition { column: usize },
    Footnote,
    Quote,
    Div { indent: usize },
    List { marker: ListMarker, indent: usize },
    Item { column: usize, empty_opener: bool },
}

pub(super) enum Prefix<'a> {
    Quote(usize, bool),
    Indent(&'a str),
}

struct Planner<'a, 'o> {
    options: &'o ParserOptions,
    math: ParagraphMath,
    frames: Vec<Frame>,
    events: Vec<Event<'a>>,
    paragraph: ParagraphBuffer,
    paragraph_kind: K,
}

impl<'a> Planner<'a, '_> {
    fn flush(&mut self) {
        self.math.clear();
        if !self.paragraph.is_empty() {
            self.events.push(Event::Paragraph(
                self.paragraph_kind,
                std::mem::take(&mut self.paragraph),
                self.frames
                    .iter()
                    .any(|frame| matches!(frame, Frame::Footnote)),
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

    fn push_block_prefixes(&mut self, prefixes: Vec<Prefix<'a>>) {
        let in_list_item = matches!(self.frames.last(), Some(Frame::Item { .. }));
        for prefix in prefixes {
            let event = match prefix {
                Prefix::Quote(indent, space) if in_list_item => {
                    Event::QuoteLinePrefix(indent, space)
                }
                Prefix::Quote(indent, space) => Event::QuoteMarker(indent, space),
                Prefix::Indent(indent) => Event::Whitespace(indent),
            };
            self.events.push(event);
        }
    }

    fn push_dash_table(
        &mut self,
        window: StrippedLines<'a, '_>,
        table: PreparedDashTable,
        mut prefixes: Vec<Prefix<'a>>,
        task_body: bool,
    ) -> usize {
        if task_body && let Some(Event::Item(_, _, claim_task_checkbox)) = self.events.last_mut() {
            *claim_task_checkbox = false;
        }
        if window
            .dispatch_override()
            .is_some_and(|(_, indent)| !indent.is_empty())
        {
            while matches!(prefixes.last(), Some(Prefix::Indent(_))) {
                prefixes.pop();
            }
        }
        self.structural_prefixes(prefixes);
        let consumed = table.lines_consumed;
        self.events.push(Event::DashTable(
            window.pos(),
            window.prefix().clone(),
            table,
            window.dispatch_override(),
        ));
        consumed
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
            self.paragraph_kind = if matches!(
                self.frames.last(),
                Some(Frame::Item { .. } | Frame::Definition { .. })
            ) {
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
        if !remaining.is_empty() {
            let loose_definition = matches!(self.frames.last(), Some(Frame::Definition { .. }))
                && matches!(self.events.last(), Some(Event::Blank(_)));
            self.math.update(
                remaining,
                self.options,
                self.paragraph_kind == K::PARAGRAPH || loose_definition,
            );
            if loose_definition && !self.math.environments.is_empty() {
                self.paragraph_kind = K::PARAGRAPH;
            }
        }
    }
}

fn quote_prefix(line: &str) -> Option<(Prefix<'_>, &str)> {
    let (marker_end, tail) = try_parse_blockquote_marker(line)?;
    Some((
        Prefix::Quote(marker_end - 1, tail > marker_end),
        &line[tail..],
    ))
}

fn alpha_hint(marker: &ListMarker) -> OpenListHint {
    match marker {
        ListMarker::Ordered(OrderedMarker::LowerAlpha { .. }) => OpenListHint::LowerAlpha,
        ListMarker::Ordered(OrderedMarker::UpperAlpha { .. }) => OpenListHint::UpperAlpha,
        _ => OpenListHint::None,
    }
}

fn marker(line: &str, options: &ParserOptions) -> Option<ListMarkerMatch> {
    (leading_indent(line).0 < 4)
        .then(|| try_parse_list_marker(line, options, OpenListHint::None))
        .flatten()
}

fn pandoc_list_continues(
    line: &str,
    first: &ListMarker,
    base_indent: usize,
    options: &ParserOptions,
) -> bool {
    let indent = leading_indent(line).0;
    let hint = if indent == base_indent {
        alpha_hint(first)
    } else {
        OpenListHint::None
    };
    indent < 4
        && try_parse_list_marker(line, options, hint)
            .is_some_and(|next| markers_match(first, &next.marker, options.dialect))
}

fn list_interrupts_quote_paragraph(
    marker: &ListMarkerMatch,
    tail: &str,
    options: &ParserOptions,
) -> bool {
    if options.dialect == Dialect::CommonMark {
        marker_start_number(&marker.marker).is_none_or(|start| start == 1) && !is_blank_line(tail)
    } else {
        false
    }
}

fn owns_body_frame(frames: &[Frame]) -> bool {
    matches!(
        frames.last(),
        Some(Frame::Item { .. } | Frame::Definition { .. } | Frame::Footnote)
    )
}

fn can_start_body_block(frames: &[Frame], lines: &[&str], pos: usize) -> bool {
    !owns_body_frame(frames)
        || pos == 0
        || is_blank_line(lines[pos - 1])
        || (frames.iter().any(|frame| matches!(frame, Frame::Quote)) && {
            let (depth, tail) = count_blockquote_markers(lines[pos - 1]);
            depth > 0 && is_blank_line(tail)
        })
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
            Frame::List { .. } | Frame::Div { .. } => {}
            Frame::Item { column: indent, .. } => column += indent,
            _ => return None,
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
            Frame::Definition { column } => ops.push(StripOp::ContentIndent(*column as u32)),
            Frame::Footnote => ops.push(StripOp::ContentIndent(FOOTNOTE_INDENT_COLUMNS as u32)),
            _ => {}
        }
    }
    ops
}

fn block_prefix(
    ops: &[StripOp],
    frames: &[Frame],
    marker_consumed: bool,
    outer_div_indent: Option<usize>,
    options: &ParserOptions,
) -> ContainerPrefix {
    let div = frames
        .iter()
        .rposition(|frame| matches!(frame, Frame::Div { .. }));
    let div_indent = div
        .and_then(|i| match frames[i] {
            Frame::Div { indent } => Some(indent),
            _ => None,
        })
        .or(outer_div_indent);
    let inside_div = &frames[div.map_or(0, |index| index + 1)..];
    let collects = inside_div
        .iter()
        .any(|frame| matches!(frame, Frame::Item { .. } | Frame::Definition { .. }));
    let appends_blank = inside_div
        .iter()
        .any(|frame| matches!(frame, Frame::Quote | Frame::Footnote));
    let section = frames
        .iter()
        .rposition(|frame| matches!(frame, Frame::Quote) || body_column(frame).is_some());
    let columns = frames[section.map_or(0, |index| index + 1)..]
        .iter()
        .filter_map(|frame| match frame {
            Frame::Item { column, .. } => Some(*column as u32),
            _ => None,
        });
    ContainerPrefix::from_ops(ops, marker_consumed)
        .with_div_closer_boundary(div_indent, !collects || appends_blank)
        .with_list_start_boundary(options, columns)
}

fn trailing_prefix_indent<'a>(line: &'a str, tail: &str, prefixes: &[Prefix<'_>]) -> &'a str {
    let bytes = prefixes
        .iter()
        .rev()
        .map_while(|part| match part {
            Prefix::Indent(indent) => Some(indent.len()),
            _ => None,
        })
        .sum::<usize>();
    let start = line.len() - tail.len();
    &line[start - bytes..start]
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
    options: &ParserOptions,
) -> Option<PlannedReference<'a>> {
    let ops = container_ops(frames);
    let quoted = ops.contains(&StripOp::BlockQuoteMarker);
    let strict_eol = !options.extensions.mmd_link_attributes;
    let spans_for = |text| reference_definition_spans(text, strict_eol, options.dialect, false);
    let continuation = |raw: &'a str| -> Option<&'a str> {
        let tail = ContainerPrefix::strip_required_ops(raw, &ops)?;
        (!is_blank_line(tail)).then_some(if quoted { tail } else { raw })
    };

    // A complete first line can extend only with a title on the next line.
    // The Pandoc dispatcher only reads the first line of a quoted definition.
    let single_line = quoted && options.dialect == Dialect::Pandoc;
    if let Some(spans) = spans_for(first)
        && (single_line
            || (spans.title.is_some()
                || following
                    .first()
                    .and_then(|raw| continuation(raw))
                    .is_none_or(|tail| {
                        !tail
                            .trim_start_matches([' ', '\t'])
                            .starts_with(['"', '\'', '('])
                    }))
                && (strict_eol
                    || following.first().is_none_or(|raw| {
                        continuation(raw).is_none() || !line_is_mmd_link_attribute_continuation(raw)
                    })))
    {
        return Some(PlannedReference {
            source: Cow::Borrowed(first),
            spans,
            markers: Vec::new(),
            consumed_lines: 1,
        });
    }
    if single_line {
        return None;
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
    let spans = spans_for(&source)?;
    let mut consumed_lines = starts.partition_point(|&start| start < spans.consumed);
    if !strict_eol {
        consumed_lines += following[consumed_lines - 1..starts.len() - 1]
            .iter()
            .take_while(|raw| line_is_mmd_link_attribute_continuation(raw))
            .count();
    }
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
    lines: &'a [&'a str],
    options: &ParserOptions,
    jupyter_lines: &RefCell<LookaheadCache>,
    outer_div_indent: Option<usize>,
) -> Result<(usize, Vec<Event<'a>>), UnsupportedLine<'a>> {
    if lines.is_empty() {
        return Ok((0, Vec::new()));
    }
    let mut planner = Planner {
        options,
        math: ParagraphMath::default(),
        frames: Vec::new(),
        events: Vec::new(),
        paragraph: ParagraphBuffer::new(),
        paragraph_kind: K::PARAGRAPH,
    };
    let commonmark = options.dialect == Dialect::CommonMark;
    let mut pos = 0;
    let mut next_nonblank = 0;
    let mut blank_lookahead = (0, 0);
    let mut div_closed_by_list = false;
    let mut body_quote_run = false;
    let mut after_item_html = false;
    'lines: while let Some(&line) = lines.get(pos) {
        if matches!(planner.math.display, Some(OpenDisplayMath::Jupyter))
            && !matches!(planner.frames.last(), Some(Frame::Definition { .. }))
        {
            let ops = container_ops(&planner.frames);
            let mut prefix = ContainerPrefix::from_ops(&ops, false);
            prefix.lazy_blockquote_gobble = !commonmark && prefix.bq_depth() > 0;
            let (held, content) = prefix.split_pieces(line);
            if planner.math.jupyter_continues(pos, content, || {
                planner
                    .paragraph
                    .jupyter_math_closing_line(pos, &prefix, &lines[pos..])
            }) {
                for text in held.token_texts() {
                    planner.paragraph.push_indent(text);
                }
                planner.push_text(content, Vec::new());
                pos += 1;
                continue;
            }
        }

        let in_body_quote_run = std::mem::take(&mut body_quote_run);
        let continues_item_html = std::mem::take(&mut after_item_html);
        let after_html = !commonmark && matches!(planner.events.last(), Some(Event::PandocHtml(_)));
        let after_line_or_tex_block = !commonmark
            && matches!(
                planner.events.last(),
                Some(Event::LineBlock(..) | Event::RawTex(..))
            )
            || !commonmark
                && options.extensions.tex_math_gfm
                && match planner.events.last() {
                    Some(
                        Event::Fence(_, fence, _, None, _)
                        | Event::PlannedFence(_, fence, _, _, _, true)
                        | Event::BodyFence(_, fence, _, _, _, true),
                    ) => is_gfm_math_fence(fence),
                    _ => false,
                };
        let mut lazy_html_opener = false;
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
                        if !commonmark
                            && let Some(Frame::List {
                                marker: first,
                                indent: base,
                            }) = planner.frames.get(matched + 1)
                            && indent.abs_diff(*base) <= 3
                        {
                            let hint = if indent == *base {
                                alpha_hint(first)
                            } else {
                                OpenListHint::None
                            };
                            sibling = try_parse_list_marker(remaining, options, hint)
                                .filter(|next| markers_match(first, &next.marker, options.dialect));
                            if sibling.is_some() {
                                matched += 2;
                            }
                        }
                        break;
                    };
                    if commonmark {
                        absorb_quote_indent(&mut prefix, &mut prefixes);
                    } else if matches!(
                        prefixes.as_slice(),
                        [.., Prefix::Indent(_), Prefix::Indent(_)]
                    ) && planner.frames[..matched]
                        .iter()
                        .any(|frame| body_column(frame).is_some())
                    {
                        let first = restore_trailing_indent(line, remaining, &mut prefixes);
                        prefixes.push(Prefix::Indent(&first[..first.len() - remaining.len()]));
                    }
                    prefixes.push(prefix);
                    remaining = tail;
                }
                Frame::DefinitionList | Frame::DefinitionItem | Frame::Div { .. } => {}
                Frame::Definition { .. } | Frame::Footnote => {
                    let column = body_column(frame).unwrap();
                    if indent < column || is_blank_line(remaining) {
                        break;
                    }
                    let bytes = gobbled_indent_prefix_len(remaining, column);
                    let (indent, tail) = remaining.split_at(bytes);
                    prefixes.push(Prefix::Indent(indent));
                    remaining = tail;
                }
                Frame::List {
                    marker: first,
                    indent: list_indent,
                } => {
                    let hint = if indent == *list_indent {
                        alpha_hint(first)
                    } else {
                        OpenListHint::None
                    };
                    let item_column = match planner.frames.get(matched + 1) {
                        Some(Frame::Item { column, .. }) => *column,
                        _ => 4,
                    };
                    sibling = (indent < item_column && indent < 4)
                        .then(|| try_parse_list_marker(remaining, options, hint))
                        .flatten()
                        .filter(|next| markers_match(first, &next.marker, options.dialect));
                    if sibling.is_some() {
                        matched += 1;
                        break;
                    }
                }
                Frame::Item { column, .. } => {
                    if indent < *column || (!commonmark && is_blank_line(remaining)) {
                        break;
                    }
                    let bytes = gobbled_indent_prefix_len(remaining, *column);
                    let (indent, tail) = remaining.split_at(bytes);
                    list_columns += column;
                    prefixes.push(Prefix::Indent(indent));
                    remaining = tail;
                }
            }
            matched += 1;
        }

        if sibling.is_none()
            && planner.math.holds(remaining, options)
            && !(options.extensions.fenced_divs
                && is_div_closing_fence(remaining)
                && (outer_div_indent.is_some_and(|indent| leading_indent(line).0 <= indent
                    && remaining.len() == line.len())
                    || planner.frames[..matched].iter().any(|frame| {
                        matches!(frame, Frame::Div { indent } if leading_indent(remaining).0 <= *indent)
                    })))
            && !(try_parse_setext_heading(&["text\n", remaining], 0).is_some()
                && (commonmark || (planner.math.lines == 1 && planner.paragraph_kind == K::PARAGRAPH)))
            && (matched == planner.frames.len()
                || !commonmark
                    && planner.frames[matched..]
                        .iter()
                        .all(|frame| matches!(frame, Frame::Quote | Frame::Item { .. })))
            && !(matched < planner.frames.len()
                && !options.extensions.blank_before_header
                && try_parse_atx_heading(remaining).is_some())
            && !(commonmark && marker(remaining, options).is_some_and(|next| {
                let (_, indent) = leading_indent(remaining);
                list_interrupts_quote_paragraph(&next, &remaining[indent + next.marker_len..], options)
            }))
            && !(matched < planner.frames.len()
                && options.extensions.raw_html
                && html::detect(remaining.trim_start(), false).is_some())
            && (quote_prefix(remaining).is_none() || options.extensions.blank_before_blockquote)
        {
            if planner.paragraph_kind == K::PLAIN
                && planner
                    .frames
                    .last()
                    .is_some_and(|frame| matches!(frame, Frame::Item { .. }))
                && prefixes
                    .iter()
                    .any(|part| matches!(part, Prefix::Quote(..)))
            {
                remaining = restore_trailing_indent(line, remaining, &mut prefixes);
            }
            planner.push_text(remaining, prefixes);
            pos += 1;
            continue;
        }

        let in_body = planner
            .frames
            .iter()
            .any(|frame| body_column(frame).is_some() || matches!(frame, Frame::DefinitionList));
        if (in_body || in_body_quote_run) && is_blank_line(remaining) {
            let quote_depth = prefixes
                .iter()
                .filter(|part| matches!(part, Prefix::Quote(..)))
                .count();
            if blank_lookahead.0 <= pos || blank_lookahead.1 != quote_depth {
                let mut next = pos + 1;
                while lines.get(next).is_some_and(|line| {
                    is_blank_line(line) || quote_tail(line, quote_depth).is_some_and(is_blank_line)
                }) {
                    next += 1;
                }
                blank_lookahead = (next, quote_depth);
            }
            let depth =
                if is_blank_line(line) && matches!(planner.frames.first(), Some(Frame::Quote)) {
                    0
                } else {
                    body_blank_depth(&planner.frames, lines, blank_lookahead.0, options)
                };
            // A marked blank belongs to the quote even when the next
            // nonblank line will continue it without a marker.
            let explicit_quote_depth = planner.frames[..matched]
                .iter()
                .rposition(|frame| matches!(frame, Frame::Quote))
                .map_or(0, |index| index + 1);
            let depth = depth.max(explicit_quote_depth);
            body_quote_run = explicit_quote_depth > 0 && depth == explicit_quote_depth;
            planner.close_to(depth);
            if depth == 0 && prefixes.is_empty() {
                break;
            }
            planner.structural_prefixes(prefixes);
            planner.events.push(Event::Blank(remaining));
            pos += 1;
            continue;
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
                    Frame::Div { .. } => {}
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
                    _ => unreachable!("Pandoc body in CommonMark planner"),
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

        if !commonmark
            && options.extensions.fenced_divs
            && remaining.trim_start_matches(' ').starts_with(':')
            && is_div_closing_fence(remaining)
            && let Some(depth) = planner.frames[..matched].iter().rposition(|frame| {
                matches!(frame, Frame::Div { indent } if leading_indent(remaining).0 <= *indent)
            })
        {
            let unstripped = restore_trailing_indent(line, remaining, &mut prefixes);
            let indent = &unstripped[..unstripped.len() - remaining.len()];
            if planner.paragraph.is_empty() {
                planner.structural_prefixes(prefixes);
            } else {
                planner.push_text("", prefixes);
            }
            planner.close_to(depth + 1);
            planner.frames.pop();
            if planner.frames.last().is_some_and(|frame| body_column(frame).is_some()) {
                planner.events.push(Event::CloseDiv(unstripped, ""));
            } else {
                planner.events.push(Event::CloseDiv(remaining, indent));
            }
            pos += 1;
            continue;
        }

        if in_body
            && remaining.len() == line.len()
            && outer_div_indent.is_some_and(|indent| leading_indent(line).0 <= indent)
            && is_div_closing_fence(line)
        {
            break;
        }

        if matches!(planner.frames.get(matched), Some(Frame::Definition { .. }))
            && planner.frames[matched + 1..]
                .iter()
                .any(|frame| matches!(frame, Frame::Quote))
            && !planner.paragraph.is_empty()
            && try_parse_definition_marker(remaining).is_some()
        {
            // A definition marker does not interrupt lazy prose in a quote
            // that started inside the current definition.
            planner.push_text(remaining, prefixes);
            pos += 1;
            continue;
        }

        if matches!(planner.frames.get(matched), Some(Frame::Quote))
            && options.extensions.definition_lists
            && try_parse_definition_marker(remaining).is_some()
            && let Some(item) = planner
                .frames
                .iter()
                .enumerate()
                .skip(matched)
                .take_while(|(_, frame)| !matches!(frame, Frame::Item { .. } | Frame::Footnote))
                .find_map(|(depth, frame)| matches!(frame, Frame::DefinitionItem).then_some(depth))
        {
            matched = item + 1;
        }

        if !planner.paragraph.is_empty()
            && !prefixes.is_empty()
            && prefixes
                .iter()
                .all(|prefix| matches!(prefix, Prefix::Quote(..)))
        {
            match planner.frames.get(matched) {
                Some(Frame::Footnote) if footnote_marker(remaining, options).is_some() => {
                    planner.push_text("", std::mem::take(&mut prefixes));
                }
                Some(Frame::Definition { .. })
                    if definition_marker(
                        remaining,
                        lines,
                        pos,
                        &planner.frames[..matched],
                        options,
                    )
                    .is_some() =>
                {
                    planner.flush();
                    for prefix in prefixes.drain(..) {
                        if let Prefix::Quote(indent, space) = prefix {
                            planner.events.push(Event::QuoteLinePrefix(indent, space));
                        }
                    }
                }
                _ => {}
            }
        }

        if sibling.is_none() && matched < planner.frames.len() {
            if is_blank_line(remaining) {
                let quote_depth = prefixes
                    .iter()
                    .filter(|prefix| matches!(prefix, Prefix::Quote(..)))
                    .count();
                if blank_lookahead.0 <= pos || blank_lookahead.1 != quote_depth {
                    let mut next = pos + 1;
                    while lines.get(next).is_some_and(|line| {
                        is_blank_line(line)
                            || (!commonmark
                                && quote_tail(line, quote_depth).is_some_and(is_blank_line))
                    }) {
                        next += 1;
                    }
                    blank_lookahead = (next, quote_depth);
                }
                let next = lines.get(blank_lookahead.0);
                let mut tail = next.copied();
                for frame in &planner.frames[..matched] {
                    tail = match frame {
                        Frame::Quote => {
                            tail.and_then(|line| quote_prefix(line).map(|(_, tail)| tail))
                        }
                        Frame::Item { column, .. } => {
                            tail.and_then(|line| list_tail(line, *column))
                        }
                        _ => tail,
                    };
                }
                if let Some(Frame::Item { column, .. }) = planner.frames.get(matched)
                    && let Some(tail) = tail.and_then(|line| {
                        (!commonmark || leading_indent(line).0 >= *column)
                            .then(|| list_tail(line, *column))
                            .flatten()
                    })
                {
                    let mut depth = matched + 1;
                    let mut tail = tail;
                    while let Some(frame) = planner.frames.get(depth) {
                        match frame {
                            Frame::List { marker, indent } => {
                                if !pandoc_list_continues(tail, marker, *indent, options)
                                    && !matches!(planner.frames.get(depth + 1),
                                        Some(Frame::Item { column, .. }) if list_tail(tail, *column).is_some())
                                {
                                    break;
                                }
                            }
                            Frame::Item { column, .. } => {
                                let Some(content) = list_tail(tail, *column) else {
                                    break;
                                };
                                tail = content;
                            }
                            Frame::Quote => break,
                            _ => {}
                        }
                        depth += 1;
                    }
                    planner.close_to(depth);
                    planner.structural_prefixes(prefixes);
                    planner.events.push(Event::Blank(remaining));
                    pos += 1;
                    continue;
                }
                if let Some(Frame::List {
                    marker: first,
                    indent,
                }) = planner.frames.get(matched.wrapping_sub(1))
                    && tail.is_some_and(|line| pandoc_list_continues(line, first, *indent, options))
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
                        && pandoc_list_continues(next, first, *indent, options)
                    {
                        planner.close_to(1);
                        planner.events.push(Event::Blank(line));
                        pos += 1;
                        continue;
                    }
                    break;
                } else if !commonmark
                    && matches!(
                        planner.frames.get(matched.wrapping_sub(1)),
                        Some(Frame::List { .. })
                    )
                {
                    planner.close_to(matched - 1);
                    planner.structural_prefixes(prefixes);
                    planner.events.push(Event::Blank(remaining));
                    pos += 1;
                    continue;
                }
            }
            if !commonmark
                && matches!(planner.frames.get(matched), Some(Frame::Quote))
                && ((matches!(planner.frames.get(matched + 1), Some(Frame::List { .. }))
                    && planner.paragraph_kind == K::PLAIN
                    && !planner.paragraph.is_empty())
                    || (matched == 0
                        && leading_indent(remaining).0 > 0
                        && planner.frames.len() == 1
                        && planner.paragraph.is_empty()))
            {
                let first = remaining.trim_start_matches([' ', '\t']);
                let (_, bytes) = leading_indent(remaining);
                if let Some(fence) = try_parse_fence_open(first, options.dialect)
                    .filter(|fence| fence_enabled(fence, options))
                {
                    let prefix =
                        ContainerPrefix::from_scalars(1, 0, true, 0, false, options.dialect);
                    let end = lines[pos + 1..]
                        .iter()
                        .take_while(|raw| !is_blank_line(raw))
                        .position(|raw| is_closing_fence(prefix.strip(raw), &fence));
                    if let Some(close) = end {
                        planner.close_to(matched + 1);
                        planner.structural_prefixes(prefixes);
                        if bytes > 0 {
                            planner.events.push(Event::Whitespace(&remaining[..bytes]));
                        }
                        let end = pos + close + 2;
                        let math_fence =
                            options.extensions.tex_math_gfm && is_gfm_math_fence(&fence);
                        planner.events.push(Event::PlannedFence(
                            pos..end,
                            fence,
                            prefix,
                            first,
                            true,
                            math_fence,
                        ));
                        pos = end;
                        continue;
                    }
                }
            }
            let unmatched = planner.frames.get(matched);
            let active_body = matches!(unmatched, Some(Frame::Definition { .. } | Frame::Footnote));
            let note_quote = matches!(unmatched, Some(Frame::Quote))
                && planner.frames[..matched]
                    .iter()
                    .any(|frame| matches!(frame, Frame::Footnote));
            let after_body_block = active_body
                && pos > 0
                && !is_blank_line(lines[pos - 1])
                && planner.paragraph.is_empty();
            let lazy_quote_line = in_body_quote_run
                && matched + 1 == planner.frames.len()
                && matches!(unmatched, Some(Frame::Quote));
            let lazy_html = !commonmark
                && options.extensions.raw_html
                && (!planner.paragraph.is_empty() || after_line_or_tex_block)
                && matches!(unmatched, Some(Frame::Quote | Frame::Item { .. }))
                && remaining.trim_start().starts_with('<');
            let lazy_html_interrupts = lazy_html
                && html::detect(remaining.trim_start(), false).is_some_and(|kind| {
                    let ops = container_ops(&planner.frames);
                    let mut prefix = ContainerPrefix::from_ops(&ops, false);
                    prefix.lazy_blockquote_gobble = true;
                    html::valid_start(lines, pos, &prefix.without_innermost_list_advance(), &kind)
                });
            let continues_after_html = after_html
                && sibling.is_none()
                && matches!(unmatched, Some(Frame::Quote | Frame::Item { .. }));
            let continues_after_special_block = after_line_or_tex_block
                && matches!(unmatched, Some(Frame::Quote | Frame::Item { .. }));
            if ((continues_after_html || continues_after_special_block)
                && try_parse_atx_heading(remaining).is_some())
                || (continues_after_special_block
                    && (try_parse_horizontal_rule(remaining).is_some()
                        || (options.extensions.line_blocks
                            && try_parse_line_block_start(remaining).is_some())))
            {
                // Pandoc collects the unmarked heading in the same item or
                // quote and dispatches it as a block in that frame.
                matched = planner.frames.len();
            } else if lazy_html_interrupts {
                lazy_html_opener = true;
                let content = remaining.trim_start_matches([' ', '\t']);
                let indent = &remaining[..remaining.len() - content.len()];
                remaining = content;
                if !indent.is_empty() {
                    prefixes.push(Prefix::Indent(indent));
                }
                matched = planner.frames.len();
            } else if lazy_quote_line {
                // The marked blank can be followed by an unmarked line run.
                // Dispatch those lines inside the quote, including new blocks.
                matched += 1;
                body_quote_run = true;
            } else if (!planner.paragraph.is_empty()
                || after_body_block
                || continues_after_html
                || continues_after_special_block)
                && !is_blank_line(remaining)
                && !(in_body && options.extensions.fenced_divs && is_div_closing_fence(remaining))
                && !(in_body && !note_quote && footnote_marker(remaining, options).is_some())
                && !(in_body
                    && try_parse_definition_marker(remaining).is_some()
                    && matches!(planner.frames.get(matched), Some(Frame::Definition { .. })))
                && (is_quote_prose(remaining, options)
                    || lazy_html
                    || (note_quote && footnote_marker(remaining, options).is_some())
                    || (matches!(unmatched, Some(Frame::Footnote))
                        && options.extensions.blank_before_header
                        && try_parse_atx_heading(remaining).is_some())
                    || (div_closed_by_list && is_div_closing_fence(remaining))
                    || (!commonmark
                        && planner.paragraph_kind == K::PLAIN
                        && (try_parse_horizontal_rule(remaining).is_some()
                            || (options.extensions.blank_before_header
                                && try_parse_atx_heading(remaining).is_some())
                            || (options.extensions.blank_before_blockquote
                                && quote_prefix(remaining).is_some())))
                    || (leading_indent(remaining).0 >= 4)
                    || (matches!(planner.frames.get(matched), Some(Frame::Quote))
                        && marker(remaining, options).is_some_and(|next| {
                            let (_, indent) = leading_indent(remaining);
                            let tail = &remaining[indent + next.marker_len..];
                            !list_interrupts_quote_paragraph(&next, tail, options)
                                && (commonmark
                                    || !options.extensions.lists_without_preceding_blankline)
                        })))
            {
                // Missing quote markers do not erase the indent of an
                // enclosing list item on the same lazy continuation.
                let matched_prefixes = prefixes.len();
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
                if div_closed_by_list && is_div_closing_fence(remaining) {
                    div_closed_by_list = false;
                }
                if !commonmark
                    && (planner.paragraph_kind == K::PARAGRAPH
                        || (matches!(unmatched, Some(Frame::Definition { .. }))
                            && prefixes.len() == matched_prefixes))
                {
                    remaining = restore_trailing_indent(line, remaining, &mut prefixes);
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
            if !commonmark
                && sibling.is_some()
                && matches!(planner.frames.get(matched), Some(Frame::Item { .. }))
                && planner.frames[matched + 1..]
                    .iter()
                    .any(|frame| matches!(frame, Frame::Quote))
            {
                planner.close_to(matched + 1);
                for prefix in prefixes.drain(..) {
                    match prefix {
                        Prefix::Quote(indent, space) => {
                            planner.events.push(Event::QuoteLinePrefix(indent, space))
                        }
                        Prefix::Indent(indent) => planner.events.push(Event::Whitespace(indent)),
                    }
                }
            }
            planner.close_to(matched);
            if planner.frames.is_empty() {
                break;
            }
        }

        if matches!(planner.frames.last(), Some(Frame::Footnote))
            && footnote_marker(remaining, options).is_some()
        {
            planner.close_to(planner.frames.len() - 1);
            planner.structural_prefixes(std::mem::take(&mut prefixes));
        }
        if !commonmark
            && options.extensions.definition_lists
            && !planner.paragraph.is_empty()
            && matches!(
                planner.frames.last(),
                Some(Frame::Definition { .. } | Frame::Item { .. })
            )
            && definition_marker(remaining, lines, pos, &planner.frames, options).is_some()
        {
            planner.flush();
        }

        let mut opened_item = false;
        let mut opened_body = false;
        let mut note_opener = false;
        let mut task_body = false;
        let mut quote_on_item_opener = false;
        let mut opener_as_text = false;
        loop {
            if let Some((mut prefix, tail)) = quote_prefix(remaining) {
                if !commonmark
                    && matches!(planner.frames.last(), Some(Frame::Quote))
                    && options.extensions.pipe_tables
                    && planner.paragraph.is_empty()
                    && remaining.contains('|')
                    && lines
                        .get(pos + 1)
                        .and_then(|raw| {
                            ContainerPrefix::strip_required_ops(
                                raw,
                                &container_ops(&planner.frames),
                            )
                        })
                        .is_some_and(|tail| try_parse_pipe_separator(tail).is_some())
                {
                    break;
                }
                quote_on_item_opener |= opened_item;
                if options.extensions.blank_before_blockquote && !planner.paragraph.is_empty() {
                    opener_as_text = true;
                    break;
                }
                planner.flush();
                if commonmark {
                    absorb_quote_indent(&mut prefix, &mut prefixes);
                }
                if !commonmark && matches!(planner.frames.last(), Some(Frame::Item { .. })) {
                    let indent_start = prefixes
                        .iter()
                        .rposition(|prefix| matches!(prefix, Prefix::Quote(..)))
                        .map_or(0, |index| index + 1);
                    let indent = prefixes.split_off(indent_start);
                    planner.structural_prefixes(std::mem::replace(&mut prefixes, indent));
                }
                planner.events.push(Event::Open(K::BLOCK_QUOTE));
                planner.structural_prefixes(std::mem::take(&mut prefixes));
                planner.structural_prefixes(vec![prefix]);
                planner.frames.push(Frame::Quote);
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
                    && !commonmark
                    && matches!(planner.frames.last(), Some(Frame::Footnote))
                {
                    opener_as_text = true;
                    break;
                }
                if !planner.paragraph.is_empty()
                    && (commonmark || matches!(planner.frames.last(), Some(Frame::Quote)))
                    && !list_interrupts_quote_paragraph(&next, tail, options)
                {
                    opener_as_text = true;
                    break;
                }
                if !commonmark
                    && matches!(planner.frames.last(), Some(Frame::Div { .. }))
                    && planner
                        .frames
                        .iter()
                        .any(|frame| matches!(frame, Frame::Item { .. }))
                {
                    planner.close_to(planner.frames.len() - 1);
                    div_closed_by_list = true;
                }
                task_body = is_task_checkbox(tail);
                let nested_start_restricted = !commonmark
                    && !quote_on_item_opener
                    && !matches!(planner.frames.last(), Some(Frame::List { .. }))
                    && planner
                        .frames
                        .iter()
                        .any(|frame| matches!(frame, Frame::Item { .. }))
                    && options.pandoc_compat.restricts_ordered_sublist_start()
                    && marker_start_number(&next.marker).is_some_and(|start| start != 1);
                if nested_start_restricted {
                    opener_as_text = true;
                    planner.flush();
                    if prefixes
                        .iter()
                        .all(|prefix| matches!(prefix, Prefix::Indent(_)))
                    {
                        remaining = restore_trailing_indent(line, remaining, &mut prefixes);
                    }
                    break;
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
                let continuing_list = matches!(planner.frames.last(), Some(Frame::List { .. }));
                let body_owns_indent = !commonmark
                    && planner
                        .frames
                        .iter()
                        .any(|frame| body_column(frame).is_some());
                let item_owns_indent = (continuing_list && !body_owns_indent)
                    || matches!(planner.frames.last(), Some(Frame::Item { .. }));
                if item_owns_indent
                    && matches!(prefixes.last(), Some(Prefix::Indent(_)))
                    && let Some(Prefix::Indent(indent)) = prefixes.pop()
                {
                    let mut bytes = indent.len();
                    while let Some(Prefix::Indent(indent)) = prefixes.last() {
                        bytes += indent.len();
                        prefixes.pop();
                    }
                    item_line = &line[line.len() - remaining.len() - bytes..];
                    for prefix in prefixes.drain(..) {
                        match prefix {
                            Prefix::Quote(indent, space) => {
                                let event = if continuing_list && !commonmark {
                                    Event::QuoteMarker(indent, space)
                                } else {
                                    Event::QuoteLinePrefix(indent, space)
                                };
                                planner.events.push(event);
                            }
                            Prefix::Indent(indent) => {
                                planner.events.push(Event::Whitespace(indent))
                            }
                        }
                    }
                }
                if body_owns_indent {
                    let start = restore_trailing_indent(line, remaining, &mut prefixes);
                    let indent = &start[..start.len() - remaining.len()];
                    if !indent.is_empty() {
                        prefixes.push(Prefix::Indent(indent));
                    }
                }
                let open_list = !matches!(planner.frames.last(), Some(Frame::List { .. }));
                let prefix_in_list = planner.frames.last().and_then(body_column).is_some();
                if prefix_in_list {
                    planner.events.push(Event::Open(K::LIST));
                }
                planner.structural_prefixes(std::mem::take(&mut prefixes));
                if open_list {
                    if !prefix_in_list {
                        planner.events.push(Event::Open(K::LIST));
                    }
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
                planner.events.push(Event::Item(item_line, next, true));
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
            if !commonmark && !is_blank_line(remaining) {
                if matches!(planner.frames.last(), Some(Frame::DefinitionItem))
                    && matches!(prefixes.last(), Some(Prefix::Indent(_)))
                    && let Some(tail) = remaining.strip_prefix('\t')
                    && try_parse_definition_marker(tail).is_some()
                {
                    prefixes.push(Prefix::Indent(&remaining[..1]));
                    remaining = tail;
                }
                if matches!(planner.frames.last(), Some(Frame::DefinitionItem))
                    && let Some((marker, indent, spaces, bytes)) =
                        definition_marker(remaining, lines, pos, &planner.frames, options)
                {
                    let start = byte_index_at_column(remaining, indent);
                    let end = start + 1 + bytes;
                    let marker_line = restore_trailing_indent(line, remaining, &mut prefixes);
                    planner.structural_prefixes(std::mem::take(&mut prefixes));
                    planner.events.push(Event::Open(K::DEFINITION));
                    let parent_indent = &marker_line[..marker_line.len() - remaining.len()];
                    if !parent_indent.is_empty() {
                        planner.events.push(Event::Whitespace(parent_indent));
                    }
                    planner.events.push(Event::DefinitionMarker(
                        &remaining[..start],
                        marker,
                        &remaining[start + 1..end],
                    ));
                    planner.frames.push(Frame::Definition {
                        column: indent + 1 + spaces,
                    });
                    remaining = &remaining[end..];
                    opened_body = true;
                    continue;
                }
                if planner.paragraph.is_empty()
                    && indent < 4
                    && try_parse_atx_heading(remaining).is_none()
                    && try_parse_horizontal_rule(remaining).is_none()
                    && !try_parse_fence_open(remaining, options.dialect)
                        .is_some_and(|fence| fence_enabled(&fence, options))
                    && !(options.extensions.raw_html
                        && html::detect(remaining, true).is_some_and(|kind| {
                            let ops = container_ops(&planner.frames);
                            let prefix = block_prefix(
                                &ops,
                                &planner.frames,
                                opened_item,
                                outer_div_indent,
                                options,
                            );
                            html::valid_start(lines, pos, &prefix, &kind)
                        }))
                    && (opened_body
                        || !options.extensions.line_blocks
                        || try_parse_line_block_start(remaining).is_none())
                    && !raw_blocks::can_start_raw_block(remaining, options)
                    && let Some(blanks) = definition_term(lines, pos, &planner.frames, options)
                {
                    let term = restore_trailing_indent(line, remaining, &mut prefixes);
                    planner.structural_prefixes(std::mem::take(&mut prefixes));
                    if !matches!(planner.frames.last(), Some(Frame::DefinitionList)) {
                        planner.events.push(Event::Open(K::DEFINITION_LIST));
                        planner.frames.push(Frame::DefinitionList);
                    }
                    planner.events.push(Event::Open(K::DEFINITION_ITEM));
                    planner.frames.push(Frame::DefinitionItem);
                    planner.events.push(Event::Term(term));
                    for &raw in &lines[pos + 1..pos + 1 + blanks] {
                        let mut tail = raw;
                        while let Some((prefix, rest)) = quote_prefix(tail) {
                            planner.structural_prefixes(vec![prefix]);
                            tail = rest;
                        }
                        planner.events.push(Event::Blank(tail));
                    }
                    pos += 1 + blanks;
                    continue 'lines;
                }
                if !opened_body
                    && !(opened_item && matches!(planner.frames.last(), Some(Frame::Item { .. })))
                    && !planner
                        .frames
                        .iter()
                        .any(|frame| matches!(frame, Frame::Footnote))
                    && let Some((indent, id, end)) = footnote_marker(remaining, options)
                {
                    if !planner.paragraph.is_empty()
                        && planner.paragraph_kind == K::PARAGRAPH
                        && matches!(planner.frames.last(), Some(Frame::Quote))
                    {
                        planner.push_text("", std::mem::take(&mut prefixes));
                    }
                    planner.flush();
                    let marker_line =
                        if matches!(planner.frames.last(), Some(Frame::Definition { .. })) {
                            remaining
                        } else {
                            restore_trailing_indent(line, remaining, &mut prefixes)
                        };
                    planner.structural_prefixes(std::mem::take(&mut prefixes));
                    let padding = indent + id.len() + 4;
                    planner.events.push(Event::Open(K::FOOTNOTE_DEFINITION));
                    planner.events.push(Event::FootnoteMarker(
                        &marker_line[..marker_line.len() - remaining.len() + indent],
                        id,
                        &remaining[padding..end],
                    ));
                    planner.frames.push(Frame::Footnote);
                    remaining = &remaining[end..];
                    opened_body = true;
                    note_opener = true;
                    continue;
                }
            }
            break;
        }

        if planner.frames.is_empty() {
            return Err((remaining, "container opener"));
        }
        let quote_inside_list = commonmark
            && matches!(planner.frames.last(), Some(Frame::Quote))
            && planner
                .frames
                .iter()
                .any(|frame| matches!(frame, Frame::Item { .. }));
        let in_body = planner
            .frames
            .iter()
            .any(|frame| body_column(frame).is_some() || matches!(frame, Frame::DefinitionList));
        let definition_opener = opened_body
            && !note_opener
            && matches!(planner.frames.last(), Some(Frame::Definition { .. }));
        let note_opener = note_opener && matches!(planner.frames.last(), Some(Frame::Footnote));
        let task_content = if task_body {
            &remaining[3..]
        } else {
            remaining
        };
        let html_content = if task_body {
            task_content.trim_start_matches([' ', '\t'])
        } else {
            task_content
        };
        if !commonmark
            && !opener_as_text
            && options.extensions.raw_html
            && html_content.trim_start().starts_with('<')
        {
            let ops = container_ops(&planner.frames);
            let in_list = ops.iter().any(|op| matches!(op, StripOp::ListAdvance(_)));
            let quote_depth = ops
                .iter()
                .filter(|op| matches!(op, StripOp::BlockQuoteMarker))
                .count();
            let after_blank = planner.paragraph.is_empty()
                && (!owns_body_frame(&planner.frames)
                    || opened_item
                    || opened_body
                    || matches!(planner.events.last(), Some(Event::PandocHtml(_)))
                    || pos == 0
                    || is_blank_line(lines[pos - 1])
                    || quote_tail(lines[pos - 1], quote_depth).is_some_and(is_blank_line));
            if let Some(kind) = html::detect(html_content, after_blank)
                && (!note_opener
                    || !super::super::block_dispatcher::html_block_cannot_interrupt(
                        &kind,
                        html_content,
                        true,
                    ))
            {
                let mut prefix = block_prefix(
                    &ops,
                    &planner.frames,
                    opened_item,
                    outer_div_indent,
                    options,
                )
                .with_note_boundary(
                    planner
                        .frames
                        .iter()
                        .position(|frame| matches!(frame, Frame::Footnote))
                        .map(|i| container_ops(&planner.frames[..i]).len()),
                );
                prefix.lazy_blockquote_gobble = quote_depth > 0;
                let in_div = outer_div_indent.is_some()
                    || planner
                        .frames
                        .iter()
                        .any(|frame| matches!(frame, Frame::Div { .. }));
                let owns_indent = !lazy_html_opener
                    && matches!(
                        ops.last(),
                        Some(StripOp::ListAdvance(_) | StripOp::ContentIndent(_))
                    );
                let indent = if owns_indent {
                    trailing_prefix_indent(line, remaining, &prefixes)
                } else {
                    ""
                };
                let mode = if opened_item && owns_indent {
                    Some(html::BodyHtml::ListItem)
                } else if note_opener {
                    Some(html::BodyHtml::FootnoteStart)
                } else if definition_opener {
                    Some(html::BodyHtml::DefinitionStart)
                } else if matches!(ops.last(), Some(StripOp::ContentIndent(_)))
                    && (!matches!(planner.frames.last(), Some(Frame::Footnote))
                        || quote_depth > 0
                        || after_blank)
                {
                    Some(html::BodyHtml::Indented)
                } else {
                    None
                };
                let block = if let Some(mode) = mode {
                    html::prepare_body(
                        StrippedLines::new(lines, pos, &prefix),
                        html_content,
                        indent,
                        mode,
                        in_body && pos > 0 && is_blank_line(lines[pos - 1]),
                        options,
                    )
                } else {
                    let fusion = if in_body {
                        SoftbreakFusion::None
                    } else if matches!(ops.last(), Some(StripOp::BlockQuoteMarker)) {
                        SoftbreakFusion::ToBlockquoteEnd
                    } else if in_list {
                        SoftbreakFusion::None
                    } else if in_div {
                        SoftbreakFusion::ToFencedDivClose
                    } else {
                        SoftbreakFusion::ToDocEnd
                    };
                    html::prepare(
                        StrippedLines::new(lines, pos, &prefix),
                        kind,
                        options,
                        fusion,
                        (!owns_indent).then_some(html_content),
                    )
                };
                if let Some(block) = block {
                    let interrupted = !planner.paragraph.is_empty();
                    if interrupted {
                        planner.paragraph_kind = K::PLAIN;
                        if !in_body && (!in_list || !owns_indent) {
                            planner.push_text("", std::mem::take(&mut prefixes));
                        }
                    }
                    planner.flush();
                    if owns_indent {
                        while matches!(prefixes.last(), Some(Prefix::Indent(_))) {
                            prefixes.pop();
                        }
                    }
                    if in_list && (owns_indent || interrupted) {
                        for prefix in prefixes {
                            match prefix {
                                Prefix::Quote(indent, space) => {
                                    planner.events.push(Event::QuoteLinePrefix(indent, space));
                                }
                                Prefix::Indent(indent) => {
                                    planner.events.push(Event::LinePrefix(indent));
                                }
                            }
                        }
                    } else {
                        planner.structural_prefixes(prefixes);
                    }
                    if task_body {
                        let padding = &task_content[..task_content.len() - html_content.len()];
                        planner.events.push(Event::Whitespace(padding));
                    }
                    pos = block.end;
                    after_item_html = opened_item && owns_indent;
                    planner.events.push(Event::PandocHtml(block));
                    continue;
                }
            }
        }
        if !task_body
            && !opener_as_text
            && !definition_opener
            && options.extensions.grid_tables
            && planner.paragraph.is_empty()
            && (remaining.starts_with('+')
                || (options.extensions.table_captions && is_table_caption_start(remaining)))
        {
            let ops = container_ops(&planner.frames);
            let prefix = block_prefix(
                &ops,
                &planner.frames,
                in_body || opened_item,
                outer_div_indent,
                options,
            );
            let takes_indent = in_body || matches!(ops.last(), Some(StripOp::ListAdvance(_)));
            let indent = if takes_indent {
                trailing_prefix_indent(line, remaining, &prefixes)
            } else {
                ""
            };
            let mut window = StrippedLines::new(lines, pos, &prefix);
            if !task_body && in_body {
                window = window.with_dispatch_override(remaining, indent);
            }
            if window.detect_at(pos) == remaining
                && let Some(table) = prepare_grid_table(&window)
                && (!opened_item || table.can_open_on_list_marker(&window))
            {
                if takes_indent {
                    while matches!(prefixes.last(), Some(Prefix::Indent(_))) {
                        prefixes.pop();
                    }
                }
                planner.structural_prefixes(prefixes);
                let consumed = table.lines_consumed;
                planner
                    .events
                    .push(Event::GridTable(pos, prefix, table, remaining, indent));
                pos += consumed;
                continue;
            }
        }
        let dash_prefix = (!opener_as_text
            && !definition_opener
            && planner.paragraph.is_empty()
            && (options.extensions.multiline_tables || options.extensions.simple_tables)
            && (remaining.trim_start().starts_with('-')
                || (options.extensions.table_captions && is_table_caption_start(remaining))
                || (options.extensions.simple_tables
                    && lines.get(pos + 1).is_some_and(|next| {
                        next.trim_start_matches([' ', '\t', '>']).starts_with('-')
                    })))
            && try_parse_atx_heading(remaining).is_none()
            && !try_parse_fence_open(remaining, options.dialect)
                .is_some_and(|fence| fence_enabled(&fence, options))
            && (!options.extensions.fenced_divs || try_parse_div_fence_open(remaining).is_none())
            && (!options.extensions.raw_html
                || try_parse_html_block_start(remaining, commonmark).is_none()))
        .then(|| {
            block_prefix(
                &container_ops(&planner.frames),
                &planner.frames,
                in_body || opened_item,
                outer_div_indent,
                options,
            )
            .with_note_boundary(
                planner
                    .frames
                    .iter()
                    .position(|frame| matches!(frame, Frame::Footnote))
                    .map(|i| container_ops(&planner.frames[..i]).len()),
            )
        });
        let dash_window = dash_prefix.as_ref().map(|prefix| {
            let indent = if in_body || matches!(prefix.ops().last(), Some(StripOp::ListAdvance(_)))
            {
                trailing_prefix_indent(line, remaining, &prefixes)
            } else {
                ""
            };
            StrippedLines::new(lines, pos, prefix).with_dispatch_tail(remaining, indent)
        });
        let on_list_marker =
            opened_item && matches!(planner.frames.last(), Some(Frame::Item { .. }));
        if let Some(window) = dash_window
            && let Some(table) = prepare_dash_table_at(&window, options, DashTableKind::Multiline)
            && (!on_list_marker || (options.extensions.table_captions && table.has_caption()))
        {
            pos += planner.push_dash_table(window, table, prefixes, task_body);
            continue;
        }
        if in_body
            && !definition_opener
            && options.extensions.pipe_tables
            && planner.paragraph.is_empty()
            && (remaining.contains('|')
                || (options.extensions.table_captions && is_table_caption_start(remaining)))
        {
            let ops = container_ops(&planner.frames);
            let prefix = block_prefix(&ops, &planner.frames, true, outer_div_indent, options);
            let indent = trailing_prefix_indent(line, remaining, &prefixes);
            let window =
                StrippedLines::new(lines, pos, &prefix).with_dispatch_override(remaining, indent);
            if let Some(table) =
                prepare_pipe_table(&window.with_jupyter_lookahead(jupyter_lines), options)
            {
                while matches!(prefixes.last(), Some(Prefix::Indent(_))) {
                    prefixes.pop();
                }
                planner.structural_prefixes(prefixes);
                let consumed = table.lines_consumed;
                planner.events.push(Event::BodyPipeTable(
                    pos..pos + consumed,
                    prefix,
                    table,
                    remaining,
                    indent,
                ));
                pos += consumed;
                continue;
            }
        }
        if !task_body
            && !in_body
            && !commonmark
            && options.extensions.pipe_tables
            && planner.paragraph.is_empty()
            && (remaining.contains('|')
                || (options.extensions.table_captions && is_table_caption_start(remaining)))
            && let Some(next) = lines.get(pos + 1)
        {
            let ops = container_ops(&planner.frames);
            let prefix = ContainerPrefix::from_ops(&ops, opened_item);
            if prefix.strip_dispatch_line(line) == remaining
                && ((options.extensions.table_captions && is_table_caption_start(remaining))
                    || ContainerPrefix::strip_required_ops(next, &ops)
                        .is_some_and(|tail| try_parse_pipe_separator(tail).is_some()))
            {
                let in_list = ops.iter().any(|op| matches!(op, StripOp::ListAdvance(_)));
                let mut end = pos + 1;
                let mut separator_seen = false;
                while let Some(raw) = lines.get(end) {
                    let tail = ContainerPrefix::strip_required_ops(raw, &ops);
                    if is_blank_line(raw) || tail.is_some_and(is_blank_line) {
                        let mut after_blank = end + 1;
                        while lines.get(after_blank).is_some_and(|line| {
                            is_blank_line(line)
                                || ContainerPrefix::strip_required_ops(line, &ops)
                                    .is_some_and(is_blank_line)
                        }) {
                            after_blank += 1;
                        }
                        if !separator_seen
                            || (options.extensions.table_captions
                                && lines
                                    .get(after_blank)
                                    .and_then(|line| {
                                        ContainerPrefix::strip_required_ops(line, &ops)
                                    })
                                    .is_some_and(is_table_caption_start))
                        {
                            end = after_blank;
                            continue;
                        }
                        break;
                    }
                    let Some(tail) = tail else { break };
                    let separator = try_parse_pipe_separator(tail).is_some();
                    if (in_list && marker(tail, options).is_some() && !separator)
                        || (separator_seen && !tail.contains('|') && !is_table_caption_start(tail))
                    {
                        break;
                    }
                    separator_seen |= separator;
                    end += 1;
                }
                let window = StrippedLines::new(&lines[..end], pos, &prefix);
                if let Some(table) =
                    prepare_pipe_table(&window.with_jupyter_lookahead(jupyter_lines), options)
                {
                    if matches!(ops.last(), Some(StripOp::ListAdvance(_))) {
                        while matches!(prefixes.last(), Some(Prefix::Indent(_))) {
                            prefixes.pop();
                        }
                    }
                    planner.structural_prefixes(prefixes);
                    let consumed = table.lines_consumed;
                    planner
                        .events
                        .push(Event::PipeTable(pos, prefix, table, opened_item));
                    pos += consumed;
                    continue;
                }
            }
        }
        if let Some(window) = dash_window
            && let Some(table) = prepare_dash_table_at(&window, options, DashTableKind::Simple)
            && (!on_list_marker || (options.extensions.table_captions && table.has_caption()))
        {
            pos += planner.push_dash_table(window, table, prefixes, task_body);
            continue;
        }
        if task_body {
            planner.push_text(&remaining[3..], prefixes);
            pos += 1;
            continue;
        }
        if is_blank_line(remaining) {
            planner.flush();
            if opened_body && !opened_item {
                if !remaining.is_empty() {
                    planner.events.push(Event::Newline(remaining));
                }
                pos += 1;
                continue;
            }
            if !opened_item || (commonmark && matches!(planner.frames.last(), Some(Frame::Quote))) {
                planner.structural_prefixes(prefixes);
                planner.events.push(Event::Blank(remaining));
            } else if !remaining.is_empty() {
                let ops = container_ops(&planner.frames);
                let continuation = lines
                    .get(pos + 1)
                    .and_then(|line| ContainerPrefix::strip_required_ops(line, &ops));
                if !commonmark
                    && options.extensions.pipe_tables
                    && matches!(remaining, "\n" | "\r\n")
                    && continuation.is_some_and(|line| line.contains('|'))
                    && lines
                        .get(pos + 2)
                        .and_then(|line| ContainerPrefix::strip_required_ops(line, &ops))
                        .is_some_and(|line| try_parse_pipe_separator(line).is_some())
                {
                    planner.events.push(Event::Newline(remaining));
                } else {
                    planner.push_text(remaining, prefixes);
                    if !continuation.is_some_and(|line| {
                        !is_blank_line(line)
                            && (is_quote_prose(line, options)
                                || (!commonmark
                                    && options.extensions.blank_before_header
                                    && try_parse_atx_heading(line).is_some()))
                    }) {
                        planner.flush();
                    }
                }
            }
        } else if !commonmark
            && options.extensions.fenced_divs
            && planner.paragraph.is_empty()
            && remaining.trim_start_matches(' ').starts_with(':')
            && let Some(fence) = try_parse_div_fence_open(remaining)
        {
            let opener = if planner
                .frames
                .last()
                .is_some_and(|frame| body_column(frame).is_some())
            {
                restore_trailing_indent(line, remaining, &mut prefixes)
            } else {
                remaining
            };
            planner.structural_prefixes(prefixes);
            planner.events.push(Event::OpenDiv(opener, fence));
            planner.frames.push(Frame::Div {
                indent: leading_indent(remaining).0,
            });
        } else if !opener_as_text
            && planner.paragraph.is_empty()
            && leading_indent_from(remaining, list_columns).0 >= 4
            && (commonmark
                || in_body
                || opened_item
                || matches!(
                    planner.frames.as_slice(),
                    [Frame::List { .. }, Frame::Item { .. }] | [Frame::Quote]
                ))
        {
            let columns = if in_body {
                None
            } else {
                opaque_block_columns(&planner.frames, commonmark).filter(|_| !quote_inside_list)
            };
            let Some((quote_depth, column, _)) = columns else {
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
                    let body = prefix.strip_through_last_quote(raw).unwrap_or(raw);
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
        } else if let Some(fence) = try_parse_fence_open(remaining, options.dialect)
            && fence_enabled(&fence, options)
        {
            if !commonmark
                && !planner.paragraph.is_empty()
                && (fence.fence_char == '~'
                    || (fence.info_string.is_empty()
                        && fence.fence_char == '`'
                        && planner
                            .paragraph
                            .pending_code_span_openers()
                            .contains(&fence.fence_count))
                    || (leading_indent(remaining).0 > 0
                        && (planner.paragraph_kind == K::PLAIN
                            || options.extensions.tex_math_gfm && is_gfm_math_fence(&fence))))
            {
                planner.push_text(remaining, prefixes);
                pos += 1;
                continue;
            }
            let direct_item =
                opened_item && matches!(planner.frames.last(), Some(Frame::Item { .. }));
            let math_info = options.extensions.tex_math_gfm && is_gfm_math_fence(&fence);
            let math_fence = math_info && !direct_item;
            if math_info
                && direct_item
                && (in_body || opaque_block_columns(&planner.frames, true).is_none())
                && let Some(Frame::Item { column, .. }) = planner.frames.last()
            {
                let depth = planner
                    .frames
                    .iter()
                    .filter(|frame| matches!(frame, Frame::Quote))
                    .count();
                let prefix = ContainerPrefix::from_scalars(
                    depth,
                    *column,
                    depth > 0,
                    0,
                    true,
                    options.dialect,
                );
                let mut end = pos + 1;
                let mut closed = false;
                for &raw in &lines[end..] {
                    if count_blockquote_markers(raw).0 < depth && (commonmark || is_blank_line(raw))
                    {
                        break;
                    }
                    end += 1;
                    if is_closing_fence(prefix.strip(raw), &fence) {
                        closed = true;
                        break;
                    }
                }
                planner.flush();
                planner.structural_prefixes(prefixes);
                planner.events.push(Event::Fence(
                    pos..end,
                    fence,
                    prefix,
                    Some(remaining),
                    closed,
                ));
                pos = end;
                continue;
            }
            if in_body {
                let ops = container_ops(&planner.frames);
                let prefix = ContainerPrefix::from_ops(&ops, true);
                let mut end = pos + 1;
                let mut closed = false;
                let mut blank_seen = false;
                for raw in &lines[end..] {
                    let tail = ContainerPrefix::strip_required_ops(raw, &ops).or_else(|| {
                        if math_fence && !blank_seen {
                            return Some(prefix.strip(raw));
                        }
                        if (math_fence && blank_seen)
                            || !matches!(planner.frames.last(), Some(Frame::Footnote))
                        {
                            return None;
                        }
                        let tail = ContainerPrefix::strip_required_ops(raw, &ops[..ops.len() - 1])?;
                        is_closing_fence(tail, &fence).then_some(tail)
                    });
                    let Some(tail) = tail else { break };
                    blank_seen = is_blank_line(tail);
                    end += 1;
                    if is_closing_fence(tail, &fence) {
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    if math_fence {
                        planner.push_text(remaining, prefixes);
                        pos += 1;
                        continue;
                    }
                    return Err((remaining, "nested fenced code"));
                }
                planner.flush();
                let first = restore_trailing_indent(line, remaining, &mut prefixes);
                let indent = &first[..first.len() - remaining.len()];
                planner.structural_prefixes(prefixes);
                planner.events.push(Event::BodyFence(
                    pos..end,
                    fence,
                    prefix,
                    remaining,
                    indent,
                    math_fence,
                ));
                pos = end;
                continue;
            }
            if (commonmark || math_fence) && opaque_block_columns(&planner.frames, true).is_none() {
                let mut ops = container_ops(&planner.frames);
                let mut end = pos + 1;
                let mut closed = false;
                for raw in &lines[end..] {
                    let tail = ContainerPrefix::strip_required_ops(raw, &ops).or_else(|| {
                        (!commonmark && math_fence && !is_blank_line(raw))
                            .then(|| raw.trim_start_matches([' ', '\t']))
                    });
                    let Some(tail) = tail else { break };
                    end += 1;
                    if is_closing_fence(tail, &fence) {
                        closed = true;
                        break;
                    }
                }
                if !closed && !commonmark {
                    if planner.paragraph.is_empty()
                        && matches!(planner.frames.last(), Some(Frame::Quote))
                    {
                        planner.structural_prefixes(std::mem::take(&mut prefixes));
                    }
                    planner.push_text(remaining, prefixes);
                    pos += 1;
                    continue;
                }
                // The existing CST owns a short initial indent as part of
                // the first quote marker on fenced-code body lines.
                if !math_fence
                    && matches!(
                        ops.as_slice(),
                        [StripOp::ListAdvance(_), StripOp::BlockQuoteMarker, ..]
                    )
                    && lines[pos + 1..end]
                        .iter()
                        .all(|raw| quote_prefix(raw).is_some())
                {
                    ops.remove(0);
                }
                let prefix = ContainerPrefix::from_ops(&ops, true);
                let first = planner.interrupt_commonmark(line, remaining, prefixes);
                planner.events.push(Event::PlannedFence(
                    pos..end,
                    fence,
                    prefix,
                    first,
                    closed,
                    math_fence,
                ));
                pos = end;
                continue;
            }
            let (quote_depth, column, quotes_first) =
                opaque_block_columns(&planner.frames, commonmark || math_fence)
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
            let mut exit = super::super::blocks::code_blocks::ContainerExitScan::new(column);
            for &raw in &lines[end..] {
                let tail = if !commonmark && quote_depth > 0 && column == 0 {
                    (!is_blank_line(raw)).then(|| prefix.strip(raw))
                } else if math_fence && !commonmark {
                    let inner = if quotes_first {
                        quote_tail(raw, quote_depth).unwrap_or(raw)
                    } else {
                        raw
                    };
                    (!(quote_depth > 0 && is_blank_line(raw)) && !exit.exits(inner))
                        .then(|| prefix.strip(raw))
                } else if quotes_first {
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
                if math_fence {
                    if planner.paragraph.is_empty()
                        && matches!(planner.frames.last(), Some(Frame::Quote))
                    {
                        planner.structural_prefixes(std::mem::take(&mut prefixes));
                    }
                    planner.push_text(remaining, prefixes);
                    pos += 1;
                    continue;
                }
                return Err((remaining, "nested fenced code"));
            }
            if (commonmark || math_fence) && quote_depth > 0 && !quotes_first {
                let first = planner.interrupt_commonmark(line, remaining, prefixes);
                let mut prefix = prefix;
                prefix.list_marker_consumed_on_line_0 = true;
                planner.events.push(Event::PlannedFence(
                    pos..end,
                    fence,
                    prefix,
                    first,
                    closed,
                    math_fence,
                ));
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
                closed,
            ));
            pos = end;
            continue;
        } else if !note_opener
            && (!matches!(planner.frames.last(), Some(Frame::Definition { .. }))
                || opened_body
                || matches!(planner.math.display, Some(OpenDisplayMath::Jupyter))
                || leading_indent(line).0 < 4
                || (pos > 0 && is_blank_line(lines[pos - 1])))
            && let Some(level) = try_parse_atx_heading(remaining)
        {
            if !planner.paragraph.is_empty() && options.extensions.blank_before_header {
                planner.push_text(remaining, prefixes);
            } else if after_line_or_tex_block
                && (continues_item_html
                    || matches!(
                        planner.frames.as_slice(),
                        [.., Frame::Item { .. }, Frame::Div { .. }]
                    ))
            {
                let indent = trailing_prefix_indent(line, remaining, &prefixes);
                while matches!(prefixes.last(), Some(Prefix::Indent(_))) {
                    prefixes.pop();
                }
                planner.push_block_prefixes(prefixes);
                planner
                    .events
                    .push(Event::AtxWithPrefix(remaining, level, indent));
            } else {
                let heading = if commonmark {
                    planner.interrupt_commonmark(line, remaining, prefixes)
                } else {
                    let after_jupyter_definition =
                        matches!(planner.math.display, Some(OpenDisplayMath::Jupyter))
                            && matches!(planner.frames.last(), Some(Frame::Definition { .. }));
                    planner.flush();
                    let heading = if after_jupyter_definition
                        || matches!(planner.frames.last(), Some(Frame::Footnote))
                        || (matches!(planner.frames.last(), Some(Frame::Definition { .. }))
                            && pos > 0
                            && is_blank_line(lines[pos - 1]))
                    {
                        remaining
                    } else {
                        restore_trailing_indent(line, remaining, &mut prefixes)
                    };
                    if after_line_or_tex_block {
                        planner.push_block_prefixes(prefixes);
                    } else {
                        planner.structural_prefixes(prefixes);
                    }
                    heading
                };
                planner.events.push(Event::Atx(heading, level));
            }
        } else if !planner.paragraph.is_empty()
            && (commonmark || list_columns == 0 || leading_indent(line).0 < 4)
            && try_parse_setext_heading(&["text\n", remaining], 0).is_some()
            && (commonmark || planner.paragraph.get_text_for_parsing().lines().count() == 1)
            && !planner.paragraph.get_text_for_parsing().trim().is_empty()
        {
            let mut buffer = std::mem::take(&mut planner.paragraph);
            let underline = if commonmark
                && planner.math.lines > 1
                && planner.math.holds(remaining, options)
                && matches!(planner.frames.last(), Some(Frame::Quote))
            {
                let raw = buffer.raw_text();
                buffer = ParagraphBuffer::new();
                buffer.push_text(&raw);
                prefixes.clear();
                line
            } else {
                restore_trailing_indent(line, remaining, &mut prefixes)
            };
            planner.math.clear();
            planner
                .events
                .push(Event::Setext(buffer, prefixes, underline));
        } else if !definition_opener && try_parse_horizontal_rule(remaining).is_some() {
            if !commonmark && !opened_item && !opened_body && !planner.paragraph.is_empty() {
                planner.push_text(remaining, prefixes);
                pos += 1;
                continue;
            }
            let rule = if commonmark {
                planner.interrupt_commonmark(line, remaining, prefixes)
            } else {
                planner.flush();
                if after_line_or_tex_block
                    && matches!(planner.frames.last(), Some(Frame::Item { .. }))
                {
                    let rule = restore_trailing_indent(line, remaining, &mut prefixes);
                    planner.push_block_prefixes(prefixes);
                    rule
                } else {
                    planner.structural_prefixes(prefixes);
                    remaining
                }
            };
            planner.events.push(Event::Rule(rule));
        } else if !note_opener
            && !definition_opener
            && (commonmark || !opened_item)
            && options.extensions.reference_links
            && planner.paragraph.is_empty()
            && remaining.trim_start().starts_with('[')
            && !remaining.trim_start().starts_with("[^")
            && let Some(reference) =
                plan_reference(remaining, &lines[pos + 1..], &planner.frames, options)
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
        } else if !opener_as_text
            && options.extensions.raw_tex
            && remaining.trim_start().starts_with('\\')
            && !note_opener
            && !definition_opener
            && (!opened_item || matches!(planner.frames.last(), Some(Frame::Quote)))
            && let Some((end, prefix, first)) = {
                let after_blank = planner.paragraph.is_empty()
                    && (continues_item_html || can_start_body_block(&planner.frames, lines, pos));
                let ops = container_ops(&planner.frames);
                let prefix = block_prefix(&ops, &planner.frames, false, outer_div_indent, options);
                let window = StrippedLines::new(lines, pos, &prefix);
                let list_div = matches!(
                    planner.frames.as_slice(),
                    [.., Frame::Item { .. }, Frame::Div { .. }]
                );
                let first = if continues_item_html || list_div {
                    Some((
                        remaining,
                        trailing_prefix_indent(line, remaining, &prefixes),
                    ))
                } else {
                    (opened_item || opened_body).then_some((remaining, ""))
                };
                let window = match first {
                    Some((tail, indent)) if continues_item_html => {
                        window.with_dispatch_override(tail, indent)
                    }
                    Some((tail, indent)) => window.with_dispatch_tail(tail, indent),
                    None => window,
                };
                if let Some(env) = raw_blocks::raw_tex_environment(remaining) {
                    Some((
                        raw_blocks::raw_tex_environment_end(&window, env),
                        prefix,
                        first,
                    ))
                } else if after_blank && raw_blocks::can_start_raw_command(remaining) {
                    Some((raw_blocks::raw_tex_commands_end(&window), prefix, first))
                } else {
                    None
                }
            }
        {
            while matches!(prefixes.last(), Some(Prefix::Indent(_))) {
                prefixes.pop();
            }
            if !planner.paragraph.is_empty() && planner.paragraph_kind == K::PARAGRAPH {
                planner.push_text("", std::mem::take(&mut prefixes));
            }
            planner.flush();
            planner.push_block_prefixes(prefixes);
            after_item_html = continues_item_html;
            planner
                .events
                .push(Event::RawTex(pos..end, prefix, first, continues_item_html));
            pos = end;
            continue;
        } else if !opener_as_text
            && options.extensions.line_blocks
            && !note_opener
            && !definition_opener
            && planner.paragraph.is_empty()
            && try_parse_line_block_start(remaining).is_some()
            && (remaining.starts_with('|')
                || !matches!(planner.events.last(), Some(Event::LineBlock(..))))
            && (opened_item
                || after_html
                || after_line_or_tex_block
                || can_start_body_block(&planner.frames, lines, pos))
        {
            let ops = container_ops(&planner.frames);
            let mut prefix = block_prefix(
                &ops,
                &planner.frames,
                opened_item,
                outer_div_indent,
                options,
            );
            prefix.lazy_blockquote_gobble = !commonmark;
            let indent = trailing_prefix_indent(line, remaining, &prefixes);
            let window =
                StrippedLines::new(lines, pos, &prefix).with_dispatch_tail(remaining, indent);
            let block = PreparedLineBlock::new(&window);
            while matches!(prefixes.last(), Some(Prefix::Indent(_))) {
                prefixes.pop();
            }
            planner.push_block_prefixes(prefixes);
            after_item_html = continues_item_html;
            let end = block.end(pos);
            planner
                .events
                .push(Event::LineBlock(pos, prefix, block, remaining, indent));
            pos = end;
            continue;
        } else if opener_as_text
            || (!commonmark && leading_indent(remaining).0 < 4)
            || (leading_indent(remaining).0 >= 4
                && leading_indent_from(remaining, list_columns).0 < 4)
            || is_quote_prose(remaining, options)
            || (commonmark
                && !planner.paragraph.is_empty()
                && remaining.trim_start().starts_with('[')
                && remaining.contains("]:"))
            || (leading_indent(remaining).0 >= 4 && !planner.paragraph.is_empty())
        {
            // Within a quoted list the incumbent retains list indentation in
            // the text after the quote marker.
            let after_item_heading = !commonmark
                && matches!(planner.frames.last(), Some(Frame::Item { .. }))
                && matches!(
                    planner.events.as_slice(),
                    [.., Event::Item(..), Event::Atx(..)]
                );
            if !continues_item_html
                && (after_item_heading
                    || planner.paragraph.is_empty()
                        && (commonmark
                            || matches!(planner.frames.last(), Some(Frame::Item { .. }))))
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
            } else if !continues_item_html
                && matches!(planner.frames.last(), Some(Frame::Item { .. }))
                && matches!(prefixes.last(), Some(Prefix::Indent(_)))
                && (planner.paragraph.is_empty()
                    || prefixes.iter().any(|p| matches!(p, Prefix::Quote(..))))
                && let Some(Prefix::Indent(indent)) = prefixes.pop()
            {
                remaining = &line[line.len() - remaining.len() - indent.len()..];
            }
            if matches!(
                prefixes.as_slice(),
                [Prefix::Indent(_), Prefix::Indent(_), ..]
            ) {
                let unstripped = restore_trailing_indent(line, remaining, &mut prefixes);
                prefixes.push(Prefix::Indent(
                    &unstripped[..unstripped.len() - remaining.len()],
                ));
            }
            if planner.paragraph.is_empty()
                && matches!(
                    planner.frames.last(),
                    Some(Frame::Definition { .. } | Frame::Footnote)
                )
                && let Some(index) = prefixes
                    .iter()
                    .rposition(|part| matches!(part, Prefix::Quote(..)))
                && !planner
                    .frames
                    .iter()
                    .rev()
                    .take_while(|part| body_column(part).is_none())
                    .any(|part| matches!(part, Frame::Quote))
            {
                planner.structural_prefixes(prefixes.drain(..=index).collect());
            }
            if planner.paragraph.is_empty()
                && (matches!(planner.frames.last(), Some(Frame::Quote))
                    || matches!(
                        (planner.frames.last(), prefixes.first()),
                        (Some(Frame::Div { .. }), Some(Prefix::Quote(..)))
                    )
                    || (matches!(planner.frames.last(), Some(Frame::Div { .. }))
                        && matches!(planner.events.last(), Some(Event::OpenDiv(..)))
                        && prefixes
                            .iter()
                            .any(|prefix| matches!(prefix, Prefix::Quote(..)))))
            {
                planner.structural_prefixes(std::mem::take(&mut prefixes));
            }
            planner.push_text(remaining, prefixes);
        } else {
            return Err((remaining, "nested block"));
        }
        pos += 1;
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
            Event::Item(line, marker, claim_task_checkbox) => {
                let (indent_cols, indent_bytes) = leading_indent(line);
                emit_list_item_with_checkbox(
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
                    claim_task_checkbox,
                );
            }
            Event::Paragraph(kind, buffer, suppress_footnote_refs) => {
                emit_buffered_paragraph(builder, kind, &buffer, options, suppress_footnote_refs);
            }
            Event::Term(line) => emit_term(builder, line, None, options),
            Event::DefinitionMarker(indent, marker, padding) => {
                emit_definition_marker(builder, marker, indent);
                if !padding.is_empty() {
                    builder.token(K::WHITESPACE.into(), padding);
                }
            }
            Event::FootnoteMarker(indent, id, padding) => {
                emit_footnote_marker(builder, indent, &id, padding);
            }
            Event::Blank(line) => emit_blank(builder, line),
            Event::Newline(line) => builder.token(K::NEWLINE.into(), line),
            Event::Whitespace(indent) => builder.token(K::WHITESPACE.into(), indent),
            Event::LinePrefix(indent) => builder.token(K::LINE_PREFIX.into(), indent),
            Event::Fence(range, fence, prefix, first, closed) => {
                let window =
                    StrippedLines::new(&lines[..start + range.end], start + range.start, &prefix);
                // The incumbent's direct list-marker path emits ordinary code.
                if first.is_none() && options.extensions.tex_math_gfm && is_gfm_math_fence(&fence) {
                    emit_planned_fenced_math_block(
                        builder,
                        &window,
                        &fence,
                        first,
                        start + range.end,
                        closed,
                    );
                } else {
                    let window = if let Some(first) = first {
                        window.with_dispatch_override(first, "")
                    } else {
                        window
                    };
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
            }
            Event::PlannedFence(range, fence, prefix, first, closed, math_fence) => {
                let window =
                    StrippedLines::new(&lines[..start + range.end], start + range.start, &prefix);
                if math_fence {
                    emit_planned_fenced_math_block(
                        builder,
                        &window.with_dispatch_override(first, ""),
                        &fence,
                        None,
                        start + range.end,
                        closed,
                    );
                } else {
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
            }
            Event::BodyFence(range, fence, prefix, tail, indent, math_fence) => {
                let window =
                    StrippedLines::new(&lines[..start + range.end], start + range.start, &prefix)
                        .with_dispatch_override(tail, indent);
                if math_fence {
                    emit_planned_fenced_math_block(
                        builder,
                        &window,
                        &fence,
                        Some(tail),
                        start + range.end,
                        true,
                    );
                } else {
                    emit_planned_fenced_code_block(
                        builder,
                        &window,
                        fence,
                        tail,
                        diagnostics,
                        options.flavor,
                        true,
                    );
                }
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
            Event::OpenDiv(line, fence) => {
                builder.start_node(K::FENCED_DIV.into());
                emit_div_fence_open(builder, line, &fence);
            }
            Event::CloseDiv(line, indent) => {
                emit_div_fence_close_with_prefix(builder, line, indent);
                builder.finish_node();
            }
            Event::Atx(line, level) => emit_atx_heading(builder, line, level, options),
            Event::AtxWithPrefix(line, level, indent) => {
                let mut inner = GreenNodeBuilder::new();
                emit_atx_heading(&mut inner, line, level, options);
                let heading = crate::syntax::SyntaxNode::new_root(inner.finish());
                let mut prefix =
                    ContainerPrefixState::new(vec![ContainerPrefixLine::list_only(indent.into())]);
                graft_node(builder, &heading, &mut prefix);
            }
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
            Event::GridTable(pos, prefix, table, tail, indent) => {
                let window = StrippedLines::new(&lines[start..], pos, &prefix)
                    .with_dispatch_override(tail, indent);
                table.emit(builder, &window, options);
            }
            Event::DashTable(pos, prefix, table, dispatch_override) => {
                let mut window = StrippedLines::new(&lines[start..], pos, &prefix);
                if let Some((tail, indent)) = dispatch_override {
                    window = window.with_dispatch_tail(tail, indent);
                }
                table.emit(builder, &window, options);
            }
            Event::PipeTable(pos, prefix, table, inject_list_indent) => {
                let window = StrippedLines::new(&lines[start..], pos, &prefix);
                table.emit_with_row_prefix(builder, &window, options, inject_list_indent);
            }
            Event::BodyPipeTable(range, prefix, table, tail, indent) => {
                let window =
                    StrippedLines::new(&lines[start..start + range.end], range.start, &prefix)
                        .with_dispatch_override(tail, indent);
                table.emit(builder, &window, options);
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
            Event::LineBlock(pos, prefix, block, first, indent) => {
                let window = StrippedLines::new(lines, start + pos, &prefix)
                    .with_dispatch_tail(first, indent);
                block.emit(&window, builder, options);
            }
            Event::RawTex(range, prefix, first, grafted) => {
                let window = StrippedLines::new(lines, start + range.start, &prefix);
                let window = match first {
                    Some((tail, indent)) if grafted => window.with_dispatch_override(tail, indent),
                    Some((tail, indent)) => window.with_dispatch_tail(tail, indent),
                    None => window,
                };
                emit_raw_tex_block(builder, &window, start + range.end);
            }
            Event::PandocHtml(block) => block.emit(builder),
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
