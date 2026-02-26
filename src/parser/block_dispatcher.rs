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

/// Result of attempting to parse a block element.
pub(crate) enum BlockParseResult {
    /// Block was successfully parsed and emitted. Contains number of lines consumed.
    Parsed { lines_consumed: usize },

    /// This parser cannot handle this content (try next parser)
    NotApplicable,

    /// Block was recognized but should not be parsed (e.g., needs different context)
    #[allow(dead_code)] // Will be used for complex blocks like lists
    Skip,
}

/// Trait for block-level parsers.
///
/// Each block type implements this trait to provide:
/// 1. Detection: Can this block type parse this content?
/// 2. Emission: Parse and emit the block to the builder
///
/// Note: This is purely organizational - the trait doesn't introduce
/// backtracking or multiple passes. Each parser operates during the
/// single forward pass through the document.
pub(crate) trait BlockParser {
    /// Try to parse and emit this block type.
    ///
    /// If this parser recognizes and can handle the content:
    /// - Emit the block structure to the builder
    /// - Return BlockParseResult::Parsed with number of lines consumed
    ///
    /// If this parser cannot handle the content:
    /// - Return BlockParseResult::NotApplicable (dispatcher tries next parser)
    ///
    /// # Arguments
    /// - `ctx`: Context about the current parsing state
    /// - `builder`: Builder to emit syntax nodes to
    /// - `lines`: Full document lines (for multi-line blocks)
    /// - `line_pos`: Current line position in the document
    ///
    /// # Single-pass guarantee
    /// This method is called during the single forward pass. It should:
    /// - Read ahead in `lines` if needed (tables, code blocks, etc.)
    /// - Emit inline elements immediately via inline_emission
    /// - Not modify any state outside of builder emission
    fn try_parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockParseResult;

    /// Name of this block parser (for debugging/logging)
    fn name(&self) -> &'static str;
}

// ============================================================================
// Concrete Block Parser Implementations
// ============================================================================

/// Horizontal rule parser
pub(crate) struct HorizontalRuleParser;

impl BlockParser for HorizontalRuleParser {
    fn try_parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockParseResult {
        // Must have blank line before (handled by caller checking ctx.has_blank_before)
        if !ctx.has_blank_before {
            return BlockParseResult::NotApplicable;
        }

        // Try to parse horizontal rule
        if try_parse_horizontal_rule(ctx.content).is_none() {
            return BlockParseResult::NotApplicable;
        }

        // Emit the horizontal rule
        let line = lines[line_pos];
        emit_horizontal_rule(builder, line);

        BlockParseResult::Parsed { lines_consumed: 1 }
    }

    fn name(&self) -> &'static str {
        "horizontal_rule"
    }
}

/// ATX heading parser (# Heading)
pub(crate) struct AtxHeadingParser;

impl BlockParser for AtxHeadingParser {
    fn try_parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockParseResult {
        // Must have blank line before (checked by caller)
        if !ctx.has_blank_before {
            return BlockParseResult::NotApplicable;
        }

        // Try to parse ATX heading
        let Some(heading_level) = try_parse_atx_heading(ctx.content) else {
            return BlockParseResult::NotApplicable;
        };

        // Emit the heading
        let line = lines[line_pos];
        emit_atx_heading(builder, line, heading_level, ctx.config);

        BlockParseResult::Parsed { lines_consumed: 1 }
    }

    fn name(&self) -> &'static str {
        "atx_heading"
    }
}

/// Figure parser (standalone image on its own line)
pub(crate) struct FigureParser;

impl BlockParser for FigureParser {
    fn try_parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockParseResult {
        // Must have blank line before (checked by caller)
        if !ctx.has_blank_before {
            return BlockParseResult::NotApplicable;
        }

        // Try to parse figure
        if !try_parse_figure(ctx.content) {
            return BlockParseResult::NotApplicable;
        }

        // Emit the figure
        let line = lines[line_pos];
        parse_figure(builder, line, ctx.config);

        BlockParseResult::Parsed { lines_consumed: 1 }
    }

    fn name(&self) -> &'static str {
        "figure"
    }
}

/// Reference definition parser ([label]: url "title")
pub(crate) struct ReferenceDefinitionParser;

impl BlockParser for ReferenceDefinitionParser {
    fn try_parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> BlockParseResult {
        // Reference definitions don't need blank line before

        // Try to parse reference definition
        let Some((_len, _label, _url, _title)) = try_parse_reference_definition(ctx.content) else {
            return BlockParseResult::NotApplicable;
        };

        // Emit as REFERENCE_DEFINITION node with inline LINK structure
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

        BlockParseResult::Parsed { lines_consumed: 1 }
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
    /// Returns the first successful parse result, or None if no parser matched.
    pub fn try_parse(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &[&str],
        line_pos: usize,
    ) -> Option<BlockParseResult> {
        for parser in &self.parsers {
            let result = parser.try_parse(ctx, builder, lines, line_pos);
            match result {
                BlockParseResult::Parsed { .. } => {
                    log::debug!("Block parsed by: {}", parser.name());
                    return Some(result);
                }
                BlockParseResult::Skip => {
                    log::trace!("Block skipped by: {}", parser.name());
                    return Some(result);
                }
                BlockParseResult::NotApplicable => {
                    // Try next parser
                    continue;
                }
            }
        }
        None
    }
}
