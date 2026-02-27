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

use crate::config::Config;
use rowan::GreenNodeBuilder;
use std::any::Any;

use super::blocks::blockquotes::{
    can_start_blockquote, count_blockquote_markers, strip_n_blockquote_markers,
};
use super::blocks::code_blocks::{
    CodeBlockType, FenceInfo, InfoString, parse_fenced_code_block, try_parse_fence_open,
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
use super::blocks::latex_envs::{LatexEnvInfo, parse_latex_environment, try_parse_latex_env_begin};
use super::blocks::line_blocks::{parse_line_block, try_parse_line_block_start};
use super::blocks::lists::{ListMarker, is_content_nested_bullet_marker, try_parse_list_marker};
use super::blocks::metadata::{try_parse_pandoc_title_block, try_parse_yaml_block};
use super::blocks::reference_links::{try_parse_footnote_marker, try_parse_reference_definition};
use super::blocks::tables::{
    is_caption_followed_by_table, try_parse_grid_table, try_parse_multiline_table,
    try_parse_pipe_table, try_parse_simple_table,
};
use super::inlines::links::try_parse_inline_image;
use super::utils::container_stack::byte_index_at_column;
use super::utils::helpers::strip_newline;
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
    pub config: &'a Config,

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

    /// Next line content for lookahead (used by setext headings)
    pub next_line: Option<&'a str>,
}

/// Result of detecting whether a block can be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockDetectionResult {
    /// Can parse this block, requires blank line before
    Yes,

    /// Can parse this block and can interrupt paragraphs (no blank line needed)
    #[allow(dead_code)]
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
    /// Detect if this parser can handle the content (no emission).
    fn can_parse(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockDetectionResult;

    fn effect(&self) -> BlockEffect {
        BlockEffect::None
    }

    /// Prepared detection hook.
    ///
    /// Default implementation just calls `can_parse()` and returns no payload.
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let detection = self.can_parse(ctx, lines, line_pos);
        match detection {
            BlockDetectionResult::Yes | BlockDetectionResult::YesCanInterrupt => {
                Some((detection, None))
            }
            BlockDetectionResult::No => None,
        }
    }

    /// Parse and emit this block type to the builder.
    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize;

    /// Prepared parse hook.
    ///
    /// Default implementation ignores payload and calls `parse()`.
    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
        _payload: Option<&dyn Any>,
    ) -> usize {
        self.parse(ctx, builder, lines, line_pos)
    }

    /// Name of this block parser (for debugging/logging)
    fn name(&self) -> &'static str;
}

// ============================================================================
// Concrete Block Parser Implementations
// ============================================================================

/// Horizontal rule parser
pub(crate) struct HorizontalRuleParser;

impl BlockParser for HorizontalRuleParser {
    fn can_parse(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> BlockDetectionResult {
        // Must have blank line before
        if !ctx.has_blank_before {
            return BlockDetectionResult::No;
        }

        // Check if this looks like a horizontal rule
        if try_parse_horizontal_rule(ctx.content).is_some() {
            BlockDetectionResult::Yes
        } else {
            BlockDetectionResult::No
        }
    }

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        // Use ctx.content (blockquote markers already stripped)
        // But preserve newline from original line
        let (_, newline_str) = strip_newline(lines[line_pos]);
        let content_with_newline = if !newline_str.is_empty() {
            format!("{}{}", ctx.content.trim_end(), newline_str)
        } else {
            ctx.content.to_string()
        };

        emit_horizontal_rule(builder, &content_with_newline);
        1 // Consumed 1 line
    }

    fn name(&self) -> &'static str {
        "horizontal_rule"
    }
}

/// ATX heading parser (# Heading)
pub(crate) struct AtxHeadingParser;

