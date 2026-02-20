//! Syntax tree types and AST node wrappers for Quarto/Pandoc documents.
//!
//! This module provides a typed API over the raw concrete syntax tree (CST)
//! produced by the parser. The CST is based on the `rowan` library and uses
//! the red-green tree pattern for efficient incremental parsing.

mod ast;
mod chunk_options;
mod headings;
mod kind;
mod links;
mod lists;
mod references;
mod tables;

pub use ast::*;
pub use chunk_options::*;
pub use headings::*;
pub use kind::*;
pub use links::*;
pub use lists::*;
pub use references::*;
pub use tables::*;

pub type SyntaxNode = rowan::SyntaxNode<QuartoLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<QuartoLanguage>;
pub type SyntaxElement = rowan::SyntaxElement<QuartoLanguage>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heading_wrapper() {
        use crate::Config;
        use crate::parser::parse;

        let input = "# Hello World\n\nParagraph.";
        let tree = parse(input, Some(Config::default()));

        // Find heading using typed wrapper (root is now DOCUMENT directly)
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
