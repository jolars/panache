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
//! - [`ParserOptions`]: Parser configuration and extension toggles.
//! - [`syntax`]: Typed syntax wrappers and syntax kinds.
//! - [`parser`]: Lower-level parser modules and incremental helpers.
//!
pub mod config;
pub mod parser;
pub mod range_utils;
pub mod syntax;

pub use config::BlankLines;
pub use config::Config;
pub use config::ConfigBuilder;
pub use config::ParserOptions;
pub use parser::parse;
pub use syntax::SyntaxNode;
