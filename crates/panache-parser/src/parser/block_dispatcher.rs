//! Block parser dispatcher for organizing block-level parsing.
//!
//! This module provides a trait-based abstraction for block parsers,
//! making it easier to add new block types and reducing duplication in parse_inner_content.
//!
//! Design principles:
//! - Single-pass parsing preserved (no backtracking)
//! - Each block parser operates independently
//! - Inline parsing still integrated (called from within block parsing)
//! - Maintains exact CST structure and losslessness

use crate::options::{Dialect, ParserOptions};
use rowan::GreenNodeBuilder;
use std::any::Any;
use std::sync::OnceLock;

use super::diagnostics::Diagnostics;

use super::blocks::admonitions::{AdmonitionOpen, try_parse_admonition_open};
use super::blocks::blockquotes::{
    can_start_blockquote, count_blockquote_markers, emit_one_blockquote_marker,
    strip_n_blockquote_markers,
};
use super::blocks::code_blocks::{
    CodeBlockType, ContainerExitScan, FenceInfo, InfoString, is_closing_fence, is_gfm_math_fence,
    parse_fenced_code_block, parse_fenced_math_block, try_parse_fence_open,
};
use super::blocks::container_prefix::{
    ContainerPrefix, StrippedLines, bq_outer_of_list, strip_list_indent,
};
use super::blocks::definition_lists::{
    definition_marker_in_list_frame, next_line_is_definition_marker,
};
use super::blocks::fenced_divs::{DivFenceInfo, is_div_closing_fence, try_parse_div_fence_open};
use super::blocks::headings::{
    emit_atx_heading, emit_setext_heading_text, emit_setext_underline, try_parse_atx_heading,
    try_parse_setext_heading,
};
use super::blocks::horizontal_rules::{emit_horizontal_rule, try_parse_horizontal_rule};
use super::blocks::html_blocks::{
    HtmlBlockType, SoftbreakFusion, is_pandoc_inline_block_tag_name, is_pandoc_void_block_tag_name,
    pandoc_html_block_start_is_valid, pandoc_html_open_tag_closes, parse_html_block_with_wrapper,
    probe_open_tag_line_has_close_gt, try_parse_html_block_start,
};
use super::blocks::indented_code::{is_indented_code_line, parse_indented_code_block};
use super::blocks::latex_envs::LatexEnvInfo;
use super::blocks::line_blocks::{parse_line_block, try_parse_line_block_start};
use super::blocks::lists::{
    ListDelimiter, ListMarker, OrderedMarker, is_content_nested_bullet_marker,
    try_parse_list_marker,
};
use super::blocks::metadata::{
    YamlContentOutcome, collect_yaml_content, emit_yaml_block, find_yaml_block_closing_pos,
    is_metadata_open_delim, prepare_yaml_content, try_parse_mmd_title_block,
    try_parse_pandoc_title_block, try_parse_yaml_block,
};
use super::blocks::myst_directives::{
    DirectiveOpen, DirectiveOption, is_directive_closing_fence, try_parse_directive_open,
    try_parse_directive_option,
};
use super::blocks::myst_targets::{
    BlockBreak, Target, is_comment_line, try_parse_block_break, try_parse_target,
};
use super::blocks::raw_blocks;
use super::blocks::raw_blocks::extract_environment_name;
use super::blocks::reference_links::{
    ReferenceSpans, line_is_mmd_link_attribute_continuation, reference_definition_spans,
    try_parse_footnote_marker, try_parse_reference_definition_with_options,
};
use super::blocks::tables::{
    is_caption_followed_by_table, try_parse_grid_table, try_parse_multiline_table,
    try_parse_pipe_table, try_parse_simple_table,
};
use super::inlines::svelte::{SvelteKind, emit_svelte_template, try_parse_svelte_template};
use super::utils::attributes::{emit_div_info_node, parse_html_tag_attributes};
use super::utils::container_stack::{byte_index_at_column, leading_indent};
use super::utils::helpers::{strip_newline, trim_end_newlines};
use super::utils::marker_utils::parse_blockquote_marker_info;
use super::utils::tree_copy::copy_green_node;

/// Information about list indentation context.
///
/// Used by block parsers that need to handle indentation stripping
/// when parsing inside list items (e.g., fenced code blocks).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ListIndentInfo {
    /// Number of columns to strip for list content
    pub content_col: usize,
}

/// Context passed to block parsers for decision-making.
///
/// Contains immutable references to parser state that block parsers need
/// to check conditions (e.g., blank line before, blockquote depth, etc.).
pub(crate) struct BlockContext<'a> {
    /// Whether there was a blank line before this line (relaxed, container-aware)
    pub has_blank_before: bool,

    /// Whether there was a strict blank line before this line (no container exceptions)
    pub has_blank_before_strict: bool,

    /// Whether we're currently inside a fenced div (container-owned state)
    pub in_fenced_div: bool,

    /// Indent (columns) of the innermost open fenced div's opening fence, in the
    /// container-prefix-stripped frame. `None` when not inside a fenced div. A
    /// closer more indented than this is not the div's closer at the top level
    /// (pandoc rule); see `FencedDivCloseParser::detect_prepared`.
    pub fenced_div_open_indent: Option<usize>,

    /// Expected closer of the innermost open MyST directive, as
    /// `(fence_char, min_count)`. `None` when not inside a directive. Lets
    /// `MystDirectiveCloseParser` match a closing fence against the opener.
    pub myst_directive_closer: Option<(u8, usize)>,

    /// Whether we're at document start (pos == 0)
    pub at_document_start: bool,

    /// Current blockquote depth
    pub blockquote_depth: usize,

    /// Parser configuration
    pub config: &'a ParserOptions,

    /// Sink for embedded-sublanguage syntax errors (malformed YAML). An owned
    /// `Rc`-backed clone, so it threads here without borrowing `self` (which
    /// would clash with the `&mut GreenNodeBuilder` held during emission).
    pub diags: Diagnostics,

    /// Base indentation from container context (footnotes, definitions)
    pub content_indent: usize,

    /// Indentation stripped from the current line that should be emitted for losslessness
    pub indent_to_emit: Option<&'a str>,

    /// List indentation info if inside a list
    pub list_indent_info: Option<ListIndentInfo>,

    /// Whether we're currently inside any list
    pub in_list: bool,

    /// Whether we're currently inside a definition list. A definition marker
    /// (`:`/`~`) only opens a `Definition` when a preceding term already
    /// established the list; without one, `DefinitionListParser::detect_prepared`
    /// falls through to the term check (pandoc `: foo` / `: bar`) or leaves the
    /// line as a paragraph. Precomputed from the container stack (which is
    /// intentionally not threaded through `BlockContext`).
    pub in_definition_list: bool,

    /// Whether the innermost open fenced div wraps the innermost open list (the
    /// div is outer to the list). When true, a `:::` at the list content column
    /// is list content rather than the div's closer. See issue #439 and
    /// `FencedDivCloseParser::detect_prepared`.
    pub fenced_div_wraps_list: bool,

    /// Whether the immediate enclosing container is a list item that has so
    /// far seen only its marker (no content yet). Equivalent to the
    /// `marker_only` flag on `Container::ListItem`. Used by indented code
    /// detection so that the line *after* an empty list marker can still
    /// open an indented code block when its indent is ≥ content_col + 4,
    /// even though there is no blank line separating the marker line from
    /// the indented line.
    pub in_marker_only_list_item: bool,

    /// If the immediate enclosing `Container::ListItem`'s buffer starts
    /// with a Pandoc matched-pair HTML open tag (e.g. `<div>`,
    /// `<section>`, `<pre>`) whose opens outnumber its closes, this is
    /// the (lowercase) tag name. Used by `HtmlBlockParser::detect_prepared`
    /// to suppress the close-form dispatch (`</div>` etc.) that would
    /// otherwise interrupt the buffer mid-construct — letting the buffer
    /// accumulate the full matched-pair text so the emit-time structural
    /// lift in `ListItemBuffer::emit_as_block` produces a single lifted
    /// HTML block as the list item's content.
    pub list_item_unclosed_html_block_tag: Option<String>,

    /// Whether a `Container::Paragraph` is currently open and buffering
    /// content. When `true`, the *previous* source line was buffered as
    /// paragraph text — even if its shape would have been a heading or HR
    /// in isolation — so paragraph-non-interrupting blocks (notably
    /// indented code under Pandoc) must treat it as paragraph continuation,
    /// not as a "terminal one-liner" that opens a new section.
    pub paragraph_open: bool,

    /// Whether the innermost container is a `Container::ListItem` whose content
    /// is still buffered. That buffer holds bytes not yet written to the green
    /// builder, so it is the analogue of an open paragraph: detectors that must
    /// not emit a sibling block ahead of already-consumed text have to consult
    /// this alongside [`Self::paragraph_open`].
    pub list_item_content_open: bool,

    /// Backtick runs buffered into the innermost open paragraph (or list item)
    /// that are still waiting for a closer.
    ///
    /// Pandoc only reaches `endline`, where a block start may interrupt a
    /// paragraph, between inlines — never from inside a code span. A line that
    /// closes one of these runs is therefore code-span content, not a block
    /// start, which is what keeps ```` b ```r\nc\n``` ```` a single
    /// `Para [Str "b", Code "r c"]`. See
    /// [`pending_code_span_openers`](crate::parser::inlines::code_spans::pending_code_span_openers).
    ///
    /// Empty unless the current line could actually close a run (it opens with
    /// a backtick once the container prefix is stripped), since filling it
    /// costs a scan of the whole buffer.
    pub open_code_span_openers: Vec<usize>,

    /// Next line content for lookahead (used by setext headings)
    pub next_line: Option<&'a str>,

    /// Open-alpha-at-indent hint for `ListParser::detect_prepared`.
    /// Precomputed by the parser core from `self.containers` (which is
    /// intentionally not threaded through `BlockContext` — see the note
    /// above). Lets marker detection resolve single-letter Roman
    /// candidates {i,v,x,I,V,X} against an open alpha list in a single
    /// classification pass under Pandoc dialect.
    pub open_alpha_hint: super::blocks::lists::OpenListHint,

    /// Whether the current line's ordered marker would open a *new* sublist
    /// inside an enclosing list item or definition body while declaring a
    /// start number other than 1.
    ///
    /// Pandoc 3.10.1 stopped recognizing those as lists (jgm/pandoc#11735);
    /// under `PandocCompat::V3_10` they stay paragraph text. Precomputed by
    /// the parser core because answering it needs the container stack (which
    /// is intentionally not threaded through `BlockContext` — see the note
    /// above) to tell a genuinely new sublist from a sibling item continuing
    /// an already-open list, where any start number is still fine.
    pub restricted_ordered_sublist: bool,
}

/// Result of detecting whether a block can be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockDetectionResult {
    /// Can parse this block, requires blank line before
    Yes,

    /// Can parse this block and can interrupt paragraphs (no blank line needed)
    YesCanInterrupt,

    /// Cannot parse this content
    No,
}

/// A prepared (cached) detection result.
///
/// This allows expensive detection logic (e.g., fence parsing) to be performed once,
/// while emission happens only after the caller prepares (flushes buffers/closes paragraphs).
pub(crate) struct PreparedBlockMatch {
    pub parser_index: usize,
    pub detection: BlockDetectionResult,
    pub effect: BlockEffect,
    pub payload: Option<Box<dyn Any>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockEffect {
    None,
    OpenFencedDiv,
    CloseFencedDiv,
    OpenMystDirective,
    CloseMystDirective,
    OpenAdmonition,
    OpenFootnoteDefinition,
    OpenList,
    OpenDefinitionList,
    OpenBlockQuote,
}

/// Trait for block-level parsers.
///
/// Each block type implements this trait with a two-phase approach:
/// 1. Detection: Can this block type parse this content? (lightweight, no emission)
/// 2. Parsing: Actually parse and emit the block to the builder (called after preparation)
///
/// This separation allows the caller to:
/// - Prepare for block elements (close paragraphs, flush buffers) BEFORE emission
/// - Handle blocks that can interrupt paragraphs vs those that need blank lines
/// - Maintain correct CST node ordering
///
/// Note: This is purely organizational - the trait doesn't introduce
/// backtracking or multiple passes. Each parser operates during the
/// single forward pass through the document.
pub(crate) trait BlockParser: Send + Sync {
    fn effect(&self) -> BlockEffect {
        BlockEffect::None
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)>;

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize;

