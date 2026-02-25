//! Inline element parsing for panache.
//!
//! This module handles parsing of inline elements within block-level content.
//! All inline parsing is now integrated into block parsing (single-pass architecture).
//! The functions in this module are used for recursive inline parsing within blocks.

use crate::config::Config;
use rowan::GreenNodeBuilder;

mod bracketed_spans;
mod citations;
mod code_spans;
pub mod core; // Public for use in block_parser inline_emission
mod escapes;
mod inline_footnotes;
mod latex;
pub mod links; // Public for try_parse_inline_image used by block parser
mod math;
mod native_spans;
mod raw_inline;
mod shortcodes;
mod strikeout;
mod subscript;
mod superscript;
mod tests;

/// Parse inline elements from concatenated text that may include newlines.
/// This function handles multi-line inline patterns (like display math) by checking for them first,
/// then emits NEWLINE tokens to preserve losslessness for the remaining text.
/// Used when parsing paragraphs and other blocks that concatenate TEXT/NEWLINE tokens.
pub fn parse_inline_text_with_newlines(
    builder: &mut GreenNodeBuilder,
    text: &str,
    config: &Config,
    _allow_reference_links: bool,
) {
    log::trace!(
        "Parsing inline text with newlines: {:?} ({} bytes)",
        &text[..text.len().min(40)],
        text.len(),
    );

    // Use the recursive parser which handles newlines as part of the inline content stream
    // It will emit NEWLINE tokens to preserve losslessness, and properly handles multi-line
    // constructs like emphasis, links, and display math
    core::parse_inline_text_recursive(builder, text, config);
}

/// Parse inline elements from text content.
/// This is a standalone function used for recursive inline parsing within blocks.
///
/// The `allow_reference_links` parameter controls whether reference links/images should be parsed.
/// Set to `false` in nested contexts (inside link text, image alt, spans) to prevent recursive parsing.
///
/// **IMPLEMENTATION NOTE**: This uses the Pandoc-style emphasis parsing algorithm which correctly
/// handles emphasis with nested inline elements (code, math, links, etc.).
pub fn parse_inline_text(
    builder: &mut GreenNodeBuilder,
    text: &str,
    config: &Config,
    _allow_reference_links: bool,
) {
    log::trace!(
        "Parsing inline text (recursive): {:?} ({} bytes)",
        &text[..text.len().min(40)],
        text.len()
    );

    // Use recursive parsing with Pandoc's algorithm for emphasis
    core::parse_inline_text_recursive(builder, text, config);
}
