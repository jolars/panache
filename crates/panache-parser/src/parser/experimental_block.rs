//! Experimental block planning followed by lossless CST emission.
//!
//! The implementation covers a subset of the CommonMark and Quarto flavors
//! with resolved extension settings. Compare the result with the incumbent parser before using a
//! document as a benchmark workload.

use std::{cell::OnceCell, ops::Range, sync::Arc};

use rowan::{GreenNode, GreenNodeBuilder};

use crate::options::{Dialect, Flavor, ParserOptions};
use crate::syntax::{SyntaxKind as K, SyntaxNode};

use super::block_dispatcher::{
    emit_div_fence_close, emit_div_fence_open, emit_reference_definition_lines, fence_enabled,
};
use super::blocks::blockquotes::emit_one_blockquote_marker;
use super::blocks::code_blocks::{
    FenceInfo, is_closing_fence, is_gfm_math_fence, parse_fenced_code_block, try_parse_fence_open,
};
use super::blocks::container_prefix::{ContainerPrefix, StrippedLines};
use super::blocks::definition_lists::try_parse_definition_marker;
use super::blocks::fenced_divs::{DivFenceInfo, is_div_closing_fence, try_parse_div_fence_open};
use super::blocks::figures::paragraph_is_standalone_image;
use super::blocks::headings::{
    emit_atx_heading, emit_setext_heading_body, try_parse_atx_heading, try_parse_setext_heading,
};
use super::blocks::horizontal_rules::{emit_horizontal_rule, try_parse_horizontal_rule};
use super::blocks::html_blocks::{
    HtmlBlockContext, HtmlBlockType, SoftbreakFusion, ends_at_blank_line, is_closing_marker,
    parse_html_block_with_wrapper, try_parse_html_block_start,
};
use super::blocks::indented_code::parse_indented_code_block;
use super::blocks::line_blocks::try_parse_line_block_start;
use super::blocks::lists::{
    ListMarkerMatch, OpenListHint, marker_start_number, markers_match, try_parse_list_marker,
};
use super::blocks::metadata::{
    YamlContentOutcome, collect_yaml_content, emit_yaml_block, find_yaml_block_closing_pos,
    is_metadata_open_delim, prepare_yaml_content,
};
use super::blocks::reference_links::{ReferenceSpans, reference_definition_spans};
use super::blocks::tables::{PreparedPipeTable, prepare_pipe_table, try_parse_grid_separator};
use super::core::ParseOrigin;
use super::diagnostics::Diagnostics;
use super::utils::container_stack::{byte_index_at_column, leading_indent};
use super::utils::helpers::{is_blank_line, split_lines_inclusive};
use super::utils::inline_emission::emit_inlines;
use super::utils::marker_utils::try_parse_blockquote_marker;
use super::utils::text_buffer::ParagraphBuffer;
use super::{
    CostGuards, Edit, Reparsed, SyntaxError, populate_refdef_labels, reparse::reparse_using,
};

/// A block feature that the experimental parser has yet to implement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Unsupported {
    pub offset: usize,
    pub construct: &'static str,
}

enum Block<'a> {
    Blank(usize),
    Paragraph(Range<usize>),
    Atx(usize, usize),
    Setext(Range<usize>, usize),
    Rule(usize),
    Reference(Range<usize>, ReferenceSpans),
    PipeTable(usize, PreparedPipeTable<'a>),
    List(usize, Vec<ListItem>),
    Quote(Range<usize>),
    IndentedCode(Range<usize>),
    Html(Range<usize>, HtmlBlockType),
    Code(Range<usize>, FenceInfo),
    Metadata(usize, usize, YamlContentOutcome),
    OpenDiv(usize, DivFenceInfo),
    CloseDiv(Option<usize>),
}

struct ListItem {
    lines: Range<usize>,
    marker: ListMarkerMatch,
    nested: Vec<ListItem>,
    restricted_markers: Vec<usize>,
}

fn plain_container_text(text: &str) -> bool {
    let body = text.trim_start();
    !body.starts_with(['#', '>', ':', '-', '+', '<'])
        && (!body.starts_with('*') || body.starts_with("**") && !body.starts_with("***"))
        && !body.starts_with("```")
        && !body.starts_with("~~~")
        && !body.starts_with("$$")
}

