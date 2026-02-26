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

use super::blocks::figures::{parse_figure, try_parse_figure};
use super::blocks::headings::{emit_atx_heading, try_parse_atx_heading};
use super::blocks::horizontal_rules::{emit_horizontal_rule, try_parse_horizontal_rule};
use super::blocks::reference_links::try_parse_reference_definition;
use super::utils::container_stack::ContainerStack;
use super::utils::helpers::strip_newline;

/// Context passed to block parsers for decision-making.
///
/// Contains immutable references to parser state that block parsers need
/// to check conditions (e.g., blank line before, blockquote depth, etc.).
pub(crate) struct BlockContext<'a> {
    /// Current line content (after blockquote markers stripped if any)
    pub content: &'a str,

    /// Whether there was a blank line before this line
    pub has_blank_before: bool,

    /// Current blockquote depth
    #[allow(dead_code)] // Will be used as we migrate more blocks
    pub blockquote_depth: usize,

    /// Parser configuration
    pub config: &'a Config,

    /// Container stack for checking context (lists, blockquotes, etc.)
    #[allow(dead_code)] // Will be used as we migrate more blocks
    pub containers: &'a ContainerStack,
}

/// Result of detecting whether a block can be parsed.
pub(crate) enum BlockDetectionResult {
    /// Can parse this block, requires blank line before
    Yes,

    /// Can parse this block and can interrupt paragraphs (no blank line needed)
    #[allow(dead_code)] // Will be used when we migrate fenced code blocks
    YesCanInterrupt,

    /// Cannot parse this content
    No,
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
    /// Detect if this parser can handle the content (lightweight check, no emission).
    ///
    /// Returns:
    /// - `Yes`: Can parse, requires blank line before
    /// - `YesCanInterrupt`: Can parse and can interrupt paragraphs
    /// - `No`: Cannot parse this content
    ///
    /// This method should be fast and do minimal work (peek at first few characters).
    /// It should NOT emit anything to the builder.
    fn can_parse(&self, ctx: &BlockContext) -> BlockDetectionResult;

    /// Parse and emit this block type to the builder.
    ///
    /// Called only after `can_parse` returns `Yes` or `YesCanInterrupt`, and after
    /// the caller has prepared (closed paragraphs, flushed buffers).
    ///
    /// # Arguments
    /// - `ctx`: Context about the current parsing state
    /// - `builder`: Builder to emit syntax nodes to
    /// - `lines`: Full document lines (for multi-line blocks)
    /// - `line_pos`: Current line position in the document
    ///
    /// # Returns
    /// Number of lines consumed by this block
    ///
    /// # Single-pass guarantee
    /// This method is called during the single forward pass. It should:
    /// - Read ahead in `lines` if needed (tables, code blocks, etc.)
    /// - Emit inline elements immediately via inline_emission
    /// - Not modify any state outside of builder emission
    fn parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
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
    fn can_parse(&self, ctx: &BlockContext) -> BlockDetectionResult {
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
    fn can_parse(&self, ctx: &BlockContext) -> BlockDetectionResult {
        // Must have blank line before
        if !ctx.has_blank_before {
            return BlockDetectionResult::No;
        }

        // Check if this looks like an ATX heading
        if try_parse_atx_heading(ctx.content).is_some() {
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
        let line = lines[line_pos];
        let heading_level = try_parse_atx_heading(ctx.content).unwrap();
        emit_atx_heading(builder, line, heading_level, ctx.config);
        1 // Consumed 1 line
    }

    fn name(&self) -> &'static str {
        "atx_heading"
    }
}

/// Figure parser (standalone image on its own line)
pub(crate) struct FigureParser;

impl BlockParser for FigureParser {
    fn can_parse(&self, ctx: &BlockContext) -> BlockDetectionResult {
        // Must have blank line before
        if !ctx.has_blank_before {
            return BlockDetectionResult::No;
        }

        // Check if this looks like a figure
        if try_parse_figure(ctx.content) {
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
        let line = lines[line_pos];
        parse_figure(builder, line, ctx.config);
        1 // Consumed 1 line
    }

    fn name(&self) -> &'static str {
        "figure"
    }
}

/// Reference definition parser ([label]: url "title")
pub(crate) struct ReferenceDefinitionParser;

impl BlockParser for ReferenceDefinitionParser {
    fn can_parse(&self, ctx: &BlockContext) -> BlockDetectionResult {
        // Reference definitions don't need blank line before
        // Check if this looks like a reference definition
        if try_parse_reference_definition(ctx.content).is_some() {
            BlockDetectionResult::Yes
        } else {
            BlockDetectionResult::No
        }
    }

    fn parse(
        &self,
        _ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> usize {
        use crate::syntax::SyntaxKind;

        builder.start_node(SyntaxKind::REFERENCE_DEFINITION.into());

        let full_line = lines[line_pos];
        let (content_without_newline, line_ending) = strip_newline(full_line);

        // Parse the reference definition with inline structure for the label
        emit_reference_definition_content(builder, content_without_newline);

        // Emit newline separately if present
        if !line_ending.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), line_ending);
        }

        builder.finish_node();

        1 // Consumed 1 line
    }

    fn name(&self) -> &'static str {
        "reference_definition"
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
    pub fn new() -> Self {
        let parsers: Vec<Box<dyn BlockParser>> = vec![
            // Try horizontal rules first (simple, unambiguous)
            Box::new(HorizontalRuleParser),
            // Then headings (also unambiguous)
            Box::new(AtxHeadingParser),
            // Figures (standalone images)
            Box::new(FigureParser),
            // Reference definitions last (lower priority, can be confused with other syntax)
            Box::new(ReferenceDefinitionParser),
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
    pub fn detect(&self, ctx: &BlockContext) -> Option<(usize, BlockDetectionResult)> {
        for (i, parser) in self.parsers.iter().enumerate() {
            let result = parser.can_parse(ctx);
            match result {
                BlockDetectionResult::Yes | BlockDetectionResult::YesCanInterrupt => {
                    log::debug!("Block detected by: {}", parser.name());
                    return Some((i, result));
                }
                BlockDetectionResult::No => {
                    // Try next parser
                    continue;
                }
            }
        }
        None
    }

    /// Parse a block using the specified parser (by index from detect()).
    ///
    /// Should only be called after detect() returns Some and after
    /// caller has prepared for the block element.
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
}
