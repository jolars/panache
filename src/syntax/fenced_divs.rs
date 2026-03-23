//! Fenced div AST node wrappers.

use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};

pub struct FencedDiv(SyntaxNode);

impl AstNode for FencedDiv {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::FENCED_DIV
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self(syntax))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl FencedDiv {
    pub fn opening_fence(&self) -> Option<DivFenceOpen> {
        self.0.children().find_map(DivFenceOpen::cast)
    }

    pub fn closing_fence(&self) -> Option<DivFenceClose> {
        self.0.children().find_map(DivFenceClose::cast)
    }

    pub fn info(&self) -> Option<DivInfo> {
        self.opening_fence().and_then(|fence| fence.info())
    }

    pub fn info_text(&self) -> Option<String> {
        self.info().map(|info| info.text())
    }

    pub fn body_blocks(&self) -> impl Iterator<Item = SyntaxNode> {
        self.0.children().filter(|child| {
            !matches!(
                child.kind(),
                SyntaxKind::DIV_FENCE_OPEN | SyntaxKind::DIV_FENCE_CLOSE
            )
        })
    }
}

pub struct DivFenceOpen(SyntaxNode);

impl AstNode for DivFenceOpen {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::DIV_FENCE_OPEN
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self(syntax))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl DivFenceOpen {
    pub fn info(&self) -> Option<DivInfo> {
        self.0.children().find_map(DivInfo::cast)
    }
}

pub struct DivFenceClose(SyntaxNode);

impl AstNode for DivFenceClose {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::DIV_FENCE_CLOSE
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self(syntax))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

pub struct DivInfo(SyntaxNode);

impl AstNode for DivInfo {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::DIV_INFO
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self(syntax))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl DivInfo {
    pub fn text(&self) -> String {
        self.0.text().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn fenced_div_wrapper_with_braced_attributes() {
        let tree = parse("::: {.callout-note #tip}\nText\n:::\n", None);
        let div = tree
            .descendants()
            .find_map(FencedDiv::cast)
            .expect("fenced div");

        assert_eq!(div.info_text().as_deref(), Some("{.callout-note #tip}"));
        assert!(div.opening_fence().is_some());
        assert!(div.closing_fence().is_some());
    }

    #[test]
    fn fenced_div_body_blocks_excludes_fences() {
        let tree = parse("::: note\n# Heading\n\nText\n:::\n", None);
        let div = tree
            .descendants()
            .find_map(FencedDiv::cast)
            .expect("fenced div");

        let kinds: Vec<_> = div.body_blocks().map(|n| n.kind()).collect();
        assert!(kinds.contains(&SyntaxKind::HEADING));
        assert!(kinds.contains(&SyntaxKind::PARAGRAPH));
        assert!(!kinds.contains(&SyntaxKind::DIV_FENCE_OPEN));
        assert!(!kinds.contains(&SyntaxKind::DIV_FENCE_CLOSE));
    }

    #[test]
    fn fenced_div_open_info_node_cast() {
        let tree = parse("::: warning\nBody\n:::\n", None);
        let open = tree
            .descendants()
            .find_map(DivFenceOpen::cast)
            .expect("div fence open");
        let info = open.info().expect("div info");
        assert_eq!(info.text(), "warning");
    }
}