fn is_quote_prose(text: &str, options: &ParserOptions) -> bool {
    leading_indent(text).0 < 4
        && plain_container_text(text)
        && try_parse_list_marker(text, options, OpenListHint::None).is_none()
        && !(text.starts_with('[') && text.contains("]:"))
}

fn list_marker_at(line: &str, indent: usize, options: &ParserOptions) -> Option<ListMarkerMatch> {
    let (columns, bytes) = leading_indent(line);
    (columns == indent && bytes == indent)
        .then(|| try_parse_list_marker(&line[bytes..], options, OpenListHint::None))
        .flatten()
}

fn plan_list(
    lines: &[&str],
    pos: &mut usize,
    first: ListMarkerMatch,
    indent: usize,
    depth: usize,
    options: &ParserOptions,
) -> Option<Vec<ListItem>> {
    if depth == 32 {
        return None;
    }
    let mut items = Vec::new();
    let mut marker = first;
    loop {
        let content_start = indent + marker.marker_len + marker.spaces_after_bytes;
        let content_col = indent + marker.marker_len + marker.spaces_after_cols;
        let tail = &lines[*pos][content_start..];
        if marker.virtual_marker_space || is_blank_line(tail) || !plain_container_text(tail) {
            return None;
        }
        let item_start = *pos;
        *pos += 1;
        let mut nested = Vec::new();
        let mut restricted_markers = Vec::new();
        while let Some(&line) = lines.get(*pos) {
            let (columns, _) = leading_indent(line);
            if is_blank_line(line) || columns < content_col {
                break;
            }
            if let Some(marker) = list_marker_at(line, content_col, options) {
                if options.dialect == Dialect::Pandoc
                    && options.pandoc_compat.restricts_ordered_sublist_start()
                    && marker_start_number(&marker.marker).is_some_and(|start| start != 1)
                {
                    restricted_markers.push(*pos);
                    *pos += 1;
                    continue;
                }
                nested = plan_list(lines, pos, marker, content_col, depth + 1, options)?;
                break;
            }
            if !restricted_markers.is_empty() {
                return None;
            }
            if columns >= content_col + 4 || !plain_container_text(line) {
                return None;
            }
            *pos += 1;
        }

        let paragraph_end = nested.first().map_or(*pos, |first| first.lines.start);
        let next_pos = *pos
            + lines[*pos..]
                .iter()
                .take_while(|line| is_blank_line(line))
                .count();
        let next = lines
            .get(next_pos)
            .and_then(|line| list_marker_at(line, indent, options))
            .filter(|next| markers_match(&marker.marker, &next.marker, options.dialect));
        items.push(ListItem {
            lines: item_start..paragraph_end,
            marker,
            nested,
            restricted_markers,
        });
        if let Some(next) = next {
            *pos = next_pos;
            marker = next;
            continue;
        }
        if let Some(line) = lines.get(next_pos) {
            let (columns, _) = leading_indent(line);
            if (next_pos == *pos && depth == 0) || columns > indent {
                return None;
            }
        }
        return Some(items);
    }
}

fn source_span<'a>(input: &'a str, lines: &[&str], range: Range<usize>) -> &'a str {
    let first = lines[range.start].as_ptr() as usize - input.as_ptr() as usize;
    let last = lines[range.end - 1];
    let end = last.as_ptr() as usize - input.as_ptr() as usize + last.len();
    &input[first..end]
}

fn push_paragraph(blocks: &mut Vec<Block<'_>>, pos: usize) {
    if let Some(Block::Paragraph(range)) = blocks.last_mut() {
        range.end = pos + 1;
    } else {
        blocks.push(Block::Paragraph(pos..pos + 1));
    }
}

