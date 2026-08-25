//! Math AST node wrappers.

use rowan::TextRange;

use super::{AstNode, PanacheLanguage, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};

/// Reconstruct the raw math content from its ordered TeX segments and host
/// equation labels, keeping only source bytes that belong to the math span.
///
/// Container machinery (blockquotes, list continuations, …) interleaves host
/// prefix tokens (`LINE_PREFIX`, `NEWLINE`) into the subtree on continuation
/// lines for lossless capture. Those prefixes are not part of the math, so
/// they are excluded here — otherwise e.g. a blockquote `>` would leak into
/// the content and re-accumulate on every format pass.
pub fn math_content_text(math: &SyntaxNode) -> String {
    math.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| is_math_content_token(tok.kind()))
        .map(|tok| tok.text().to_string())
        .collect()
}

fn is_math_content_token(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::MATH_WORD
            | SyntaxKind::MATH_SPACE
            | SyntaxKind::MATH_NEWLINE
            | SyntaxKind::MATH_CONTROL_WORD
            | SyntaxKind::MATH_CONTROL_SYMBOL
            | SyntaxKind::MATH_GROUP_OPEN
            | SyntaxKind::MATH_GROUP_CLOSE
            | SyntaxKind::MATH_BRACKET_OPEN
            | SyntaxKind::MATH_BRACKET_CLOSE
            | SyntaxKind::MATH_ALIGN
            | SyntaxKind::MATH_CARET
            | SyntaxKind::MATH_UNDERSCORE
            | SyntaxKind::MATH_COMMENT
            | SyntaxKind::MATH_EQUATION_LABEL
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DisplayMath(SyntaxNode);

impl AstNode for DisplayMath {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::DISPLAY_MATH
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

impl DisplayMath {
    /// Ordered TeX-content and host equation-label segments between the math
    /// delimiters.
    pub fn content_segments(&self) -> impl Iterator<Item = MathContentSegment> + '_ {
        math_content_segments(&self.0)
    }

    pub fn opening_marker(&self) -> Option<String> {
        self.0.children_with_tokens().find_map(|child| {
            child.into_token().and_then(|token| {
                (token.kind() == SyntaxKind::DISPLAY_MATH_MARKER).then(|| token.text().to_string())
            })
        })
    }

    pub fn closing_marker(&self) -> Option<String> {
        self.0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .filter(|token| token.kind() == SyntaxKind::DISPLAY_MATH_MARKER)
            .nth(1)
            .map(|token| token.text().to_string())
    }

    /// The raw content between the delimiters, reconstructed from ordered TeX
    /// segments and host labels (excluding container prefixes—see
    /// [`math_content_text`]).
    pub fn content(&self) -> String {
        math_content_text(&self.0)
    }

    pub fn is_environment_form(&self) -> bool {
        let opening = self.opening_marker().unwrap_or_default();
        let closing = self.closing_marker().unwrap_or_default();
        opening.starts_with("\\begin{") && closing.starts_with("\\end{")
    }

    pub fn has_unescaped_single_dollar_in_content(&self) -> bool {
        let content = self.content();
        let chars: Vec<char> = content.chars().collect();
        let mut idx = 0usize;
        let mut backslashes = 0usize;

        while idx < chars.len() {
            let ch = chars[idx];
            if ch == '\\' {
                backslashes += 1;
                idx += 1;
                continue;
            }

            let escaped = backslashes % 2 == 1;
            backslashes = 0;
            if ch == '$' && !escaped {
                if idx + 1 < chars.len() && chars[idx + 1] == '$' {
                    idx += 2;
                    continue;
                }
                return true;
            }
            idx += 1;
        }

        false
    }
}

/// The `MATH_CONTENT` subtree root: the parsed TeX content between (but
/// excluding) the math delimiters. Spliced into the host document tree, so its
/// tokens carry host-aligned ranges.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MathContent(SyntaxNode);

impl AstNode for MathContent {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MATH_CONTENT
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then_some(Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl MathContent {
    /// The math-owned direct elements in source order.
    ///
    /// Host container prefixes interleaved by block parsing are omitted.
    pub fn elements(&self) -> impl Iterator<Item = SyntaxElement> + '_ {
        self.0
            .children_with_tokens()
            .filter(is_math_content_element)
    }

    /// Reconstruct the math source while excluding interleaved host prefixes.
    pub fn text(&self) -> String {
        math_content_text(&self.0)
    }

    /// Structural problems in this subtree (see [`math_diagnostics`]).
    pub fn diagnostics(&self) -> Vec<MathDiagnostic> {
        math_diagnostics(&self.0)
    }
}

/// One ordered host-level segment inside inline or display math.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MathContentSegment {
    /// A native TeX-math content subtree.
    Content(MathContent),
    /// A Bookdown equation label owned by the Markdown host.
    EquationLabel(SyntaxToken),
}

impl MathContentSegment {
    /// Reconstruct this segment's source bytes.
    pub fn text(&self) -> String {
        match self {
            Self::Content(content) => content.text(),
            Self::EquationLabel(token) => token.text().to_owned(),
        }
    }
}

/// Ordered typed content segments belonging to an inline or display math host.
pub fn math_content_segments(math: &SyntaxNode) -> impl Iterator<Item = MathContentSegment> + '_ {
    math.children_with_tokens()
        .filter_map(|element| match element {
            rowan::NodeOrToken::Node(node) => {
                MathContent::cast(node).map(MathContentSegment::Content)
            }
            rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::MATH_EQUATION_LABEL => {
                Some(MathContentSegment::EquationLabel(token))
            }
            _ => None,
        })
}

