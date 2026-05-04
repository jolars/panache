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

use crate::options::ParserOptions;
use rowan::GreenNodeBuilder;
use std::any::Any;

use super::blocks::blockquotes::{
    can_start_blockquote, count_blockquote_markers, emit_one_blockquote_marker,
    strip_n_blockquote_markers,
};
use super::blocks::code_blocks::{
    CodeBlockType, FenceInfo, InfoString, is_closing_fence, is_gfm_math_fence,
    parse_fenced_code_block, parse_fenced_math_block, try_parse_fence_open,
};
use super::blocks::definition_lists::{
    next_line_is_definition_marker, try_parse_definition_marker,
};
use super::blocks::fenced_divs::{DivFenceInfo, is_div_closing_fence, try_parse_div_fence_open};
use super::blocks::figures::parse_figure;
use super::blocks::headings::{
    emit_atx_heading, emit_setext_heading, try_parse_atx_heading, try_parse_setext_heading,
};
use super::blocks::horizontal_rules::{emit_horizontal_rule, try_parse_horizontal_rule};
use super::blocks::html_blocks::{HtmlBlockType, parse_html_block, try_parse_html_block_start};
use super::blocks::indented_code::{is_indented_code_line, parse_indented_code_block};
use super::blocks::latex_envs::LatexEnvInfo;
use super::blocks::line_blocks::{parse_line_block, try_parse_line_block_start};
use super::blocks::lists::{
    ListDelimiter, ListMarker, OrderedMarker, is_content_nested_bullet_marker,
    try_parse_list_marker,
};
use super::blocks::metadata::{
    emit_yaml_block, find_yaml_block_closing_pos, try_parse_mmd_title_block,
    try_parse_pandoc_title_block, try_parse_yaml_block,
};
use super::blocks::raw_blocks;
use super::blocks::raw_blocks::extract_environment_name;
use super::blocks::reference_links::{
    line_is_mmd_link_attribute_continuation, try_parse_footnote_marker,
    try_parse_reference_definition, try_parse_reference_definition_lax,
};
use super::blocks::tables::{
    is_caption_followed_by_table, try_parse_grid_table, try_parse_multiline_table,
    try_parse_pipe_table, try_parse_simple_table,
};
use super::inlines::links::{LinkScanContext, try_parse_inline_image};
use super::utils::container_stack::{byte_index_at_column, leading_indent};
use super::utils::helpers::{strip_newline, trim_end_newlines};
use super::utils::marker_utils::parse_blockquote_marker_info;

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
    /// Current line content (after blockquote markers stripped if any)
    pub content: &'a str,

    /// Whether there was a blank line before this line (relaxed, container-aware)
    pub has_blank_before: bool,

    /// Whether there was a strict blank line before this line (no container exceptions)
    pub has_blank_before_strict: bool,

    /// Whether we're currently inside a fenced div (container-owned state)
    pub in_fenced_div: bool,

    /// Whether we're at document start (pos == 0)
    pub at_document_start: bool,

    /// Current blockquote depth
    pub blockquote_depth: usize,

    /// Parser configuration
    pub config: &'a ParserOptions,

    // NOTE: we intentionally do not store `&ContainerStack` here to avoid
    // long-lived borrows of `self` in the main parser loop.
    /// Base indentation from container context (footnotes, definitions)
    pub content_indent: usize,

    /// Indentation stripped from the current line that should be emitted for losslessness
    pub indent_to_emit: Option<&'a str>,

    /// List indentation info if inside a list
    pub list_indent_info: Option<ListIndentInfo>,

    /// Whether we're currently inside any list
    pub in_list: bool,

    /// Whether the immediate enclosing container is a list item that has so
    /// far seen only its marker (no content yet). Equivalent to the
    /// `marker_only` flag on `Container::ListItem`. Used by indented code
    /// detection so that the line *after* an empty list marker can still
    /// open an indented code block when its indent is ≥ content_col + 4,
    /// even though there is no blank line separating the marker line from
    /// the indented line.
    pub in_marker_only_list_item: bool,

    /// Whether a `Container::Paragraph` is currently open and buffering
    /// content. When `true`, the *previous* source line was buffered as
    /// paragraph text — even if its shape would have been a heading or HR
    /// in isolation — so paragraph-non-interrupting blocks (notably
    /// indented code under Pandoc) must treat it as paragraph continuation,
    /// not as a "terminal one-liner" that opens a new section.
    pub paragraph_open: bool,

    /// Next line content for lookahead (used by setext headings)
    pub next_line: Option<&'a str>,
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
pub(crate) trait BlockParser {
    fn effect(&self) -> BlockEffect {
        BlockEffect::None
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)>;

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
        payload: Option<&dyn Any>,
    ) -> usize;

    /// Name of this block parser (for debugging/logging)
    fn name(&self) -> &'static str;
}

// ============================================================================
// Concrete Block Parser Implementations
// ============================================================================

/// Horizontal rule parser
pub(crate) struct HorizontalRuleParser;

impl BlockParser for HorizontalRuleParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        // CommonMark §4.1: thematic breaks can interrupt a paragraph (no
        // blank line required). Pandoc-markdown disagrees and treats a
        // would-be thematic break inside a paragraph as plain text. Branch
        // on dialect.
        let common_mark = ctx.config.dialect == crate::options::Dialect::CommonMark;
        if !common_mark && !ctx.has_blank_before {
            return None;
        }

        // Check if this looks like a horizontal rule
        if try_parse_horizontal_rule(ctx.content).is_some() {
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
        _lines: &[&str],
        _line_pos: usize,
        _payload: Option<&dyn Any>,
    ) -> usize {
        emit_horizontal_rule(builder, ctx.content);
        1 // Consumed 1 line
    }

    fn name(&self) -> &'static str {
        "horizontal_rule"
    }
}

/// ATX heading parser (# Heading)
pub(crate) struct AtxHeadingParser;