fn plan<'a>(
    input: &str,
    lines: &'a [&'a str],
    options: &ParserOptions,
    origin: ParseOrigin,
    jupyter_lines: &'a OnceCell<Arc<super::blocks::jupyter::Lookahead>>,
) -> Result<Vec<Block<'a>>, Unsupported> {
    let prefix = ContainerPrefix::default();
    let mut blocks = Vec::new();
    let mut divs = Vec::new();
    let mut pos = 0;
    let commonmark = options.dialect == Dialect::CommonMark;
    while let Some(&line) = lines.get(pos) {
        let paragraph = matches!(blocks.last(), Some(Block::Paragraph(_)));
        let fresh = !paragraph;
        let at_start = pos == 0 && origin == ParseOrigin::Document;
        let (indent, indent_bytes) = leading_indent(line);
        let content = &line[indent_bytes..];
        let marker = content.as_bytes().first().copied();
        let offset = line.as_ptr() as usize - input.as_ptr() as usize;
        let unsupported = |construct| Unsupported { offset, construct };

        if is_blank_line(line) {
            blocks.push(Block::Blank(pos));
        } else if options.extensions.yaml_metadata_block
            && is_metadata_open_delim(line)
            && (at_start || (!commonmark && fresh))
            && let Some(close) = find_yaml_block_closing_pos(lines, pos, at_start, |i| lines[i])
            && let Some(outcome) =
                prepare_yaml_content(&collect_yaml_content(lines, pos, close), options.flavor)
        {
            blocks.push(Block::Metadata(pos, close, outcome));
            pos = close + 1;
            continue;
        } else if indent < 4
            && matches!(marker, Some(b'`' | b'~'))
            && let Some(fence) = try_parse_fence_open(line, options.dialect)
            && fence_enabled(&fence, options)
            && (commonmark || fresh || (fence.fence_char != '~' && indent == 0))
        {
            if options.extensions.tex_math_gfm && is_gfm_math_fence(&fence) {
                return Err(unsupported("fenced math"));
            }
            let close = lines[pos + 1..]
                .iter()
                .position(|line| is_closing_fence(line, &fence));
            if close.is_some() || commonmark {
                let end = close.map_or(lines.len(), |close| pos + close + 2);
                blocks.push(Block::Code(pos..end, fence));
                pos = end;
                continue;
            }
            push_paragraph(&mut blocks, pos);
        } else if options.extensions.fenced_divs
            && marker == Some(b':')
            && divs.last().is_some_and(|open| indent <= *open)
            && is_div_closing_fence(line)
        {
            divs.pop();
            blocks.push(Block::CloseDiv(Some(pos)));
        } else if options.extensions.fenced_divs
            && indent < 4
            && marker == Some(b':')
            && let Some(fence) = try_parse_div_fence_open(line)
        {
            divs.push(indent);
            blocks.push(Block::OpenDiv(pos, fence));
        } else if matches!(marker, Some(b'=' | b'-'))
            && let Some(Block::Paragraph(range)) = blocks.last_mut()
            && (commonmark || range.len() == 1)
            && try_parse_setext_heading(lines, pos - 1).is_some()
        {
            let range = range.clone();
            *blocks.last_mut().unwrap() = Block::Setext(range, pos);
        } else if indent < 4
            && marker == Some(b'#')
            && (fresh || !options.extensions.blank_before_header)
            && let Some(level) = try_parse_atx_heading(line)
        {
            blocks.push(Block::Atx(pos, level));
        } else if indent < 4
            && matches!(marker, Some(b'*' | b'-' | b'_'))
            && (commonmark || fresh)
            && try_parse_horizontal_rule(line).is_some()
        {
            blocks.push(Block::Rule(pos));
        } else if fresh
            && indent < 4
            && options.extensions.reference_links
            && marker == Some(b'[')
            && !content.starts_with("[^")
            && let Some(spans) = reference_definition_spans(
                source_span(
                    input,
                    lines,
                    pos..pos
                        + lines[pos..]
                            .iter()
                            .take_while(|line| !is_blank_line(line))
                            .count(),
                ),
                true,
                options.dialect,
                false,
            )
        {
            let mut end = pos;
            let mut bytes = 0;
            while bytes < spans.consumed {
                bytes += lines[end].len();
                end += 1;
            }
            blocks.push(Block::Reference(pos..end, spans));
            pos = end;
            continue;
        } else if fresh
            && indent < 4
            && options.extensions.pipe_tables
            && (content.contains('|') || content.starts_with(':') || content.starts_with("Table:"))
            && let Some(table) = prepare_pipe_table(
                &StrippedLines::new(lines, pos, &prefix),
                options,
                jupyter_lines,
            )
        {
            let consumed = table.lines_consumed;
            blocks.push(Block::PipeTable(pos, table));
            pos += consumed;
            continue;
        } else if indent >= 4 && fresh {
            let mut end = pos + 1;
            let mut cursor = end;
            while let Some(line) = lines.get(cursor) {
                if !is_blank_line(line) {
                    if leading_indent(line).0 < 4 {
                        break;
                    }
                    end = cursor + 1;
                }
                cursor += 1;
            }
            blocks.push(Block::IndentedCode(pos..end));
            pos = end;
            continue;
        } else if marker == Some(b'>') {
            if !fresh {
                return Err(unsupported("block quote interruption"));
            }
            let start = pos;
            while let Some(&line) = lines.get(pos) {
                if let Some((_, tail)) = try_parse_blockquote_marker(line) {
                    if !is_blank_line(&line[tail..]) && !is_quote_prose(&line[tail..], options) {
                        return Err(unsupported("block quote inner block"));
                    }
                } else if is_blank_line(line) {
                    break;
                } else if !commonmark
                    || !is_quote_prose(line, options)
                    || pos == start
                    || lines[pos - 1].trim() == ">"
                {
                    return Err(unsupported("block quote continuation"));
                }
                pos += 1;
            }
            blocks.push(Block::Quote(start..pos));
            continue;
        } else if indent < 4
            && (fresh || commonmark)
            && matches!(marker, Some(b'-' | b'+' | b'*' | b'0'..=b'9'))
            && let Some(first) = try_parse_list_marker(line, options, OpenListHint::None)
        {
            if !fresh || indent_bytes != indent {
                return Err(unsupported("list interruption or indentation"));
            }
            let items = plan_list(lines, &mut pos, first, indent, 0, options)
                .ok_or_else(|| unsupported("list continuation or inner block"))?;
            blocks.push(Block::List(indent, items));
            continue;
        } else if commonmark
            && options.extensions.raw_html
            && marker == Some(b'<')
            && indent < 4
            && let Some(kind) = try_parse_html_block_start(line, true)
            && (fresh || !matches!(kind, HtmlBlockType::Type7))
        {
            let mut end = pos + 1;
            if !is_closing_marker(line, &kind) {
                while let Some(line) = lines.get(end) {
                    if ends_at_blank_line(&kind) && is_blank_line(line) {
                        break;
                    }
                    end += 1;
                    if is_closing_marker(line, &kind) {
                        break;
                    }
                }
            }
            blocks.push(Block::Html(pos..end, kind));
            pos = end;
            continue;
        } else if !commonmark && options.extensions.raw_html && marker == Some(b'<') {
            return Err(unsupported("Pandoc HTML block"));
        } else if !commonmark
            && options.extensions.footnotes
            && marker == Some(b'[')
            && content.starts_with("[^")
            && content.contains("]:")
        {
            return Err(unsupported("footnote definition"));
        } else if options.extensions.grid_tables
            && marker == Some(b'+')
            && try_parse_grid_separator(line).is_some()
        {
            return Err(unsupported("grid table"));
        } else if options.extensions.line_blocks
            && fresh
            && marker == Some(b'|')
            && try_parse_line_block_start(line).is_some()
        {
            return Err(unsupported("line block"));
        } else if !commonmark
            && ((options.extensions.definition_lists
                && matches!(marker, Some(b':' | b'~'))
                && try_parse_definition_marker(line).is_some())
                || (options.extensions.pandoc_title_block && marker == Some(b'%'))
                || (options.extensions.raw_tex && marker == Some(b'\\'))
                || (options.extensions.tex_math_dollars && content.starts_with("$$")))
        {
            return Err(unsupported("extended Pandoc block"));
        } else {
            push_paragraph(&mut blocks, pos);
        }
        pos += 1;
    }
    blocks.extend(divs.into_iter().map(|_| Block::CloseDiv(None)));
    Ok(blocks)
}