    fn name(&self) -> &'static str;
}

fn emit_content_indent(builder: &mut GreenNodeBuilder<'static>, ctx: &BlockContext) {
    if let Some(indent_str) = ctx.indent_to_emit {
        builder.token(crate::syntax::SyntaxKind::WHITESPACE.into(), indent_str);
    }
}

pub(crate) struct HorizontalRuleParser;

impl BlockParser for HorizontalRuleParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let common_mark = ctx.config.dialect == crate::options::Dialect::CommonMark;
        if !common_mark && !ctx.has_blank_before {
            return None;
        }

        if try_parse_horizontal_rule(lines.first()).is_some() {
            let detection = if common_mark {
                BlockDetectionResult::YesCanInterrupt
            } else {
                BlockDetectionResult::Yes
            };
            Some((detection, None))
        } else {
            None
        }
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        _payload: Option<&dyn Any>,
    ) -> usize {
        emit_content_indent(builder, ctx);
        emit_horizontal_rule(builder, lines.first());
        1 // Consumed 1 line
    }

    fn name(&self) -> &'static str {
        "horizontal_rule"
    }
}

/// ATX heading parser (# Heading)
pub(crate) struct AtxHeadingParser;

struct AtxHeadingPrepared {
    level: usize,
    shortcode: Option<super::blocks::hugo::PreparedShortcode>,
}

impl BlockParser for AtxHeadingParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if ctx.config.extensions.blank_before_header && !ctx.has_blank_before {
            return None;
        }

        let level = try_parse_atx_heading(lines.first())?;
        let shortcode = if ctx.config.extensions.hugo_shortcodes {
            super::blocks::hugo::prepare(lines, true, None)
        } else {
            None
        };
        let detection = if ctx.config.extensions.blank_before_header {
            BlockDetectionResult::Yes
        } else {
            BlockDetectionResult::YesCanInterrupt
        };
        Some((
            detection,
            Some(Box::new(AtxHeadingPrepared { level, shortcode })),
        ))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        let content = lines.first();
        let prepared = payload
            .and_then(|p| p.downcast_ref::<AtxHeadingPrepared>())
            .expect("ATX detection carries its heading and continuations");
        if let Some(shortcode) = &prepared.shortcode {
            shortcode.emit_heading(builder, prepared.level, ctx.config);
            return shortcode.line_count;
        }
        emit_content_indent(builder, ctx);
        emit_atx_heading(builder, content, prepared.level, ctx.config);
        1
    }

    fn name(&self) -> &'static str {
        "atx_heading"
    }
}

/// Pandoc title block parser (% Title ...)
pub(crate) struct PandocTitleBlockParser;

impl BlockParser for PandocTitleBlockParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let line_pos = lines.pos();
        if !ctx.config.extensions.pandoc_title_block {
            return None;
        }

        if !ctx.at_document_start || line_pos != 0 {
            return None;
        }

        if !lines.first().trim_start().starts_with('%') {
            return None;
        }

        Some((BlockDetectionResult::Yes, None))
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        _payload: Option<&dyn Any>,
    ) -> usize {
        let line_pos = lines.pos();
        let lines = lines.raw();
        let new_pos =
            try_parse_pandoc_title_block(lines, line_pos, builder).unwrap_or(line_pos + 1);
        new_pos - line_pos
    }

    fn name(&self) -> &'static str {
        "pandoc_title_block"
    }
}

/// MultiMarkdown title block parser (Key: Value ...)
pub(crate) struct MmdTitleBlockParser;

impl BlockParser for MmdTitleBlockParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let line_pos = lines.pos();
        if !ctx.config.extensions.mmd_title_block {
            return None;
        }

        if !ctx.at_document_start || line_pos != 0 || ctx.blockquote_depth > 0 {
            return None;
        }

        let first = lines.first();
        if first.trim().is_empty() || !first.contains(':') {
            return None;
        }

        Some((BlockDetectionResult::Yes, None))
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        _payload: Option<&dyn Any>,
    ) -> usize {
        let line_pos = lines.pos();
        let lines = lines.raw();
        let new_pos = try_parse_mmd_title_block(lines, line_pos, builder).unwrap_or(line_pos + 1);
        new_pos - line_pos
    }

    fn name(&self) -> &'static str {
        "mmd_title_block"
    }
}

/// YAML metadata block parser (--- ... ---/...)
pub(crate) struct YamlMetadataParser;
#[derive(Debug, Clone)]
pub(crate) struct YamlMetadataPrepared {
    pub at_document_start: bool,
    pub closing_pos: usize,
    pub outcome: YamlContentOutcome,
}

impl BlockParser for YamlMetadataParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let stripped = lines;
        let content = stripped.first_unconditional();
        let line_pos = stripped.pos();
        let lines = stripped.raw();
        if !ctx.config.extensions.yaml_metadata_block {
            return None;
        }

        if ctx.blockquote_depth > 0 {
            return None;
        }

        if !is_metadata_open_delim(content) {
            return None;
        }

        if !ctx.has_blank_before && !ctx.at_document_start {
            return None;
        }

        if !ctx.at_document_start && ctx.config.dialect == Dialect::CommonMark {
            return None;
        }

        let next_line = lines.get(line_pos + 1)?;
        if next_line.trim().is_empty() {
            return None;
        }

        let closing_pos =
            find_yaml_block_closing_pos(lines, line_pos, ctx.at_document_start, |i| {
                stripped.detect_at(i)
            })?;

        let content = collect_yaml_content(lines, line_pos, closing_pos);
        let outcome = prepare_yaml_content(&content, ctx.config.flavor)?;

        Some((
            BlockDetectionResult::Yes,
            Some(Box::new(YamlMetadataPrepared {
                at_document_start: ctx.at_document_start,
                closing_pos,
                outcome,
            })),
        ))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        let line_pos = lines.pos();
        let lines = lines.raw();
        if let Some(prepared) = payload.and_then(|p| p.downcast_ref::<YamlMetadataPrepared>())
            && let Some(new_pos) = emit_yaml_block(
                lines,
                line_pos,
                prepared.closing_pos,
                builder,
                &ctx.diags,
                &prepared.outcome,
            )
        {
            return new_pos - line_pos;
        }

        let at_document_start = payload
            .and_then(|p| p.downcast_ref::<YamlMetadataPrepared>())
            .map(|p| p.at_document_start)
            .unwrap_or(ctx.at_document_start);
        try_parse_yaml_block(
            lines,
            line_pos,
            builder,
            at_document_start,
            &ctx.diags,
            ctx.config.flavor,
        )
        .map(|new_pos| new_pos - line_pos)
        .unwrap_or(1)
    }

    fn name(&self) -> &'static str {
        "yaml_metadata"
    }
}

