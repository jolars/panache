//! Math AST node wrappers.

use rowan::TextRange;

use super::{AstNode, PanacheLanguage, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};

/// Reconstruct the raw math content of a math node from its `MATH_CONTENT`
/// subtree, keeping only the math tokens.
///
/// Container machinery (blockquotes, list continuations, …) interleaves host
/// prefix tokens (`BLOCK_QUOTE_MARKER`, `WHITESPACE`, `NEWLINE`) into the
/// subtree on continuation lines for lossless capture. Those prefixes are not
/// part of the math, so they are excluded here — otherwise e.g. a blockquote
/// `>` would leak into the content and re-accumulate on every format pass.
pub fn math_content_text(math: &SyntaxNode) -> String {
    let Some(content) = math
        .children()
        .find(|node| node.kind() == SyntaxKind::MATH_CONTENT)
    else {
        return String::new();
    };
    content
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| is_math_content_token(tok.kind()))
        .map(|tok| tok.text().to_string())
        .collect()
}

/// Whether `kind` is a math-content token emitted by the math parser (as
/// opposed to a host container prefix interleaved into the subtree).
fn is_math_content_token(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::MATH_TEXT
            | SyntaxKind::MATH_SPACE
            | SyntaxKind::MATH_NEWLINE
            | SyntaxKind::MATH_COMMAND
            | SyntaxKind::MATH_GROUP_OPEN
            | SyntaxKind::MATH_GROUP_CLOSE
            | SyntaxKind::MATH_ALIGN
            | SyntaxKind::MATH_SCRIPT
            | SyntaxKind::MATH_OPERATOR
            | SyntaxKind::MATH_OPEN
            | SyntaxKind::MATH_CLOSE
            | SyntaxKind::MATH_PUNCT
            | SyntaxKind::MATH_LINE_BREAK
            | SyntaxKind::MATH_COMMENT
            | SyntaxKind::MATH_EQUATION_LABEL
    )
}

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

    /// The raw math content between the delimiters, reconstructed from the
    /// `MATH_CONTENT` subtree (excluding host container prefixes — see
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
    /// Structural problems in this subtree (see [`math_diagnostics`]).
    pub fn diagnostics(&self) -> Vec<MathDiagnostic> {
        math_diagnostics(&self.0)
    }
}

/// A `{ ... }` brace group.
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
}

/// A `\begin{env} ... \end{env}` environment.
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
    /// The `\begin` command token.
    pub fn begin_token(&self) -> Option<SyntaxToken> {
        command_child(&self.0, r"\begin")
    }

    /// The `\end` command token, absent when the environment is unclosed.
    pub fn end_token(&self) -> Option<SyntaxToken> {
        command_child(&self.0, r"\end")
    }

    /// Whether the environment carries a matching `\end`.
    pub fn is_closed(&self) -> bool {
        self.end_token().is_some()
    }

    /// The `{name}` group following `\begin`, braces stripped.
    pub fn begin_name(&self) -> Option<String> {
        let children: Vec<SyntaxElement> = self.0.children_with_tokens().collect();
        let bi = children.iter().position(|c| is_command(c, r"\begin"))?;
        group_name_after(&children, bi)
    }

    /// The `{name}` group following `\end`, braces stripped.
    pub fn end_name(&self) -> Option<String> {
        let children: Vec<SyntaxElement> = self.0.children_with_tokens().collect();
        let ei = children.iter().position(|c| is_command(c, r"\end"))?;
        group_name_after(&children, ei)
    }
}