/// Parse a document with the experimental engine.
pub fn try_parse_with_errors(
    input: &str,
    options: &ParserOptions,
) -> Result<(SyntaxNode, Vec<SyntaxError>), Unsupported> {
    let mut options = options.clone();
    populate_refdef_labels(input, &mut options);
    parse_at(input, &options, ParseOrigin::Document)
}

fn parse_at(
    input: &str,
    options: &ParserOptions,
    origin: ParseOrigin,
) -> Result<(SyntaxNode, Vec<SyntaxError>), Unsupported> {
    if !matches!(options.flavor, Flavor::CommonMark | Flavor::Quarto)
        || options.dialect != Dialect::for_flavor(options.flavor)
    {
        return Err(Unsupported {
            offset: 0,
            construct: "parser options",
        });
    }
    let lines = split_lines_inclusive(input);
    let jupyter_lines = OnceCell::new();
    let blocks = plan(input, &lines, options, origin, &jupyter_lines)?;
    let mut builder = GreenNodeBuilder::new();
    let diagnostics = Diagnostics::new();
    let prefix = ContainerPrefix::default();
    builder.start_node(K::DOCUMENT.into());
    for block in blocks {
        match block {
            Block::Blank(pos) => {
                emit_blank(&mut builder, lines[pos]);
            }
            Block::Paragraph(range) => {
                let text = source_span(input, &lines, range);
                let kind = if paragraph_is_standalone_image(text, options) {
                    K::FIGURE
                } else {
                    K::PARAGRAPH
                };
                builder.start_node(kind.into());
                emit_inlines(&mut builder, text, options, false);
                builder.finish_node();
            }
            Block::Atx(pos, level) => emit_atx_heading(&mut builder, lines[pos], level, options),
            Block::Setext(range, underline) => {
                builder.start_node(K::HEADING.into());
                emit_setext_heading_body(
                    &mut builder,
                    source_span(input, &lines, range),
                    lines[underline],
                    1,
                    options,
                );
                builder.finish_node();
            }
            Block::Rule(pos) => emit_horizontal_rule(&mut builder, lines[pos]),
            Block::Reference(range, spans) => {
                builder.start_node(K::REFERENCE_DEFINITION.into());
                emit_reference_definition_lines(&mut builder, &lines[range], Some(spans));
                builder.finish_node();
            }
            Block::PipeTable(pos, table) => {
                let window = StrippedLines::new(&lines, pos, &prefix);
                table.emit(&mut builder, &window, options);
            }
            Block::List(indent, items) => {
                emit_list(&mut builder, &lines, items, indent, options);
            }
            Block::Quote(range) => emit_quote(&mut builder, &lines[range], options),
            Block::IndentedCode(range) => {
                parse_indented_code_block(&mut builder, &lines[..range.end], range.start, 0, 0);
            }
            Block::Html(range, kind) => {
                let context = HtmlBlockContext {
                    prefix: &prefix,
                    wrapper_kind: K::HTML_BLOCK,
                    fusion: SoftbreakFusion::None,
                    config: options,
                };
                parse_html_block_with_wrapper(
                    &mut builder,
                    &lines[..range.end],
                    range.start,
                    kind,
                    &context,
                    None,
                    None,
                );
            }
            Block::Code(range, fence) => {
                let window = StrippedLines::new(&lines[..range.end], range.start, &prefix);
                parse_fenced_code_block(
                    &mut builder,
                    &window,
                    fence,
                    None,
                    &diagnostics,
                    options.flavor,
                    None,
                );
            }
            Block::Metadata(start, end, outcome) => {
                emit_yaml_block(&lines, start, end, &mut builder, &diagnostics, &outcome);
            }
            Block::OpenDiv(pos, fence) => {
                builder.start_node(K::FENCED_DIV.into());
                emit_div_fence_open(&mut builder, lines[pos], &fence);
            }
            Block::CloseDiv(pos) => {
                if let Some(pos) = pos {
                    emit_div_fence_close(&mut builder, lines[pos]);
                }
                builder.finish_node();
            }
        }
    }
    builder.finish_node();
    Ok((SyntaxNode::new_root(builder.finish()), diagnostics.take()))
}

