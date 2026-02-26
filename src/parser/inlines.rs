//! Inline-level parsing for Pandoc/Quarto documents.
//!
//! This module contains parsers for all inline constructs like emphasis, links,
//! code spans, math, citations, etc. Inline parsing is integrated into block
//! parsing for true single-pass architecture.

#[path = "inlines/bracketed_spans.rs"]
pub mod bracketed_spans;
#[path = "inlines/citations.rs"]
pub mod citations;
#[path = "inlines/code_spans.rs"]
pub mod code_spans;
#[path = "inlines/core.rs"]
pub mod core; // Public for use in block parsing and list postprocessor
#[path = "inlines/escapes.rs"]
pub mod escapes;
#[path = "inlines/inline_footnotes.rs"]
pub mod inline_footnotes;
#[path = "inlines/latex.rs"]
pub mod latex;
#[path = "inlines/links.rs"]
pub mod links; // Public for try_parse_inline_image used by blocks/figures
#[path = "inlines/math.rs"]
pub mod math;
#[path = "inlines/native_spans.rs"]
pub mod native_spans;
#[path = "inlines/raw_inline.rs"]
pub mod raw_inline;
#[path = "inlines/shortcodes.rs"]
pub mod shortcodes;
#[path = "inlines/strikeout.rs"]
pub mod strikeout;
#[path = "inlines/subscript.rs"]
pub mod subscript;
#[path = "inlines/superscript.rs"]
pub mod superscript;

#[cfg(test)]
#[path = "inlines/tests.rs"]
mod tests;