/// Reference definition parser ([label]: url "title")
pub(crate) struct ReferenceDefinitionParser;
#[derive(Debug, Clone, Copy)]
struct ReferenceDefinitionPrepared {
    consumed_lines: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct FootnoteDefinitionPrepared {
    pub content_start: usize,
    /// Byte length of the whitespace preceding `[^` on the marker line, in the
    /// container-prefix-stripped frame. Pandoc's `noteBlock` accepts
    /// `nonindentSpaces` before the marker, and inside a list item it reads the
    /// body from the item's content column, so both show up here.
    pub indent_len: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct BlockQuotePrepared {
    pub depth: usize,
    pub marker_info: Vec<crate::parser::utils::marker_utils::BlockQuoteMarkerInfo>,
    #[allow(dead_code)]
    pub inner_content: String,
    pub can_start: bool,
    pub can_nest: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ListPrepared {
    pub marker: ListMarker,
    pub marker_len: usize,
    pub spaces_after: usize,
    pub spaces_after_cols: usize,
    pub indent_cols: usize,
    pub indent_bytes: usize,
    pub nested_marker: Option<char>,
    pub virtual_marker_space: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum DefinitionPrepared {
    Term {
        blank_count: usize,
    },
    Definition {
        marker_char: char,
        indent: usize,
        spaces_after: usize,
        spaces_after_cols: usize,
        has_content: bool,
    },
}

/// List marker parser
pub(crate) struct ListParser;

/// Definition list parser (term lines and definition markers)
pub(crate) struct DefinitionListParser;

/// Blockquote parser (detection only; core handles emission)
pub(crate) struct BlockQuoteParser;

impl BlockParser for ListParser {
    fn effect(&self) -> BlockEffect {
        BlockEffect::OpenList
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let content = lines.first();
        let marker_match = try_parse_list_marker(content, ctx.config, ctx.open_alpha_hint)?;
        if ctx.restricted_ordered_sublist {
            return None;
        }
        let after_marker_text = {
            let (_, indent_bytes) = super::utils::container_stack::leading_indent(content);
            let marker_end = indent_bytes + marker_match.marker_len;
            if marker_end <= content.len() {
                &content[marker_end..]
            } else {
                ""
            }
        };
        if marker_match.spaces_after_cols == 0 {
            if !trim_end_newlines(after_marker_text).is_empty() {
                return None;
            }
            if !ctx.at_document_start && !ctx.has_blank_before && !ctx.in_list {
                return None;
            }
        }
        if !ctx.has_blank_before
            && ctx.in_list
            && matches!(
                marker_match.marker,
                ListMarker::Ordered(OrderedMarker::Decimal {
                    style: ListDelimiter::RightParen,
                    ..
                })
            )
            && after_marker_text.trim() == ")"
        {
            return None;
        }
        if (ctx.has_blank_before
            || ctx.at_document_start
            || ctx.config.dialect == crate::options::Dialect::CommonMark)
            && try_parse_horizontal_rule(content).is_some()
        {
            return None;
        }
        let (indent_cols, indent_bytes) = super::utils::container_stack::leading_indent(content);

        if indent_cols >= 4 && !ctx.in_list {
            return None;
        }
        if ctx.in_list
            && let Some(list_indent) = ctx.list_indent_info
            && indent_cols >= list_indent.content_col + 4
            && marker_match.spaces_after_cols == 0
            && trim_end_newlines(after_marker_text).is_empty()
        {
            return None;
        }

        if matches!(marker_match.marker, ListMarker::Ordered(_))
            && !ctx.in_list
            && (ctx.has_blank_before || ctx.at_document_start)
        {
            let mut probe = GreenNodeBuilder::new();
            if try_parse_pipe_table(lines, &mut probe, ctx.config).is_some() {
                return None;
            }
        }

        let nested_marker = is_content_nested_bullet_marker(
            content,
            marker_match.marker_len,
            marker_match.spaces_after_bytes,
        );
        let detection = if ctx.has_blank_before || ctx.at_document_start {
            BlockDetectionResult::Yes
        } else {
            BlockDetectionResult::YesCanInterrupt
        };

        Some((
            detection,
            Some(Box::new(ListPrepared {
                marker: marker_match.marker,
                marker_len: marker_match.marker_len,
                spaces_after: marker_match.spaces_after_bytes,
                spaces_after_cols: marker_match.spaces_after_cols,
                indent_cols,
                indent_bytes,
                nested_marker,
                virtual_marker_space: marker_match.virtual_marker_space,
            })),
        ))
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        _builder: &mut GreenNodeBuilder<'static>,
        _lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        let prepared = payload.and_then(|p| p.downcast_ref::<ListPrepared>());
        if prepared.is_none() {
            return 1;
        }

        1
    }

    fn name(&self) -> &'static str {
        "list"
    }
}

impl BlockParser for BlockQuoteParser {
    fn effect(&self) -> BlockEffect {
        BlockEffect::OpenBlockQuote
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let line_pos = lines.pos();
        let lines = lines.raw();
        if ctx.blockquote_depth > 0 {
            return None;
        }

        let line = lines.get(line_pos)?;
        let (depth, inner_content) = count_blockquote_markers(line);
        if depth == 0 {
            return None;
        }

        let marker_info = parse_blockquote_marker_info(line);
        let at_document_start = ctx.at_document_start;
        let require_blank_before = ctx.config.extensions.blank_before_blockquote;
        let can_start = !require_blank_before
            || ctx.has_blank_before
            || can_start_blockquote(line_pos, lines, ctx.config.extensions.fenced_divs);

        let prev_line = lines.get(line_pos.wrapping_sub(1)).unwrap_or(&"");
        let prev_line_blank = prev_line.trim().is_empty();
        let (prev_depth, prev_inner) = count_blockquote_markers(prev_line);
        let prev_line_is_quoted_blank = prev_depth > 0 && prev_inner.trim().is_empty();

        let can_nest = if require_blank_before {
            depth <= 1 || at_document_start || prev_line_blank || prev_line_is_quoted_blank
        } else {
            true
        };

        let has_blank_before = ctx.has_blank_before;
        let detection = if has_blank_before || at_document_start {
            BlockDetectionResult::Yes
        } else {
            BlockDetectionResult::YesCanInterrupt
        };

        Some((
            detection,
            Some(Box::new(BlockQuotePrepared {
                depth,
                marker_info,
                inner_content: inner_content.to_string(),
                can_start,
                can_nest,
            })),
        ))
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        _lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        let prepared = payload.and_then(|p| p.downcast_ref::<BlockQuotePrepared>());
        let Some(prepared) = prepared else {
            return 0;
        };

        let marker_info = &prepared.marker_info;

        for level in 0..prepared.depth {
            builder.start_node(SyntaxKind::BLOCK_QUOTE.into());
            if let Some(info) = marker_info.get(level) {
                emit_one_blockquote_marker(builder, info.leading_spaces, info.has_trailing_space);
            }
        }

        0
    }

    fn name(&self) -> &'static str {
        "blockquote"
    }
}

impl BlockParser for DefinitionListParser {
    fn effect(&self) -> BlockEffect {
        BlockEffect::OpenDefinitionList
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let content = lines.first();
        let line_pos = lines.pos();
        let prefix = lines.prefix();
        let raw = lines.raw();
        if !ctx.config.extensions.definition_lists {
            return None;
        }
        let stripped = StrippedLines::with_dispatch(raw, line_pos, line_pos, prefix);

        if let Some((marker_char, indent, spaces_after_cols, spaces_after_bytes)) =
            definition_marker_in_list_frame(content, ctx.list_indent_info.map(|i| i.content_col))
        {
            if marker_char == ':'
                && ctx.config.extensions.table_captions
                && is_caption_followed_by_table(&stripped, line_pos)
            {
                return None;
            }

            let orphan_guard_applies = !ctx.in_definition_list;
            if !orphan_guard_applies {
                let indent_bytes =
                    super::utils::container_stack::byte_index_at_column(content, indent);
                let has_content = content
                    .get(indent_bytes + 1 + spaces_after_bytes..)
                    .map(|slice| !slice.trim().is_empty())
                    .unwrap_or(false);
                return Some((
                    BlockDetectionResult::YesCanInterrupt,
                    Some(Box::new(DefinitionPrepared::Definition {
                        marker_char,
                        indent,
                        spaces_after: spaces_after_bytes,
                        spaces_after_cols,
                        has_content,
                    })),
                ));
            }
        }

        if !ctx.paragraph_open
            && !ctx.list_item_content_open
            && let Some(blank_count) = next_line_is_definition_marker(&stripped, line_pos)
            && !content.trim().is_empty()
        {
            return Some((
                BlockDetectionResult::YesCanInterrupt,
                Some(Box::new(DefinitionPrepared::Term { blank_count })),
            ));
        }

        None
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        _builder: &mut GreenNodeBuilder<'static>,
        _lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        let prepared = payload.and_then(|p| p.downcast_ref::<DefinitionPrepared>());
        if prepared.is_none() {
            return 1;
        }

        1
    }

    fn name(&self) -> &'static str {
        "definition_list"
    }
}

/// Byte length of the whitespace a footnote marker may sit behind on `line`,
/// or `None` when the line is indented past what `noteBlock` accepts.
///
/// Two indents stack: the enclosing list item's content column (pandoc reparses
/// item contents from there) and `nonindentSpaces` — at most 3 further spaces —
/// before the marker itself. Four would be an indented code block, which the
/// registry reaches first.
fn footnote_marker_indent_len(ctx: &BlockContext, line: &str) -> Option<usize> {
    let base = match ctx.list_indent_info {
        Some(list_info) if leading_indent(line).0 >= list_info.content_col => {
            byte_index_at_column(line, list_info.content_col)
        }
        _ => 0,
    };

    let extra = line[base..]
        .bytes()
        .take_while(|byte| *byte == b' ')
        .count();
    if extra > 3 {
        return None;
    }
    Some(base + extra)
}

/// Footnote definition parser ([^id]: content)
pub(crate) struct FootnoteDefinitionParser;

impl BlockParser for FootnoteDefinitionParser {
    fn effect(&self) -> BlockEffect {
        BlockEffect::OpenFootnoteDefinition
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.footnotes {
            return None;
        }

        let line = lines.first();
        let indent_len = footnote_marker_indent_len(ctx, line)?;
        let content = &line[indent_len..];
        if !content.starts_with("[^") {
            return None;
        }

        let (_id, content_start) = try_parse_footnote_marker(content)?;
        Some((
            BlockDetectionResult::YesCanInterrupt,
            Some(Box::new(FootnoteDefinitionPrepared {
                content_start: indent_len + content_start,
                indent_len,
            })),
        ))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        let content = lines.first();
        let prepared = payload.and_then(|p| p.downcast_ref::<FootnoteDefinitionPrepared>());
        let (content_start, indent_len) = match prepared {
            Some(prepared) => (Some(prepared.content_start), prepared.indent_len),
            None => {
                let indent_len = footnote_marker_indent_len(ctx, content).unwrap_or(0);
                let start = try_parse_footnote_marker(&content[indent_len..])
                    .map(|(_, pos)| indent_len + pos);
                (start, indent_len)
            }
        };

        let Some(content_start) = content_start else {
            return 1;
        };

        if let Some(indent_str) = ctx.indent_to_emit {
            builder.token(SyntaxKind::WHITESPACE.into(), indent_str);
        }

        builder.start_node(SyntaxKind::FOOTNOTE_DEFINITION.into());
        if indent_len > 0 {
            builder.token(SyntaxKind::WHITESPACE.into(), &content[..indent_len]);
        }
        let marker_text = &content[indent_len..content_start];
        if let Some((id, _)) = try_parse_footnote_marker(marker_text) {
            builder.token(SyntaxKind::FOOTNOTE_LABEL_START.into(), "[^");
            builder.token(SyntaxKind::FOOTNOTE_LABEL_ID.into(), &id);
            builder.token(SyntaxKind::FOOTNOTE_LABEL_END.into(), "]");
            builder.token(SyntaxKind::FOOTNOTE_LABEL_COLON.into(), ":");
            let marker_suffix = marker_text
                .strip_prefix("[^")
                .and_then(|tail| tail.strip_prefix(id.as_str()))
                .and_then(|tail| tail.strip_prefix("]:"))
                .unwrap_or("");
            if !marker_suffix.is_empty() {
                builder.token(SyntaxKind::WHITESPACE.into(), marker_suffix);
            }
        } else {
            builder.token(SyntaxKind::FOOTNOTE_REFERENCE.into(), marker_text);
        }

        1
    }

    fn name(&self) -> &'static str {
        "footnote_definition"
    }
}

impl BlockParser for ReferenceDefinitionParser {
    fn effect(&self) -> BlockEffect {
        BlockEffect::None
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let content = lines.first();
        let line_pos = lines.pos();
        let lines = lines.raw();
        if !ctx.config.extensions.reference_links {
            return None;
        }

        {
            let bytes = content.as_bytes();
            let mut i = 0;
            while i < bytes.len() && i < 3 && bytes[i] == b' ' {
                i += 1;
            }
            if bytes.get(i) != Some(&b'[') {
                return None;
            }
        }

        let parse_fn = |text: &str| try_parse_reference_definition_with_options(text, ctx.config);

        let consumed = if ctx.blockquote_depth > 0 {
            parse_fn(content)?;
            1usize
        } else {
            let mut multi = String::new();
            let mut joined_lines = 0usize;
            for line in lines.iter().skip(line_pos) {
                if line.trim().is_empty() {
                    break;
                }
                multi.push_str(line);
                joined_lines += 1;
            }
            if joined_lines == 0 {
                return None;
            }

            let (bytes_consumed, _label, _url, _title) = parse_fn(&multi)?;

            let mut consumed = 0usize;
            let mut byte_cursor = 0usize;
            for line in lines.iter().skip(line_pos).take(joined_lines) {
                if byte_cursor >= bytes_consumed {
                    break;
                }
                byte_cursor += line.len();
                consumed += 1;
            }
            if consumed == 0 {
                consumed = 1;
            }
            consumed
        };

        let mut consumed = consumed;

        if ctx.config.extensions.mmd_link_attributes {
            let mut i = line_pos + consumed;
            while i < lines.len() {
                let line = lines[i];

                if line.trim().is_empty() {
                    break;
                }
                if line_is_mmd_link_attribute_continuation(line) {
                    consumed += 1;
                    i += 1;
                    continue;
                }
                break;
            }
        }

        Some((
            BlockDetectionResult::Yes,
            Some(Box::new(ReferenceDefinitionPrepared {
                consumed_lines: consumed,
            })),
        ))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        let content = lines.first();
        let line_pos = lines.pos();
        let lines = lines.raw();

        builder.start_node(SyntaxKind::REFERENCE_DEFINITION.into());

        let consumed_lines = payload
            .and_then(|p| p.downcast_ref::<ReferenceDefinitionPrepared>())
            .map(|p| p.consumed_lines)
            .unwrap_or(1);

        let strict_eol = !ctx.config.extensions.mmd_link_attributes;
        let dialect = ctx.config.dialect;

        if ctx.blockquote_depth > 0 {
            let single = [content];
            let spans = reference_definition_spans(
                content,
                strict_eol,
                dialect,
                ctx.config.extensions.hugo_shortcodes,
            );
            emit_reference_definition_lines(builder, &single, spans);
        } else {
            let target_lines: Vec<&str> = lines
                .iter()
                .skip(line_pos)
                .take(consumed_lines)
                .copied()
                .collect();
            let joined: String = target_lines.concat();
            let spans = reference_definition_spans(
                &joined,
                strict_eol,
                dialect,
                ctx.config.extensions.hugo_shortcodes,
            );
            emit_reference_definition_lines(builder, &target_lines, spans);
        }

        builder.finish_node();

        consumed_lines
    }

    fn name(&self) -> &'static str {
        "reference_definition"
    }
}

pub(crate) struct TableParser;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableKind {
    Grid,
    Multiline,
    Pipe,
    Simple,
}

#[derive(Debug, Clone)]
struct TablePrepared {
    /// The table subtree built during detection. Emission replays this verbatim
    /// (`copy_green_node`) instead of re-parsing — losslessness is guaranteed by
    /// construction since the exact bytes detection validated are what we emit.
    green: rowan::GreenNode,
    /// Lines the table spans, returned to the dispatcher by emission.
    consumed: usize,
}

fn resolve_table_pos(
    ctx: &BlockContext,
    raw: &[&str],
    line_pos: usize,
    prefix: &ContainerPrefix,
) -> usize {
    if !ctx.config.extensions.table_captions {
        return line_pos;
    }
    let window = StrippedLines::with_dispatch(raw, line_pos, line_pos, prefix);
    if !is_caption_followed_by_table(&window, line_pos) {
        return line_pos;
    }
    let mut pos = line_pos + 1;
    while pos < raw.len() && !window.strip_at(pos).trim().is_empty() {
        pos += 1;
    }
    if pos < raw.len() && window.strip_at(pos).trim().is_empty() {
        pos += 1;
    }
    pos
}

fn try_parse_kind(
    ctx: &BlockContext,
    kind: TableKind,
    raw: &[&str],
    pos: usize,
    dispatch: usize,
    prefix: &ContainerPrefix,
    builder: &mut GreenNodeBuilder<'static>,
) -> Option<usize> {
    let window = StrippedLines::with_dispatch(raw, pos, dispatch, prefix);
    match kind {
        TableKind::Grid if ctx.config.extensions.grid_tables => {
            try_parse_grid_table(&window, builder, ctx.config)
        }
        TableKind::Multiline if ctx.config.extensions.multiline_tables => {
            try_parse_multiline_table(&window, builder, ctx.config)
        }
        TableKind::Pipe if ctx.config.extensions.pipe_tables => {
            try_parse_pipe_table(&window, builder, ctx.config)
        }
        TableKind::Simple if ctx.config.extensions.simple_tables => {
            try_parse_simple_table(&window, builder, ctx.config)
        }
        _ => None,
    }
}