fn emit_blank(builder: &mut GreenNodeBuilder<'static>, line: &str) {
    builder.start_node(K::BLANK_LINE.into());
    builder.token(K::BLANK_LINE.into(), line);
    builder.finish_node();
}

fn emit_list(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    items: Vec<ListItem>,
    indent: usize,
    options: &ParserOptions,
) -> usize {
    builder.start_node(K::LIST.into());
    let mut end = items[0].lines.start;
    for item in items {
        for line in &lines[end..item.lines.start] {
            emit_blank(builder, line);
        }
        let line = lines[item.lines.start];
        let marker = item.marker;
        builder.start_node(K::LIST_ITEM.into());
        if indent != 0 {
            builder.token(K::WHITESPACE.into(), &line[..indent]);
        }
        let marker_end = indent + marker.marker_len;
        builder.token(K::LIST_MARKER.into(), &line[indent..marker_end]);
        let content_start = marker_end + marker.spaces_after_bytes;
        builder.token(K::WHITESPACE.into(), &line[marker_end..content_start]);
        builder.start_node(K::PLAIN.into());
        let content_col = marker_end + marker.spaces_after_cols;
        let paragraph_end = item
            .restricted_markers
            .first()
            .copied()
            .unwrap_or(item.lines.end);
        if paragraph_end == item.lines.start + 1 {
            emit_inlines(builder, &line[content_start..], options, false);
        } else {
            let mut paragraph = ParagraphBuffer::new();
            paragraph.push_text(&line[content_start..]);
            for line in &lines[item.lines.start + 1..paragraph_end] {
                let indent = byte_index_at_column(line, content_col);
                paragraph.push_indent(&line[..indent]);
                paragraph.push_text(&line[indent..]);
            }
            paragraph.emit_with_inlines(builder, options, false);
        }
        builder.finish_node();
        for pos in item.restricted_markers {
            builder.start_node(K::PLAIN.into());
            emit_inlines(builder, lines[pos], options, false);
            builder.finish_node();
        }
        end = item.lines.end;
        if !item.nested.is_empty() {
            end = emit_list(builder, lines, item.nested, content_col, options);
        }
        builder.finish_node();
    }
    builder.finish_node();
    end
}