/// A `\left<d> ... \right<d>` paired-delimiter run.
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
    for node in content.descendants() {
        if let Some(group) = MathGroup::cast(node.clone()) {
            check_group(&group, &mut out);
        } else if let Some(env) = MathEnvironment::cast(node.clone()) {
            check_environment(&env, &mut out);
        } else if let Some(delim) = MathDelimited::cast(node.clone()) {
            check_delimited(&delim, &mut out);
        }
        // Stray tokens are flagged by their parent context: each token is a
        // direct child of exactly one node, so iterating every node's direct
        // children visits every token once.
        for child in node.children_with_tokens() {
            let Some(token) = child.as_token() else {
                continue;
            };
            match token.kind() {
                SyntaxKind::MATH_GROUP_CLOSE if node.kind() != SyntaxKind::MATH_GROUP => {
                    out.push(MathDiagnostic {
                        kind: MathDiagnosticKind::UnexpectedCloseBrace,
                        range: token.text_range(),
                    });
                }
                SyntaxKind::MATH_COMMAND
                    if node.kind() != SyntaxKind::MATH_ENVIRONMENT && token.text() == r"\end" =>
                {
                    out.push(MathDiagnostic {
                        kind: MathDiagnosticKind::UnexpectedEnd,
                        range: token.text_range(),
                    });
                }
                SyntaxKind::MATH_COMMAND
                    if node.kind() != SyntaxKind::MATH_DELIMITED && token.text() == r"\right" =>
                {
                    out.push(MathDiagnostic {
                        kind: MathDiagnosticKind::UnexpectedRight,
                        range: token.text_range(),
                    });
                }
                _ => {}
            }
        }
    }
    out
}

/// A `MATH_GROUP` is well-formed only if it carries a closing `}`; the parser
/// emits `MATH_GROUP_CLOSE` solely when the brace is matched.
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

fn check_environment(env: &MathEnvironment, out: &mut Vec<MathDiagnostic>) {
    let Some(end) = env.end_token() else {
        // No closing `\end`: point at the opening `\begin` (or the whole node).
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
        // Point at the `\end` name group (or the `\end` token if unnamed).
        let children: Vec<SyntaxElement> = env.syntax().children_with_tokens().collect();
        let range = children
            .iter()
            .position(|c| is_command(c, r"\end"))
            .and_then(|ei| group_range_after(&children, ei))
            .unwrap_or_else(|| end.text_range());
        out.push(MathDiagnostic {
            kind: MathDiagnosticKind::MismatchedEnvironment,
            range,
        });
    }
}

/// A `MATH_DELIMITED` run is well-formed only if it carries a closing `\right`;
/// the parser emits it solely when the `\left` was matched.
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

/// The first direct token child of `node` with the given `kind`.
fn token_child(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|c| c.into_token())
        .find(|t| t.kind() == kind)
}

/// The first direct `MATH_COMMAND` token child with exactly `text`.
fn command_child(node: &SyntaxNode, text: &str) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|c| c.into_token())
        .find(|t| t.kind() == SyntaxKind::MATH_COMMAND && t.text() == text)
}

fn is_command(el: &SyntaxElement, text: &str) -> bool {
    el.as_token()
        .is_some_and(|t| t.kind() == SyntaxKind::MATH_COMMAND && t.text() == text)
}

/// Inner text of the first `MATH_GROUP` after `idx` (the environment name
/// group), with its braces stripped — mirrors `parse_environment_name`.
fn group_name_after(children: &[SyntaxElement], idx: usize) -> Option<String> {
    children[idx + 1..].iter().find_map(|c| {
        c.as_node()
            .filter(|n| n.kind() == SyntaxKind::MATH_GROUP)
            .map(|g| {
                g.text()
                    .to_string()
                    .trim_start_matches('{')
                    .trim_end_matches('}')
                    .to_string()
            })
    })
}

fn group_range_after(children: &[SyntaxElement], idx: usize) -> Option<TextRange> {
    children[idx + 1..].iter().find_map(|c| {
        c.as_node()
            .filter(|n| n.kind() == SyntaxKind::MATH_GROUP)
            .map(|g| g.text_range())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

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

    // --- Diagnostics derived from the realized MATH_CONTENT subtree ---

    use crate::parser::math::{MathParseOptions, parse_math_content};

    /// Build a standalone `MATH_CONTENT` root from raw content and report its
    /// diagnostic kinds. The sub-parse root is itself `MATH_CONTENT`, matching
    /// the embedded-node case the linter/formatter feed in.
    fn diag_kinds(content: &str) -> Vec<MathDiagnosticKind> {
        let node = SyntaxNode::new_root(parse_math_content(content, MathParseOptions::default()));
        math_diagnostics(&node)
            .into_iter()
            .map(|d| d.kind)
            .collect()
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