/// Try each table kind (Grid → Multiline → Pipe → Simple) at `pos`, anchored at
/// `dispatch`, parsing the first match into `builder`. Returns the matched kind
/// and the line count consumed. The single home for the kind cascade; callers
/// pick the position-ordering policy.
fn first_kind_at(
    ctx: &BlockContext,
    raw: &[&str],
    pos: usize,
    dispatch: usize,
    prefix: &ContainerPrefix,
    builder: &mut GreenNodeBuilder<'static>,
) -> Option<(TableKind, usize)> {
    for kind in [
        TableKind::Grid,
        TableKind::Multiline,
        TableKind::Pipe,
        TableKind::Simple,
    ] {
        if let Some(consumed) = try_parse_kind(ctx, kind, raw, pos, dispatch, prefix, builder) {
            return Some((kind, consumed));
        }
    }
    None
}

fn emit_table_kind(
    ctx: &BlockContext,
    kind: TableKind,
    raw: &[&str],
    line_pos: usize,
    table_pos: usize,
    prefix: &ContainerPrefix,
    builder: &mut GreenNodeBuilder<'static>,
) -> Option<usize> {
    if let Some(n) = try_parse_kind(ctx, kind, raw, line_pos, line_pos, prefix, builder) {
        return Some(n);
    }
    if table_pos != line_pos
        && let Some(n) = try_parse_kind(ctx, kind, raw, table_pos, line_pos, prefix, builder)
    {
        return Some(n);
    }
    None
}

impl BlockParser for TableParser {
    fn effect(&self) -> BlockEffect {
        BlockEffect::None
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let line_pos = lines.pos();
        let prefix = lines.prefix();
        let lines = lines.raw();
        if !ctx.has_blank_before && !ctx.at_document_start {
            return None;
        }

        if !(ctx.config.extensions.simple_tables
            || ctx.config.extensions.multiline_tables
            || ctx.config.extensions.grid_tables
            || ctx.config.extensions.pipe_tables)
        {
            return None;
        }

        let detection = if ctx.has_blank_before || ctx.at_document_start {
            BlockDetectionResult::Yes
        } else {
            BlockDetectionResult::YesCanInterrupt
        };

        let table_pos = resolve_table_pos(ctx, lines, line_pos, prefix);

        let mut probe = GreenNodeBuilder::new();
        let (kind, probe_consumed) =
            first_kind_at(ctx, lines, table_pos, line_pos, prefix, &mut probe)?;

        let (green, consumed) = if table_pos == line_pos {
            (probe.finish(), probe_consumed)
        } else {
            let mut b = GreenNodeBuilder::new();
            let consumed = emit_table_kind(ctx, kind, lines, line_pos, table_pos, prefix, &mut b)?;
            (b.finish(), consumed)
        };

        Some((detection, Some(Box::new(TablePrepared { green, consumed }))))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        if let Some(p) = payload.and_then(|p| p.downcast_ref::<TablePrepared>()) {
            copy_green_node(builder, &p.green);
            return p.consumed;
        }

        let line_pos = lines.pos();
        let prefix = lines.prefix();
        let lines = lines.raw();
        let table_pos = resolve_table_pos(ctx, lines, line_pos, prefix);

        if let Some((_, n)) = first_kind_at(ctx, lines, line_pos, line_pos, prefix, builder) {
            return n;
        }
        if table_pos != line_pos
            && let Some((_, n)) = first_kind_at(ctx, lines, table_pos, line_pos, prefix, builder)
        {
            return n;
        }

        debug_assert!(false, "TableParser::parse called without a matching table");
        1
    }

    fn name(&self) -> &'static str {
        "table"
    }
}

/// Emit a (possibly multi-line) reference definition's content tokens with
/// full inline structure:
/// `WHITESPACE? LINK<LINK_START "[", LINK_TEXT, "]"> TEXT(":") sep
///  REFERENCE_URL sep REFERENCE_TITLE? trailing`.
///
/// The destination/title byte ranges come from `spans` —
/// [`reference_definition_spans`], the same walker detection uses — so the
/// `REFERENCE_URL` / `REFERENCE_TITLE` nodes wrap exactly the bytes detection
/// recognized and the two phases never drift. The LINK_TEXT may span multiple
/// lines via interleaved TEXT/NEWLINE tokens when the label wraps
/// (e.g. `[Foo\n  bar]: /url`, CommonMark example #541).
///
/// When `spans` is `None` (the dispatcher only calls this after a successful
/// detection, so this is defensive), each input line is emitted verbatim via
/// `emit_line_tokens` to preserve CST losslessness.
fn emit_reference_definition_lines(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    spans: Option<ReferenceSpans>,
) {
    use crate::parser::utils::helpers::emit_line_tokens;
    use crate::syntax::SyntaxKind;

    if lines.is_empty() {
        return;
    }

    let Some(spans) = spans else {
        for line in lines {
            emit_line_tokens(builder, line);
        }
        return;
    };

    fn emit_separator(builder: &mut GreenNodeBuilder<'static>, seg: &str) {
        let bytes = seg.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\n' => {
                    builder.token(SyntaxKind::NEWLINE.into(), "\n");
                    i += 1;
                }
                b'\r' => {
                    let n = if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                        2
                    } else {
                        1
                    };
                    builder.token(SyntaxKind::NEWLINE.into(), &seg[i..i + n]);
                    i += n;
                }
                _ => {
                    let start = i;
                    while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
                        i += 1;
                    }
                    builder.token(SyntaxKind::WHITESPACE.into(), &seg[start..i]);
                }
            }
        }
    }

    fn emit_text_lines(builder: &mut GreenNodeBuilder<'static>, seg: &str) {
        let bytes = seg.as_bytes();
        let mut i = 0;
        let mut start = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\n' => {
                    if i > start {
                        builder.token(SyntaxKind::TEXT.into(), &seg[start..i]);
                    }
                    builder.token(SyntaxKind::NEWLINE.into(), "\n");
                    i += 1;
                    start = i;
                }
                b'\r' => {
                    if i > start {
                        builder.token(SyntaxKind::TEXT.into(), &seg[start..i]);
                    }
                    let n = if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                        2
                    } else {
                        1
                    };
                    builder.token(SyntaxKind::NEWLINE.into(), &seg[i..i + n]);
                    i += n;
                    start = i;
                }
                _ => i += 1,
            }
        }
        if start < bytes.len() {
            builder.token(SyntaxKind::TEXT.into(), &seg[start..]);
        }
    }

    let joined: String = lines.concat();
    let s = &joined[..];

    if spans.indent > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &s[..spans.indent]);
    }

    builder.start_node(SyntaxKind::LINK.into());

    builder.start_node(SyntaxKind::LINK_START.into());
    builder.token(SyntaxKind::LINK_START.into(), "[");
    builder.finish_node();

    builder.start_node(SyntaxKind::LINK_TEXT.into());
    emit_text_lines(builder, &s[spans.indent + 1..spans.label_close]);
    builder.finish_node();

    builder.token(SyntaxKind::TEXT.into(), "]");
    builder.finish_node(); // LINK

    builder.token(SyntaxKind::TEXT.into(), ":");
    emit_separator(builder, &s[spans.colon + 1..spans.url.start]);

    builder.start_node(SyntaxKind::REFERENCE_URL.into());
    if spans.url_is_angle {
        builder.token(SyntaxKind::LINK_DEST_START.into(), "<");
        let inner = &s[spans.url.start + 1..spans.url.end - 1];
        if !inner.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), inner);
        }
        builder.token(SyntaxKind::LINK_DEST_END.into(), ">");
    } else {
        builder.token(SyntaxKind::TEXT.into(), &s[spans.url.clone()]);
    }
    builder.finish_node(); // REFERENCE_URL

    let last_end = if let Some(title) = spans.title.clone() {
        emit_separator(builder, &s[spans.url.end..title.start]);
        builder.start_node(SyntaxKind::REFERENCE_TITLE.into());
        builder.token(SyntaxKind::TEXT.into(), &s[title.clone()]);
        builder.finish_node(); // REFERENCE_TITLE
        title.end
    } else {
        spans.url.end
    };

    emit_text_lines(builder, &s[last_end..]);
}

/// Fenced code block parser (``` or ~~~)
pub(crate) struct FencedCodeBlockParser;

impl BlockParser for FencedCodeBlockParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let content = lines.first();
        let line_pos = lines.pos();
        let lines = lines.raw();
        let content_to_check = if let Some(list_info) = ctx.list_indent_info {
            if list_info.content_col > 0 && !content.is_empty() {
                let idx = byte_index_at_column(content, list_info.content_col);
                &content[idx..]
            } else {
                content
            }
        } else {
            content
        };

        let fence = try_parse_fence_open(content_to_check, ctx.config.dialect)?;
        if (fence.fence_char == '`' && !ctx.config.extensions.backtick_code_blocks)
            || (fence.fence_char == '~' && !ctx.config.extensions.fenced_code_blocks)
        {
            return None;
        }

        if ctx.config.dialect == crate::options::Dialect::Pandoc
            && !ctx.has_blank_before
            && (ctx.paragraph_open || ctx.list_item_content_open)
            && content_to_check.starts_with([' ', '\t'])
        {
            return None;
        }

        if ctx.config.dialect == crate::options::Dialect::Pandoc {
            let trimmed_info = fence.info_string.trim();
            if trimmed_info.starts_with('{') && trimmed_info.ends_with('}') {
                if trimmed_info.starts_with("{=") {
                    if !ctx.config.extensions.raw_attribute {
                        return None;
                    }
                } else if !ctx.config.extensions.fenced_code_attributes {
                    return None;
                }
            }

            let info = InfoString::parse(&fence.info_string);

            let is_executable = matches!(info.block_type, CodeBlockType::Executable { .. });
            if is_executable && !ctx.config.extensions.executable_code {
                return None;
            }
        }

        let has_info = !fence.info_string.trim().is_empty();

        let closes_open_code_span = !has_info
            && fence.fence_char == '`'
            && ctx.open_code_span_openers.contains(&fence.fence_count);

        let has_matching_closer = {
            let mut found = false;
            let fence_col = ctx.content_indent + leading_indent(content).0;
            let container_content_col = (ctx.content_indent
                + ctx
                    .list_indent_info
                    .map(|list_info| list_info.content_col)
                    .unwrap_or(0))
            .min(fence_col);
            let mut container_scan = ContainerExitScan::new(container_content_col);
            for raw_line in lines.iter().skip(line_pos + 1) {
                let (line_bq_depth, inner) = count_blockquote_markers(raw_line);
                let gobbled_lazily = ctx.config.dialect == crate::options::Dialect::Pandoc
                    && ctx.blockquote_depth > 0
                    && !raw_line.trim().is_empty();
                if line_bq_depth < ctx.blockquote_depth && !gobbled_lazily {
                    break;
                }
                if container_scan.exits(inner) {
                    break;
                }
                let candidate = if line_bq_depth < ctx.blockquote_depth {
                    inner.trim_start_matches([' ', '\t'])
                } else if container_content_col > 0 && !inner.is_empty() {
                    let idx = byte_index_at_column(inner, container_content_col);
                    if idx <= inner.len() {
                        &inner[idx..]
                    } else {
                        inner
                    }
                } else {
                    inner
                };
                if is_closing_fence(candidate, &fence) {
                    found = true;
                    break;
                }
            }
            found
        };

        let common_mark_dialect = ctx.config.dialect == crate::options::Dialect::CommonMark;
        if !has_matching_closer && !common_mark_dialect {
            return None;
        }

        let tilde_requires_blank_before = fence.fence_char == '~' && !common_mark_dialect;

        let detection = if tilde_requires_blank_before {
            if ctx.has_blank_before {
                BlockDetectionResult::Yes
            } else {
                BlockDetectionResult::No
            }
        } else if has_info || (has_matching_closer && !closes_open_code_span) || common_mark_dialect
        {
            BlockDetectionResult::YesCanInterrupt
        } else if ctx.has_blank_before {
            BlockDetectionResult::Yes
        } else {
            BlockDetectionResult::No
        };

        match detection {
            BlockDetectionResult::No => None,
            _ => Some((detection, Some(Box::new(fence)))),
        }
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        let content = lines.first();
        let line_pos = lines.pos();
        let list_indent_stripped = ctx.list_indent_info.map(|i| i.content_col).unwrap_or(0);

        let fence = if let Some(fence) = payload.and_then(|p| p.downcast_ref::<FenceInfo>()) {
            fence.clone()
        } else {
            let content_to_check = if list_indent_stripped > 0 && !content.is_empty() {
                let idx = byte_index_at_column(content, list_indent_stripped);
                &content[idx..]
            } else {
                content
            };
            try_parse_fence_open(content_to_check, ctx.config.dialect).expect("Fence should exist")
        };

        let new_pos = if ctx.config.extensions.tex_math_gfm && is_gfm_math_fence(&fence) {
            parse_fenced_math_block(builder, lines, fence, None, ctx.config.dialect)
        } else {
            parse_fenced_code_block(builder, lines, fence, None, &ctx.diags, ctx.config.flavor)
        };

        new_pos - line_pos
    }

    fn name(&self) -> &'static str {
        "fenced_code_block"
    }
}