fn emit_quote(builder: &mut GreenNodeBuilder<'static>, lines: &[&str], options: &ParserOptions) {
    builder.start_node(K::BLOCK_QUOTE.into());
    let mut paragraph = ParagraphBuffer::new();
    for &line in lines {
        let marker = try_parse_blockquote_marker(line);
        let content = marker.map_or(line, |(_, tail)| &line[tail..]);
        if is_blank_line(content) && !paragraph.is_empty() {
            paragraph.emit_with_inlines(builder, options, false);
            paragraph.clear();
            builder.finish_node();
        }
        let open = !paragraph.is_empty();
        if let Some((marker_end, tail)) = marker {
            if open {
                paragraph.push_marker(marker_end - 1, tail > marker_end);
            } else {
                emit_one_blockquote_marker(builder, marker_end - 1, tail > marker_end);
            }
        }
        if is_blank_line(content) {
            emit_blank(builder, content);
        } else {
            if !open {
                builder.start_node(K::PARAGRAPH.into());
            }
            paragraph.push_text(content);
        }
    }
    if !paragraph.is_empty() {
        paragraph.emit_with_inlines(builder, options, false);
        builder.finish_node();
    }
    builder.finish_node();
}

/// Splice an edit into a tree produced by the experimental engine.
///
/// Keep the parse options and cached reference labels from the previous
/// revision. A decline requires a new full parse and reference-label scan.
pub fn reparse(
    previous: &GreenNode,
    errors: &[SyntaxError],
    edit: &Edit,
    text: &str,
    options: &ParserOptions,
) -> Option<Reparsed> {
    reparse_using(
        previous,
        errors,
        edit,
        text,
        options,
        CostGuards::Enforced,
        &parse_fragment,
    )
}

fn parse_fragment(
    text: &str,
    options: &ParserOptions,
    origin: ParseOrigin,
) -> Option<(SyntaxNode, Vec<SyntaxError>)> {
    parse_at(text, options, origin).ok()
}

/// Compare fragment work outside timed benchmark samples. The count includes
/// failed probes and excludes the full parse used by the debug oracle.
#[doc(hidden)]
pub fn measure_fragment_work(
    previous: &GreenNode,
    errors: &[SyntaxError],
    edit: &Edit,
    text: &str,
    options: &ParserOptions,
    experimental: bool,
) -> (Option<Reparsed>, usize) {
    let bytes = std::cell::Cell::new(0usize);
    let parse = |fragment: &str, options: &ParserOptions, origin| {
        bytes.set(bytes.get() + fragment.len());
        if experimental {
            parse_fragment(fragment, options, origin)
        } else {
            super::reparse::parse_current(fragment, options, origin)
        }
    };
    let result = reparse_using(
        previous,
        errors,
        edit,
        text,
        options,
        CostGuards::Enforced,
        &parse,
    );
    let oracle_bytes = if cfg!(debug_assertions) && result.is_some() {
        text.len()
    } else {
        0
    };
    (result, bytes.get() - oracle_bytes)
}
