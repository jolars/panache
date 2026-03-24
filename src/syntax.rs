//! Syntax tree types and AST node wrappers for Quarto/Pandoc documents.
//!
//! This module provides a typed API over the raw concrete syntax tree (CST)
//! produced by the parser. The CST is based on the `rowan` library and uses
//! the red-green tree pattern for efficient incremental parsing.

mod ast;
mod attributes;
mod block_quotes;
mod blocks;
mod chunk_options;
mod citations;
mod code_blocks;
mod definitions;
mod fenced_divs;
mod headings;
mod inlines;
mod json;
mod kind;
mod links;
mod lists;
mod math;
mod references;
mod shortcodes;
mod tables;
mod yaml;

pub use ast::*;
pub use attributes::*;
pub use block_quotes::*;
pub use blocks::*;
pub use chunk_options::*;
pub use citations::*;
pub use code_blocks::*;
pub use definitions::*;
pub use fenced_divs::*;
pub use headings::*;
pub use inlines::*;
pub use json::*;
pub use kind::*;
pub use links::*;
pub use lists::*;
pub use math::*;
pub use references::*;
pub use shortcodes::*;
pub use tables::*;
pub use yaml::*;

pub type SyntaxNode = rowan::SyntaxNode<PanacheLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<PanacheLanguage>;
pub type SyntaxElement = rowan::SyntaxElement<PanacheLanguage>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heading_wrapper() {
        use crate::Config;
        use crate::parser::parse;

        let input = "# Hello World\n\nParagraph.";
        let tree = parse(input, Some(Config::default()));

        let heading = tree
            .children()
            .find_map(Heading::cast)
            .expect("should find heading");

        assert_eq!(heading.level(), 1);
        assert_eq!(heading.text(), "Hello World");
    }

    #[test]
    fn test_link_wrapper() {
        use crate::Config;
        use crate::parser::parse;

        let input = "Click [here](https://example.com).";
        let tree = parse(input, Some(Config::default()));

        // Find link using typed wrapper
        let link = tree
            .descendants()
            .find_map(Link::cast)
            .expect("should find link");

        assert_eq!(
            link.text().map(|t| t.text_content()),
            Some("here".to_string())
        );
        assert_eq!(
            link.dest().map(|d| d.url_content()),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn test_image_wrapper() {
        use crate::Config;
        use crate::parser::parse;

        let input = "![Alt text](image.png)";
        let tree = parse(input, Some(Config::default()));

        let image = tree
            .descendants()
            .find_map(ImageLink::cast)
            .expect("should find image");

        assert_eq!(image.alt().map(|a| a.text()), Some("Alt text".to_string()));
    }

    #[test]
    fn test_autolink_wrapper() {
        use crate::Config;
        use crate::parser::parse;

        let input = "<https://example.com>";
        let tree = parse(input, Some(Config::default()));

        let autolink = tree
            .descendants()
            .find_map(AutoLink::cast)
            .expect("should find autolink");

        assert_eq!(autolink.target(), "https://example.com");
    }

    #[test]
    fn test_shortcode_wrapper() {
        use crate::Config;
        use crate::parser::parse;

        let input = "{{< include \"chapters/part 1.qmd\" >}}";
        let tree = parse(input, Some(Config::default()));

        let shortcode = tree
            .descendants()
            .find_map(Shortcode::cast)
            .expect("should find shortcode");

        assert_eq!(shortcode.name().as_deref(), Some("include"));
        assert_eq!(
            shortcode.args(),
            vec!["include".to_string(), "chapters/part 1.qmd".to_string()]
        );
    }

    #[test]
    fn test_table_wrapper() {
        use crate::Config;
        use crate::parser::parse;

        let input = r#"| A | B |
|---|---|
| 1 | 2 |

Table: My caption
"#;
        let tree = parse(input, Some(Config::default()));

        let table = tree
            .descendants()
            .find_map(PipeTable::cast)
            .expect("should find table");

        assert_eq!(
            table.caption().map(|c| c.text()),
            Some("My caption".to_string())
        );
        assert!(table.rows().count() > 0);
    }
}