/// Whether the leading `<script ...>` open tag in `content` has a
/// `type` attribute whose value starts with `math/tex` (case-insensitive).
/// Mirrors pandoc's `isInlineTag` special case for `<script>` opens:
/// only the `math/tex…` flavor is treated as inline mid-paragraph;
/// every other `<script>` open is a `RawBlock` start.
fn is_math_tex_script_open(content: &str) -> bool {
    let trimmed = content.trim_start();
    if !trimmed
        .get(..7)
        .is_some_and(|s| s.eq_ignore_ascii_case("<script"))
    {
        return false;
    }
    let Some(attrs) = parse_html_tag_attributes(trimmed) else {
        return false;
    };
    attrs.key_values.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("type") && v.to_ascii_lowercase().starts_with("math/tex")
    })
}

/// Whether an HTML block start cannot interrupt a running paragraph under the
/// given dialect (pandoc's `isInlineTag` set, plus the issue-#10643 special
/// cases). `content` is the raw (bq/list-indent-stripped) first line, needed
/// for the `<script type="math/tex…">` attribute probe. Shared between the
/// block dispatcher and the footnote-body marker-line HTML dispatch, which
/// lifts only tags that CAN interrupt (i.e. `!cannot_interrupt`).
pub(crate) fn html_block_cannot_interrupt(
    block_type: &HtmlBlockType,
    content: &str,
    is_pandoc: bool,
) -> bool {
    matches!(block_type, HtmlBlockType::Type7)
        || (matches!(block_type, HtmlBlockType::Comment) && is_pandoc)
        || (matches!(block_type, HtmlBlockType::ProcessingInstruction) && is_pandoc)
        || (is_pandoc
            && matches!(block_type, HtmlBlockType::BlockTag { tag_name, is_closing, .. }
                if is_pandoc_inline_block_tag_name(tag_name)
                    || is_pandoc_void_block_tag_name(tag_name)
                    || tag_name.eq_ignore_ascii_case("style")
                    || (*is_closing && tag_name.eq_ignore_ascii_case("script"))
                    || (!*is_closing
                        && tag_name.eq_ignore_ascii_case("script")
                        && is_math_tex_script_open(content))))
}

pub(crate) struct HtmlBlockParser;

impl BlockParser for HtmlBlockParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let content = lines.first();
        let detect_line = lines.first_unconditional();
        let prefix = lines.prefix();
        let line_pos = lines.pos();
        let lines = lines.raw();
        if !ctx.config.extensions.raw_html {
            return None;
        }

        {
            // Measure the indent against the innermost list item's content
            // column: `content` keeps that item's indent for emission, which
            // two list levels deep is already four columns.
            let bytes = detect_line.as_bytes();
            let mut i = 0;
            while i < bytes.len() && i < 3 && bytes[i] == b' ' {
                i += 1;
            }
            if bytes.get(i) != Some(&b'<') {
                return None;
            }
        }

        let is_commonmark = ctx.config.dialect == crate::options::Dialect::CommonMark;
        let block_type = try_parse_html_block_start(detect_line, is_commonmark)?;

        if let HtmlBlockType::BlockTag {
            tag_name,
            is_closing: true,
            ..
        } = &block_type
            && ctx.list_item_unclosed_html_block_tag.as_deref()
                == Some(tag_name.to_ascii_lowercase().as_str())
        {
            return None;
        }

        if !is_commonmark
            && matches!(block_type, HtmlBlockType::BlockTag { .. })
            && !pandoc_html_block_start_is_valid(lines, line_pos, prefix, &block_type)
        {
            return None;
        }

        let is_pandoc = ctx.config.dialect == crate::options::Dialect::Pandoc;
        let cannot_interrupt = html_block_cannot_interrupt(&block_type, content, is_pandoc);
        if is_pandoc && cannot_interrupt {
            let leading_spaces = content
                .as_bytes()
                .iter()
                .take_while(|&&b| b == b' ')
                .count();
            let container_col = ctx.list_indent_info.map(|i| i.content_col).unwrap_or(0);
            if leading_spaces > container_col {
                return None;
            }
        }
        let detection = if cannot_interrupt {
            if ctx.has_blank_before || ctx.at_document_start {
                BlockDetectionResult::Yes
            } else {
                return None;
            }
        } else if ctx.has_blank_before || ctx.at_document_start {
            BlockDetectionResult::Yes
        } else {
            BlockDetectionResult::YesCanInterrupt
        };

        Some((detection, Some(Box::new(block_type))))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        let content = lines.first();
        let prefix = lines.prefix();
        let line_pos = lines.pos();
        let lines = lines.raw();
        let is_commonmark = ctx.config.dialect == crate::options::Dialect::CommonMark;
        let block_type = if let Some(bt) = payload.and_then(|p| p.downcast_ref::<HtmlBlockType>()) {
            bt.clone()
        } else {
            try_parse_html_block_start(content, is_commonmark)
                .expect("HTML block type should exist")
        };

        let wrapper_kind = match &block_type {
            HtmlBlockType::BlockTag {
                tag_name,
                is_closing: false,
                ..
            } if tag_name == "div"
                && ctx.config.dialect == crate::options::Dialect::Pandoc
                && ctx.config.extensions.native_divs
                && !(ctx.blockquote_depth > 0 && ctx.content_indent > 0)
                && (probe_open_tag_line_has_close_gt(content, "div")
                    || pandoc_html_open_tag_closes(lines, line_pos, prefix)) =>
            {
                crate::syntax::SyntaxKind::HTML_BLOCK_DIV
            }
            _ => crate::syntax::SyntaxKind::HTML_BLOCK,
        };

        let fusion =
            if ctx.in_list || ctx.content_indent != 0 || ctx.myst_directive_closer.is_some() {
                SoftbreakFusion::None
            } else if ctx.blockquote_depth > 0 {
                SoftbreakFusion::ToBlockquoteEnd
            } else if !ctx.in_fenced_div {
                SoftbreakFusion::ToDocEnd
            } else if ctx.config.extensions.fenced_divs {
                SoftbreakFusion::ToFencedDivClose
            } else {
                SoftbreakFusion::None
            };

        let new_pos = parse_html_block_with_wrapper(
            builder,
            lines,
            line_pos,
            block_type,
            prefix,
            wrapper_kind,
            fusion,
            ctx.config,
            None,
        );
        new_pos - line_pos
    }

    fn name(&self) -> &'static str {
        "html_block"
    }
}

pub(crate) struct LatexEnvironmentParser;

impl BlockParser for LatexEnvironmentParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.raw_tex {
            return None;
        }

        let env_name = extract_environment_name(lines.first())?.to_string();
        let env_info = LatexEnvInfo { env_name };

        use super::blocks::raw_blocks::is_inline_math_environment;
        if is_inline_math_environment(&env_info.env_name) {
            return None;
        }

        let detection = if ctx.has_blank_before || ctx.at_document_start {
            BlockDetectionResult::Yes
        } else {
            BlockDetectionResult::YesCanInterrupt
        };

        Some((detection, Some(Box::new(env_info))))
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        let content = lines.first();
        let line_pos = lines.pos();
        let lines = lines.raw();

        let env_info = if let Some(info) = payload.and_then(|p| p.downcast_ref::<LatexEnvInfo>()) {
            info.clone()
        } else {
            let env_name = extract_environment_name(content)
                .expect("LaTeX env info should exist")
                .to_string();
            LatexEnvInfo { env_name }
        };

        builder.start_node(SyntaxKind::TEX_BLOCK.into());

        let mut current_pos = line_pos;
        let end_marker = format!("\\end{{{}}}", env_info.env_name);
        let mut first_line = true;

        while current_pos < lines.len() {
            let line = lines[current_pos];

            if !first_line {
                builder.token(SyntaxKind::NEWLINE.into(), "\n");
            }
            first_line = false;

            let content = trim_end_newlines(line);
            builder.token(SyntaxKind::TEXT.into(), content);

            current_pos += 1;

            if line.trim_start().starts_with(&end_marker) {
                break;
            }
        }

        if current_pos > line_pos {
            builder.token(SyntaxKind::NEWLINE.into(), "\n");
        }

        builder.finish_node(); // TEX_BLOCK

        current_pos - line_pos
    }

    fn name(&self) -> &'static str {
        "latex_environment"
    }
}

pub(crate) struct RawTexBlockParser;

impl BlockParser for RawTexBlockParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.raw_tex {
            return None;
        }

        if !ctx.has_blank_before && !ctx.at_document_start {
            return None;
        }

        if !raw_blocks::can_start_raw_block(lines.first(), ctx.config) {
            return None;
        }

        Some((BlockDetectionResult::Yes, None))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        _payload: Option<&dyn Any>,
    ) -> usize {
        let line_pos = lines.pos();
        let lines = lines.raw();
        raw_blocks::parse_raw_tex_block(builder, lines, line_pos, ctx.blockquote_depth)
    }

    fn name(&self) -> &'static str {
        "raw_tex_block"
    }
}

pub(crate) struct LineBlockParser;

impl BlockParser for LineBlockParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let content = lines.first();
        if !ctx.config.extensions.line_blocks {
            return None;
        }

        try_parse_line_block_start(content)?;

        if !ctx.has_blank_before && !ctx.at_document_start {
            return None;
        }

        let detection = BlockDetectionResult::Yes;

        Some((detection, None))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        _payload: Option<&dyn Any>,
    ) -> usize {
        let line_pos = lines.pos();
        let new_pos = parse_line_block(lines, builder, ctx.config);
        new_pos - line_pos
    }

    fn name(&self) -> &'static str {
        "line_block"
    }
}

pub(crate) struct FencedDivOpenParser;

fn content_for_fenced_div_detection<'a>(ctx: &BlockContext, content: &'a str) -> &'a str {
    if let Some(list_info) = ctx.list_indent_info {
        let (indent_cols, _) = leading_indent(content);
        if indent_cols >= list_info.content_col {
            let idx = byte_index_at_column(content, list_info.content_col);
            return &content[idx..];
        }
    }
    content
}

impl BlockParser for FencedDivOpenParser {
    fn effect(&self) -> BlockEffect {
        BlockEffect::OpenFencedDiv
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.fenced_divs {
            return None;
        }

        let content = content_for_fenced_div_detection(ctx, lines.first());
        {
            let bytes = content.as_bytes();
            let mut i = 0;
            while i < bytes.len() && i < 3 && bytes[i] == b' ' {
                i += 1;
            }
            if bytes.get(i) != Some(&b':') {
                return None;
            }
        }
        let mut div_fence = try_parse_div_fence_open(content)?;
        div_fence.open_indent_cols = leading_indent(lines.first()).0;
        Some((BlockDetectionResult::Yes, Some(Box::new(div_fence))))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        let first = lines.first();
        let line_pos = lines.pos();
        let lines = lines.raw();

        let div_fence = payload
            .and_then(|p| p.downcast_ref::<DivFenceInfo>())
            .cloned()
            .or_else(|| try_parse_div_fence_open(content_for_fenced_div_detection(ctx, first)))
            .unwrap_or(DivFenceInfo {
                attributes: String::new(),
                fence_count: 3,
                open_indent_cols: 0,
            });

        builder.start_node(SyntaxKind::FENCED_DIV.into());

        builder.start_node(SyntaxKind::DIV_FENCE_OPEN.into());

        let full_line = lines[line_pos];
        let line_no_bq = strip_n_blockquote_markers(full_line, ctx.blockquote_depth);
        let trimmed = line_no_bq.trim_start();

        let leading_ws_len = line_no_bq.len() - trimmed.len();
        if leading_ws_len > 0 {
            builder.token(SyntaxKind::WHITESPACE.into(), &line_no_bq[..leading_ws_len]);
        }

        let fence_str: String = ":".repeat(div_fence.fence_count);
        builder.token(SyntaxKind::TEXT.into(), &fence_str);

        let after_colons = &trimmed[div_fence.fence_count..];
        let (content_before_newline, newline_str) = strip_newline(after_colons);

        if !div_fence.attributes.is_empty() {
            let content_after_space = content_before_newline.trim_start();
            let leading_space_len = content_before_newline.len() - content_after_space.len();
            if leading_space_len > 0 {
                builder.token(
                    SyntaxKind::WHITESPACE.into(),
                    &content_before_newline[..leading_space_len],
                );
            }

            emit_div_info_node(builder, &div_fence.attributes);

            let after_attrs = if div_fence.attributes.starts_with('{') {
                if let Some(close_idx) = content_after_space.find('}') {
                    &content_after_space[close_idx + 1..]
                } else {
                    ""
                }
            } else {
                &content_after_space[div_fence.attributes.len()..]
            };

            if !after_attrs.is_empty() {
                let suffix_trimmed = after_attrs.trim_start();
                let ws_len = after_attrs.len() - suffix_trimmed.len();
                if ws_len > 0 {
                    builder.token(SyntaxKind::WHITESPACE.into(), &after_attrs[..ws_len]);
                }
                if !suffix_trimmed.is_empty() {
                    builder.token(SyntaxKind::TEXT.into(), suffix_trimmed);
                }
            }
        }

        if !newline_str.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline_str);
        }

        builder.finish_node(); // DIV_FENCE_OPEN

        1
    }

    fn name(&self) -> &'static str {
        "fenced_div_open"
    }
}