impl BlockParser for AtxHeadingParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if ctx.config.extensions.blank_before_header && !ctx.has_blank_before {
            return None;
        }

        let level = try_parse_atx_heading(ctx.content)?;
        // CommonMark §4.2: an ATX heading can interrupt a paragraph (no blank
        // line required between the paragraph and the heading). Pandoc-markdown
        // disagrees: without a blank line, `# foo` inside a paragraph stays
        // text. Branch on dialect — `YesCanInterrupt` triggers the dispatcher
        // path that closes an open paragraph before emitting the heading.
        let detection = match ctx.config.dialect {
            crate::options::Dialect::CommonMark => BlockDetectionResult::YesCanInterrupt,
            crate::options::Dialect::Pandoc => BlockDetectionResult::Yes,
        };
        Some((detection, Some(Box::new(level))))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        _lines: &[&str],
        _line_pos: usize,
        payload: Option<&dyn Any>,
    ) -> usize {
        let heading_level = payload
            .and_then(|p| p.downcast_ref::<usize>().copied())
            .or_else(|| try_parse_atx_heading(ctx.content))
            .unwrap_or(1);
        emit_atx_heading(builder, ctx.content, heading_level, ctx.config);
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
        _lines: &[&str],
        line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.pandoc_title_block {
            return None;
        }

        // Must be at document start.
        if !ctx.at_document_start || line_pos != 0 {
            return None;
        }

        // Must start with % (allow leading spaces).
        if !ctx.content.trim_start().starts_with('%') {
            return None;
        }

        Some((BlockDetectionResult::Yes, None))
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
        _payload: Option<&dyn Any>,
    ) -> usize {
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
        _lines: &[&str],
        line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.mmd_title_block {
            return None;
        }

        // Must be at top-level document start.
        if !ctx.at_document_start || line_pos != 0 || ctx.blockquote_depth > 0 {
            return None;
        }

        // Quick guard to avoid work on obvious non-matches.
        if ctx.content.trim().is_empty() || !ctx.content.contains(':') {
            return None;
        }

        Some((BlockDetectionResult::Yes, None))
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
        _payload: Option<&dyn Any>,
    ) -> usize {
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
}

impl BlockParser for YamlMetadataParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.yaml_metadata_block {
            return None;
        }

        // Must be at top level (not inside blockquotes)
        if ctx.blockquote_depth > 0 {
            return None;
        }

        // Must start with ---
        if ctx.content.trim() != "---" {
            return None;
        }

        // Fast guard: mid-document YAML requires a preceding blank line.
        if !ctx.has_blank_before && !ctx.at_document_start {
            return None;
        }

        // Look ahead: next line must NOT be blank (to distinguish from horizontal rule)
        let next_line = lines.get(line_pos + 1)?;
        if next_line.trim().is_empty() {
            // This is a horizontal rule, not YAML
            return None;
        }

        let closing_pos = find_yaml_block_closing_pos(lines, line_pos, ctx.at_document_start)?;

        // Cache the `at_document_start` flag for emission (avoids any ambiguity if ctx changes).
        Some((
            BlockDetectionResult::Yes,
            Some(Box::new(YamlMetadataPrepared {
                at_document_start: ctx.at_document_start,
                closing_pos,
            })),
        ))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
        payload: Option<&dyn Any>,
    ) -> usize {
        if let Some(prepared) = payload.and_then(|p| p.downcast_ref::<YamlMetadataPrepared>())
            && let Some(new_pos) = emit_yaml_block(lines, line_pos, prepared.closing_pos, builder)
        {
            return new_pos - line_pos;
        }

        let at_document_start = payload
            .and_then(|p| p.downcast_ref::<YamlMetadataPrepared>())
            .map(|p| p.at_document_start)
            .unwrap_or(ctx.at_document_start);
        try_parse_yaml_block(lines, line_pos, builder, at_document_start)
            .map(|new_pos| new_pos - line_pos)
            .unwrap_or(1)
    }

    fn name(&self) -> &'static str {
        "yaml_metadata"
    }
}

/// Figure parser (standalone image on its own line)
pub(crate) struct FigureParser;

