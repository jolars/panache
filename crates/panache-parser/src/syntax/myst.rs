//! MyST AST node wrappers.
//!
//! Typed views over the `MYST_*` CST kinds emitted by the parser (directives,
//! roles, targets, substitutions). These hide the concrete tree shape --- brace
//! delimiters, colon markers, optional whitespace --- behind a small
//! `name()`/`content()`/`label()` surface for downstream consumers (LSP
//! semantic tokens, lint rules, reference resolution).

use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};

/// Strips a single pair of surrounding braces (`{name}` -> `name`).
///
/// `MYST_DIRECTIVE_NAME` and `MYST_ROLE_NAME` tokens include their braces; the
/// wrappers expose the bare identifier. Falls back to the input unchanged when
/// the braces are absent (defensive; the parser always emits them).
fn strip_braces(text: &str) -> &str {
    text.strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .unwrap_or(text)
}

/// Returns the text of the first direct child token of `kind`.
fn child_token_text(node: &SyntaxNode, kind: SyntaxKind) -> Option<String> {
    node.children_with_tokens()
        .filter_map(|element| element.into_token())
        .find(|token| token.kind() == kind)
        .map(|token| token.text().to_string())
}

/// A MyST target definition (`(label)=`).
pub struct MystTarget(SyntaxNode);

impl AstNode for MystTarget {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MYST_TARGET
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

impl MystTarget {
    /// Returns the bare target label (no `(` / `)=` delimiters).
    pub fn label(&self) -> Option<String> {
        child_token_text(&self.0, SyntaxKind::MYST_TARGET_LABEL)
    }
}

/// A MyST inline role (`` {name}`content` ``).
pub struct MystRole(SyntaxNode);

impl AstNode for MystRole {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MYST_ROLE
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

impl MystRole {
    /// Returns the bare role name (braces stripped): `{math}` -> `math`.
    pub fn name(&self) -> Option<String> {
        child_token_text(&self.0, SyntaxKind::MYST_ROLE_NAME)
            .map(|text| strip_braces(&text).to_string())
    }

    /// Returns the verbatim content between the backtick markers.
    ///
    /// An empty role (`` {x}`` ``) yields `Some("")`; a role with no content
    /// node yields `None`.
    pub fn content(&self) -> Option<String> {
        self.0
            .children()
            .find(|node| node.kind() == SyntaxKind::MYST_ROLE_CONTENT)
            .map(|node| node.text().to_string())
    }
}

/// A MyST inline substitution (`{{ name }}`).
pub struct MystSubstitution(SyntaxNode);

impl AstNode for MystSubstitution {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MYST_SUBSTITUTION
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

impl MystSubstitution {
    /// Returns the substitution key, trimmed of the inner whitespace the token
    /// preserves (`{{ version }}` stores `" version "` -> `"version"`).
    pub fn name(&self) -> Option<String> {
        child_token_text(&self.0, SyntaxKind::MYST_SUBSTITUTION_NAME)
            .map(|text| text.trim().to_string())
    }
}

/// A MyST directive (```` ```{name} ```` or, with `colon_fence`, `:::{name}`).
pub struct MystDirective(SyntaxNode);

impl AstNode for MystDirective {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MYST_DIRECTIVE
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

impl MystDirective {
    /// The opener line node (`MYST_DIRECTIVE_OPEN`), which holds the name and
    /// argument tokens.
    fn open(&self) -> Option<SyntaxNode> {
        self.0
            .children()
            .find(|node| node.kind() == SyntaxKind::MYST_DIRECTIVE_OPEN)
    }

    /// Returns the bare directive name (braces stripped): `{note}` -> `note`.
    pub fn name(&self) -> Option<String> {
        let open = self.open()?;
        child_token_text(&open, SyntaxKind::MYST_DIRECTIVE_NAME)
            .map(|text| strip_braces(&text).to_string())
    }

    /// Returns the trimmed argument following the name on the opener line, if
    /// any (e.g. `` ```{code-block} python `` -> `python`).
    pub fn argument(&self) -> Option<String> {
        let open = self.open()?;
        child_token_text(&open, SyntaxKind::MYST_DIRECTIVE_ARG).map(|text| text.trim().to_string())
    }