/// An atom with one or more attached subscript or superscript nodes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MathScripted(SyntaxNode);

impl AstNode for MathScripted {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MATH_SCRIPTED
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then_some(Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl MathScripted {
    /// The atom to which the scripts attach.
    pub fn base(&self) -> Option<SyntaxElement> {
        self.0.children_with_tokens().find(|element| {
            !matches!(
                element.kind(),
                SyntaxKind::MATH_SUBSCRIPT
                    | SyntaxKind::MATH_SUPERSCRIPT
                    | SyntaxKind::MATH_SPACE
                    | SyntaxKind::MATH_NEWLINE
                    | SyntaxKind::LINE_PREFIX
                    | SyntaxKind::NEWLINE
            )
        })
    }

    /// The first attached subscript, if present.
    pub fn subscript(&self) -> Option<MathSubscript> {
        self.0.children().find_map(MathSubscript::cast)
    }

    /// The first attached superscript, if present.
    pub fn superscript(&self) -> Option<MathSuperscript> {
        self.0.children().find_map(MathSuperscript::cast)
    }

    /// Every attached script in source order.
    pub fn scripts(&self) -> impl Iterator<Item = MathScript> + '_ {
        self.0.children().filter_map(MathScript::cast)
    }
}

/// A subscript or superscript attached to a scripted base.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MathScript {
    Subscript(MathSubscript),
    Superscript(MathSuperscript),
}

impl AstNode for MathScript {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::MATH_SUBSCRIPT | SyntaxKind::MATH_SUPERSCRIPT
        )
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        match syntax.kind() {
            SyntaxKind::MATH_SUBSCRIPT => MathSubscript::cast(syntax).map(Self::Subscript),
            SyntaxKind::MATH_SUPERSCRIPT => MathSuperscript::cast(syntax).map(Self::Superscript),
            _ => None,
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Subscript(script) => script.syntax(),
            Self::Superscript(script) => script.syntax(),
        }
    }
}

impl MathScript {
    /// The `_` or `^` marker token.
    pub fn marker_token(&self) -> Option<SyntaxToken> {
        match self {
            Self::Subscript(script) => script.marker_token(),
            Self::Superscript(script) => script.marker_token(),
        }
    }

    /// The optional one-atom argument after the marker and layout trivia.
    pub fn argument(&self) -> Option<SyntaxElement> {
        match self {
            Self::Subscript(script) => script.argument(),
            Self::Superscript(script) => script.argument(),
        }
    }
}

/// An `_` marker and its optional one-atom argument.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MathSubscript(SyntaxNode);

impl AstNode for MathSubscript {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MATH_SUBSCRIPT
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then_some(Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl MathSubscript {
    /// The `_` marker.
    pub fn marker_token(&self) -> Option<SyntaxToken> {
        token_child(&self.0, SyntaxKind::MATH_UNDERSCORE)
    }

    /// The optional one-atom argument after the marker and layout trivia.
    pub fn argument(&self) -> Option<SyntaxElement> {
        script_argument(&self.0)
    }
}

/// A `^` marker and its optional one-atom argument.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MathSuperscript(SyntaxNode);

impl AstNode for MathSuperscript {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MATH_SUPERSCRIPT
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then_some(Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl MathSuperscript {
    /// The `^` marker.
    pub fn marker_token(&self) -> Option<SyntaxToken> {
        token_child(&self.0, SyntaxKind::MATH_CARET)
    }

    /// The optional one-atom argument after the marker and layout trivia.
    pub fn argument(&self) -> Option<SyntaxElement> {
        script_argument(&self.0)
    }
}

/// A control word together with the brace and optional arguments it owns
/// (`\sqrt[3]{x}`, or a bare `\alpha` with none).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MathCommand(SyntaxNode);

impl AstNode for MathCommand {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MATH_COMMAND
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then_some(Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl MathCommand {
    /// The `\name` control-word token.
    pub fn name_token(&self) -> Option<SyntaxToken> {
        token_child(&self.0, SyntaxKind::MATH_CONTROL_WORD)
    }

    /// The command name without its leading backslash.
    pub fn name(&self) -> Option<String> {
        self.name_token()
            .map(|token| token.text().trim_start_matches('\\').to_string())
    }

    /// The attached brace-group arguments, in source order.
    ///
    /// Retained as an alias for [`Self::groups`]. Optional arguments are
    /// available through [`Self::optionals`].
    pub fn arguments(&self) -> impl Iterator<Item = MathGroup> + '_ {
        self.groups()
    }

    /// The attached brace-group arguments, in source order.
    pub fn groups(&self) -> impl Iterator<Item = MathGroup> + '_ {
        self.0.children().filter_map(MathGroup::cast)
    }

    /// The attached optional arguments, in source order.
    pub fn optionals(&self) -> impl Iterator<Item = MathOptional> + '_ {
        self.0.children().filter_map(MathOptional::cast)
    }

    /// Every greedily attached brace or bracket argument in source order.
    pub fn attached_arguments(&self) -> impl Iterator<Item = MathArgument> + '_ {
        self.0.children().filter_map(MathArgument::cast)
    }

    /// A tightly attached starred-variant marker, when present.
    pub fn star_token(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| token.kind() == SyntaxKind::MATH_WORD && token.text() == "*")
    }
}

/// A greedily attached command argument.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MathArgument {
    Brace(MathGroup),
    Bracket(MathOptional),
}

impl AstNode for MathArgument {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(kind, SyntaxKind::MATH_GROUP | SyntaxKind::MATH_OPTIONAL)
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        match syntax.kind() {
            SyntaxKind::MATH_GROUP => MathGroup::cast(syntax).map(Self::Brace),
            SyntaxKind::MATH_OPTIONAL => MathOptional::cast(syntax).map(Self::Bracket),
            _ => None,
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Brace(group) => group.syntax(),
            Self::Bracket(optional) => optional.syntax(),
        }
    }
}