impl BlockParser for FigureParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        // Pandoc-only behavior; CommonMark/GFM keep the image inline within
        // the paragraph and do not promote it to a figure block.
        if !ctx.config.extensions.implicit_figures {
            return None;
        }

        // Must have blank line before
        if !ctx.has_blank_before {
            return None;
        }

        let trimmed = ctx.content.trim();

        // Must start with ![
        if !trimmed.starts_with("![") {
            return None;
        }

        // Run the expensive inline-image validation once here.
        let (len, _alt, _dest, _attrs) =
            try_parse_inline_image(trimmed, LinkScanContext::from_options(ctx.config))?;
        let after_image = &trimmed[len..];
        if !after_image.trim().is_empty() {
            return None;
        }

        Some((BlockDetectionResult::Yes, Some(Box::new(len))))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
        payload: Option<&dyn Any>,
    ) -> usize {
        // If detection succeeded, we already validated that this is a standalone image.
        // Payload currently only caches the parsed length (future-proofing).
        let _len = payload.and_then(|p| p.downcast_ref::<usize>().copied());

        let line = lines[line_pos];
        parse_figure(builder, line, ctx.config);
        1
    }

    fn name(&self) -> &'static str {
        "figure"
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
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let marker_match = try_parse_list_marker(ctx.content, ctx.config)?;
        let after_marker_text = {
            let (_, indent_bytes) = super::utils::container_stack::leading_indent(ctx.content);
            let marker_end = indent_bytes + marker_match.marker_len;
            if marker_end <= ctx.content.len() {
                &ctx.content[marker_end..]
            } else {
                ""
            }
        };
        if marker_match.spaces_after_cols == 0 {
            // The marker parser allows two cases with zero trailing whitespace:
            // a bare marker (no content after on this line) or a
            // task-checkbox immediately following the marker. Only the bare
            // marker is a real list opener; reject the task-checkbox case.
            // (Trailing CR/LF is not "content" for this check.)
            if !trim_end_newlines(after_marker_text).is_empty() {
                return None;
            }
            // CommonMark: an empty list item cannot interrupt a paragraph at
            // document level. Inside an existing list a bare marker still
            // opens a sibling list item.
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
            && try_parse_horizontal_rule(ctx.content).is_some()
        {
            return None;
        }
        let (indent_cols, indent_bytes) =
            super::utils::container_stack::leading_indent(ctx.content);
        if !ctx.has_blank_before
            && ctx.in_list
            && let Some(list_indent) = ctx.list_indent_info
            && list_indent.content_col >= 4
            && indent_cols == list_indent.content_col
            && indent_cols <= 4
        {
            let should_suppress = match &marker_match.marker {
                ListMarker::Ordered(OrderedMarker::Decimal {
                    number,
                    style: ListDelimiter::Parens | ListDelimiter::Period,
                }) => number != "1",
                ListMarker::Ordered(OrderedMarker::LowerAlpha {
                    style: ListDelimiter::Parens,
                    ..
                })
                | ListMarker::Ordered(OrderedMarker::UpperAlpha {
                    style: ListDelimiter::Parens,
                    ..
                })
                | ListMarker::Ordered(OrderedMarker::LowerRoman {
                    style: ListDelimiter::Parens,
                    ..
                })
                | ListMarker::Ordered(OrderedMarker::UpperRoman {
                    style: ListDelimiter::Parens,
                    ..
                }) => true,
                _ => false,
            };

            if should_suppress {
                return None;
            }
        }

        if indent_cols >= 4 && !ctx.in_list {
            return None;
        }

        let nested_marker = is_content_nested_bullet_marker(
            ctx.content,
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
        _lines: &[&str],
        _line_pos: usize,
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
        lines: &[&str],
        line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
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
        let can_start = !require_blank_before || can_start_blockquote(line_pos, lines);

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
        _lines: &[&str],
        _line_pos: usize,
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
        lines: &[&str],
        line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.definition_lists {
            return None;
        }

        if let Some((marker_char, indent, spaces_after_cols, spaces_after_bytes)) =
            try_parse_definition_marker(ctx.content)
        {
            // If this `:` line is actually a table caption marker and a table
            // follows, let TableParser claim it instead of starting a definition list.
            if marker_char == ':'
                && ctx.config.extensions.table_captions
                && is_caption_followed_by_table(lines, line_pos)
            {
                return None;
            }

            let indent_bytes =
                super::utils::container_stack::byte_index_at_column(ctx.content, indent);
            let has_content = ctx
                .content
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

        if let Some(blank_count) = next_line_is_definition_marker(lines, line_pos)
            && !ctx.content.trim().is_empty()
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
        _lines: &[&str],
        _line_pos: usize,
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

/// Footnote definition parser ([^id]: content)
pub(crate) struct FootnoteDefinitionParser;

impl BlockParser for FootnoteDefinitionParser {
    fn effect(&self) -> BlockEffect {
        BlockEffect::OpenFootnoteDefinition
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.footnotes {
            return None;
        }

        // A footnote def starts with `[^` after no leading indent.
        if !ctx.content.starts_with("[^") {
            return None;
        }

        let (_id, content_start) = try_parse_footnote_marker(ctx.content)?;
        Some((
            BlockDetectionResult::YesCanInterrupt,
            Some(Box::new(FootnoteDefinitionPrepared { content_start })),
        ))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        _lines: &[&str],
        _line_pos: usize,
        payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        let prepared = payload.and_then(|p| p.downcast_ref::<FootnoteDefinitionPrepared>());
        let content_start = prepared
            .map(|p| p.content_start)
            .or_else(|| try_parse_footnote_marker(ctx.content).map(|(_, pos)| pos));

        let Some(content_start) = content_start else {
            return 1;
        };

        if let Some(indent_str) = ctx.indent_to_emit {
            builder.token(SyntaxKind::WHITESPACE.into(), indent_str);
        }

        builder.start_node(SyntaxKind::FOOTNOTE_DEFINITION.into());
        let marker_text = &ctx.content[..content_start];
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
        lines: &[&str],
        line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.reference_links {
            return None;
        }

        // Cheap leading-byte gate: a reference definition starts with `[`
        // after up to 3 leading spaces (CommonMark §4.7). Bail before the
        // multi-line String::new() build below if the gate fails — this
        // is the by-far common case on a typical doc.
        {
            let bytes = ctx.content.as_bytes();
            let mut i = 0;
            while i < bytes.len() && i < 3 && bytes[i] == b' ' {
                i += 1;
            }
            if bytes.get(i) != Some(&b'[') {
                return None;
            }
        }

        // Build a multi-line candidate from consecutive non-blank lines so the
        // ref-def parser can recognize destinations and titles that wrap across
        // lines (CommonMark §4.7). Blank lines terminate the definition, so we
        // stop the input there.
        //
        // Inside blockquotes, the raw `lines` carry the `>` markers. The
        // dispatcher already strips them into `ctx.content`, but a multi-line
        // join here would feed those markers back to the parser. Fall back to
        // a single-line attempt in that case — multi-line ref defs inside
        // blockquotes are tracked separately.
        type RefDefParseFn =
            fn(&str, crate::options::Dialect) -> Option<(usize, String, String, Option<String>)>;
        let parse_fn: RefDefParseFn = if ctx.config.extensions.mmd_link_attributes {
            try_parse_reference_definition_lax
        } else {
            try_parse_reference_definition
        };
        let dialect = ctx.config.dialect;

        let consumed = if ctx.blockquote_depth > 0 {
            parse_fn(ctx.content, dialect)?;
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

            let (bytes_consumed, _label, _url, _title) = parse_fn(&multi, dialect)?;

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
        lines: &[&str],
        line_pos: usize,
        payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        builder.start_node(SyntaxKind::REFERENCE_DEFINITION.into());

        let consumed_lines = payload
            .and_then(|p| p.downcast_ref::<ReferenceDefinitionPrepared>())
            .map(|p| p.consumed_lines)
            .unwrap_or(1);

        // Inside a blockquote, BLOCK_QUOTE_MARKER + WHITESPACE were already
        // emitted by the dispatcher; using lines[line_pos] would duplicate the
        // `>` marker (CST losslessness violation). detect_prepared restricts
        // blockquote-context defs to a single line, so we can rely on
        // ctx.content here.
        if ctx.blockquote_depth > 0 {
            let single = [ctx.content];
            emit_reference_definition_lines(builder, &single);
        } else {
            let target_lines: Vec<&str> = lines
                .iter()
                .skip(line_pos)
                .take(consumed_lines)
                .copied()
                .collect();
            emit_reference_definition_lines(builder, &target_lines);
        }

        builder.finish_node();

        consumed_lines
    }

    fn name(&self) -> &'static str {
        "reference_definition"
    }
}

// ============================================================================
// Table Parser (position #10)
// ============================================================================

pub(crate) struct TableParser;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableKind {
    Grid,
    Multiline,
    Pipe,
    Simple,
}

#[derive(Debug, Clone, Copy)]
struct TablePrepared {
    kind: TableKind,
}

impl BlockParser for TableParser {
    fn effect(&self) -> BlockEffect {
        BlockEffect::None
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
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

        // Correctness first: only claim a match if a real parse would succeed.
        // (Otherwise we can steal list items/paragraphs and drop content.)
        let mut tmp = GreenNodeBuilder::new();

        let detection = if ctx.has_blank_before || ctx.at_document_start {
            BlockDetectionResult::Yes
        } else {
            BlockDetectionResult::YesCanInterrupt
        };

        // Handle caption-before-table lines by matching the *table kind* starting
        // after the caption, but parsing from the caption line so the caption is
        // included and consumed.
        if ctx.config.extensions.table_captions && is_caption_followed_by_table(lines, line_pos) {
            // Skip caption continuation lines and one optional blank line.
            let mut table_pos = line_pos + 1;
            while table_pos < lines.len() && !lines[table_pos].trim().is_empty() {
                table_pos += 1;
            }
            if table_pos < lines.len() && lines[table_pos].trim().is_empty() {
                table_pos += 1;
            }

            if ctx.config.extensions.grid_tables
                && try_parse_grid_table(lines, table_pos, &mut tmp, ctx.config).is_some()
            {
                return Some((
                    detection,
                    Some(Box::new(TablePrepared {
                        kind: TableKind::Grid,
                    })),
                ));
            }

            if ctx.config.extensions.multiline_tables
                && try_parse_multiline_table(lines, table_pos, &mut tmp, ctx.config).is_some()
            {
                return Some((
                    detection,
                    Some(Box::new(TablePrepared {
                        kind: TableKind::Multiline,
                    })),
                ));
            }

            if ctx.config.extensions.pipe_tables
                && try_parse_pipe_table(lines, table_pos, &mut tmp, ctx.config).is_some()
            {
                return Some((
                    detection,
                    Some(Box::new(TablePrepared {
                        kind: TableKind::Pipe,
                    })),
                ));
            }

            if ctx.config.extensions.simple_tables
                && try_parse_simple_table(lines, table_pos, &mut tmp, ctx.config).is_some()
            {
                return Some((
                    detection,
                    Some(Box::new(TablePrepared {
                        kind: TableKind::Simple,
                    })),
                ));
            }

            return None;
        }

        if ctx.config.extensions.grid_tables
            && try_parse_grid_table(lines, line_pos, &mut tmp, ctx.config).is_some()
        {
            return Some((
                BlockDetectionResult::Yes,
                Some(Box::new(TablePrepared {
                    kind: TableKind::Grid,
                })),
            ));
        }

        if ctx.config.extensions.multiline_tables
            && try_parse_multiline_table(lines, line_pos, &mut tmp, ctx.config).is_some()
        {
            return Some((
                BlockDetectionResult::Yes,
                Some(Box::new(TablePrepared {
                    kind: TableKind::Multiline,
                })),
            ));
        }

        if ctx.config.extensions.pipe_tables
            && try_parse_pipe_table(lines, line_pos, &mut tmp, ctx.config).is_some()
        {
            return Some((
                BlockDetectionResult::Yes,
                Some(Box::new(TablePrepared {
                    kind: TableKind::Pipe,
                })),
            ));
        }

        if ctx.config.extensions.simple_tables
            && try_parse_simple_table(lines, line_pos, &mut tmp, ctx.config).is_some()
        {
            return Some((
                BlockDetectionResult::Yes,
                Some(Box::new(TablePrepared {
                    kind: TableKind::Simple,
                })),
            ));
        }

        // (Optional) Caption-only lookahead without table parse shouldn't match.
        // The real parsers already handle captions when invoked on the caption line.

        None
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
        payload: Option<&dyn Any>,
    ) -> usize {
        let prepared = payload.and_then(|p| p.downcast_ref::<TablePrepared>().copied());
        let caption_before_table =
            ctx.config.extensions.table_captions && is_caption_followed_by_table(lines, line_pos);
        let table_pos = if caption_before_table {
            let mut pos = line_pos + 1;
            while pos < lines.len() && !lines[pos].trim().is_empty() {
                pos += 1;
            }
            if pos < lines.len() && lines[pos].trim().is_empty() {
                pos += 1;
            }
            pos
        } else {
            line_pos
        };

        let try_kind_at = |kind: TableKind,
                           pos: usize,
                           builder: &mut GreenNodeBuilder<'static>|
         -> Option<usize> {
            match kind {
                TableKind::Grid => {
                    if ctx.config.extensions.grid_tables {
                        try_parse_grid_table(lines, pos, builder, ctx.config)
                    } else {
                        None
                    }
                }
                TableKind::Multiline => {
                    if ctx.config.extensions.multiline_tables {
                        try_parse_multiline_table(lines, pos, builder, ctx.config)
                    } else {
                        None
                    }
                }
                TableKind::Pipe => {
                    if ctx.config.extensions.pipe_tables {
                        try_parse_pipe_table(lines, pos, builder, ctx.config)
                    } else {
                        None
                    }
                }
                TableKind::Simple => {
                    if ctx.config.extensions.simple_tables {
                        try_parse_simple_table(lines, pos, builder, ctx.config)
                    } else {
                        None
                    }
                }
            }
        };

        if let Some(prepared) = prepared
            && let Some(n) = try_kind_at(prepared.kind, line_pos, builder)
        {
            return n;
        }
        if let Some(prepared) = prepared
            && caption_before_table
            && let Some(n) = try_kind_at(prepared.kind, table_pos, builder)
        {
            return n;
        }

        // Fallback (should be rare) - match core order.
        if let Some(n) = try_kind_at(TableKind::Grid, line_pos, builder) {
            return n;
        }
        if let Some(n) = try_kind_at(TableKind::Multiline, line_pos, builder) {
            return n;
        }
        if let Some(n) = try_kind_at(TableKind::Pipe, line_pos, builder) {
            return n;
        }
        if let Some(n) = try_kind_at(TableKind::Simple, line_pos, builder) {
            return n;
        }
        if caption_before_table {
            if let Some(n) = try_kind_at(TableKind::Grid, table_pos, builder) {
                return n;
            }
            if let Some(n) = try_kind_at(TableKind::Multiline, table_pos, builder) {
                return n;
            }
            if let Some(n) = try_kind_at(TableKind::Pipe, table_pos, builder) {
                return n;
            }
            if let Some(n) = try_kind_at(TableKind::Simple, table_pos, builder) {
                return n;
            }
        }

        debug_assert!(false, "TableParser::parse called without a matching table");
        1
    }

    fn name(&self) -> &'static str {
        "table"
    }
}

/// Emit a (possibly multi-line) reference definition's content tokens with
/// the appropriate inline structure: `WHITESPACE? LINK<LINK_START "[",
/// LINK_TEXT, "]"> TEXT NEWLINE? ...`. The LINK_TEXT may span multiple
/// lines via interleaved TEXT/NEWLINE tokens when the spec example uses a
/// multi-line label (e.g. `[Foo\n  bar]: /url`, CommonMark example #541).
///
/// The walker mirrors the escape-and-bracket logic in
/// `try_parse_reference_definition_with_mode` so the emit shape stays
/// consistent with detection.
///
/// On any structural mismatch (label > 3 spaces of indent, missing `[`,
/// no closing `]`, missing `:` after `]`), each input line is emitted
/// verbatim via `emit_line_tokens` to preserve CST losslessness.
fn emit_reference_definition_lines(builder: &mut GreenNodeBuilder<'static>, lines: &[&str]) {
    use crate::parser::utils::helpers::{emit_line_tokens, strip_newline};
    use crate::syntax::SyntaxKind;

    let fallback = |b: &mut GreenNodeBuilder<'static>| {
        for line in lines {
            emit_line_tokens(b, line);
        }
    };

    if lines.is_empty() {
        return;
    }

    let first = lines[0];
    let leading = first.chars().take_while(|&c| c == ' ').count();
    if leading > 3 || !first[leading..].starts_with('[') {
        fallback(builder);
        return;
    }

    let mut line_idx = 0usize;
    let mut col = leading + 1; // skip past `[`
    let mut escape_next = false;
    let close: Option<(usize, usize)> = 'outer: loop {
        let bytes = lines[line_idx].as_bytes();
        while col < bytes.len() {
            let b = bytes[col];
            if escape_next {
                escape_next = false;
                col += 1;
                continue;
            }
            match b {
                b'\\' => {
                    escape_next = true;
                    col += 1;
                }
                b']' => break 'outer Some((line_idx, col)),
                b'[' => break 'outer None,
                b'\r' | b'\n' => break,
                _ => col += 1,
            }
        }
        line_idx += 1;
        if line_idx >= lines.len() {
            break 'outer None;
        }
        col = 0;
    };

    let Some((close_line, close_col)) = close else {
        fallback(builder);
        return;
    };

    let after_close_col = close_col + 1;
    let close_bytes = lines[close_line].as_bytes();
    if after_close_col >= close_bytes.len() || close_bytes[after_close_col] != b':' {
        fallback(builder);
        return;
    }

    if leading > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &first[..leading]);
    }

    builder.start_node(SyntaxKind::LINK.into());

    builder.start_node(SyntaxKind::LINK_START.into());
    builder.token(SyntaxKind::LINK_START.into(), "[");
    builder.finish_node();

    builder.start_node(SyntaxKind::LINK_TEXT.into());
    if close_line == 0 {
        let label = &first[leading + 1..close_col];
        if !label.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), label);
        }
    } else {
        let (content0, nl0) = strip_newline(first);
        let part0 = &content0[leading + 1..];
        if !part0.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), part0);
        }
        if !nl0.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), nl0);
        }
        for line in &lines[1..close_line] {
            let (content, nl) = strip_newline(line);
            if !content.is_empty() {
                builder.token(SyntaxKind::TEXT.into(), content);
            }
            if !nl.is_empty() {
                builder.token(SyntaxKind::NEWLINE.into(), nl);
            }
        }
        let part_close = &lines[close_line][..close_col];
        if !part_close.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), part_close);
        }
    }
    builder.finish_node(); // LINK_TEXT

    builder.token(SyntaxKind::TEXT.into(), "]");
    builder.finish_node(); // LINK

    let after_close_text = &lines[close_line][after_close_col..];
    let (after_content, after_nl) = strip_newline(after_close_text);
    if !after_content.is_empty() {
        builder.token(SyntaxKind::TEXT.into(), after_content);
    }
    if !after_nl.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), after_nl);
    }
    for line in &lines[close_line + 1..] {
        emit_line_tokens(builder, line);
    }
}