pub(crate) struct FencedDivCloseParser;

impl BlockParser for FencedDivCloseParser {
    fn effect(&self) -> BlockEffect {
        BlockEffect::CloseFencedDiv
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.fenced_divs {
            return None;
        }

        if !ctx.in_fenced_div {
            return None;
        }

        let first = lines.first();
        if ctx.fenced_div_wraps_list
            && let Some(list_info) = ctx.list_indent_info
        {
            let (indent_cols, _) = leading_indent(first);
            if indent_cols >= list_info.content_col {
                return None;
            }
            if ctx
                .fenced_div_open_indent
                .is_some_and(|open_indent| indent_cols > open_indent)
            {
                return None;
            }
        }

        if ctx.list_indent_info.is_none()
            && let Some(open_indent) = ctx.fenced_div_open_indent
        {
            let (closer_indent, _) = leading_indent(first);
            if closer_indent > open_indent {
                return None;
            }
        }

        let content = content_for_fenced_div_detection(ctx, first);
        // A div wrapping a nested list can close at its original indentation,
        // which may exceed the three spaces accepted by the fence recognizer.
        let content = if ctx.fenced_div_wraps_list {
            content.trim_start_matches([' ', '\t'])
        } else {
            content
        };
        if !is_div_closing_fence(content) {
            return None;
        }

        Some((BlockDetectionResult::YesCanInterrupt, None))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        _payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        let line_pos = lines.pos();
        let lines = lines.raw();

        builder.start_node(SyntaxKind::DIV_FENCE_CLOSE.into());

        let full_line = lines[line_pos];
        let line_no_bq = strip_n_blockquote_markers(full_line, ctx.blockquote_depth);
        let trimmed = line_no_bq.trim_start();

        let leading_ws_len = line_no_bq.len() - trimmed.len();
        if leading_ws_len > 0 {
            builder.token(SyntaxKind::WHITESPACE.into(), &line_no_bq[..leading_ws_len]);
        }

        let (content_without_newline, line_ending) = strip_newline(trimmed);
        builder.token(SyntaxKind::TEXT.into(), content_without_newline);

        if !line_ending.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), line_ending);
        }

        builder.finish_node();
        1
    }

    fn name(&self) -> &'static str {
        "fenced_div_close"
    }
}

/// Opener for MyST directives (```` ```{name} ```` / `~~~{name}` / colon
/// `:::{name}`). Opens a `MYST_DIRECTIVE` container whose body is parsed
/// recursively as markdown and closed by a matching fence. Registered before
/// [`FencedCodeBlockParser`] so the brace-tagged opener wins over the generic
/// code-fence path; a non-directive fence falls through to it.
pub(crate) struct MystDirectiveOpenParser;

impl BlockParser for MystDirectiveOpenParser {
    fn effect(&self) -> BlockEffect {
        BlockEffect::OpenMystDirective
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let open = try_parse_directive_open(lines.first(), &ctx.config.extensions)?;
        Some((BlockDetectionResult::YesCanInterrupt, Some(Box::new(open))))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        let line = lines.first();
        let open = payload
            .and_then(|p| p.downcast_ref::<DirectiveOpen>())
            .cloned()
            .or_else(|| try_parse_directive_open(line, &ctx.config.extensions))
            .expect("directive opener should exist");

        builder.start_node(SyntaxKind::MYST_DIRECTIVE.into());
        builder.start_node(SyntaxKind::MYST_DIRECTIVE_OPEN.into());

        let (content, newline) = strip_newline(line);

        let mut cursor = 0;
        if open.indent_len > 0 {
            builder.token(SyntaxKind::WHITESPACE.into(), &content[..open.indent_len]);
            cursor = open.indent_len;
        }

        let fence_end = cursor + open.fence_count;
        builder.token(
            SyntaxKind::MYST_DIRECTIVE_FENCE.into(),
            &content[cursor..fence_end],
        );

        let name_end = fence_end + open.name_len;
        builder.token(
            SyntaxKind::MYST_DIRECTIVE_NAME.into(),
            &content[fence_end..name_end],
        );

        let rest = &content[name_end..];
        if !rest.is_empty() {
            let trimmed = rest.trim_start();
            let lead_ws = rest.len() - trimmed.len();
            if lead_ws > 0 {
                builder.token(SyntaxKind::WHITESPACE.into(), &rest[..lead_ws]);
            }
            let arg = trimmed.trim_end();
            if arg.is_empty() {
                if !trimmed.is_empty() {
                    builder.token(SyntaxKind::WHITESPACE.into(), trimmed);
                }
            } else {
                builder.token(SyntaxKind::MYST_DIRECTIVE_ARG.into(), arg);
                let trail = &trimmed[arg.len()..];
                if !trail.is_empty() {
                    builder.token(SyntaxKind::WHITESPACE.into(), trail);
                }
            }
        }

        if !newline.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline);
        }

        builder.finish_node(); // MYST_DIRECTIVE_OPEN

        let mut consumed = 0;
        loop {
            let idx = 1 + consumed;
            if idx >= lines.remaining() {
                break;
            }
            let opt_line = lines.get(idx);
            if is_directive_closing_fence(opt_line, open.fence_char, open.fence_count) {
                break;
            }
            let Some(opt) = try_parse_directive_option(opt_line) else {
                break;
            };
            emit_directive_option(builder, opt_line, &opt);
            consumed += 1;
        }

        if !open.is_verbatim {
            return 1 + consumed;
        }

        let total = emit_verbatim_directive_body(builder, lines, &open, 1 + consumed);
        builder.finish_node(); // MYST_DIRECTIVE
        total
    }

    fn name(&self) -> &'static str {
        "myst_directive_open"
    }
}

fn emit_verbatim_directive_body(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &StrippedLines<'_, '_>,
    open: &DirectiveOpen,
    body_rel: usize,
) -> usize {
    use crate::syntax::SyntaxKind;

    let raw = lines.raw();
    let start = lines.pos();
    let body_start = start + body_rel;

    let prefix = lines.prefix();
    let bq_depth = prefix.bq_depth();
    let list_content_col = prefix.list_content_col();
    let bq_outer = bq_outer_of_list(prefix);

    let mut scan = body_start;
    let mut found_closer = false;
    while scan < raw.len() {
        let probe = if bq_outer {
            raw[scan]
        } else {
            strip_list_indent(raw[scan], list_content_col)
        };
        let (line_bq_depth, _) = count_blockquote_markers(probe);
        if line_bq_depth < bq_depth {
            break;
        }
        if is_directive_closing_fence(lines.strip_at(scan), open.fence_char, open.fence_count) {
            found_closer = true;
            break;
        }
        scan += 1;
    }

    if scan > body_start {
        builder.start_node(SyntaxKind::MYST_DIRECTIVE_BODY.into());
        for i in body_start..scan {
            let tail = lines.emit_prefix_at(builder, i);
            let (text, newline) = strip_newline(tail);
            if !text.is_empty() {
                builder.token(SyntaxKind::TEXT.into(), text);
            }
            if !newline.is_empty() {
                builder.token(SyntaxKind::NEWLINE.into(), newline);
            }
        }
        builder.finish_node(); // MYST_DIRECTIVE_BODY
    }

    if found_closer {
        let tail = lines.emit_prefix_at(builder, scan);
        emit_directive_close(builder, tail, open.fence_char);
        scan += 1;
    }

    scan - start
}

fn emit_directive_option(
    builder: &mut GreenNodeBuilder<'static>,
    line: &str,
    opt: &DirectiveOption,
) {
    use crate::syntax::SyntaxKind;

    let (content, newline) = strip_newline(line);

    builder.start_node(SyntaxKind::MYST_DIRECTIVE_OPTION.into());

    let mut cursor = 0;
    if opt.indent_len > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &content[..opt.indent_len]);
        cursor = opt.indent_len;
    }

    builder.token(
        SyntaxKind::MYST_DIRECTIVE_OPTION_MARKER.into(),
        &content[cursor..cursor + 1],
    );
    cursor += 1;
    let name_end = cursor + opt.name_len;
    builder.token(
        SyntaxKind::MYST_DIRECTIVE_OPTION_NAME.into(),
        &content[cursor..name_end],
    );
    builder.token(
        SyntaxKind::MYST_DIRECTIVE_OPTION_MARKER.into(),
        &content[name_end..name_end + 1],
    );

    let rest = &content[name_end + 1..];
    if !rest.is_empty() {
        let trimmed = rest.trim_start();
        let lead_ws = rest.len() - trimmed.len();
        if lead_ws > 0 {
            builder.token(SyntaxKind::WHITESPACE.into(), &rest[..lead_ws]);
        }
        let value = trimmed.trim_end();
        if value.is_empty() {
            if !trimmed.is_empty() {
                builder.token(SyntaxKind::WHITESPACE.into(), trimmed);
            }
        } else {
            builder.token(SyntaxKind::MYST_DIRECTIVE_OPTION_VALUE.into(), value);
            let trail = &trimmed[value.len()..];
            if !trail.is_empty() {
                builder.token(SyntaxKind::WHITESPACE.into(), trail);
            }
        }
    }

    if !newline.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline);
    }

    builder.finish_node(); // MYST_DIRECTIVE_OPTION
}

/// Closer for MyST directives. Active only when inside a directive (the
/// expected fence is threaded through [`BlockContext::myst_directive_closer`]).
/// Registered before [`FencedCodeBlockParser`] so a bare ```` ``` ```` closes
/// the directive rather than opening an empty code block.
pub(crate) struct MystDirectiveCloseParser;

impl BlockParser for MystDirectiveCloseParser {
    fn effect(&self) -> BlockEffect {
        BlockEffect::CloseMystDirective
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let (fence_char, open_count) = ctx.myst_directive_closer?;
        if is_directive_closing_fence(lines.first(), fence_char, open_count) {
            Some((BlockDetectionResult::YesCanInterrupt, None))
        } else {
            None
        }
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        _payload: Option<&dyn Any>,
    ) -> usize {
        let fence_char = ctx.myst_directive_closer.map(|(c, _)| c).unwrap_or(b'`');
        emit_directive_close(builder, lines.first(), fence_char);
        1
    }

    fn name(&self) -> &'static str {
        "myst_directive_close"
    }
}

fn emit_directive_close(builder: &mut GreenNodeBuilder<'static>, line: &str, fence_char: u8) {
    use crate::syntax::SyntaxKind;

    let (content, newline) = strip_newline(line);

    builder.start_node(SyntaxKind::MYST_DIRECTIVE_CLOSE.into());

    let lead_ws = content.bytes().take(3).take_while(|&b| b == b' ').count();
    if lead_ws > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &content[..lead_ws]);
    }
    let after = &content[lead_ws..];
    let fence_len = after.bytes().take_while(|&b| b == fence_char).count();
    builder.token(SyntaxKind::MYST_DIRECTIVE_FENCE.into(), &after[..fence_len]);
    let trail = &after[fence_len..];
    if !trail.is_empty() {
        builder.token(SyntaxKind::WHITESPACE.into(), trail);
    }
    if !newline.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline);
    }

    builder.finish_node(); // MYST_DIRECTIVE_CLOSE
}