impl MathArgument {
    /// The opening `{` or `[` token.
    pub fn open_token(&self) -> Option<SyntaxToken> {
        match self {
            Self::Brace(group) => group.open_token(),
            Self::Bracket(optional) => optional.open_token(),
        }
    }

    /// The closing `}` or `]` token, absent for malformed input.
    pub fn close_token(&self) -> Option<SyntaxToken> {
        match self {
            Self::Brace(group) => group.close_token(),
            Self::Bracket(optional) => optional.close_token(),
        }
    }

    /// Whether this argument carries its matching closing delimiter.
    pub fn is_closed(&self) -> bool {
        self.close_token().is_some()
    }

    /// Direct argument-body elements without the argument's outer delimiters.
    pub fn body_elements(&self) -> std::vec::IntoIter<SyntaxElement> {
        match self {
            Self::Brace(group) => group.body_elements(),
            Self::Bracket(optional) => optional.body_elements(),
        }
    }
}

/// A `[ ... ]` optional argument.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MathOptional(SyntaxNode);

impl AstNode for MathOptional {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MATH_OPTIONAL
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then_some(Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl MathOptional {
    /// The opening `[` token.
    pub fn open_token(&self) -> Option<SyntaxToken> {
        token_child(&self.0, SyntaxKind::MATH_BRACKET_OPEN)
    }

    /// The closing `]` token, absent when recovery reaches a boundary first.
    pub fn close_token(&self) -> Option<SyntaxToken> {
        token_child(&self.0, SyntaxKind::MATH_BRACKET_CLOSE)
    }

    /// Whether the optional carries a matching `]`.
    pub fn is_closed(&self) -> bool {
        self.close_token().is_some()
    }

    /// Direct body elements without the outer `[` and matching terminal `]`.
    pub fn body_elements(&self) -> std::vec::IntoIter<SyntaxElement> {
        bracketed_body_elements(
            &self.0,
            SyntaxKind::MATH_BRACKET_OPEN,
            SyntaxKind::MATH_BRACKET_CLOSE,
        )
    }
}

/// A `\\` row terminator with optional star and bracket modifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MathLineBreak(SyntaxNode);

impl AstNode for MathLineBreak {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MATH_LINE_BREAK
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then_some(Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl MathLineBreak {
    /// The `\\` control-symbol token.
    pub fn marker_token(&self) -> Option<SyntaxToken> {
        token_child(&self.0, SyntaxKind::MATH_CONTROL_SYMBOL)
    }

    /// The optional `*` marker.
    pub fn star_token(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| token.kind() == SyntaxKind::MATH_WORD && token.text() == "*")
    }

    /// The optional bracket modifier, such as `[1ex]`.
    pub fn modifier(&self) -> Option<MathOptional> {
        self.0.children().find_map(MathOptional::cast)
    }
}

/// A `{ ... }` brace group.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MathGroup(SyntaxNode);

impl AstNode for MathGroup {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MATH_GROUP
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then_some(Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl MathGroup {
    /// The opening `{` token.
    pub fn open_token(&self) -> Option<SyntaxToken> {
        token_child(&self.0, SyntaxKind::MATH_GROUP_OPEN)
    }

    /// The closing `}` token, absent when the group is unclosed.
    pub fn close_token(&self) -> Option<SyntaxToken> {
        token_child(&self.0, SyntaxKind::MATH_GROUP_CLOSE)
    }

    /// Whether the group carries a matching `}`.
    pub fn is_closed(&self) -> bool {
        self.close_token().is_some()
    }

    /// Direct body elements without the outer `{` and matching terminal `}`.
    pub fn body_elements(&self) -> std::vec::IntoIter<SyntaxElement> {
        bracketed_body_elements(
            &self.0,
            SyntaxKind::MATH_GROUP_OPEN,
            SyntaxKind::MATH_GROUP_CLOSE,
        )
    }
}

/// A `\begin{env} ... \end{env}` environment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MathEnvironment(SyntaxNode);

impl AstNode for MathEnvironment {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MATH_ENVIRONMENT
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then_some(Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl MathEnvironment {
    /// The opening delimiter node.
    pub fn begin(&self) -> Option<MathBegin> {
        self.0.children().find_map(MathBegin::cast)
    }

    /// The parsed math body between the environment delimiters.
    pub fn body(&self) -> Option<MathContent> {
        self.0.children().find_map(MathContent::cast)
    }

    /// The closing delimiter node, absent when the environment is unclosed.
    pub fn end(&self) -> Option<MathEnd> {
        self.0.children().find_map(MathEnd::cast)
    }

    /// The `\begin` command token.
    pub fn begin_token(&self) -> Option<SyntaxToken> {
        self.begin()?.command_token()
    }

    /// The `\end` command token, absent when the environment is unclosed.
    pub fn end_token(&self) -> Option<SyntaxToken> {
        self.end()?.command_token()
    }

    /// Whether the environment carries a matching `\end`.
    pub fn is_closed(&self) -> bool {
        self.end_token().is_some()
    }

    /// The `{name}` group following `\begin`, braces stripped.
    pub fn begin_name(&self) -> Option<String> {
        self.begin()?.name()
    }

    /// The `{name}` group following `\end`, braces stripped.
    pub fn end_name(&self) -> Option<String> {
        self.end()?.name()
    }
}

/// A `\begin{name}` environment opener.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MathBegin(SyntaxNode);

impl AstNode for MathBegin {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MATH_BEGIN
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then_some(Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl MathBegin {
    /// The `\begin` control-word token.
    pub fn command_token(&self) -> Option<SyntaxToken> {
        command_child(&self.0, r"\begin")
    }

    /// The environment-name group.
    pub fn name_group(&self) -> Option<MathNameGroup> {
        self.0.children().find_map(MathNameGroup::cast)
    }

    /// The trimmed environment name without braces.
    pub fn name(&self) -> Option<String> {
        self.name_group()?.name()
    }
}

/// A `\end{name}` environment closer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MathEnd(SyntaxNode);

impl AstNode for MathEnd {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MATH_END
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then_some(Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl MathEnd {
    /// The `\end` control-word token.
    pub fn command_token(&self) -> Option<SyntaxToken> {
        command_child(&self.0, r"\end")
    }

    /// The environment-name group.
    pub fn name_group(&self) -> Option<MathNameGroup> {
        self.0.children().find_map(MathNameGroup::cast)
    }

    /// The trimmed environment name without braces.
    pub fn name(&self) -> Option<String> {
        self.name_group()?.name()
    }
}

/// The `{name}` group owned by an environment delimiter.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MathNameGroup(SyntaxNode);

impl AstNode for MathNameGroup {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MATH_NAME_GROUP
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then_some(Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl MathNameGroup {
    /// The opening `{` token.
    pub fn open_token(&self) -> Option<SyntaxToken> {
        token_child(&self.0, SyntaxKind::MATH_GROUP_OPEN)
    }

    /// The closing `}` token, absent when the group is unclosed.
    pub fn close_token(&self) -> Option<SyntaxToken> {
        token_child(&self.0, SyntaxKind::MATH_GROUP_CLOSE)
    }

    /// The trimmed name between the braces.
    pub fn name(&self) -> Option<String> {
        self.open_token()?;
        let mut text = self.0.text().to_string();
        text.remove(0);
        if self.close_token().is_some() {
            text.pop();
        }
        Some(text.trim().to_owned())
    }
}

/// A `\left<d> ... \right<d>` paired-delimiter run.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MathDelimited(SyntaxNode);

impl AstNode for MathDelimited {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MATH_DELIMITED
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then_some(Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl MathDelimited {
    /// The opening `\left` command token.
    pub fn left_token(&self) -> Option<SyntaxToken> {
        command_child(&self.0, r"\left")
    }

    /// The closing `\right` command token, absent when the run is unclosed.
    pub fn right_token(&self) -> Option<SyntaxToken> {
        command_child(&self.0, r"\right")
    }

    /// The parsed math body between `\left` and `\right`.
    pub fn body(&self) -> Option<MathContent> {
        self.0.children().find_map(MathContent::cast)
    }

    /// The delimiter token following `\left`, excluding intervening trivia.
    pub fn opening_delimiter(&self) -> Option<SyntaxToken> {
        delimiter_after(&self.0, r"\left")
    }

    /// The delimiter token following `\right`, excluding intervening trivia.
    pub fn closing_delimiter(&self) -> Option<SyntaxToken> {
        delimiter_after(&self.0, r"\right")
    }

    /// Whether the run carries a matching `\right`.
    pub fn is_closed(&self) -> bool {
        self.right_token().is_some()
    }
}

/// A structural problem found in a realized `MATH_CONTENT` subtree. The `range`
/// is host-aligned (the subtree is spliced into the host document tree), so a
/// consumer turns it straight into a diagnostic span with no remapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MathDiagnostic {
    pub kind: MathDiagnosticKind,
    pub range: TextRange,
}

/// The kind of a [`MathDiagnostic`]. A neutral structural identity; downstream
/// consumers (the linter, LSP) map it to their own code and message. The parser
/// crate deliberately does not own linter code strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathDiagnosticKind {
    /// A `{` group with no closing `}` (`MATH_GROUP` lacking `MATH_GROUP_CLOSE`).
    UnclosedGroup,
    /// A `}` with no matching `{` (`MATH_GROUP_CLOSE` outside a `MATH_GROUP`).
    UnexpectedCloseBrace,
    /// A `\begin` with no matching `\end`.
    UnclosedEnvironment,
    /// A `\begin{a}` closed by `\end{b}` with a different name.
    MismatchedEnvironment,
    /// A `\end` with no open `\begin`.
    UnexpectedEnd,
    /// A `\left` with no matching `\right` (`MATH_DELIMITED` lacking `\right`).
    UnclosedDelimiter,
    /// A `\right` with no open `\left` (`\right` outside a `MATH_DELIMITED`).
    UnexpectedRight,
}

/// Walk a realized `MATH_CONTENT` subtree and report structural problems from
/// its shape. This is the single source of truth for math diagnostics, shared
/// by the `math-syntax` linter rule, the formatter (malformed math is left
/// verbatim rather than reflowed), and the LSP. Ranges are host-aligned.
///
/// `content` is expected to be a `MATH_CONTENT` node; its own descendants are
/// walked, so both a standalone sub-parse root and an embedded host-tree node
/// work.
pub fn math_diagnostics(content: &SyntaxNode) -> Vec<MathDiagnostic> {
    let mut out = Vec::new();
    let mut mismatched_ends = Vec::new();
    for node in content.descendants() {
        if let Some(group) = MathGroup::cast(node.clone()) {
            check_group(&group, &mut out);
        } else if let Some(env) = MathEnvironment::cast(node.clone()) {
            check_environment(&env, &mut out, &mut mismatched_ends);
        } else if let Some(delim) = MathDelimited::cast(node.clone()) {
            check_delimited(&delim, &mut out);
        } else if let Some(end) = MathEnd::cast(node.clone())
            && !mismatched_ends.contains(&node.text_range())
            && node
                .parent()
                .is_none_or(|parent| parent.kind() != SyntaxKind::MATH_ENVIRONMENT)
            && let Some(token) = end.command_token()
        {
            out.push(MathDiagnostic {
                kind: MathDiagnosticKind::UnexpectedEnd,
                range: token.text_range(),
            });
        }
        for child in node.children_with_tokens() {
            let Some(token) = child.as_token() else {
                continue;
            };
            match token.kind() {
                SyntaxKind::MATH_GROUP_CLOSE
                    if !matches!(
                        node.kind(),
                        SyntaxKind::MATH_GROUP | SyntaxKind::MATH_NAME_GROUP
                    ) =>
                {
                    out.push(MathDiagnostic {
                        kind: MathDiagnosticKind::UnexpectedCloseBrace,
                        range: token.text_range(),
                    });
                }
                SyntaxKind::MATH_CONTROL_WORD
                    if node.kind() != SyntaxKind::MATH_DELIMITED && token.text() == r"\right" =>
                {
                    out.push(MathDiagnostic {
                        kind: MathDiagnosticKind::UnexpectedRight,
                        range: token.text_range(),
                    });
                }
                SyntaxKind::MATH_CONTROL_WORD
                    if node.kind() != SyntaxKind::MATH_DELIMITED && token.text() == r"\left" =>
                {
                    out.push(MathDiagnostic {
                        kind: MathDiagnosticKind::UnclosedDelimiter,
                        range: token.text_range(),
                    });
                }
                _ => {}
            }
        }
    }
    out
}

fn check_group(group: &MathGroup, out: &mut Vec<MathDiagnostic>) {
    if group.is_closed() {
        return;
    }
    if let Some(open) = group.open_token() {
        out.push(MathDiagnostic {
            kind: MathDiagnosticKind::UnclosedGroup,
            range: open.text_range(),
        });
    }
}

fn check_environment(
    env: &MathEnvironment,
    out: &mut Vec<MathDiagnostic>,
    mismatched_ends: &mut Vec<TextRange>,
) {
    let Some(end) = env.end_token() else {
        if let Some(end) = following_environment_end(env.syntax()) {
            let range = end
                .name_group()
                .map(|group| group.syntax().text_range())
                .or_else(|| end.command_token().map(|token| token.text_range()))
                .unwrap_or_else(|| end.syntax().text_range());
            mismatched_ends.push(end.syntax().text_range());
            out.push(MathDiagnostic {
                kind: MathDiagnosticKind::MismatchedEnvironment,
                range,
            });
            return;
        }
        let range = env
            .begin_token()
            .map(|t| t.text_range())
            .unwrap_or_else(|| env.syntax().text_range());
        out.push(MathDiagnostic {
            kind: MathDiagnosticKind::UnclosedEnvironment,
            range,
        });
        return;
    };
    if env.begin_name().unwrap_or_default() != env.end_name().unwrap_or_default() {
        let range = env
            .end()
            .and_then(|end| end.name_group())
            .map(|group| group.syntax().text_range())
            .unwrap_or_else(|| end.text_range());
        out.push(MathDiagnostic {
            kind: MathDiagnosticKind::MismatchedEnvironment,
            range,
        });
    }
}

fn following_environment_end(environment: &SyntaxNode) -> Option<MathEnd> {
    environment.next_sibling().and_then(MathEnd::cast)
}

fn check_delimited(delim: &MathDelimited, out: &mut Vec<MathDiagnostic>) {
    if delim.is_closed() {
        return;
    }
    if let Some(left) = delim.left_token() {
        out.push(MathDiagnostic {
            kind: MathDiagnosticKind::UnclosedDelimiter,
            range: left.text_range(),
        });
    }
}

fn token_child(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|c| c.into_token())
        .find(|t| t.kind() == kind)
}

fn command_child(node: &SyntaxNode, text: &str) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|c| c.into_token())
        .find(|t| t.kind() == SyntaxKind::MATH_CONTROL_WORD && t.text() == text)
}

fn delimiter_after(node: &SyntaxNode, command: &str) -> Option<SyntaxToken> {
    let mut after_command = false;
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token)
                if token.kind() == SyntaxKind::MATH_CONTROL_WORD && token.text() == command =>
            {
                after_command = true;
            }
            rowan::NodeOrToken::Token(token)
                if after_command
                    && !matches!(
                        token.kind(),
                        SyntaxKind::MATH_SPACE
                            | SyntaxKind::MATH_NEWLINE
                            | SyntaxKind::MATH_COMMENT
                    ) =>
            {
                return Some(token);
            }
            rowan::NodeOrToken::Node(_) if after_command => return None,
            _ => {}
        }
    }
    None
}

fn script_argument(node: &SyntaxNode) -> Option<SyntaxElement> {
    node.children_with_tokens().find(|element| {
        !matches!(
            element.kind(),
            SyntaxKind::MATH_CARET
                | SyntaxKind::MATH_UNDERSCORE
                | SyntaxKind::MATH_SPACE
                | SyntaxKind::MATH_NEWLINE
                | SyntaxKind::LINE_PREFIX
                | SyntaxKind::NEWLINE
        )
    })
}

fn bracketed_body_elements(
    node: &SyntaxNode,
    open_kind: SyntaxKind,
    close_kind: SyntaxKind,
) -> std::vec::IntoIter<SyntaxElement> {
    let mut elements: Vec<_> = node
        .children_with_tokens()
        .filter(is_math_content_element)
        .collect();
    if elements
        .first()
        .is_some_and(|element| element.kind() == open_kind)
    {
        elements.remove(0);
    }
    if elements
        .last()
        .is_some_and(|element| element.kind() == close_kind)
    {
        elements.pop();
    }
    elements.into_iter()
}

fn is_math_content_element(element: &SyntaxElement) -> bool {
    match element {
        rowan::NodeOrToken::Node(node) => matches!(
            node.kind(),
            SyntaxKind::MATH_CONTENT
                | SyntaxKind::MATH_GROUP
                | SyntaxKind::MATH_OPTIONAL
                | SyntaxKind::MATH_ENVIRONMENT
                | SyntaxKind::MATH_BEGIN
                | SyntaxKind::MATH_END
                | SyntaxKind::MATH_NAME_GROUP
                | SyntaxKind::MATH_DELIMITED
                | SyntaxKind::MATH_SCRIPTED
                | SyntaxKind::MATH_SUBSCRIPT
                | SyntaxKind::MATH_SUPERSCRIPT
                | SyntaxKind::MATH_COMMAND
                | SyntaxKind::MATH_LINE_BREAK
        ),
        rowan::NodeOrToken::Token(token) => is_math_content_token(token.kind()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use crate::syntax::InlineMath;

    #[test]
    fn display_math_dollar_markers_and_content() {
        let tree = parse("$$\nx^2 + y^2\n$$\n", None);
        let math = tree
            .descendants()
            .find_map(DisplayMath::cast)
            .expect("display math");

        assert_eq!(math.opening_marker().as_deref(), Some("$$"));
        assert_eq!(math.closing_marker().as_deref(), Some("$$"));
        assert!(math.content().contains("x^2 + y^2"));
        assert!(!math.is_environment_form());
    }

    #[test]
    fn display_math_environment_form_detection() {
        let tree = parse("\\begin{align}\na &= b\\\\\n\\end{align}\n", None);
        let math = tree
            .descendants()
            .find_map(DisplayMath::cast)
            .expect("display math");

        assert!(math.is_environment_form());
        assert_eq!(math.opening_marker().as_deref(), Some("\\begin{align}"));
        assert_eq!(math.closing_marker().as_deref(), Some("\\end{align}\n"));
    }

    #[test]
    fn display_math_detects_unescaped_single_dollar() {
        let tree = parse("$$\nalpha $beta$ gamma\n$$\n", None);
        let math = tree
            .descendants()
            .find_map(DisplayMath::cast)
            .expect("display math");
        assert!(math.has_unescaped_single_dollar_in_content());
    }

    use crate::parser::math::{MathParseOptions, parse_math_content};

    fn diag_kinds(content: &str) -> Vec<MathDiagnosticKind> {
        let node = SyntaxNode::new_root(parse_math_content(content, MathParseOptions::default()));
        math_diagnostics(&node)
            .into_iter()
            .map(|d| d.kind)
            .collect()
    }

    #[test]
    fn typed_script_wrappers_expose_base_markers_and_arguments() {
        let node = SyntaxNode::new_root(parse_math_content("x^2_i", MathParseOptions::default()));
        let scripted = node
            .children()
            .find_map(MathScripted::cast)
            .expect("scripted atom");

        assert_eq!(
            scripted.base().map(|base| base.to_string()).as_deref(),
            Some("x")
        );
        let superscript = scripted.superscript().expect("superscript");
        assert_eq!(
            superscript.marker_token().as_ref().map(SyntaxToken::text),
            Some("^")
        );
        assert_eq!(
            superscript
                .argument()
                .map(|argument| argument.to_string())
                .as_deref(),
            Some("2")
        );
        let subscript = scripted.subscript().expect("subscript");
        assert_eq!(
            subscript.marker_token().as_ref().map(SyntaxToken::text),
            Some("_")
        );
        assert_eq!(
            subscript
                .argument()
                .map(|argument| argument.to_string())
                .as_deref(),
            Some("i")
        );
    }

    #[test]
    fn typed_script_wrapper_allows_a_missing_argument() {
        let node = SyntaxNode::new_root(parse_math_content("x^", MathParseOptions::default()));
        let superscript = node
            .descendants()
            .find_map(MathSuperscript::cast)
            .expect("superscript");

        assert!(superscript.argument().is_none());
    }

    #[test]
    fn typed_command_wrappers_separate_groups_and_optionals() {
        let node = SyntaxNode::new_root(parse_math_content(
            r"\sqrt[3]{x}",
            MathParseOptions::default(),
        ));
        let command = node
            .children()
            .find_map(MathCommand::cast)
            .expect("command");

        assert_eq!(command.groups().count(), 1);
        assert_eq!(command.arguments().count(), 1);
        let optional = command.optionals().next().expect("optional");
        assert_eq!(
            optional.open_token().as_ref().map(SyntaxToken::text),
            Some("[")
        );
        assert_eq!(
            optional.close_token().as_ref().map(SyntaxToken::text),
            Some("]")
        );
        assert!(optional.is_closed());
    }

    #[test]
    fn typed_command_arguments_preserve_source_order_and_bodies() {
        let node = SyntaxNode::new_root(parse_math_content(
            r"\sqrt[3]{x}{extra}",
            MathParseOptions::default(),
        ));
        let command = node
            .children()
            .find_map(MathCommand::cast)
            .expect("command");

        let arguments: Vec<_> = command.attached_arguments().collect();
        assert_eq!(arguments.len(), 3);
        assert!(matches!(arguments[0], MathArgument::Bracket(_)));
        assert!(matches!(arguments[1], MathArgument::Brace(_)));
        assert!(matches!(arguments[2], MathArgument::Brace(_)));
        assert_eq!(
            arguments
                .iter()
                .map(|argument| {
                    argument
                        .body_elements()
                        .map(|element| element.to_string())
                        .collect::<String>()
                })
                .collect::<Vec<_>>(),
            ["3", "x", "extra"]
        );
    }

    #[test]
    fn typed_command_wrapper_exposes_attached_star() {
        let node = SyntaxNode::new_root(parse_math_content(
            r"\operatorname*{arg}",
            MathParseOptions::default(),
        ));
        let command = node
            .children()
            .find_map(MathCommand::cast)
            .expect("command");

        assert_eq!(
            command.star_token().as_ref().map(SyntaxToken::text),
            Some("*")
        );
    }

    #[test]
    fn typed_scripts_preserve_source_order() {
        let node = SyntaxNode::new_root(parse_math_content("x^a_b^c", MathParseOptions::default()));
        let scripted = node
            .children()
            .find_map(MathScripted::cast)
            .expect("scripted atom");

        let scripts: Vec<_> = scripted.scripts().collect();
        assert!(matches!(scripts[0], MathScript::Superscript(_)));
        assert!(matches!(scripts[1], MathScript::Subscript(_)));
        assert!(matches!(scripts[2], MathScript::Superscript(_)));
        assert_eq!(
            scripts
                .iter()
                .filter_map(MathScript::argument)
                .map(|argument| argument.to_string())
                .collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
    }

    #[test]
    fn typed_line_break_exposes_star_and_optional_modifier() {
        let node = SyntaxNode::new_root(parse_math_content(
            r"a\\*[1ex]b",
            MathParseOptions::default(),
        ));
        let line_break = node
            .children()
            .find_map(MathLineBreak::cast)
            .expect("line break");

        assert_eq!(
            line_break.marker_token().as_ref().map(SyntaxToken::text),
            Some(r"\\")
        );
        assert_eq!(
            line_break.star_token().as_ref().map(SyntaxToken::text),
            Some("*")
        );
        let modifier = line_break.modifier().expect("line-break modifier");
        assert_eq!(
            modifier
                .body_elements()
                .map(|element| element.to_string())
                .collect::<String>(),
            "1ex"
        );
    }

    #[test]
    fn typed_line_break_preserves_an_unclosed_modifier() {
        let node =
            SyntaxNode::new_root(parse_math_content(r"a\\[1ex", MathParseOptions::default()));
        let line_break = node
            .children()
            .find_map(MathLineBreak::cast)
            .expect("line break");

        assert!(line_break.star_token().is_none());
        let modifier = line_break.modifier().expect("line-break modifier");
        assert!(!modifier.is_closed());
        assert_eq!(
            modifier
                .body_elements()
                .map(|element| element.to_string())
                .collect::<String>(),
            "1ex"
        );
    }

    #[test]
    fn typed_content_segments_keep_host_labels_between_tex_segments() {
        let mut options = crate::ParserOptions::default();
        options.extensions.bookdown_equation_references = true;
        let tree = parse(r"$x (\#eq:inline) + y$", Some(options));
        let math = tree
            .descendants()
            .find_map(InlineMath::cast)
            .expect("inline math");

        let segments: Vec<_> = math.content_segments().collect();
        assert_eq!(segments.len(), 3);
        assert!(matches!(segments[0], MathContentSegment::Content(_)));
        assert!(matches!(segments[1], MathContentSegment::EquationLabel(_)));
        assert!(matches!(segments[2], MathContentSegment::Content(_)));
        assert_eq!(
            segments
                .iter()
                .map(MathContentSegment::text)
                .collect::<Vec<_>>(),
            ["x ", r"(\#eq:inline)", " + y"]
        );
    }

    #[test]
    fn typed_display_content_segments_keep_host_labels_in_order() {
        let mut options = crate::ParserOptions::default();
        options.extensions.bookdown_equation_references = true;
        let tree = parse("$$x (\\#eq:display) + y$$\n", Some(options));
        let math = tree
            .descendants()
            .find_map(DisplayMath::cast)
            .expect("display math");

        assert_eq!(
            math.content_segments()
                .map(|segment| segment.text())
                .collect::<Vec<_>>(),
            ["x ", r"(\#eq:display)", " + y"]
        );
    }

    #[test]
    fn typed_math_content_elements_exclude_container_prefixes() {
        let tree = parse("> $x\n>  + y$\n", None);
        let content = tree
            .descendants()
            .find_map(MathContent::cast)
            .expect("math content");

        assert_eq!(content.text(), "x\n + y");
        assert_eq!(
            content
                .elements()
                .map(|element| element.to_string())
                .collect::<String>(),
            "x\n + y"
        );
    }

    #[test]
    fn typed_argument_body_preserves_nested_and_malformed_content() {
        let node = SyntaxNode::new_root(parse_math_content(
            "\\foo{a{b}% note\n c",
            MathParseOptions::default(),
        ));
        let command = node
            .children()
            .find_map(MathCommand::cast)
            .expect("command");
        let argument = command
            .attached_arguments()
            .next()
            .expect("attached argument");

        assert!(!argument.is_closed());
        assert_eq!(
            argument
                .body_elements()
                .map(|element| element.to_string())
                .collect::<String>(),
            "a{b}% note\n c"
        );
    }

    #[test]
    fn typed_delimiter_and_environment_access_respects_recovery_shapes() {
        let delimited_tree =
            SyntaxNode::new_root(parse_math_content(r"\left( x", MathParseOptions::default()));
        assert_eq!(
            delimited_tree
                .children()
                .filter_map(MathDelimited::cast)
                .count(),
            0
        );
        assert_eq!(
            math_diagnostics(&delimited_tree)
                .into_iter()
                .map(|diagnostic| diagnostic.kind)
                .collect::<Vec<_>>(),
            [MathDiagnosticKind::UnclosedDelimiter]
        );

        let environment_tree = SyntaxNode::new_root(parse_math_content(
            r"\begin{aligned}x",
            MathParseOptions::default(),
        ));
        let environment = environment_tree
            .children()
            .find_map(MathEnvironment::cast)
            .expect("environment");
        assert_eq!(environment.begin_name().as_deref(), Some("aligned"));
        assert!(environment.end().is_none());
        assert!(!environment.is_closed());
    }

    #[test]
    fn typed_environment_wrappers_expose_delimiters_names_and_body() {
        let tree = SyntaxNode::new_root(parse_math_content(
            r"\begin {aligned}x &= y\end{aligned}",
            MathParseOptions::default(),
        ));
        let environment = tree
            .descendants()
            .find_map(MathEnvironment::cast)
            .expect("environment");

        assert_eq!(environment.begin_name().as_deref(), Some("aligned"));
        assert_eq!(environment.end_name().as_deref(), Some("aligned"));
        assert_eq!(
            environment.body().expect("body").syntax().to_string(),
            "x &= y"
        );
        assert!(environment.is_closed());
    }

    #[test]
    fn typed_delimited_wrapper_exposes_delimiters_and_body() {
        let tree = SyntaxNode::new_root(parse_math_content(
            r"\left[ x \right]",
            MathParseOptions::default(),
        ));
        let delimited = tree
            .children()
            .find_map(MathDelimited::cast)
            .expect("delimited");

        assert_eq!(
            delimited
                .opening_delimiter()
                .as_ref()
                .map(SyntaxToken::text),
            Some("[")
        );
        assert_eq!(
            delimited
                .closing_delimiter()
                .as_ref()
                .map(SyntaxToken::text),
            Some("]")
        );
        assert_eq!(delimited.body().expect("body").syntax().to_string(), " x ");
    }

    #[test]
    fn typed_script_wrapper_ignores_multiline_container_prefixes() {
        let tree = parse("> $x\n>  ^ 2$\n", None);
        let math = tree
            .descendants()
            .find_map(InlineMath::cast)
            .expect("inline math");
        let scripted = math
            .syntax()
            .descendants()
            .find_map(MathScripted::cast)
            .expect("scripted atom");

        assert_eq!(math.content(), "x\n ^ 2");
        assert_eq!(
            scripted
                .superscript()
                .and_then(|script| script.argument())
                .map(|argument| argument.to_string())
                .as_deref(),
            Some("2")
        );
    }

    #[test]
    fn unclosed_group_is_diagnosed_at_the_open_brace() {
        let node = SyntaxNode::new_root(parse_math_content("{a", MathParseOptions::default()));
        let diags = math_diagnostics(&node);
        assert_eq!(
            diags.iter().map(|d| d.kind).collect::<Vec<_>>(),
            vec![MathDiagnosticKind::UnclosedGroup]
        );
        let start: usize = diags[0].range.start().into();
        let end: usize = diags[0].range.end().into();
        assert_eq!(&"{a"[start..end], "{");
    }

    #[test]
    fn stray_close_brace_is_diagnosed() {
        assert_eq!(
            diag_kinds("a}b"),
            vec![MathDiagnosticKind::UnexpectedCloseBrace]
        );
    }

    #[test]
    fn unclosed_environment_is_diagnosed() {
        assert_eq!(
            diag_kinds(r"\begin{aligned} x &= 1"),
            vec![MathDiagnosticKind::UnclosedEnvironment]
        );
    }

    #[test]
    fn mismatched_environment_is_diagnosed() {
        assert_eq!(
            diag_kinds(r"\begin{aligned}x\end{matrix}"),
            vec![MathDiagnosticKind::MismatchedEnvironment]
        );
    }

    #[test]
    fn matching_outer_end_leaves_only_the_inner_environment_unclosed() {
        assert_eq!(
            diag_kinds(r"\begin{aligned}\begin{matrix}x\end{aligned}"),
            vec![MathDiagnosticKind::UnclosedEnvironment]
        );
    }

    #[test]
    fn stray_end_is_diagnosed() {
        assert_eq!(
            diag_kinds(r"x \end{aligned}"),
            vec![MathDiagnosticKind::UnexpectedEnd]
        );
    }

    #[test]
    fn well_formed_math_has_no_diagnostics() {
        assert!(diag_kinds(r"\frac{1}{2} + x^{2}").is_empty());
        assert!(diag_kinds(r"\begin{a}\begin{b}x\end{b}\end{a}").is_empty());
        assert!(diag_kinds(r"\left( x + y \right]").is_empty());
        assert!(diag_kinds(r"\left. x \right|").is_empty());
    }

    #[test]
    fn unclosed_delimiter_is_diagnosed_at_the_left() {
        let node =
            SyntaxNode::new_root(parse_math_content(r"\left( x", MathParseOptions::default()));
        let diags = math_diagnostics(&node);
        assert_eq!(
            diags.iter().map(|d| d.kind).collect::<Vec<_>>(),
            vec![MathDiagnosticKind::UnclosedDelimiter]
        );
        let start: usize = diags[0].range.start().into();
        let end: usize = diags[0].range.end().into();
        assert_eq!(&r"\left( x"[start..end], r"\left");
    }

    #[test]
    fn stray_right_is_diagnosed() {
        assert_eq!(
            diag_kinds(r"x \right)"),
            vec![MathDiagnosticKind::UnexpectedRight]
        );
    }
}