/// Fenced code block parser (``` or ~~~)
pub(crate) struct FencedCodeBlockParser;

impl BlockParser for FencedCodeBlockParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        // Calculate content to check - may need to strip list indentation
        let content_to_check = if let Some(list_info) = ctx.list_indent_info {
            if list_info.content_col > 0 && !ctx.content.is_empty() {
                let idx = byte_index_at_column(ctx.content, list_info.content_col);
                &ctx.content[idx..]
            } else {
                ctx.content
            }
        } else {
            ctx.content
        };

        let fence = try_parse_fence_open(content_to_check)?;
        if (fence.fence_char == '`' && !ctx.config.extensions.backtick_code_blocks)
            || (fence.fence_char == '~' && !ctx.config.extensions.fenced_code_blocks)
        {
            return None;
        }

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

        // Parse info string to determine block type (expensive, but now cached via fence)
        let info = InfoString::parse(&fence.info_string);

        let is_executable = matches!(info.block_type, CodeBlockType::Executable { .. });
        if is_executable && !ctx.config.extensions.executable_code {
            return None;
        }

        // Fenced code blocks can interrupt paragraphs if they have an info string.
        // For bare fences (```), allow interruption only in explicit transcript-like
        // contexts and only when a matching closer exists later.
        let has_info = !fence.info_string.trim().is_empty();
        let has_matching_closer = {
            let mut found = false;
            let container_content_col = ctx.content_indent
                + ctx
                    .list_indent_info
                    .map(|list_info| list_info.content_col)
                    .unwrap_or(0);
            for raw_line in lines.iter().skip(line_pos + 1) {
                let (line_bq_depth, inner) = count_blockquote_markers(raw_line);
                if line_bq_depth < ctx.blockquote_depth {
                    break;
                }
                let candidate = if container_content_col > 0 && !inner.is_empty() {
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

        // CommonMark dialect: fenced code blocks always interrupt paragraphs and
        // run to end-of-document if the closing fence is missing (spec §4.5).
        // Pandoc dialect: bare fences without a closer fall through to a paragraph.
        let common_mark_dialect = ctx.config.dialect == crate::options::Dialect::CommonMark;
        if !has_matching_closer && !common_mark_dialect {
            return None;
        }

        let next_nonblank_is_command = lines
            .iter()
            .skip(line_pos + 1)
            .find(|l| !l.trim().is_empty())
            .is_some_and(|l| l.trim_start().starts_with('%'));
        let bare_fence_before_command_with_closer = has_matching_closer && next_nonblank_is_command;
        let bare_fence_after_colon_with_closer = has_matching_closer
            && next_nonblank_is_command
            && line_pos > 0
            && lines[line_pos - 1].trim_end().ends_with(':');
        let bare_fence_in_list_with_closer = has_matching_closer && ctx.list_indent_info.is_some();
        let bare_fence_after_matching_closer = has_matching_closer
            && next_nonblank_is_command
            && line_pos > 0
            && is_closing_fence(lines[line_pos - 1], &fence);

        let detection = if has_info
            || bare_fence_before_command_with_closer
            || bare_fence_after_colon_with_closer
            || bare_fence_in_list_with_closer
            || bare_fence_after_matching_closer
            || common_mark_dialect
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
        lines: &[&str],
        line_pos: usize,
        payload: Option<&dyn Any>,
    ) -> usize {
        let list_indent_stripped = ctx.list_indent_info.map(|i| i.content_col).unwrap_or(0);

        let fence = if let Some(fence) = payload.and_then(|p| p.downcast_ref::<FenceInfo>()) {
            fence.clone()
        } else {
            let content_to_check = if list_indent_stripped > 0 && !ctx.content.is_empty() {
                let idx = byte_index_at_column(ctx.content, list_indent_stripped);
                &ctx.content[idx..]
            } else {
                ctx.content
            };
            try_parse_fence_open(content_to_check).expect("Fence should exist")
        };

        // Calculate total indent: base content indent + list indent
        let total_indent = ctx.content_indent + list_indent_stripped;

        let new_pos = if ctx.config.extensions.tex_math_gfm && is_gfm_math_fence(&fence) {
            parse_fenced_math_block(
                builder,
                lines,
                line_pos,
                fence,
                ctx.blockquote_depth,
                total_indent,
                None,
            )
        } else {
            parse_fenced_code_block(
                builder,
                lines,
                line_pos,
                fence,
                ctx.blockquote_depth,
                total_indent,
                None,
            )
        };

        new_pos - line_pos
    }

    fn name(&self) -> &'static str {
        "fenced_code_block"
    }
}