/// Parser for MyST `(label)=` target lines.
pub(crate) struct MystTargetParser;

impl BlockParser for MystTargetParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.myst_targets {
            return None;
        }
        let target = try_parse_target(lines.first())?;
        Some((
            BlockDetectionResult::YesCanInterrupt,
            Some(Box::new(target)),
        ))
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        let line = lines.first();
        let target = payload
            .and_then(|p| p.downcast_ref::<Target>())
            .copied()
            .or_else(|| try_parse_target(line))
            .expect("target should exist");
        let (content, newline) = strip_newline(line);

        builder.start_node(SyntaxKind::MYST_TARGET.into());
        if target.indent_len > 0 {
            builder.token(SyntaxKind::WHITESPACE.into(), &content[..target.indent_len]);
        }
        builder.token(
            SyntaxKind::TEXT.into(),
            &content[target.indent_len..target.label.0],
        );
        builder.token(
            SyntaxKind::MYST_TARGET_LABEL.into(),
            &content[target.label.0..target.label.1],
        );
        builder.token(
            SyntaxKind::TEXT.into(),
            &content[target.label.1..target.marker_end],
        );
        let trailing = &content[target.marker_end..];
        if !trailing.is_empty() {
            builder.token(SyntaxKind::WHITESPACE.into(), trailing);
        }
        if !newline.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline);
        }
        builder.finish_node(); // MYST_TARGET
        1
    }

    fn name(&self) -> &'static str {
        "myst_target"
    }
}

/// Parser for MyST `% ...` line comments.
pub(crate) struct MystCommentParser;

impl BlockParser for MystCommentParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.myst_comments {
            return None;
        }
        if is_comment_line(lines.first()) {
            Some((BlockDetectionResult::YesCanInterrupt, None))
        } else {
            None
        }
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        _payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        let line = lines.first();
        let (content, newline) = strip_newline(line);
        let indent_len = content.bytes().take(3).take_while(|&b| b == b' ').count();

        builder.start_node(SyntaxKind::MYST_COMMENT.into());
        if indent_len > 0 {
            builder.token(SyntaxKind::WHITESPACE.into(), &content[..indent_len]);
        }
        builder.token(SyntaxKind::TEXT.into(), &content[indent_len..]);
        if !newline.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline);
        }
        builder.finish_node(); // MYST_COMMENT
        1
    }

    fn name(&self) -> &'static str {
        "myst_comment"
    }
}

struct SvelteBlockInfo {
    /// Leading-space count (≤3) before the opening `{`.
    indent_len: usize,
    /// Byte length of the balanced `{...}` span.
    span_len: usize,
    /// Span category (block logic / tag / expression).
    kind: SvelteKind,
    /// Verbatim content between the outer braces.
    content: String,
}

/// Parser for standalone Svelte template lines (mdsvex).
///
/// A line whose entire content is a single balanced Svelte span
/// (`{#if}`/`{:else}`/`{/each}`, `{@html}`, or `{expr}`) is emitted as an
/// opaque [`SyntaxKind::SVELTE_BLOCK`] leaf block rather than a paragraph. As a
/// leaf block it is not an open text paragraph, so an immediately following
/// tight list opens as a real `LIST` instead of being absorbed as paragraph
/// continuation and reflowed onto one line. Gated on `svelte_template`, so it is
/// inert for every non-mdsvex flavor. The inner span subtree is built by the
/// shared inline emitter, keeping the CST identical to the inline form.
pub(crate) struct SvelteBlockParser;

impl SvelteBlockParser {
    fn detect_line(line: &str) -> Option<SvelteBlockInfo> {
        let (content, _) = strip_newline(line);

        let indent_len = content.bytes().take_while(|&b| b == b' ').count();
        if indent_len > 3 {
            return None;
        }
        let rest = &content[indent_len..];

        let (span_len, kind, span_content) = try_parse_svelte_template(rest)?;

        if !rest[span_len..].trim().is_empty() {
            return None;
        }

        Some(SvelteBlockInfo {
            indent_len,
            span_len,
            kind,
            content: span_content,
        })
    }
}

impl BlockParser for SvelteBlockParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.svelte_template {
            return None;
        }
        let info = Self::detect_line(lines.first())?;
        Some((BlockDetectionResult::YesCanInterrupt, Some(Box::new(info))))
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        let line = lines.first();
        let (content, newline) = strip_newline(line);
        let info = payload
            .and_then(|p| p.downcast_ref::<SvelteBlockInfo>())
            .map(|info| SvelteBlockInfo {
                indent_len: info.indent_len,
                span_len: info.span_len,
                kind: info.kind,
                content: info.content.clone(),
            })
            .or_else(|| Self::detect_line(line))
            .expect("svelte block should exist");

        builder.start_node(SyntaxKind::SVELTE_BLOCK.into());
        if info.indent_len > 0 {
            builder.token(SyntaxKind::WHITESPACE.into(), &content[..info.indent_len]);
        }
        emit_svelte_template(builder, info.kind, &info.content);
        let trailing = &content[info.indent_len + info.span_len..];
        if !trailing.is_empty() {
            builder.token(SyntaxKind::WHITESPACE.into(), trailing);
        }
        if !newline.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline);
        }
        builder.finish_node(); // SVELTE_BLOCK
        1
    }

    fn name(&self) -> &'static str {
        "svelte_block"
    }
}

/// Parser for MyST `+++` block break lines.
pub(crate) struct MystBlockBreakParser;

impl BlockParser for MystBlockBreakParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.myst_block_breaks {
            return None;
        }
        let block_break = try_parse_block_break(lines.first())?;
        Some((
            BlockDetectionResult::YesCanInterrupt,
            Some(Box::new(block_break)),
        ))
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        let line = lines.first();
        let bb = payload
            .and_then(|p| p.downcast_ref::<BlockBreak>())
            .copied()
            .or_else(|| try_parse_block_break(line))
            .expect("block break should exist");
        let (content, newline) = strip_newline(line);

        builder.start_node(SyntaxKind::MYST_BLOCK_BREAK.into());
        if bb.indent_len > 0 {
            builder.token(SyntaxKind::WHITESPACE.into(), &content[..bb.indent_len]);
        }
        builder.token(
            SyntaxKind::MYST_BLOCK_BREAK_MARKER.into(),
            &content[bb.indent_len..bb.marker_end],
        );
        let (meta_start, meta_end) = bb.metadata;
        if meta_end > meta_start {
            if meta_start > bb.marker_end {
                builder.token(
                    SyntaxKind::WHITESPACE.into(),
                    &content[bb.marker_end..meta_start],
                );
            }
            builder.token(
                SyntaxKind::MYST_BLOCK_BREAK_META.into(),
                &content[meta_start..meta_end],
            );
            if meta_end < content.len() {
                builder.token(SyntaxKind::WHITESPACE.into(), &content[meta_end..]);
            }
        } else if bb.marker_end < content.len() {
            builder.token(SyntaxKind::WHITESPACE.into(), &content[bb.marker_end..]);
        }
        if !newline.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline);
        }
        builder.finish_node(); // MYST_BLOCK_BREAK
        1
    }

    fn name(&self) -> &'static str {
        "myst_block_break"
    }
}

/// Opener for python-markdown admonitions (`!!! type "title"`) and
/// pymdownx.details (`???`/`???+`). Opens an `ADMONITION` container whose
/// 4-space-indented body is parsed recursively (closed on dedent like a
/// footnote definition). Registered before [`IndentedCodeBlockParser`] so
/// the indented body is not captured as a code block.
pub(crate) struct AdmonitionOpenParser;

impl BlockParser for AdmonitionOpenParser {
    fn effect(&self) -> BlockEffect {
        BlockEffect::OpenAdmonition
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let adm = try_parse_admonition_open(lines.first(), &ctx.config.extensions)?;
        Some((BlockDetectionResult::YesCanInterrupt, Some(Box::new(adm))))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        let first = lines.first();
        let Some(adm) = payload
            .and_then(|p| p.downcast_ref::<AdmonitionOpen>())
            .cloned()
            .or_else(|| try_parse_admonition_open(first, &ctx.config.extensions))
        else {
            return 1;
        };

        if let Some(indent_str) = ctx.indent_to_emit {
            builder.token(SyntaxKind::WHITESPACE.into(), indent_str);
        }

        builder.start_node(SyntaxKind::ADMONITION.into());
        emit_admonition_marker_line(builder, first, &adm);
        1
    }

    fn name(&self) -> &'static str {
        "admonition"
    }
}

/// Emit the admonition opener line losslessly: every byte of `first` becomes
/// a marker/type/title token or interleaved/trailing `WHITESPACE`, plus the
/// trailing `NEWLINE`.
fn emit_admonition_marker_line(
    builder: &mut GreenNodeBuilder<'static>,
    first: &str,
    adm: &AdmonitionOpen,
) {
    use crate::syntax::SyntaxKind;

    let (line, newline) = strip_newline(first);

    if adm.indent_len > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &line[..adm.indent_len]);
    }
    let marker_end = adm.indent_len + adm.marker_len;
    builder.token(
        SyntaxKind::ADMONITION_MARKER.into(),
        &line[adm.indent_len..marker_end],
    );
    let mut cur = marker_end;

    if let Some((start, end)) = adm.type_range {
        if start > cur {
            builder.token(SyntaxKind::WHITESPACE.into(), &line[cur..start]);
        }
        builder.token(SyntaxKind::ADMONITION_TYPE.into(), &line[start..end]);
        cur = end;
    }

    if let Some((start, end)) = adm.title_range {
        if start > cur {
            builder.token(SyntaxKind::WHITESPACE.into(), &line[cur..start]);
        }
        builder.token(SyntaxKind::ADMONITION_TITLE.into(), &line[start..end]);
        cur = end;
    }

    if cur < line.len() {
        builder.token(SyntaxKind::WHITESPACE.into(), &line[cur..]);
    }
    if !newline.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline);
    }
}

pub(crate) struct IndentedCodeBlockParser;

impl BlockParser for IndentedCodeBlockParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let content = lines.first();
        let line_pos = lines.pos();
        let lines = lines.raw();
        let allow_marker_only = ctx.in_marker_only_list_item;
        let allow = if allow_marker_only {
            true
        } else if ctx.config.dialect == crate::options::Dialect::CommonMark {
            // A completed block can be followed by indented code without a
            // blank line, but buffered paragraph text cannot be interrupted.
            ctx.has_blank_before
                || ctx.at_document_start
                || (!ctx.paragraph_open && !ctx.list_item_content_open)
        } else {
            ctx.has_blank_before_strict
                || (!ctx.paragraph_open
                    && (prev_line_is_terminal_one_liner(lines, line_pos, ctx.blockquote_depth)
                        || (!ctx.list_item_content_open
                            && prev_line_closed_a_fence(
                                lines,
                                line_pos,
                                ctx.blockquote_depth,
                                ctx.config.dialect,
                            ))))
        };
        if !allow {
            return None;
        }

        let list_content_col = ctx
            .list_indent_info
            .map(|list_info| list_info.content_col)
            .unwrap_or(0);
        let required_indent = list_content_col + 4;

        let (indent_cols, _) = leading_indent(content);
        if indent_cols < required_indent
            && try_parse_list_marker(content, ctx.config, ctx.open_alpha_hint).is_some()
        {
            return None;
        }

        if indent_cols < required_indent || !is_indented_code_line(content) {
            return None;
        }

        let detection = if allow_marker_only {
            BlockDetectionResult::YesCanInterrupt
        } else {
            BlockDetectionResult::Yes
        };
        Some((detection, None))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        _payload: Option<&dyn Any>,
    ) -> usize {
        let line_pos = lines.pos();
        let lines = lines.raw();
        let base_indent = ctx.content_indent
            + ctx
                .list_indent_info
                .map(|list_info| list_info.content_col)
                .unwrap_or(0);

        let new_pos =
            parse_indented_code_block(builder, lines, line_pos, ctx.blockquote_depth, base_indent);
        new_pos - line_pos
    }

    fn name(&self) -> &'static str {
        "indented_code_block"
    }
}

pub(crate) struct SetextHeadingParser;