impl BlockParser for AtxHeadingParser {
    fn can_parse(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockDetectionResult {
        self.detect_prepared(ctx, lines, line_pos)
            .map(|(d, _)| d)
            .unwrap_or(BlockDetectionResult::No)
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.has_blank_before {
            return None;
        }

        let level = try_parse_atx_heading(ctx.content)?;
        Some((BlockDetectionResult::Yes, Some(Box::new(level))))
    }

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        self.parse_prepared(ctx, builder, lines, line_pos, None)
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
        payload: Option<&dyn Any>,
    ) -> usize {
        let line = lines[line_pos];
        let heading_level = payload
            .and_then(|p| p.downcast_ref::<usize>().copied())
            .or_else(|| try_parse_atx_heading(ctx.content))
            .unwrap_or(1);
        emit_atx_heading(builder, line, heading_level, ctx.config);
        1
    }

    fn name(&self) -> &'static str {
        "atx_heading"
    }
}

/// Pandoc title block parser (% Title ...)
pub(crate) struct PandocTitleBlockParser;

impl BlockParser for PandocTitleBlockParser {
    fn can_parse(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockDetectionResult {
        self.detect_prepared(ctx, lines, line_pos)
            .map(|(d, _)| d)
            .unwrap_or(BlockDetectionResult::No)
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
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

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        self.parse_prepared(ctx, builder, lines, line_pos, None)
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

/// YAML metadata block parser (--- ... ---/...)
pub(crate) struct YamlMetadataParser;

impl BlockParser for YamlMetadataParser {
    fn can_parse(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockDetectionResult {
        self.detect_prepared(ctx, lines, line_pos)
            .map(|(d, _)| d)
            .unwrap_or(BlockDetectionResult::No)
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        // Must be at top level (not inside blockquotes)
        if ctx.blockquote_depth > 0 {
            return None;
        }

        // Must start with ---
        if ctx.content.trim() != "---" {
            return None;
        }

        // YAML needs blank line before OR be at document start
        if !ctx.has_blank_before && !ctx.at_document_start {
            return None;
        }

        // Look ahead: next line must NOT be blank (to distinguish from horizontal rule)
        let next_line = lines.get(line_pos + 1)?;
        if next_line.trim().is_empty() {
            // This is a horizontal rule, not YAML
            return None;
        }

        // Cache the `at_document_start` flag for emission (avoids any ambiguity if ctx changes).
        Some((
            BlockDetectionResult::Yes,
            Some(Box::new(ctx.at_document_start)),
        ))
    }

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        self.parse_prepared(ctx, builder, lines, line_pos, None)
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
        payload: Option<&dyn Any>,
    ) -> usize {
        let at_document_start = payload
            .and_then(|p| p.downcast_ref::<bool>().copied())
            .unwrap_or(ctx.at_document_start);

        if let Some(new_pos) = try_parse_yaml_block(lines, line_pos, builder, at_document_start) {
            new_pos - line_pos
        } else {
            1
        }
    }

    fn name(&self) -> &'static str {
        "yaml_metadata"
    }
}

/// Figure parser (standalone image on its own line)
pub(crate) struct FigureParser;

impl BlockParser for FigureParser {
    fn can_parse(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockDetectionResult {
        self.detect_prepared(ctx, lines, line_pos)
            .map(|(d, _)| d)
            .unwrap_or(BlockDetectionResult::No)
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
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
        let (len, _alt, _dest, _attrs) = try_parse_inline_image(trimmed)?;
        let after_image = &trimmed[len..];
        if !after_image.trim().is_empty() {
            return None;
        }

        Some((BlockDetectionResult::Yes, Some(Box::new(len))))
    }

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        self.parse_prepared(ctx, builder, lines, line_pos, None)
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

#[derive(Debug, Clone)]
pub(crate) struct FootnoteDefinitionPrepared {
    pub content_start: usize,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct BlockQuotePrepared {
    pub depth: usize,
    pub marker_info: Vec<crate::parser::utils::marker_utils::BlockQuoteMarkerInfo>,
    pub inner_content: String,
    pub can_start: bool,
    pub can_nest: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ListPrepared {
    pub marker: ListMarker,
    pub marker_len: usize,
    pub spaces_after: usize,
    pub indent_cols: usize,
    pub indent_bytes: usize,
    pub nested_marker: Option<char>,
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

    fn can_parse(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockDetectionResult {
        self.detect_prepared(ctx, lines, line_pos)
            .map(|(d, _)| d)
            .unwrap_or(BlockDetectionResult::No)
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        let (marker, marker_len, spaces_after) = try_parse_list_marker(ctx.content, ctx.config)?;
        if spaces_after == 0 {
            return None;
        }
        if (ctx.has_blank_before || ctx.at_document_start)
            && try_parse_horizontal_rule(ctx.content).is_some()
        {
            return None;
        }
        let (indent_cols, indent_bytes) =
            super::utils::container_stack::leading_indent(ctx.content);

        if indent_cols >= 4 && !ctx.in_list {
            return None;
        }

        let nested_marker = is_content_nested_bullet_marker(ctx.content, marker_len, spaces_after);
        let detection = if ctx.has_blank_before || ctx.at_document_start {
            BlockDetectionResult::Yes
        } else {
            BlockDetectionResult::YesCanInterrupt
        };

        Some((
            detection,
            Some(Box::new(ListPrepared {
                marker,
                marker_len,
                spaces_after,
                indent_cols,
                indent_bytes,
                nested_marker,
            })),
        ))
    }

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        self.parse_prepared(ctx, builder, lines, line_pos, None)
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

    fn can_parse(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockDetectionResult {
        self.detect_prepared(ctx, lines, line_pos)
            .map(|(d, _)| d)
            .unwrap_or(BlockDetectionResult::No)
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
        let can_start = can_start_blockquote(line_pos, lines);

        let can_nest = depth <= 1 || at_document_start || {
            let prev_line = lines.get(line_pos.wrapping_sub(1)).unwrap_or(&"");
            let (prev_depth, prev_inner) = count_blockquote_markers(prev_line);
            prev_depth > 0 && prev_inner.trim().is_empty()
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

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        self.parse_prepared(ctx, builder, lines, line_pos, None)
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        _builder: &mut GreenNodeBuilder<'static>,
        _lines: &[&str],
        _line_pos: usize,
        _payload: Option<&dyn Any>,
    ) -> usize {
        // Core handles blockquote emission; consuming 0 lines here ensures we don't double-emit.
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

    fn can_parse(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockDetectionResult {
        self.detect_prepared(ctx, lines, line_pos)
            .map(|(d, _)| d)
            .unwrap_or(BlockDetectionResult::No)
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if let Some((marker_char, indent, spaces_after)) = try_parse_definition_marker(ctx.content)
        {
            let has_content = !ctx.content[indent + 1 + spaces_after..].trim().is_empty();
            return Some((
                BlockDetectionResult::YesCanInterrupt,
                Some(Box::new(DefinitionPrepared::Definition {
                    marker_char,
                    indent,
                    spaces_after,
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

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        self.parse_prepared(ctx, builder, lines, line_pos, None)
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

    fn can_parse(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> BlockDetectionResult {
        self.detect_prepared(ctx, _lines, _line_pos)
            .map(|(d, _)| d)
            .unwrap_or(BlockDetectionResult::No)
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

        let (_id, content_start) = try_parse_footnote_marker(ctx.content)?;
        Some((
            BlockDetectionResult::YesCanInterrupt,
            Some(Box::new(FootnoteDefinitionPrepared { content_start })),
        ))
    }

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        self.parse_prepared(ctx, builder, lines, line_pos, None)
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
        builder.token(SyntaxKind::FOOTNOTE_REFERENCE.into(), marker_text);

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

    fn can_parse(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockDetectionResult {
        self.detect_prepared(ctx, lines, line_pos)
            .map(|(d, _)| d)
            .unwrap_or(BlockDetectionResult::No)
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        // Parse once and cache for emission.
        let parsed = try_parse_reference_definition(ctx.content)?;
        Some((BlockDetectionResult::Yes, Some(Box::new(parsed))))
    }

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        self.parse_prepared(ctx, builder, lines, line_pos, None)
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
        payload: Option<&dyn Any>,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        builder.start_node(SyntaxKind::REFERENCE_DEFINITION.into());

        let full_line = lines[line_pos];
        let (content_without_newline, line_ending) = strip_newline(full_line);

        // Detection already cached the parsed tuple; emission should not need to re-parse.
        // If payload is missing (legacy callsites), we fall back to the old raw emission.
        debug_assert!(
            payload
                .and_then(|p| p.downcast_ref::<(usize, String, String, Option<String>)>())
                .is_some()
        );

        emit_reference_definition_content(builder, content_without_newline);

        if !line_ending.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), line_ending);
        }

        builder.finish_node();

        1
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

    fn can_parse(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockDetectionResult {
        self.detect_prepared(ctx, lines, line_pos)
            .map(|(d, _)| d)
            .unwrap_or(BlockDetectionResult::No)
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !(ctx.config.extensions.simple_tables
            || ctx.config.extensions.multiline_tables
            || ctx.config.extensions.grid_tables
            || ctx.config.extensions.pipe_tables)
        {
            return None;
        }

        if !ctx.has_blank_before && !ctx.at_document_start {
            return None;
        }

        // Correctness first: only claim a match if a real parse would succeed.
        // (Otherwise we can steal list items/paragraphs and drop content.)
        let mut tmp = GreenNodeBuilder::new();

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
                    BlockDetectionResult::Yes,
                    Some(Box::new(TablePrepared {
                        kind: TableKind::Grid,
                    })),
                ));
            }

            if ctx.config.extensions.multiline_tables
                && try_parse_multiline_table(lines, table_pos, &mut tmp, ctx.config).is_some()
            {
                return Some((
                    BlockDetectionResult::Yes,
                    Some(Box::new(TablePrepared {
                        kind: TableKind::Multiline,
                    })),
                ));
            }

            if ctx.config.extensions.pipe_tables
                && try_parse_pipe_table(lines, table_pos, &mut tmp, ctx.config).is_some()
            {
                return Some((
                    BlockDetectionResult::Yes,
                    Some(Box::new(TablePrepared {
                        kind: TableKind::Pipe,
                    })),
                ));
            }

            if ctx.config.extensions.simple_tables
                && try_parse_simple_table(lines, table_pos, &mut tmp, ctx.config).is_some()
            {
                return Some((
                    BlockDetectionResult::Yes,
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

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        self.parse_prepared(ctx, builder, lines, line_pos, None)
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

        let table_pos = if ctx.config.extensions.table_captions
            && is_caption_followed_by_table(lines, line_pos)
        {
            // Skip caption continuation lines and one optional blank line.
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

        let try_kind =
            |kind: TableKind, builder: &mut GreenNodeBuilder<'static>| -> Option<usize> {
                match kind {
                    TableKind::Grid => {
                        if ctx.config.extensions.grid_tables {
                            try_parse_grid_table(lines, table_pos, builder, ctx.config)
                        } else {
                            None
                        }
                    }
                    TableKind::Multiline => {
                        if ctx.config.extensions.multiline_tables {
                            try_parse_multiline_table(lines, table_pos, builder, ctx.config)
                        } else {
                            None
                        }
                    }
                    TableKind::Pipe => {
                        if ctx.config.extensions.pipe_tables {
                            try_parse_pipe_table(lines, table_pos, builder, ctx.config)
                        } else {
                            None
                        }
                    }
                    TableKind::Simple => {
                        if ctx.config.extensions.simple_tables {
                            try_parse_simple_table(lines, table_pos, builder, ctx.config)
                        } else {
                            None
                        }
                    }
                }
            };

        if let Some(prepared) = prepared
            && let Some(n) = try_kind(prepared.kind, builder)
        {
            return n;
        }

        // Fallback (should be rare) - match core order.
        if let Some(n) = try_kind(TableKind::Grid, builder) {
            return n;
        }
        if let Some(n) = try_kind(TableKind::Multiline, builder) {
            return n;
        }
        if let Some(n) = try_kind(TableKind::Pipe, builder) {
            return n;
        }
        if let Some(n) = try_kind(TableKind::Simple, builder) {
            return n;
        }

        debug_assert!(false, "TableParser::parse called without a matching table");
        1
    }

    fn name(&self) -> &'static str {
        "table"
    }
}

/// Helper function to emit reference definition content with inline structure.
fn emit_reference_definition_content(builder: &mut GreenNodeBuilder<'static>, text: &str) {
    use crate::syntax::SyntaxKind;

    if !text.starts_with('[') {
        builder.token(SyntaxKind::TEXT.into(), text);
        return;
    }

    let rest = &text[1..];
    if let Some(close_pos) = rest.find(']') {
        let label = &rest[..close_pos];
        let after_bracket = &rest[close_pos + 1..];

        if after_bracket.starts_with(':') {
            // Emit LINK node with the label
            builder.start_node(SyntaxKind::LINK.into());

            builder.start_node(SyntaxKind::LINK_START.into());
            builder.token(SyntaxKind::LINK_START.into(), "[");
            builder.finish_node();

            builder.start_node(SyntaxKind::LINK_TEXT.into());
            builder.token(SyntaxKind::TEXT.into(), label);
            builder.finish_node();

            builder.token(SyntaxKind::TEXT.into(), "]");
            builder.finish_node(); // LINK

            builder.token(SyntaxKind::TEXT.into(), after_bracket);
            return;
        }
    }

    builder.token(SyntaxKind::TEXT.into(), text);
}

/// Fenced code block parser (``` or ~~~)
pub(crate) struct FencedCodeBlockParser;

impl BlockParser for FencedCodeBlockParser {
    fn can_parse(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> BlockDetectionResult {
        self.detect_prepared(ctx, _lines, _line_pos)
            .map(|(d, _)| d)
            .unwrap_or(BlockDetectionResult::No)
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
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

        // Parse info string to determine block type (expensive, but now cached via fence)
        let info = InfoString::parse(&fence.info_string);

        let is_executable = matches!(info.block_type, CodeBlockType::Executable { .. });
        let is_pandoc_like = matches!(
            ctx.config.flavor,
            crate::config::Flavor::Pandoc
                | crate::config::Flavor::CommonMark
                | crate::config::Flavor::Gfm
        );
        if is_executable && is_pandoc_like {
            return None;
        }

        // Fenced code blocks can interrupt paragraphs only if they have an info string.
        let has_info = !fence.info_string.trim().is_empty();
        let detection = if has_info {
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

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        self.parse_prepared(ctx, builder, lines, line_pos, None)
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
            // Backward-compat: if called via legacy `parse()`, recompute.
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

        let new_pos = parse_fenced_code_block(
            builder,
            lines,
            line_pos,
            fence,
            ctx.blockquote_depth,
            total_indent,
        );

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
    fn can_parse(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockDetectionResult {
        self.detect_prepared(ctx, lines, line_pos)
            .map(|(d, _)| d)
            .unwrap_or(BlockDetectionResult::No)
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.raw_html {
            return None;
        }

        let block_type = try_parse_html_block_start(ctx.content)?;

        // Match previous behavior (and Pandoc-ish semantics): HTML blocks can interrupt
        // paragraphs; blank lines are not required.
        let detection = if ctx.has_blank_before || ctx.at_document_start {
            BlockDetectionResult::Yes
        } else {
            BlockDetectionResult::YesCanInterrupt
        };

        Some((detection, Some(Box::new(block_type))))
    }

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        self.parse_prepared(ctx, builder, lines, line_pos, None)
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
        payload: Option<&dyn Any>,
    ) -> usize {
        let block_type = if let Some(bt) = payload.and_then(|p| p.downcast_ref::<HtmlBlockType>()) {
            bt.clone()
        } else {
            try_parse_html_block_start(ctx.content).expect("HTML block type should exist")
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
    fn can_parse(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockDetectionResult {
        self.detect_prepared(ctx, lines, line_pos)
            .map(|(d, _)| d)
            .unwrap_or(BlockDetectionResult::No)
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.raw_tex {
            return None;
        }

        let env_info = try_parse_latex_env_begin(ctx.content)?;

        // Like HTML blocks, raw TeX blocks should be able to interrupt paragraphs.
        let detection = if ctx.has_blank_before || ctx.at_document_start {
            BlockDetectionResult::Yes
        } else {
            BlockDetectionResult::YesCanInterrupt
        };

        Some((detection, Some(Box::new(env_info))))
    }

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        self.parse_prepared(ctx, builder, lines, line_pos, None)
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
        payload: Option<&dyn Any>,
    ) -> usize {
        let env_info = if let Some(info) = payload.and_then(|p| p.downcast_ref::<LatexEnvInfo>()) {
            info.clone()
        } else {
            try_parse_latex_env_begin(ctx.content).expect("LaTeX env info should exist")
        };

        let new_pos =
            parse_latex_environment(builder, lines, line_pos, env_info, ctx.blockquote_depth);
        new_pos - line_pos
    }

    fn name(&self) -> &'static str {
        "latex_environment"
    }
}

// ============================================================================
// Line Block Parser (position #13)
// ============================================================================

pub(crate) struct LineBlockParser;

impl BlockParser for LineBlockParser {
    fn can_parse(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockDetectionResult {
        self.detect_prepared(ctx, lines, line_pos)
            .map(|(d, _)| d)
            .unwrap_or(BlockDetectionResult::No)
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.line_blocks {
            return None;
        }

        try_parse_line_block_start(ctx.content)?;

        // Line blocks can interrupt paragraphs.
        let detection = if ctx.has_blank_before || ctx.at_document_start {
            BlockDetectionResult::Yes
        } else {
            BlockDetectionResult::YesCanInterrupt
        };

        Some((detection, None))
    }

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        self.parse_prepared(ctx, builder, lines, line_pos, None)
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

impl BlockParser for FencedDivOpenParser {
    fn effect(&self) -> BlockEffect {
        BlockEffect::OpenFencedDiv
    }

    fn can_parse(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> BlockDetectionResult {
        self.detect_prepared(ctx, _lines, _line_pos)
            .map(|(d, _)| d)
            .unwrap_or(BlockDetectionResult::No)
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

        if !ctx.has_blank_before && !ctx.at_document_start {
            return None;
        }

        let div_fence = try_parse_div_fence_open(ctx.content)?;
        Some((BlockDetectionResult::Yes, Some(Box::new(div_fence))))
    }

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        self.parse_prepared(ctx, builder, lines, line_pos, None)
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
            .or_else(|| try_parse_div_fence_open(ctx.content));

        let Some(div_fence) = div_fence else {
            return 1;
        };

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

        // Trailing colons (symmetric fences)
        let (trailing_space, trailing_colons) = if div_fence.attributes.starts_with('{') {
            if let Some(close_idx) = content_after_space.find('}') {
                let after_attrs = &content_after_space[close_idx + 1..];
                let trailing = after_attrs.trim_start();
                let space_count = after_attrs.len() - trailing.len();
                if !trailing.is_empty() && trailing.chars().all(|c| c == ':') {
                    (space_count > 0, trailing)
                } else {
                    (false, "")
                }
            } else {
                (false, "")
            }
        } else {
            let after_attrs = &content_after_space[div_fence.attributes.len()..];
            if let Some(after_space) = after_attrs.strip_prefix(' ') {
                if !after_space.is_empty() && after_space.chars().all(|c| c == ':') {
                    (true, after_space)
                } else {
                    (false, "")
                }
            } else {
                (false, "")
            }
        };

        if trailing_space {
            builder.token(SyntaxKind::WHITESPACE.into(), " ");
        }
        if !trailing_colons.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), trailing_colons);
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

    fn can_parse(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> BlockDetectionResult {
        self.detect_prepared(ctx, _lines, _line_pos)
            .map(|(d, _)| d)
            .unwrap_or(BlockDetectionResult::No)
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

        if !is_div_closing_fence(ctx.content) {
            return None;
        }

        Some((BlockDetectionResult::YesCanInterrupt, None))
    }

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        self.parse_prepared(ctx, builder, lines, line_pos, None)
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
    fn can_parse(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> BlockDetectionResult {
        self.detect_prepared(ctx, _lines, _line_pos)
            .map(|(d, _)| d)
            .unwrap_or(BlockDetectionResult::No)
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        // Indented code blocks require a strict blank line before (or doc start).
        if !ctx.has_blank_before_strict {
            return None;
        }

        // Don't treat as code if it's a list marker (list takes precedence).
        if try_parse_list_marker(ctx.content, ctx.config).is_some() {
            return None;
        }

        if !is_indented_code_line(ctx.content) {
            return None;
        }

        Some((BlockDetectionResult::Yes, None))
    }

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        self.parse_prepared(ctx, builder, lines, line_pos, None)
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
        _payload: Option<&dyn Any>,
    ) -> usize {
        let new_pos = parse_indented_code_block(
            builder,
            lines,
            line_pos,
            ctx.blockquote_depth,
            ctx.content_indent,
        );
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
    fn can_parse(
        &self,
        ctx: &BlockContext,
        _lines: &[&str],
        _line_pos: usize,
    ) -> BlockDetectionResult {
        // Setext headings require blank line before (unless at document start)
        if !ctx.has_blank_before && !ctx.at_document_start {
            return BlockDetectionResult::No;
        }

        // Need next line for lookahead
        let next_line = match ctx.next_line {
            Some(line) => line,
            None => return BlockDetectionResult::No,
        };

        // Create lines array for detection function (avoid allocation)
        let lines = [ctx.content, next_line];

        // Try to detect setext heading
        if try_parse_setext_heading(&lines, 0).is_some() {
            // Setext headings need blank line before (normal case)
            BlockDetectionResult::Yes
        } else {
            BlockDetectionResult::No
        }
    }

    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        pos: usize,
    ) -> usize {
        // Get text line and underline line
        let text_line = lines[pos];
        let underline_line = lines[pos + 1];

        // Determine level from underline character (no need to call try_parse again)
        // can_parse() already validated this is a valid setext heading
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
            // (12) LaTeX environment blocks
            Box::new(LatexEnvironmentParser),
            // (10) Tables
            Box::new(TableParser),
            // (13) Line blocks
            Box::new(LineBlockParser),
            // (14) Block quotes (detection-only for now)
            Box::new(BlockQuoteParser),
            // (11) Indented code blocks (AFTER fenced!)
            Box::new(IndentedCodeBlockParser),
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
            // TODO: Migrate remaining blocks in Pandoc order:
            // - (4-6) Lists and divs (bulletList, divHtml)
            // - (16) Ordered lists
            // - (17) Definition lists
        ];

        Self { parsers }
    }

    /// Try to parse a block using the registered parsers.
    ///
    /// This method implements the two-phase parsing:
    /// 1. Detection: Check if any parser can handle this content
    /// 2. Caller prepares (closes paragraphs, flushes buffers)
    /// 3. Parser emits the block
    ///
    /// Returns (parser_index, detection_result) if a parser can handle this,
    /// or None if no parser matched.
    #[allow(dead_code)]
    pub fn detect(
        &self,
        ctx: &BlockContext,
        lines: &[&str],
        line_pos: usize,
    ) -> Option<(usize, BlockDetectionResult)> {
        for (i, parser) in self.parsers.iter().enumerate() {
            let result = parser.can_parse(ctx, lines, line_pos);
            match result {
                BlockDetectionResult::Yes | BlockDetectionResult::YesCanInterrupt => {
                    log::debug!("Block detected by: {}", parser.name());
                    return Some((i, result));
                }
                BlockDetectionResult::No => continue,
            }
        }
        None
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
                log::debug!("Block detected by: {}", parser.name());
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

    /// Parse a block using the specified parser (by index from detect()).
    ///
    /// Should only be called after detect() returns Some and after
    /// caller has prepared for the block element.
    #[allow(dead_code)]
    pub fn parse(
        &self,
        parser_index: usize,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        let parser = &self.parsers[parser_index];
        log::debug!("Block parsed by: {}", parser.name());
        parser.parse(ctx, builder, lines, line_pos)
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
        log::debug!("Block parsed by: {}", parser.name());
        parser.parse_prepared(
            ctx,
            builder,
            lines,
            line_pos,
            block_match.payload.as_deref(),
        )
    }
}