// ============================================================================
// HTML Block Parser (position #9)
// ============================================================================

pub(crate) struct HtmlBlockParser;

impl BlockParser for HtmlBlockParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.raw_html {
            return None;
        }

        // HTML block must start with `<` after up to 3 leading spaces.
        {
            let bytes = ctx.content.as_bytes();
            let mut i = 0;
            while i < bytes.len() && i < 3 && bytes[i] == b' ' {
                i += 1;
            }
            if bytes.get(i) != Some(&b'<') {
                return None;
            }
        }

        let is_commonmark = ctx.config.dialect == crate::options::Dialect::CommonMark;
        let block_type = try_parse_html_block_start(ctx.content, is_commonmark)?;

        // Type 7 cannot interrupt a paragraph (CommonMark §4.6). Other
        // types can. Pandoc-dialect additionally treats HTML comments as
        // non-interrupting: a comment line directly following a paragraph
        // line (no blank above) stays inline as `RawInline (Format "html")`
        // rather than splitting the paragraph into a `RawBlock`.
        let cannot_interrupt = matches!(block_type, HtmlBlockType::Type7)
            || (matches!(block_type, HtmlBlockType::Comment)
                && ctx.config.dialect == crate::options::Dialect::Pandoc);
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
        lines: &[&str],
        line_pos: usize,
        payload: Option<&dyn Any>,
    ) -> usize {
        let is_commonmark = ctx.config.dialect == crate::options::Dialect::CommonMark;
        let block_type = if let Some(bt) = payload.and_then(|p| p.downcast_ref::<HtmlBlockType>()) {
            bt.clone()
        } else {
            try_parse_html_block_start(ctx.content, is_commonmark)
                .expect("HTML block type should exist")
        };

        let new_pos = parse_html_block(builder, lines, line_pos, block_type, ctx.blockquote_depth);
        new_pos - line_pos
    }

    fn name(&self) -> &'static str {
        "html_block"
    }
}

