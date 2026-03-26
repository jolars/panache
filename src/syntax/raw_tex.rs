//! Raw TeX AST node wrappers.

use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TexBlock(SyntaxNode);

impl AstNode for TexBlock {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::TEX_BLOCK
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then(|| Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl TexBlock {
    pub fn text(&self) -> String {
        self.0.text().to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LatexCommand(SyntaxNode);

impl AstNode for LatexCommand {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::LATEX_COMMAND
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then(|| Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl LatexCommand {
    pub fn text(&self) -> String {
        self.0.text().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn tex_block_wrapper_casts_and_exposes_text() {
        let tree = parse("\\newcommand{\\foo}{bar}\n", None);
        let block = tree
            .descendants()
            .find_map(TexBlock::cast)
            .expect("tex block");
        assert!(block.text().contains("\\newcommand"));
    }

    #[test]
    fn latex_command_wrapper_casts_and_exposes_text() {
        let tree = parse("Inline \\cite{ref} text\n", None);
        let cmd = tree
            .descendants()
            .find_map(LatexCommand::cast)
            .expect("latex command");
        assert_eq!(cmd.text(), "\\cite{ref}");
    }
}