impl BlockParser for SetextHeadingParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let stripped = lines;
        let content = lines.first();
        let line_pos = lines.pos();
        let lines = lines.raw();
        let next_line_raw = lines.get(line_pos + 1).copied();
        let follows_setext_heading =
            if line_pos >= 2 && !ctx.paragraph_open && !ctx.list_item_content_open {
                let prev_text = count_blockquote_markers(lines[line_pos - 2]).1;
                let prev_underline = count_blockquote_markers(lines[line_pos - 1]).1;
                try_parse_setext_heading(&[prev_text, prev_underline], 0).is_some()
            } else {
                false
            };

        let requires_blank_before = ctx.config.extensions.blank_before_header
            || ctx.config.dialect == crate::options::Dialect::Pandoc;
        if requires_blank_before
            && !ctx.has_blank_before
            && !ctx.at_document_start
            && !follows_setext_heading
        {
            return None;
        }

        let next_line = ctx.next_line?;

        {
            let bytes = next_line.as_bytes();
            let mut i = 0;
            while i < bytes.len() && i < 3 && bytes[i] == b' ' {
                i += 1;
            }
            match bytes.get(i) {
                Some(&b'=') | Some(&b'-') => {}
                _ => return None,
            }
        }

        let lines = [content, next_line];

        // An underline inside a held shortcode belongs to its opaque payload.
        if ctx.config.extensions.hugo_shortcodes
            && super::blocks::hugo::prepare(stripped, true, None).is_some()
        {
            return None;
        }

        if try_parse_setext_heading(&lines, 0).is_some() {
            if ctx.config.dialect == crate::options::Dialect::CommonMark
                && try_parse_horizontal_rule(content).is_some()
            {
                return None;
            }
            if ctx.config.dialect == crate::options::Dialect::CommonMark
                && ctx.config.extensions.reference_links
                && try_parse_reference_definition_with_options(content, ctx.config).is_some()
            {
                return None;
            }
            let text_bq_depth = match ctx.config.dialect {
                crate::options::Dialect::CommonMark => {
                    ctx.blockquote_depth + count_blockquote_markers(content).0
                }
                _ => ctx.blockquote_depth,
            };
            if next_line_raw.map_or(0, |line| count_blockquote_markers(line).0) != text_bq_depth {
                return None;
            }
            if let Some(list_info) = ctx.list_indent_info {
                let (next_indent_cols, _) = leading_indent(next_line);
                if next_indent_cols < list_info.content_col {
                    return None;
                }
            }
            Some((BlockDetectionResult::Yes, None))
        } else {
            None
        }
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        _payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        let text_line = lines.dispatch_tail();

        builder.start_node(SyntaxKind::HEADING.into());
        emit_setext_heading_text(builder, text_line, ctx.config);
        let underline_line = lines.emit_or_dispatch_tail(builder, lines.dispatch_pos() + 1);
        emit_setext_underline(builder, underline_line);
        builder.finish_node(); // HEADING

        2
    }

    fn name(&self) -> &'static str {
        "setext_heading"
    }
}

/// Whether the immediately-previous source line (after stripping `expected_bq_depth`
/// blockquote markers) is itself a complete one-liner block — currently an ATX
/// heading or a horizontal rule. Used by the indented-code-block dispatcher
/// under Pandoc dialect to allow `# Heading\n    foo` (and the analogous HR
/// case) to emit a CodeBlock without requiring an intervening blank line,
/// matching pandoc's behavior. Returns false on lazy-blockquote-continuation
/// lines (where the prev line is paragraph-like content rather than a
/// self-contained block).
fn prev_line_is_terminal_one_liner(
    lines: &[&str],
    line_pos: usize,
    expected_bq_depth: usize,
) -> bool {
    if line_pos == 0 {
        return false;
    }
    let prev_line = lines[line_pos - 1];
    let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
    if prev_bq_depth != expected_bq_depth {
        return false;
    }
    let (prev_inner_no_nl, _) = strip_newline(prev_inner);
    try_parse_atx_heading(prev_inner_no_nl).is_some()
        || try_parse_horizontal_rule(prev_inner_no_nl).is_some()
}

/// Whether the immediately-previous source line (after stripping
/// `expected_bq_depth` blockquote markers) closed a fenced code block, making
/// that block a complete neighbour the way an ATX heading or an HR is.
///
/// Only the shape is checked, because at a block start that shape can only be a
/// closer: an *opener* on the previous line would have made this line the
/// block's content instead, and a fence that failed to open one is paragraph or
/// list-item text, which the caller excludes. A closer carries no info string,
/// so one that does is some other fence entirely.
fn prev_line_closed_a_fence(
    lines: &[&str],
    line_pos: usize,
    expected_bq_depth: usize,
    dialect: crate::options::Dialect,
) -> bool {
    if line_pos == 0 {
        return false;
    }
    let prev_line = lines[line_pos - 1];
    let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
    if prev_bq_depth != expected_bq_depth {
        return false;
    }
    let (prev_inner_no_nl, _) = strip_newline(prev_inner);
    try_parse_fence_open(prev_inner_no_nl, dialect)
        .is_some_and(|fence| fence.info_string.trim().is_empty())
}

/// Registry of block parsers, ordered by priority.
///
/// This dispatcher tries each parser in order until one succeeds.
/// The ordering follows Pandoc's approach - explicit list order rather
/// than numeric priorities.
pub(crate) struct BlockParserRegistry {
    parsers: Vec<Box<dyn BlockParser>>,
    /// Index of [`BlockQuoteParser`] in `parsers`, cached so callers can
    /// compare a match's rank against it. See [`Self::outranks_blockquote`].
    blockquote_index: usize,
}

impl BlockParserRegistry {
    /// Return the registry shared by every parser in this process.
    pub fn shared() -> &'static Self {
        static REGISTRY: OnceLock<BlockParserRegistry> = OnceLock::new();

        REGISTRY.get_or_init(Self::new)
    }

    /// Create a new registry with all block parsers.
    ///
    /// Order matters! Parsers are tried in the order listed here.
    /// This follows Pandoc's design where ordering is explicit and documented.
    ///
    /// **Pandoc reference order** (from pandoc/src/Text/Pandoc/Readers/Markdown.hs:487-515):
    /// 1. blanklines (handled separately in our parser)
    /// 2. codeBlockFenced
    /// 3. yamlMetaBlock' ← YAML metadata comes early!
    /// 4. bulletList
    /// 5. divHtml
    /// 6. divFenced
    /// 7. header ← ATX and Setext headers
    /// 8. lhsCodeBlock
    /// 9. htmlBlock
    /// 10. table
    /// 11. codeBlockIndented
    /// 12. rawTeXBlock (LaTeX)
    /// 13. lineBlock
    /// 14. blockQuote
    /// 15. hrule ← Horizontal rules come AFTER headers!
    /// 16. orderedList
    /// 17. definitionList
    /// 18. noteBlock (footnotes)
    /// 19. referenceKey ← Reference definitions
    /// 20. abbrevKey
    /// 21. para
    /// 22. plain
    pub fn new() -> Self {
        let parsers: Vec<Box<dyn BlockParser>> = vec![
            Box::new(PandocTitleBlockParser),
            Box::new(MmdTitleBlockParser),
            Box::new(MystDirectiveCloseParser),
            Box::new(MystDirectiveOpenParser),
            Box::new(FencedCodeBlockParser),
            Box::new(YamlMetadataParser),
            // Front matter wins over MyST block breaks only at document start.
            Box::new(super::blocks::hugo::TomlMetadataParser),
            Box::new(MystBlockBreakParser),
            Box::new(ListParser),
            Box::new(FencedDivCloseParser),
            Box::new(FencedDivOpenParser),
            Box::new(MystTargetParser),
            Box::new(MystCommentParser),
            Box::new(SetextHeadingParser),
            Box::new(AtxHeadingParser),
            Box::new(HtmlBlockParser),
            Box::new(SvelteBlockParser),
            // Hugo calls protect their closing lines and arbitrary template bodies.
            Box::new(super::blocks::hugo::HugoShortcodeParser),
            Box::new(TableParser),
            Box::new(AdmonitionOpenParser),
            Box::new(IndentedCodeBlockParser),
            Box::new(LatexEnvironmentParser),
            Box::new(RawTexBlockParser),
            Box::new(LineBlockParser),
            Box::new(BlockQuoteParser),
            Box::new(HorizontalRuleParser),
            Box::new(DefinitionListParser),
            Box::new(FootnoteDefinitionParser),
            Box::new(ReferenceDefinitionParser),
        ];

        let blockquote_index = parsers
            .iter()
            .position(|p| matches!(p.effect(), BlockEffect::OpenBlockQuote))
            .expect("registry must contain a blockquote parser");

        Self {
            parsers,
            blockquote_index,
        }
    }

    /// True when `block_match`'s parser precedes [`BlockQuoteParser`] in the
    /// registry — pandoc's reader would have tried it before `blockQuote`,
    /// so it wins the line.
    ///
    /// Rank, not effect, is the right test: `BlockQuoteParser` declines
    /// outright once `ctx.blockquote_depth > 0`, so at any probe depth
    /// `k >= 1` its silence is an artifact rather than a verdict, and
    /// "the winner isn't the blockquote parser" would wrongly let
    /// lower-ranked parsers (definition lists, thematic breaks) claim a
    /// line pandoc keeps inside the quote.
    pub fn outranks_blockquote(&self, block_match: &PreparedBlockMatch) -> bool {
        block_match.parser_index < self.blockquote_index
    }

    /// Like `detect()`, but allows parsers to return cached payload for emission.
    pub fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<PreparedBlockMatch> {
        for (i, parser) in self.parsers.iter().enumerate() {
            if let Some((detection, payload)) = parser.detect_prepared(ctx, lines) {
                log::trace!("Block detected by: {}", parser.name());
                return Some(PreparedBlockMatch {
                    parser_index: i,
                    detection,
                    effect: parser.effect(),
                    payload,
                });
            }
        }
        None
    }

    pub fn parser_name(&self, block_match: &PreparedBlockMatch) -> &'static str {
        self.parsers[block_match.parser_index].name()
    }

    pub fn parse_prepared(
        &self,
        block_match: &PreparedBlockMatch,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
    ) -> usize {
        let parser = &self.parsers[block_match.parser_index];
        log::trace!("Block parsed by: {}", parser.name());
        parser.parse_prepared(ctx, builder, lines, block_match.payload.as_deref())
    }
}

#[cfg(test)]
mod registry_tests {
    use super::BlockParserRegistry;

    #[test]
    fn shared_registry_is_a_singleton() {
        let first = BlockParserRegistry::shared();
        let second = BlockParserRegistry::shared();

        assert!(std::ptr::eq(first, second));
    }
}

#[cfg(test)]
mod svelte_block_tests {
    use super::{SvelteBlockParser, SvelteKind};

    #[test]
    fn detects_block_logic_line() {
        let info = SvelteBlockParser::detect_line("{#each items as item}\n").unwrap();
        assert_eq!(info.kind, SvelteKind::BlockLogic);
        assert_eq!(info.indent_len, 0);
        assert_eq!(info.content, "#each items as item");
    }

    #[test]
    fn detects_tag_and_expression_lines() {
        assert_eq!(
            SvelteBlockParser::detect_line("{@html body}\n")
                .unwrap()
                .kind,
            SvelteKind::Tag
        );
        assert_eq!(
            SvelteBlockParser::detect_line("{count}\n").unwrap().kind,
            SvelteKind::Expression
        );
    }

    #[test]
    fn accepts_up_to_three_leading_spaces() {
        let info = SvelteBlockParser::detect_line("   {/if}\n").unwrap();
        assert_eq!(info.indent_len, 3);
    }

    #[test]
    fn rejects_four_leading_spaces() {
        assert!(SvelteBlockParser::detect_line("    {/if}\n").is_none());
    }

    #[test]
    fn accepts_trailing_whitespace_after_span() {
        assert!(SvelteBlockParser::detect_line("{/if}   \n").is_some());
    }

    #[test]
    fn rejects_trailing_text_after_span() {
        assert!(SvelteBlockParser::detect_line("{count} today\n").is_none());
    }

    #[test]
    fn rejects_unbalanced_span() {
        assert!(SvelteBlockParser::detect_line("{#if x\n").is_none());
    }

    #[test]
    fn rejects_shortcode_opener() {
        assert!(SvelteBlockParser::detect_line("{{< meta x >}}\n").is_none());
    }
}