// ============================================================================
// LaTeX Environment Parser (position #12)
// ============================================================================

pub(crate) struct LatexEnvironmentParser;

impl BlockParser for LatexEnvironmentParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.raw_tex {
            return None;
        }

        let env_name = extract_environment_name(ctx.content)?.to_string();
        let env_info = LatexEnvInfo { env_name };

        // Skip inline math environments - they should be parsed inline in paragraphs
        // Import and use the function from raw_blocks module
        use super::blocks::raw_blocks::is_inline_math_environment;
        if is_inline_math_environment(&env_info.env_name) {
            return None;
        }

        // Like HTML blocks, raw TeX blocks should be able to interrupt paragraphs.
        let detection = if ctx.has_blank_before || ctx.at_document_start {
            BlockDetectionResult::Yes
        } else {
            BlockDetectionResult::YesCanInterrupt
        };

        Some((detection, Some(Box::new(env_info))))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
        payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        let env_info = if let Some(info) = payload.and_then(|p| p.downcast_ref::<LatexEnvInfo>()) {
            info.clone()
        } else {
            let env_name = extract_environment_name(ctx.content)
                .expect("LaTeX env info should exist")
                .to_string();
            LatexEnvInfo { env_name }
        };

        // Use TEX_BLOCK for all non-math environments
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

            // Emit the line content (strip newline)
            let content = trim_end_newlines(line);
            builder.token(SyntaxKind::TEXT.into(), content);

            current_pos += 1;

            // Check if this line contains the end marker
            if line.trim_start().starts_with(&end_marker) {
                break;
            }
        }

        // Emit final newline
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

// ============================================================================
// Raw TeX Block Parser (position #12)
// ============================================================================

pub(crate) struct RawTexBlockParser;

impl BlockParser for RawTexBlockParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.raw_tex {
            return None;
        }

        // Raw TeX blocks require blank line before (cannot interrupt paragraphs)
        // This is important to avoid intercepting display math content
        if !ctx.has_blank_before && !ctx.at_document_start {
            return None;
        }

        if !raw_blocks::can_start_raw_block(ctx.content, ctx.config) {
            return None;
        }

        Some((BlockDetectionResult::Yes, None))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
        _payload: Option<&dyn Any>,
    ) -> usize {
        raw_blocks::parse_raw_tex_block(builder, lines, line_pos, ctx.blockquote_depth)
    }

    fn name(&self) -> &'static str {
        "raw_tex_block"
    }
}

// ============================================================================
// Line Block Parser (position #13)
// ============================================================================

pub(crate) struct LineBlockParser;

impl BlockParser for LineBlockParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.line_blocks {
            return None;
        }

        try_parse_line_block_start(ctx.content)?;
        // Ensure the raw source line at the current parser position also starts
        // a line block marker. This prevents false positives when `ctx.content`
        // was stripped from container markers (e.g. blockquote prefixes).
        let raw_line = lines.get(line_pos)?;
        try_parse_line_block_start(raw_line)?;

        // Require a blank line (or document start) before a line block.
        // This prevents accidental line-block parsing for wrapped paragraph lines
        // that happen to start with "| ".
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
        lines: &[&str],
        line_pos: usize,
        _payload: Option<&dyn Any>,
    ) -> usize {
        let new_pos = parse_line_block(lines, line_pos, builder, ctx.config);
        new_pos - line_pos
    }

    fn name(&self) -> &'static str {
        "line_block"
    }
}

// ============================================================================
// Fenced Div Parsers (position #6)
// ============================================================================

pub(crate) struct FencedDivOpenParser;

fn content_for_fenced_div_detection<'a>(ctx: &BlockContext<'a>) -> &'a str {
    if let Some(list_info) = ctx.list_indent_info {
        let (indent_cols, _) = leading_indent(ctx.content);
        if indent_cols >= list_info.content_col {
            let idx = byte_index_at_column(ctx.content, list_info.content_col);
            return &ctx.content[idx..];
        }
    }
    ctx.content
}

