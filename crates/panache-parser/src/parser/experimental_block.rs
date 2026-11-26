//! Experimental block planning followed by lossless CST emission.
//!
//! The implementation covers a subset of the CommonMark and Quarto flavors
//! with resolved extension settings. Compare the result with the incumbent parser before using a
//! document as a benchmark workload.

mod containers;
mod html;

use std::{borrow::Cow, cell::RefCell, ops::Range, sync::Arc};

use rowan::{GreenNode, GreenNodeBuilder};

use crate::options::{Dialect, Flavor, ParserOptions};
use crate::syntax::{SyntaxKind as K, SyntaxNode};

use super::block_dispatcher::{
    emit_div_fence_close, emit_div_fence_open, emit_reference_definition_lines, fence_enabled,
};
use super::blocks::code_blocks::{
    FenceInfo, is_closing_fence, is_gfm_math_fence, parse_fenced_code_block, try_parse_fence_open,
};
use super::blocks::container_prefix::{ContainerPrefix, StrippedLines};
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
use super::blocks::lists::{ListMarker, OpenListHint, marker_start_number, try_parse_list_marker};
use super::blocks::metadata::{
    YamlContentOutcome, collect_yaml_content, emit_yaml_block, find_yaml_block_closing_pos,
    is_metadata_open_delim, prepare_yaml_content,
};
use super::blocks::reference_links::{ReferenceSpans, reference_definition_spans};
use super::blocks::tables::{
    DashTableKind, PreparedDashTable, PreparedGridTable, PreparedPipeTable, is_table_caption_start,
    prepare_dash_table_at, prepare_grid_table, prepare_pipe_table,
};
use super::core::ParseOrigin;
use super::diagnostics::Diagnostics;
use super::utils::container_stack::leading_indent;
use super::utils::helpers::{is_blank_line, split_lines_inclusive};
use super::utils::inline_emission::emit_inlines;
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
    Paragraph(Range<usize>, K),
    Atx(usize, usize),
    Setext(Range<usize>, usize),
    Rule(usize),
    Reference(Range<usize>, ReferenceSpans),
    GridTable(usize, PreparedGridTable),
    DashTable(usize, PreparedDashTable),
    PipeTable(usize, PreparedPipeTable),
    Containers(usize, Vec<containers::Event<'a>>),
    IndentedCode(Range<usize>),
    Html(Range<usize>, HtmlBlockType),
    PandocHtml(html::PreparedHtml),
    Code(Range<usize>, FenceInfo),
    Metadata(usize, usize, YamlContentOutcome),
    OpenDiv(usize, DivFenceInfo),
    CloseDiv(Option<usize>),
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
    let (columns, indent) = leading_indent(text);
    let content = &text[indent..];
    if options.dialect == Dialect::CommonMark {
        return columns < 4
            && !content.starts_with('>')
            && try_parse_atx_heading(text).is_none()
            && try_parse_horizontal_rule(text).is_none()
            && !try_parse_fence_open(text, options.dialect)
                .is_some_and(|fence| fence_enabled(&fence, options))
            && (!options.extensions.raw_html
                || try_parse_html_block_start(text, true)
                    .is_none_or(|kind| matches!(kind, HtmlBlockType::Type7)))
            && try_parse_list_marker(text, options, OpenListHint::None).is_none()
            && !(options.extensions.reference_links
                && content.starts_with('[')
                && reference_definition_spans(text, true, options.dialect, false).is_some());
    }
    columns < 4
        && plain_container_text(content)
        && try_parse_list_marker(text, options, OpenListHint::None).is_none()
        && !(content.starts_with('[') && content.contains("]:"))
}

fn source_span<'a>(input: &'a str, lines: &[&str], range: Range<usize>) -> &'a str {
    let first = lines[range.start].as_ptr() as usize - input.as_ptr() as usize;
    let last = lines[range.end - 1];
    let end = last.as_ptr() as usize - input.as_ptr() as usize + last.len();
    &input[first..end]
}

