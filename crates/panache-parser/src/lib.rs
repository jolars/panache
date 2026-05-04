//! `panache-parser` is a lossless Concrete Syntax Tree (CST) parser for Pandoc
//! Markdown, Quarto, and R Markdown documents.
//!
//! It preserves source structure and trivia (including markers and whitespace),
//! making it suitable for editor tooling and formatting pipelines that require
//! deterministic round-tripping.
//!
//! # Quick start
//!
//! ```rust
//! use panache_parser::parse;
//!
//! let tree = parse("# Heading\n\nParagraph text.", None);
//! println!("{:#?}", tree);
//! ```
//!
//! # Main entry points
//!
//! - [`parse`]: Parse input text into a [`SyntaxNode`].
//! - [`to_pandoc_ast`]: Project a [`SyntaxNode`] into pandoc-native AST text.
//! - [`ParserOptions`]: Parser configuration and extension toggles.
//! - [`syntax`]: Typed syntax wrappers and syntax kinds.
//! - [`parser`]: Lower-level parser modules and incremental helpers.
//!
mod options;
pub mod pandoc_ast;
pub mod parser;
pub mod range_utils;
pub mod syntax;

pub use options::Dialect;
pub use options::Extensions;
pub use options::Flavor;
pub use options::PandocCompat;
pub use options::ParserOptions;
pub use pandoc_ast::to_pandoc_ast;
pub use parser::inlines::refdef_map::{RefdefMap, collect_refdef_labels};
pub use parser::parse;
pub use parser::parse_with_refdefs;
pub use syntax::SyntaxNode;