impl BlockParser for FencedDivOpenParser {
    fn effect(&self) -> BlockEffect {
        BlockEffect::OpenFencedDiv
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.fenced_divs {
            return None;
        }

        let content = content_for_fenced_div_detection(ctx);
        // A fenced-div open fence starts with `:::` (Pandoc dialect)
        // after up to 3 leading spaces. Bail before the full
        // `try_parse_div_fence_open` scan when this byte gate fails.
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
        let div_fence = try_parse_div_fence_open(content)?;
        Some((BlockDetectionResult::Yes, Some(Box::new(div_fence))))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
        payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        let div_fence = payload
            .and_then(|p| p.downcast_ref::<DivFenceInfo>())
            .cloned()
            .or_else(|| try_parse_div_fence_open(content_for_fenced_div_detection(ctx)))
            .unwrap_or(DivFenceInfo {
                attributes: String::new(),
                fence_count: 3,
            });

        // Start FENCED_DIV node (container push happens in core based on `effect`).
        builder.start_node(SyntaxKind::FENCED_DIV.into());

        // Emit opening fence with attributes as child node to avoid duplication.
        builder.start_node(SyntaxKind::DIV_FENCE_OPEN.into());

        // Use full original line to preserve indentation and newline.
        let full_line = lines[line_pos];
        let line_no_bq = strip_n_blockquote_markers(full_line, ctx.blockquote_depth);
        let trimmed = line_no_bq.trim_start();

        // Leading whitespace
        let leading_ws_len = line_no_bq.len() - trimmed.len();
        if leading_ws_len > 0 {
            builder.token(SyntaxKind::WHITESPACE.into(), &line_no_bq[..leading_ws_len]);
        }

        // Fence colons
        let fence_str: String = ":".repeat(div_fence.fence_count);
        builder.token(SyntaxKind::TEXT.into(), &fence_str);

        // Everything after colons
        let after_colons = &trimmed[div_fence.fence_count..];
        let (content_before_newline, newline_str) = strip_newline(after_colons);

        if !div_fence.attributes.is_empty() {
            // Optional space before attributes
            let has_leading_space = content_before_newline.starts_with(' ');
            if has_leading_space {
                builder.token(SyntaxKind::WHITESPACE.into(), " ");
            }

            let content_after_space = if has_leading_space {
                &content_before_newline[1..]
            } else {
                content_before_newline
            };

            // Attributes
            builder.start_node(SyntaxKind::DIV_INFO.into());
            builder.token(SyntaxKind::TEXT.into(), &div_fence.attributes);
            builder.finish_node();

            // Preserve any suffix after attributes (e.g., trailing spaces, optional symmetric colons).
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
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.fenced_divs {
            return None;
        }

        if !ctx.in_fenced_div {
            return None;
        }

        if !is_div_closing_fence(content_for_fenced_div_detection(ctx)) {
            return None;
        }

        Some((BlockDetectionResult::YesCanInterrupt, None))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
        _payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

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

// ============================================================================
// Indented Code Block Parser (position #11)
// ============================================================================

pub(crate) struct IndentedCodeBlockParser;

impl BlockParser for IndentedCodeBlockParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        // CommonMark §4.4: indented code blocks cannot interrupt a paragraph,
        // but they CAN follow non-paragraph blocks (headings, fenced code,
        // HRs) without an intervening blank line. The relaxed
        // `has_blank_before` captures that "no continuation-eligible block is
        // open" signal — use it under CommonMark so `# Heading\n    foo`
        // correctly emits a code block (spec examples #115, #236, #252).
        //
        // Under Pandoc-markdown the construct diverges: a `>` blockquote with
        // an indented code line followed by an unmarked indented line lazily
        // extends the blockquote (verified with `pandoc -f markdown` for
        // `>     foo\n    bar`). Keep the literal strict gate there to avoid
        // regressing lazy-continuation behavior.
        //
        // Marker-only list items have no buffered content yet, so an indented
        // line on the *next* line cannot interrupt anything; allow the code
        // block to open under either dialect (spec example #278's third item:
        // `-\n      baz` → indented code block inside the list item). Both
        // dialects agree here (verified via `pandoc -f commonmark / -f
        // markdown`). Returned as `YesCanInterrupt` so the parser core flushes
        // the list-item buffer (which holds the marker line's trailing
        // newline) *before* emitting the code block, preserving lossless byte
        // ordering.
        let allow_marker_only = ctx.in_marker_only_list_item;
        let allow = if allow_marker_only {
            true
        } else if ctx.config.dialect == crate::options::Dialect::CommonMark {
            ctx.has_blank_before || ctx.at_document_start
        } else {
            // Pandoc dialect: strict literal blank, OR the previous source line
            // (at the same blockquote depth) was a complete one-liner block
            // (ATX heading or HR). Pandoc allows an indented code block to
            // immediately follow a heading or HR without an intervening blank
            // line; lazy-blockquote-continuation cases are still rejected
            // because their previous line is paragraph-like content, not a
            // self-contained block.
            //
            // The one-liner shortcut is purely textual, so it must additionally
            // require that no `Container::Paragraph` is currently buffering
            // content: if the parser already absorbed the heading-shaped line
            // as paragraph text (e.g. Pandoc's `blank_before_header` is on, or
            // the buffered line was indented past the heading limit), the
            // indented line that follows is paragraph continuation, not a new
            // code block.
            ctx.has_blank_before_strict
                || (!ctx.paragraph_open
                    && prev_line_is_terminal_one_liner(_lines, line_pos, ctx.blockquote_depth))
        };
        if !allow {
            return None;
        }

        let list_content_col = ctx
            .list_indent_info
            .map(|list_info| list_info.content_col)
            .unwrap_or(0);
        let required_indent = list_content_col + 4;

        let (indent_cols, _) = leading_indent(ctx.content);
        // Don't treat as code if it's a list marker and not indented enough for code.
        if indent_cols < required_indent && try_parse_list_marker(ctx.content, ctx.config).is_some()
        {
            return None;
        }

        if indent_cols < required_indent || !is_indented_code_line(ctx.content) {
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
        lines: &[&str],
        line_pos: usize,
        _payload: Option<&dyn Any>,
    ) -> usize {
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

// ============================================================================
// Setext Heading Parser (position #3)
// ============================================================================

pub(crate) struct SetextHeadingParser;

impl BlockParser for SetextHeadingParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        // Setext headings usually require blank line before (unless at document start),
        // but Pandoc also allows consecutive setext headings without an intervening blank line.
        let follows_setext_heading = if line_pos >= 2 {
            let prev_text = count_blockquote_markers(lines[line_pos - 2]).1;
            let prev_underline = count_blockquote_markers(lines[line_pos - 1]).1;
            try_parse_setext_heading(&[prev_text, prev_underline], 0).is_some()
        } else {
            false
        };

        if ctx.config.extensions.blank_before_header
            && !ctx.has_blank_before
            && !ctx.at_document_start
            && !follows_setext_heading
        {
            return None;
        }

        // Need next line for lookahead
        let next_line = ctx.next_line?;

        // Cheap leading-byte gate: a setext underline starts with `=` or
        // `-` after up to 3 spaces (CommonMark §4.3). Avoid the
        // `try_parse_setext_heading` re-scan when this can't fire — the
        // dispatcher runs SetextHeading on every non-blank line.
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

        // Create lines array for detection function (avoid allocation)
        let lines = [ctx.content, next_line];

        // Try to detect setext heading
        if try_parse_setext_heading(&lines, 0).is_some() {
            // CommonMark §4.3: a setext heading text line cannot itself be a
            // valid thematic break. Pandoc-markdown allows it (e.g. `***\n---`
            // becomes `<h2>***</h2>`), so this branch is dialect-gated.
            if ctx.config.dialect == crate::options::Dialect::CommonMark
                && try_parse_horizontal_rule(ctx.content).is_some()
            {
                return None;
            }
            // CommonMark §4.3 / §4.7: a setext heading text line cannot
            // itself be a reference definition — the ref-def takes priority,
            // and the underline becomes a separate paragraph line. Pandoc
            // disagrees: it consumes `[foo]: /url\n===\n` as an H1 with
            // text `[foo]: /url`, so this branch is dialect-gated.
            if ctx.config.dialect == crate::options::Dialect::CommonMark
                && ctx.config.extensions.reference_links
                && try_parse_reference_definition(ctx.content, ctx.config.dialect).is_some()
            {
                return None;
            }
            // CommonMark §4.3: the underline must be in the same container as
            // the text. If the text line is inside a blockquote (or nested
            // blockquotes) and the underline line is at a shallower depth,
            // the construct can't be a setext heading — the underline closes
            // the blockquote and (for `---` after a non-empty paragraph)
            // becomes a thematic break instead. Pandoc disagrees: it treats
            // `> foo\n---\n` as a top-level setext H2 with text `> foo`, so
            // gate on dialect.
            if ctx.config.dialect == crate::options::Dialect::CommonMark
                && count_blockquote_markers(next_line).0 != ctx.blockquote_depth
            {
                return None;
            }
            // Same-container rule for list items: if the text line is inside a
            // list item (content_col > 0) and the underline line's indent is
            // less than that content_col, the underline breaks out of the
            // list item — it's a sibling list marker (or HR / paragraph
            // continuation), not a setext underline. Both dialects agree on
            // this for the single-`-` case (`-\n  foo\n-\n` → two sibling
            // list items, not a setext heading), verified via
            // `pandoc -f commonmark` and `pandoc -f markdown`.
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
        lines: &[&str],
        pos: usize,
        _payload: Option<&dyn Any>,
    ) -> usize {
        // Get text line and underline line
        let text_line = lines[pos];
        let underline_line = lines[pos + 1];

        // Determine level from underline character (no need to call try_parse again)
        let underline_char = underline_line.trim().chars().next().unwrap_or('=');
        let level = if underline_char == '=' { 1 } else { 2 };

        // Emit the setext heading
        emit_setext_heading(builder, text_line, underline_line, level, ctx.config);

        // Return lines consumed: text line + underline line
        2
    }

    fn name(&self) -> &'static str {
        "setext_heading"
    }
}

// ============================================================================
// Helpers
// ============================================================================

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
    // Don't trim_start: the ATX/HR detectors enforce the ≤3-leading-space rule
    // themselves, and indented paragraph continuation lines that *look* like
    // headings (e.g. `                ## comment` inside buffered paragraph
    // text) must not be reported as terminal one-liners — otherwise an
    // indented code line that follows is wrongly allowed to interrupt the
    // open paragraph.
    try_parse_atx_heading(prev_inner_no_nl).is_some()
        || try_parse_horizontal_rule(prev_inner_no_nl).is_some()
}

// ============================================================================
// Block Parser Registry
// ============================================================================

/// Registry of block parsers, ordered by priority.
///
/// This dispatcher tries each parser in order until one succeeds.
/// The ordering follows Pandoc's approach - explicit list order rather
/// than numeric priorities.
pub(crate) struct BlockParserRegistry {
    parsers: Vec<Box<dyn BlockParser>>,
}

impl BlockParserRegistry {
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
            // Match Pandoc's ordering to ensure correct precedence:
            // (0) Pandoc title block (must be at document start).
            Box::new(PandocTitleBlockParser),
            // (0b) MultiMarkdown title block (must be at document start).
            // Pandoc title block remains first for precedence.
            Box::new(MmdTitleBlockParser),
            // (2) Fenced code blocks - can interrupt paragraphs!
            Box::new(FencedCodeBlockParser),
            // (3) YAML metadata - before headers and hrules!
            Box::new(YamlMetadataParser),
            // (4) Lists
            Box::new(ListParser),
            // (6) Fenced divs ::: (open/close)
            Box::new(FencedDivCloseParser),
            Box::new(FencedDivOpenParser),
            // (7) Setext headings (part of Pandoc's "header" parser)
            // Must come before ATX to properly handle `---` disambiguation
            Box::new(SetextHeadingParser),
            // (7) ATX headings (part of Pandoc's "header" parser)
            Box::new(AtxHeadingParser),
            // (9) HTML blocks
            Box::new(HtmlBlockParser),
            // (10) Tables
            Box::new(TableParser),
            // (11) Indented code blocks (AFTER fenced!)
            Box::new(IndentedCodeBlockParser),
            // (12) LaTeX environment blocks
            Box::new(LatexEnvironmentParser),
            // (12) Raw TeX blocks (macro definitions, etc.)
            Box::new(RawTexBlockParser),
            // (13) Line blocks
            Box::new(LineBlockParser),
            // (14) Block quotes (detection-only for now)
            Box::new(BlockQuoteParser),
            // (15) Horizontal rules - AFTER headings per Pandoc
            Box::new(HorizontalRuleParser),
            // Figures (standalone images) - Pandoc doesn't have these
            Box::new(FigureParser),
            // (17) Definition lists
            Box::new(DefinitionListParser),
            // (18) Footnote definitions (noteBlock)
            Box::new(FootnoteDefinitionParser),
            // (19) Reference definitions
            Box::new(ReferenceDefinitionParser),
        ];

        Self { parsers }
    }

    /// Like `detect()`, but allows parsers to return cached payload for emission.
    pub fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> Option<PreparedBlockMatch> {
        for (i, parser) in self.parsers.iter().enumerate() {
            if let Some((detection, payload)) = parser.detect_prepared(ctx, lines, line_pos) {
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
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        let parser = &self.parsers[block_match.parser_index];
        log::trace!("Block parsed by: {}", parser.name());
        parser.parse_prepared(
            ctx,
            builder,
            lines,
            line_pos,
            block_match.payload.as_deref(),
        )
    }
}