    /// Returns the directive's leading `:key: value` option lines.
    pub fn options(&self) -> Vec<MystDirectiveOption> {
        self.0
            .children()
            .filter_map(MystDirectiveOption::cast)
            .collect()
    }

    /// Returns the verbatim body of a verbatim directive (`code`, `math`, ...).
    ///
    /// Prose directives whose body is recursively parsed as ordinary blocks
    /// have no `MYST_DIRECTIVE_BODY` node and yield `None`.
    pub fn body(&self) -> Option<String> {
        self.0
            .children()
            .find(|node| node.kind() == SyntaxKind::MYST_DIRECTIVE_BODY)
            .map(|node| node.text().to_string())
    }
}

/// A single `:key: value` option line within a directive.
pub struct MystDirectiveOption(SyntaxNode);

impl AstNode for MystDirectiveOption {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MYST_DIRECTIVE_OPTION
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

impl MystDirectiveOption {
    /// Returns the option key (no surrounding colons): `:alt: text` -> `alt`.
    pub fn name(&self) -> Option<String> {
        child_token_text(&self.0, SyntaxKind::MYST_DIRECTIVE_OPTION_NAME)
    }

    /// Returns the option value, if present. Valueless options (`:hidden:`)
    /// yield `None`.
    pub fn value(&self) -> Option<String> {
        child_token_text(&self.0, SyntaxKind::MYST_DIRECTIVE_OPTION_VALUE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::{Dialect, Extensions, Flavor, ParserOptions};

    fn myst_options() -> ParserOptions {
        ParserOptions {
            flavor: Flavor::Myst,
            dialect: Dialect::for_flavor(Flavor::Myst),
            extensions: Extensions {
                myst_substitutions: true,
                ..Extensions::for_flavor(Flavor::Myst)
            },
            ..ParserOptions::default()
        }
    }

    fn cast_first<T: AstNode<Language = PanacheLanguage>>(input: &str) -> T {
        parse(input, Some(myst_options()))
            .descendants()
            .find_map(T::cast)
            .expect("expected a MyST node")
    }

    #[test]
    fn target_wrapper_extracts_label() {
        let target: MystTarget = cast_first("(my-target)=\n\n# Heading");
        assert_eq!(target.label().as_deref(), Some("my-target"));
    }

    #[test]
    fn role_wrapper_extracts_name_and_content() {
        let role: MystRole = cast_first("See {math}`a^2 + b^2`.");
        assert_eq!(role.name().as_deref(), Some("math"));
        assert_eq!(role.content().as_deref(), Some("a^2 + b^2"));
    }

    #[test]
    fn directive_wrapper_extracts_name_arg_and_options() {
        let directive: MystDirective =
            cast_first("````{figure} img.png\n:alt: An image\n:width: 200px\n\nCaption\n````");
        assert_eq!(directive.name().as_deref(), Some("figure"));
        assert_eq!(directive.argument().as_deref(), Some("img.png"));

        let options = directive.options();
        assert_eq!(options.len(), 2);
        assert_eq!(options[0].name().as_deref(), Some("alt"));
        assert_eq!(options[0].value().as_deref(), Some("An image"));
        assert_eq!(options[1].name().as_deref(), Some("width"));
        assert_eq!(options[1].value().as_deref(), Some("200px"));
    }

    #[test]
    fn directive_wrapper_extracts_verbatim_body() {
        let directive: MystDirective =
            cast_first("```{code} python\n:number-lines: 1\ndef five():\n  return 5\n```");
        assert_eq!(directive.name().as_deref(), Some("code"));
        assert_eq!(directive.argument().as_deref(), Some("python"));
        let body = directive.body().expect("verbatim body");
        assert!(body.contains("def five():"));
        assert!(body.contains("return 5"));
    }

    #[test]
    fn directive_wrapper_handles_valueless_option() {
        let directive: MystDirective = cast_first("```{toctree}\n:hidden:\n\nquickstart\n```");
        let options = directive.options();
        let hidden = options
            .iter()
            .find(|option| option.name().as_deref() == Some("hidden"))
            .expect("hidden option");
        assert_eq!(hidden.value(), None);
    }

    #[test]
    fn substitution_wrapper_trims_name() {
        let substitution: MystSubstitution = cast_first("Released {{ version }}.");
        assert_eq!(substitution.name().as_deref(), Some("version"));
    }
}
