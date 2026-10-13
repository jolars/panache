//! Typed access to opaque Hugo constructs and TOML metadata.

use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};

macro_rules! wrapper {
    ($name:ident, $kind:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(SyntaxNode);

        impl AstNode for $name {
            type Language = PanacheLanguage;

            fn can_cast(kind: SyntaxKind) -> bool {
                kind == SyntaxKind::$kind
            }

            fn cast(syntax: SyntaxNode) -> Option<Self> {
                Self::can_cast(syntax.kind()).then_some(Self(syntax))
            }

            fn syntax(&self) -> &SyntaxNode {
                &self.0
            }
        }
    };
}

wrapper!(HugoShortcode, HUGO_SHORTCODE);
wrapper!(HugoShortcodeTag, HUGO_SHORTCODE_TAG);
wrapper!(TomlMetadata, TOML_METADATA);

impl HugoShortcode {
    pub fn opening_tag(&self) -> Option<HugoShortcodeTag> {
        self.0.children().find_map(HugoShortcodeTag::cast)
    }

    pub fn body(&self) -> Option<SyntaxNode> {
        self.0
            .children()
            .find(|node| node.kind() == SyntaxKind::HUGO_SHORTCODE_BODY)
    }
}

impl HugoShortcodeTag {
    pub fn is_markdown(&self) -> bool {
        self.0
            .first_token()
            .is_some_and(|token| token.text() == "{{%")
    }

    pub fn content(&self) -> Option<String> {
        self.0
            .children()
            .find(|node| node.kind() == SyntaxKind::HUGO_SHORTCODE_CONTENT)
            .map(|node| node.text().to_string())
    }
}

impl TomlMetadata {
    pub fn content(&self) -> Option<SyntaxNode> {
        self.0
            .children()
            .find(|node| node.kind() == SyntaxKind::TOML_METADATA_CONTENT)
    }

    pub fn is_closed(&self) -> bool {
        self.0
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.kind() == SyntaxKind::TOML_METADATA_DELIM)
            .count()
            == 2
    }
}
