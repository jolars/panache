//! Block-level parsing for Pandoc/Quarto documents.
//!
//! This module contains parsers for all block-level constructs like headings,
//! paragraphs, code blocks, tables, lists, blockquotes, etc.

#[path = "blocks/blockquotes.rs"]
pub mod blockquotes;
#[path = "blocks/code_blocks.rs"]
pub mod code_blocks; // Public for formatter access to InfoString and CodeBlockType
#[path = "blocks/definition_lists.rs"]
pub mod definition_lists;
#[path = "blocks/fenced_divs.rs"]
pub mod fenced_divs;
#[path = "blocks/figures.rs"]
pub mod figures;
#[path = "blocks/headings.rs"]
pub mod headings;
#[path = "blocks/horizontal_rules.rs"]
pub mod horizontal_rules;
#[path = "blocks/html_blocks.rs"]
pub mod html_blocks;
#[path = "blocks/indented_code.rs"]
pub mod indented_code;
#[path = "blocks/line_blocks.rs"]
pub mod line_blocks;
#[path = "blocks/lists.rs"]
pub mod lists;
#[path = "blocks/metadata.rs"]
pub mod metadata;
#[path = "blocks/paragraphs.rs"]
pub mod paragraphs;
#[path = "blocks/raw_blocks.rs"]
pub mod raw_blocks;
#[path = "blocks/reference_links.rs"]
pub mod reference_links;
#[path = "blocks/tables.rs"]
pub mod tables;

#[cfg(test)]
#[path = "blocks/tests"]
pub mod tests {
    #[path = "blanklines.rs"]
    pub mod blanklines;
    #[path = "blockquotes.rs"]
    pub mod blockquotes;
    #[path = "code_blocks.rs"]
    pub mod code_blocks;
    #[path = "definition_lists.rs"]
    pub mod definition_lists;
    #[path = "headings.rs"]
    pub mod headings;
    #[path = "helpers.rs"]
    pub mod helpers;
    #[path = "lists.rs"]
    pub mod lists;
    #[path = "losslessness.rs"]
    pub mod losslessness;
    #[path = "metadata_guards.rs"]
    pub mod metadata_guards;
}
#[path = "blocks/latex_envs.rs"]
pub mod latex_envs;