fn push_paragraph(blocks: &mut Vec<Block<'_>>, pos: usize) {
    if let Some(Block::Paragraph(range, _)) = blocks.last_mut() {
        range.end = pos + 1;
    } else {
        blocks.push(Block::Paragraph(pos..pos + 1, K::PARAGRAPH));
    }
}

fn plan<'a>(
    input: &str,
    lines: &'a [&'a str],
    options: &ParserOptions,
    origin: ParseOrigin,
    jupyter_lines: &RefCell<super::blocks::jupyter::LookaheadCache>,
) -> Result<Vec<Block<'a>>, Unsupported> {
    let prefix = ContainerPrefix::from_ops(&[], false);
    let mut blocks = Vec::new();
    let mut divs = Vec::new();
    let mut pos = 0;
    let commonmark = options.dialect == Dialect::CommonMark;
    while let Some(&line) = lines.get(pos) {
        let paragraph = matches!(blocks.last(), Some(Block::Paragraph(..)));
        let fresh = !paragraph;
        let at_start = pos == 0 && origin == ParseOrigin::Document;
        let (indent, indent_bytes) = leading_indent(line);
        let content = &line[indent_bytes..];
        let marker = content.as_bytes().first().copied();
        let offset = line.as_ptr() as usize - input.as_ptr() as usize;
        let unsupported = |construct| Unsupported { offset, construct };
        let div_prefix = divs
            .last()
            .map(|&indent| prefix.clone().with_div_closer_boundary(Some(indent), true));
        let table_prefix = div_prefix.as_ref().unwrap_or(&prefix);
        let dash_candidate = || {
            (content.starts_with('-')
                || lines
                    .get(pos + 1)
                    .is_some_and(|next| next.trim_start().starts_with('-'))
                || (options.extensions.table_captions && is_table_caption_start(line)))
                && (!options.extensions.raw_html
                    || marker != Some(b'<')
                    || try_parse_html_block_start(line, commonmark).is_none())
                && try_parse_list_marker(line, options, OpenListHint::None).is_none()
        };

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
        } else if fresh
            && options.extensions.fenced_divs
            && indent < 4
            && marker == Some(b':')
            && let Some(fence) = try_parse_div_fence_open(line)
        {
            divs.push(indent);
            blocks.push(Block::OpenDiv(pos, fence));
        } else if matches!(marker, Some(b'=' | b'-'))
            && let Some(Block::Paragraph(range, _)) = blocks.last_mut()
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
        } else if !commonmark
            && options.extensions.raw_html
            && marker == Some(b'<')
            && let Some(kind) = html::detect(line, fresh)
            && let Some(block) = html::prepare(
                StrippedLines::new(lines, pos, &prefix),
                kind,
                options,
                if divs.is_empty() {
                    SoftbreakFusion::ToDocEnd
                } else {
                    SoftbreakFusion::ToFencedDivClose
                },
                None,
            )
        {
            if let Some(Block::Paragraph(_, kind)) = blocks.last_mut() {
                *kind = K::PLAIN;
            }
            pos = block.end;
            blocks.push(Block::PandocHtml(block));
            continue;
        } else if fresh
            && options.extensions.grid_tables
            && (line.starts_with('+')
                || (options.extensions.table_captions && is_table_caption_start(line)))
            && let Some(table) = prepare_grid_table(&StrippedLines::new(lines, pos, table_prefix))
        {
            let consumed = table.lines_consumed;
            blocks.push(Block::GridTable(pos, table));
            pos += consumed;
            continue;
        } else if fresh
            && options.extensions.multiline_tables
            && dash_candidate()
            && let Some(table) = prepare_dash_table_at(
                &StrippedLines::new(lines, pos, table_prefix),
                options,
                DashTableKind::Multiline,
            )
        {
            let consumed = table.lines_consumed;
            blocks.push(Block::DashTable(pos, table));
            pos += consumed;
            continue;
        } else if fresh
            && indent < 4
            && options.extensions.pipe_tables
            && (content.contains('|') || is_table_caption_start(line))
            && (commonmark
                || !matches!(marker, Some(b'-' | b'+' | b'*'))
                || !try_parse_list_marker(line, options, OpenListHint::None)
                    .is_some_and(|marker| matches!(marker.marker, ListMarker::Bullet(_))))
            && let Some(table) = prepare_pipe_table(
                &StrippedLines::new(lines, pos, &prefix).with_jupyter_lookahead(jupyter_lines),
                options,
            )
        {
            let consumed = table.lines_consumed;
            blocks.push(Block::PipeTable(pos, table));
            pos += consumed;
            continue;
        } else if fresh
            && options.extensions.simple_tables
            && dash_candidate()
            && let Some(table) = prepare_dash_table_at(
                &StrippedLines::new(lines, pos, table_prefix),
                options,
                DashTableKind::Simple,
            )
        {
            let consumed = table.lines_consumed;
            blocks.push(Block::DashTable(pos, table));
            pos += consumed;
            continue;
        } else if indent < 4
            && matches!(marker, Some(b'*' | b'-' | b'_'))
            && (commonmark || fresh)
            && try_parse_horizontal_rule(line).is_some()
        {
            blocks.push(Block::Rule(pos));
        } else if indent >= 4 && fresh && !line.trim().is_empty() {
            let mut end = pos + 1;
            let mut cursor = end;
            while let Some(line) = lines.get(cursor) {
                if !line.trim().is_empty() {
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
        } else if indent < 4
            && marker == Some(b'>')
            && (fresh || !options.extensions.blank_before_blockquote)
        {
            plan_containers(
                input,
                lines,
                &mut pos,
                options,
                jupyter_lines,
                divs.last().copied(),
                &mut blocks,
            )?;
            continue;
        } else if indent < 4
            && (fresh || commonmark)
            && (matches!(marker, Some(b'-' | b'+' | b'*' | b'0'..=b'9'))
                || options.extensions.fancy_lists
                    && matches!(marker, Some(b'#' | b'(' | b'a'..=b'z' | b'A'..=b'Z'))
                || options.extensions.example_lists && matches!(marker, Some(b'(' | b'@')))
            && let Some(first) = try_parse_list_marker(line, options, OpenListHint::None)
            && (fresh
                || !commonmark
                || (marker_start_number(&first.marker).is_none_or(|start| start == 1)
                    && !is_blank_line(&line[indent_bytes + first.marker_len..])))
        {
            plan_containers(
                input,
                lines,
                &mut pos,
                options,
                jupyter_lines,
                divs.last().copied(),
                &mut blocks,
            )?;
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
                    if ends_at_blank_line(&kind) && line.trim().is_empty() {
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
        } else if options.extensions.line_blocks
            && fresh
            && marker == Some(b'|')
            && try_parse_line_block_start(line).is_some()
        {
            return Err(unsupported("line block"));
        } else if !commonmark
            && ((options.extensions.pandoc_title_block && marker == Some(b'%'))
                || (options.extensions.raw_tex && marker == Some(b'\\'))
                || (options.extensions.tex_math_dollars && content.starts_with("$$")))
        {
            return Err(unsupported("extended Pandoc block"));
        } else if !commonmark
            && fresh
            && marker == Some(b'[')
            && containers::footnote_marker(line, options).is_some()
            && lines
                .get(pos + 1)
                .is_some_and(|line| leading_indent(line).0 < 4)
            && try_parse_setext_heading(lines, pos).is_some()
        {
            blocks.push(Block::Setext(pos..pos + 1, pos + 1));
            pos += 2;
            continue;
        } else if !commonmark
            && (containers::footnote_marker(line, options).is_some()
                || (fresh && containers::root_definition_term(lines, pos, options)))
        {
            plan_containers(
                input,
                lines,
                &mut pos,
                options,
                jupyter_lines,
                divs.last().copied(),
                &mut blocks,
            )?;
            continue;
        } else {
            push_paragraph(&mut blocks, pos);
        }
        pos += 1;
    }
    blocks.extend(divs.into_iter().map(|_| Block::CloseDiv(None)));
    Ok(blocks)
}

fn plan_containers<'a>(
    input: &str,
    lines: &'a [&'a str],
    pos: &mut usize,
    options: &ParserOptions,
    jupyter_lines: &RefCell<super::blocks::jupyter::LookaheadCache>,
    outer_div_indent: Option<usize>,
    blocks: &mut Vec<Block<'a>>,
) -> Result<(), Unsupported> {
    let (consumed, events) =
        containers::plan(&lines[*pos..], options, jupyter_lines, outer_div_indent).map_err(
            |(unhandled, construct)| Unsupported {
                offset: unhandled.as_ptr() as usize - input.as_ptr() as usize,
                construct,
            },
        )?;
    blocks.push(Block::Containers(*pos, events));
    *pos += consumed;
    Ok(())
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
    let jupyter_lines = RefCell::default();
    let blocks = plan(input, &lines, options, origin, &jupyter_lines)?;
    let mut emission_options = Cow::Borrowed(options);
    if options.dialect == Dialect::CommonMark {
        for block in &blocks {
            let Block::Containers(_, events) = block else {
                continue;
            };
            for event in events {
                let containers::Event::Reference(_, reference) = event else {
                    continue;
                };
                let spans = &reference.spans;
                let label = super::inlines::refdef_map::normalize_label(
                    &reference.source[spans.indent + 1..spans.label_close],
                );
                if !emission_options
                    .refdef_labels
                    .as_ref()
                    .is_some_and(|labels| labels.contains(&label))
                {
                    let labels = emission_options
                        .to_mut()
                        .refdef_labels
                        .get_or_insert_with(Default::default);
                    Arc::make_mut(labels).insert(label);
                }
            }
        }
    }
    let options = emission_options.as_ref();
    let mut builder = GreenNodeBuilder::new();
    let diagnostics = Diagnostics::new();
    let prefix = ContainerPrefix::default();
    builder.start_node(K::DOCUMENT.into());
    for block in blocks {
        match block {
            Block::Blank(pos) => {
                emit_blank(&mut builder, lines[pos]);
            }
            Block::Paragraph(range, kind) => {
                let text = source_span(input, &lines, range);
                let kind = if kind == K::PARAGRAPH && paragraph_is_standalone_image(text, options) {
                    K::FIGURE
                } else {
                    kind
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
            Block::GridTable(pos, table) => {
                let window = StrippedLines::new(&lines, pos, &prefix);
                table.emit(&mut builder, &window, options);
            }
            Block::DashTable(pos, table) => {
                let window = StrippedLines::new(&lines, pos, &prefix);
                table.emit(&mut builder, &window, options);
            }
            Block::PipeTable(pos, table) => {
                let window = StrippedLines::new(&lines, pos, &prefix);
                table.emit(&mut builder, &window, options);
            }
            Block::Containers(start, events) => {
                containers::emit(&mut builder, events, &lines, start, &diagnostics, options)
            }
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
            Block::PandocHtml(block) => block.emit(&mut builder),
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

fn emit_buffered_paragraph(
    builder: &mut GreenNodeBuilder<'static>,
    kind: K,
    paragraph: &ParagraphBuffer,
    options: &ParserOptions,
    suppress_footnote_refs: bool,
) {
    let kind = if options.extensions.implicit_figures
        && paragraph_is_standalone_image(&paragraph.get_text_for_parsing(), options)
    {
        K::FIGURE
    } else {
        kind
    };
    builder.start_node(kind.into());
    paragraph.emit_with_inlines(builder, options, suppress_footnote_refs);
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
