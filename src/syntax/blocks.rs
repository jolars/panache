use super::ast::support;
use super::{AstNode, ImageLink, PanacheLanguage, SyntaxKind, SyntaxNode};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Document(SyntaxNode);

impl AstNode for Document {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::DOCUMENT
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then(|| Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl Document {
    pub fn blocks(&self) -> impl Iterator<Item = SyntaxNode> {
        self.0.children()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Paragraph(SyntaxNode);

impl AstNode for Paragraph {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::PARAGRAPH
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then(|| Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl Paragraph {
    pub fn image_links(&self) -> impl Iterator<Item = ImageLink> {
        support::children(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Plain(SyntaxNode);

impl AstNode for Plain {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::PLAIN
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then(|| Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_blocks_iterates_top_level_nodes() {
        let input = "# H1\n\nParagraph\n";
        let tree = crate::parse(input, None);
        let document = Document::cast(tree).expect("document");
        let kinds = document
            .blocks()
            .map(|node| node.kind())
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                SyntaxKind::HEADING,
                SyntaxKind::BLANK_LINE,
                SyntaxKind::PARAGRAPH
            ]
        );
    }

    #[test]
    fn paragraph_image_links_extracts_inline_image_nodes() {
        let input = "See ![Alt](img.png) here.\n";
        let tree = crate::parse(input, None);
        let paragraph = tree
            .descendants()
            .find_map(Paragraph::cast)
            .expect("paragraph");
        let images = paragraph.image_links().collect::<Vec<_>>();
        assert_eq!(images.len(), 1);
        assert_eq!(
            images[0].alt().map(|alt| alt.text()),
            Some("Alt".to_string())
        );
    }
}
