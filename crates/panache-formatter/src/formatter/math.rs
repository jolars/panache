//! In-tree TeX math **content** formatter (experimental, opt-in).
//!
//! Consumes the lossless structural math CST built by
//! [`panache_parser::parser::math`] and re-emits the content with structurally
//! safe normalizations: inline whitespace collapse, environment-body
//! indentation, `\\` line-break normalization, `&`-column alignment, and
//! precedence-aware operator spacing. The canonical rules live in `STYLE.md`
//! (next to this file).
//!
//! Like the YAML formatter, this **re-parses the clean content string** rather
//! than walking the host-embedded subtree. The host block machinery interleaves
//! container prefixes (a blockquote `>` and its whitespace) into `MATH_CONTENT`
//! on continuation lines; re-parsing the already-prefix-stripped string (from
//! [`panache_parser::syntax::math::math_content_text`]) sidesteps that entirely.
//!
//! Operator spacing is *interpretation*, not a CST shape: the parser emits
//! neutral `MATH_WORD` runs. Migrated slices consume the parser's semantic atom
//! stream; the legacy fallback still uses [`operators`] until its remaining
//! shapes reach Badness parity. Still out of scope: `\frac` canonicalization,
//! auto-`&` insertion, and macro rewriting.
//!
//! The gate is [`crate::config::Config::experimental_format_math`]. Off (the
//! default) callers emit math verbatim and never reach this module; on, they
//! route content through [`format_math`].

use crate::config::MathDelimiterStyle;
use crate::formatter::Formatter;
use crate::syntax::{
    DisplayMath, InlineMath, MathContent, MathSubscript, MathSuperscript, SyntaxKind, SyntaxNode,
    math_diagnostics,
};
use panache_parser::parser::math::{MathParseOptions, parse_math_content};
use panache_parser::semantic::math::SignatureScope;
use rowan::NodeOrToken;
use rowan::ast::AstNode;

mod ir;
mod linebreak;
mod lower;
pub mod operators;
mod printer;
mod render;

impl Formatter {
    pub(super) fn format_inline_math_marker(&mut self, node: &SyntaxNode) {
        self.output.push_str(node.text().to_string().trim());
    }

    pub(super) fn format_inline_math(&mut self, node: &SyntaxNode) {
        let is_display_math = node.children_with_tokens().any(|child| {
            matches!(child, NodeOrToken::Token(token) if token.kind() == SyntaxKind::DISPLAY_MATH_MARKER)
        });
        let content = InlineMath::cast(node.clone())
            .map(|math| math.content())
            .unwrap_or_default();
        let marker = node
            .children_with_tokens()
            .find_map(|child| match child {
                NodeOrToken::Token(token)
                    if matches!(
                        token.kind(),
                        SyntaxKind::INLINE_MATH_MARKER | SyntaxKind::DISPLAY_MATH_MARKER
                    ) =>
                {
                    Some(token.text().to_string())
                }
                _ => None,
            })
            .unwrap_or_else(|| "$".to_string());
        let (open, close) =
            inline_delimiters(self.config.math_delimiter_style, is_display_math, &marker);

        self.output.push_str(open);
        if is_display_math {
            self.output.push(' ');
        }
        self.output.push_str(&content);
        if is_display_math {
            self.output.push(' ');
        }
        self.output.push_str(close);
    }

    pub(super) fn format_display_math(&mut self, node: &SyntaxNode) {
        let Some(display_math) = DisplayMath::cast(node.clone()) else {
            self.output.push_str(&node.text().to_string());
            return;
        };
        let content = display_math.content();
        let opening = display_math
            .opening_marker()
            .unwrap_or_else(|| "$$".to_string());
        let closing = display_math
            .closing_marker()
            .unwrap_or_else(|| "$$".to_string());
        let is_environment = display_math.is_environment_form();
        let (open, close) = if is_environment {
            (opening.as_str(), closing.as_str())
        } else {
            match self.config.math_delimiter_style {
                MathDelimiterStyle::Preserve => (opening.as_str(), closing.as_str()),
                MathDelimiterStyle::Dollars => ("$$", "$$"),
                MathDelimiterStyle::Backslash => (r"\[", r"\]"),
            }
        };

        if is_environment {
            self.output.push_str(open);
            let opts = MathFormatOptions::from_config(&self.config, MathContext::EnvironmentBody);
            match format_math(&content, &opts) {
                Some(body) => {
                    self.output.push('\n');
                    push_body_with_trailing_newline(&mut self.output, &body);
                }
                None => {
                    self.output.push_str(&content);
                    if !content.ends_with('\n') {
                        self.output.push('\n');
                    }
                }
            }
            self.output.push_str(close);
            self.output.push('\n');
            return;
        }

        self.output.push('\n');
        self.output.push_str(open);
        self.output.push('\n');
        let opts = MathFormatOptions::from_config(&self.config, MathContext::Display);
        match format_math(&content, &opts) {
            Some(body) => {
                push_body_with_trailing_newline(&mut self.output, &body);
            }
            None => {
                for line in content.trim().lines() {
                    self.output.push_str(&" ".repeat(self.config.math_indent));
                    self.output.push_str(line.trim_end());
                    self.output.push('\n');
                }
            }
        }
        self.output.push_str(close);
        self.output.push('\n');
    }
}

/// Renderers may retain an authored final newline for TeX parity, while the
/// host still needs exactly one separator before its closing delimiter.
pub(super) fn push_body_with_trailing_newline(output: &mut String, body: &str) {
    output.push_str(body);
    if !body.ends_with('\n') {
        output.push('\n');
    }
}

fn inline_delimiters(
    style: MathDelimiterStyle,
    is_display_math: bool,
    marker: &str,
) -> (&'static str, &'static str) {
    match style {
        MathDelimiterStyle::Preserve if is_display_math => match marker {
            "\\[" => (r"\[", r"\]"),
            "\\\\[" => (r"\\[", r"\\]"),
            _ => ("$$", "$$"),
        },
        MathDelimiterStyle::Preserve => match marker {
            "$`" => ("$`", "`$"),
            r"\(" => (r"\(", r"\)"),
            r"\\(" => (r"\\(", r"\\)"),
            _ => ("$", "$"),
        },
        MathDelimiterStyle::Dollars if is_display_math => ("$$", "$$"),
        MathDelimiterStyle::Dollars => ("$", "$"),
        MathDelimiterStyle::Backslash if is_display_math => (r"\[", r"\]"),
        MathDelimiterStyle::Backslash => (r"\(", r"\)"),
    }
}

/// Where a math span sits, which decides how aggressively it is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathContext {
    /// `$...$` / `` $`...`$ `` / `\(...\)`. Single line; whitespace collapse only.
    Inline,
    /// `$$...$$` / `\[...\]`. Multi-line block: free rows + nested environments.
    Display,
    /// Raw `\begin{env}...\end{env}` whose delimiters *are* the environment —
    /// the content is the bare body, laid out as one (possibly aligned) table.
    EnvironmentBody,
}

/// Inputs for [`format_math`], derived from the host [`Config`](crate::Config)
/// at each call site.
#[derive(Debug, Clone)]
pub struct MathFormatOptions {
    /// Master gate. False ⇒ [`format_math`] returns its input verbatim, so a
    /// mis-wired call site can never change bytes.
    pub enabled: bool,
    /// Flat per-line indent applied to non-environment `$$` content only
    /// (mirrors today's `math_indent`). Environment bodies ignore it.
    pub math_indent: usize,
    /// Target line width (host `line-width`). Only the display free-row
    /// line-breaker reads it: a free row wider than this is broken at its
    /// highest-priority top-level operators. Inline and environment layout
    /// ignore it.
    pub line_width: usize,
    /// Recognize bookdown `(\#eq:label)` labels — must match the host's
    /// parse-time option so the re-parse reproduces the same token shape.
    pub bookdown_equation_labels: bool,
    /// Inline vs display vs bare-environment layout.
    pub context: MathContext,
    /// Configured signatures with raw document definitions layered above them.
    pub signature_scope: SignatureScope,
}

impl MathFormatOptions {
    /// Derive options from the host config for a given span context.
    pub fn from_config(config: &crate::config::Config, context: MathContext) -> Self {
        Self {
            enabled: config.experimental_format_math,
            math_indent: config.math_indent,
            line_width: config.line_width,
            bookdown_equation_labels: config.parser_extensions.bookdown_equation_references,
            context,
            signature_scope: config.math_signature_scope.clone(),
        }
    }
}

/// Reflow clean math content (delimiters excluded, both in and out).
///
/// Returns `None` — never panicking, never erroring — on any bail condition: the
/// gate is off, the content has an unescaped lone `$` (a preservation guard
/// against cross-pass drift), or the structural parse reports a diagnostic
/// (malformed math is never reflowed). The caller then emits the content through
/// its own verbatim path, so gate-off and malformed-gate-on stay byte-identical
/// and no fence-padding normalization is duplicated here. On success it returns
/// the reflowed content per `STYLE.md`.
pub fn format_math(input: &str, opts: &MathFormatOptions) -> Option<String> {
    if !opts.enabled {
        return None;
    }
    if has_unescaped_single_dollar(input) {
        return None;
    }
    let tree = SyntaxNode::new_root(parse_math_content(
        input,
        MathParseOptions {
            bookdown_equation_labels: opts.bookdown_equation_labels,
        },
    ));
    if !math_diagnostics(&tree).is_empty() {
        return None;
    }
    if has_dangling_script(&tree) {
        return None;
    }
    if has_nested_comment(&tree) && !can_lower_nested_comments(&tree, opts) {
        return None;
    }
    Some(render::render(&tree, opts))
}

/// Whether a comment occurs inside a construct that historically rendered on
/// one line. Such comments require either typed hard-line lowering or verbatim
/// fallback; collapsing their terminating newline would absorb the remainder
/// of the construct into the comment.
fn has_nested_comment(tree: &SyntaxNode) -> bool {
    tree.descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| token.kind() == SyntaxKind::MATH_COMMENT)
        .any(|token| {
            token.parent_ancestors().any(|node| {
                !matches!(
                    node.kind(),
                    SyntaxKind::MATH_CONTENT | SyntaxKind::MATH_ENVIRONMENT
                )
            })
        })
}

fn can_lower_nested_comments(tree: &SyntaxNode, opts: &MathFormatOptions) -> bool {
    if opts.context == MathContext::EnvironmentBody {
        return render::can_render_environment_comments(tree, &opts.signature_scope);
    }

    let context_is_supported = opts.context != MathContext::Display
        || !tree
            .descendants_with_tokens()
            .any(|element| element.kind() == SyntaxKind::MATH_ENVIRONMENT);
    context_is_supported
        && MathContent::cast(tree.clone())
            .and_then(|content| lower::try_lower_content(&content, &opts.signature_scope))
            .is_some()
}

/// A script marker with no argument (`x^` at the end of the content, or with
/// the argument cut off by a comment, blank line, or structural boundary)
/// cannot be reflowed safely: collapsing the boundary trivia would let the
/// next atom re-attach as the argument on the following pass, changing both
/// the parse and the bytes. Such content is malformed TeX anyway, so it takes
/// the verbatim path like other malformed math.
fn has_dangling_script(tree: &SyntaxNode) -> bool {
    tree.descendants().any(|node| {
        MathSubscript::cast(node.clone()).is_some_and(|script| script.argument().is_none())
            || MathSuperscript::cast(node).is_some_and(|script| script.argument().is_none())
    })
}

fn has_unescaped_single_dollar(content: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(context: MathContext) -> MathFormatOptions {
        MathFormatOptions {
            enabled: true,
            math_indent: 0,
            line_width: 80,
            bookdown_equation_labels: false,
            context,
            signature_scope: SignatureScope::default(),
        }
    }

    fn fmt(input: &str, context: MathContext) -> String {
        fmt_with(input, &opts(context))
    }

    /// Reflow with explicit options; unwraps because these cases are well-formed
    /// (`format_math` only returns `None` for gate-off / lone-`$` / malformed).
    fn fmt_with(input: &str, o: &MathFormatOptions) -> String {
        format_math(input, o).expect("expected reflowable math")
    }

    /// Every well-formed case must be a fixed point of itself.
    fn assert_idempotent(input: &str, context: MathContext) {
        let once = fmt(input, context);
        let twice = fmt(&once, context);
        assert_eq!(once, twice, "not idempotent for {input:?}");
    }

    #[test]
    fn gate_off_returns_none() {
        let off = MathFormatOptions {
            enabled: false,
            ..opts(MathContext::Display)
        };
        let input = "\\begin{aligned}\nx&=1\\\\\ny &= 22\n\\end{aligned}";
        assert_eq!(format_math(input, &off), None);
    }

    #[test]
    fn inline_collapses_whitespace() {
        assert_eq!(fmt("a   +   b", MathContext::Inline), "a + b");
        assert_eq!(fmt("  a + b  ", MathContext::Inline), "a + b");
        assert_idempotent("a   +   b", MathContext::Inline);
    }

    #[test]
    fn inline_preserves_command_terminating_space() {
        assert_eq!(fmt("\\alpha   x", MathContext::Inline), "\\alpha x");
        assert_idempotent("\\alpha x", MathContext::Inline);
    }

    #[test]
    fn inline_keeps_optional_arguments_tight() {
        assert_eq!(fmt(r"\sqrt[3]{x}", MathContext::Inline), r"\sqrt[3]{x}");
        assert_eq!(
            fmt(r"\inferrule*[right]{A}{B}", MathContext::Inline),
            r"\inferrule*[right]{A}{B}"
        );
    }

    #[test]
    fn inline_formats_edge_comments_in_math_command_arguments() {
        for (input, expected) in [
            (
                "\\frac{% numerator\n a+b}{c}",
                "\\frac{% numerator\n a + b}{c}",
            ),
            (
                "\\frac{a+b % numerator\n}{c}",
                "\\frac{a + b % numerator\n }{c}",
            ),
        ] {
            assert_eq!(fmt(input, MathContext::Inline), expected);
            assert_idempotent(input, MathContext::Inline);
        }
    }

    #[test]
    fn malformed_math_bails() {
        let input = "\\frac{1}{2";
        assert_eq!(format_math(input, &opts(MathContext::Inline)), None);
        assert_eq!(format_math("\\left( x", &opts(MathContext::Display)), None);
    }

    #[test]
    fn lone_dollar_bails() {
        let input = "a $ b";
        assert_eq!(format_math(input, &opts(MathContext::Inline)), None);
    }

    #[test]
    fn display_aligns_environment() {
        let input = "\\begin{aligned}\nx &= 1\\\\\ny &= 22\n\\end{aligned}";
        let expected = "\\begin{aligned}\n  x & = 1  \\\\\n  y & = 22\n\\end{aligned}";
        assert_eq!(fmt(input, MathContext::Display), expected);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn display_hangs_embedded_environment_under_its_start() {
        let input = "f(x=\\begin{bmatrix}1 \\\\ 2\\end{bmatrix}, y)";
        let expected =
            "f(\n  x = \\begin{bmatrix}\n        1 \\\\\n        2\n      \\end{bmatrix},\n  y\n)";
        assert_eq!(fmt(input, MathContext::Display), expected);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn display_spaces_binary_operator_after_embedded_environment() {
        let input = "f(\\begin{bmatrix}1 \\\\ 2\\end{bmatrix}+z)";
        let expected = "f(\n  \\begin{bmatrix}\n    1 \\\\\n    2\n  \\end{bmatrix} + z\n)";
        assert_eq!(fmt(input, MathContext::Display), expected);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn display_hangs_top_level_environment_under_its_start() {
        let input = "x = \\begin{bmatrix}1 \\\\ 2\\end{bmatrix}";
        let expected = r"x = \begin{bmatrix}
      1 \\
      2
    \end{bmatrix}";
        assert_eq!(fmt(input, MathContext::Display), expected);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn top_level_environment_trims_boundary_newlines() {
        let input = "\n  x = \\begin{bmatrix}1 \\\\ 2\\end{bmatrix}\n";
        let expected = r"x = \begin{bmatrix}
      1 \\
      2
    \end{bmatrix}";
        assert_eq!(fmt(input, MathContext::Display), expected);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn comment_before_embedded_environment_stays_verbatim() {
        let input = "f(% reason\n\\begin{bmatrix}1 \\\\ 2\\end{bmatrix})";
        assert_eq!(fmt(input, MathContext::Display), input);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn mismatched_delimiters_around_environment_stay_verbatim() {
        let input = "f[\\begin{bmatrix}1 \\\\ 2\\end{bmatrix})";
        assert_eq!(fmt(input, MathContext::Display), input);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn trailing_punctuation_does_not_create_a_blank_segment() {
        let input = "f(\\begin{bmatrix}1 \\\\ 2\\end{bmatrix},)";
        let expected = "f(\n  \\begin{bmatrix}\n    1 \\\\\n    2\n  \\end{bmatrix},\n)";
        assert_eq!(fmt(input, MathContext::Display), expected);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn nested_comment_before_environment_stays_verbatim() {
        // A group-nested comment forces the `None` fallback (the caller emits
        // the content verbatim): reflowing would absorb the group remainder
        // into the comment.
        let input = "f({% reason\nx}+\\begin{bmatrix}1 \\\\ 2\\end{bmatrix})";
        assert_eq!(format_math(input, &opts(MathContext::Display)), None);
    }

    #[test]
    fn nested_environment_shape_stays_verbatim() {
        let input = "f({  \\begin{bmatrix}1 \\\\ 2\\end{bmatrix}  })";
        assert_eq!(fmt(input, MathContext::Display), input);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn unary_operator_before_environment_stays_tight() {
        let input = "f(-\\begin{bmatrix}1 \\\\ 2\\end{bmatrix})";
        let output = fmt(input, MathContext::Display);
        assert!(output.contains("-\\begin{bmatrix}"), "got: {output}");
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn command_operator_before_environment_is_spaced() {
        let input = "f(x\\cdot\\begin{bmatrix}1 \\\\ 2\\end{bmatrix})";
        let output = fmt(input, MathContext::Display);
        assert!(
            output.contains("x \\cdot \\begin{bmatrix}"),
            "got: {output}"
        );
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn comment_before_delimited_environment_stays_verbatim() {
        let input = "% reason\nf(\\begin{bmatrix}1 \\\\ 2\\end{bmatrix})";
        assert_eq!(fmt(input, MathContext::Display), input);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn comment_inside_embedded_environment_stays_verbatim() {
        // The comment sits inside a group within the environment row, so the
        // `None` fallback applies (the caller emits the content verbatim).
        let input = "f(\\begin{bmatrix}{% reason\nx} \\\\ 2\\end{bmatrix})";
        assert_eq!(format_math(input, &opts(MathContext::Display)), None);
    }

    #[test]
    fn authored_space_after_environment_before_group_is_preserved() {
        let input = "f(\\begin{bmatrix}1 \\\\ 2\\end{bmatrix} {x})";
        let output = fmt(input, MathContext::Display);
        assert!(output.contains("\\end{bmatrix} {x}"), "got: {output}");
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn authored_space_after_environment_before_command_is_preserved() {
        let input = "f(\\begin{bmatrix}1 \\\\ 2\\end{bmatrix} \\alpha)";
        let output = fmt(input, MathContext::Display);
        assert!(output.contains("\\end{bmatrix} \\alpha"), "got: {output}");
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn indirect_and_direct_environments_in_one_segment_stay_verbatim() {
        let input = "f({\\begin{matrix}a\\end{matrix}}+\\begin{matrix}b\\end{matrix})";
        assert_eq!(fmt(input, MathContext::Display), input);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn cell_operator_spacing_applied() {
        let input = "\\begin{aligned}\nx&=1\\\\\ny &= 22\n\\end{aligned}";
        let expected = "\\begin{aligned}\n  x & = 1  \\\\\n  y & = 22\n\\end{aligned}";
        assert_eq!(fmt(input, MathContext::Display), expected);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn inline_spaces_binary_and_relation_operators() {
        assert_eq!(fmt("a+b", MathContext::Inline), "a + b");
        assert_eq!(fmt("a*b", MathContext::Inline), "a * b");
        assert_eq!(fmt("a=b", MathContext::Inline), "a = b");
        assert_eq!(fmt("a<=b", MathContext::Inline), "a <= b");
        assert_eq!(fmt("a==b", MathContext::Inline), "a == b");
        assert_eq!(fmt("x=-y", MathContext::Inline), "x = -y");
        assert_eq!(
            fmt("\\alpha+\\beta", MathContext::Inline),
            "\\alpha + \\beta"
        );
        assert_eq!(fmt("{a+b}", MathContext::Inline), "{a + b}");
        assert_eq!(fmt("n^2-1", MathContext::Inline), "n^2 - 1");
        for case in ["a+b", "a<=b", "x=-y", "\\alpha+\\beta", "{a+b}", "n^2-1"] {
            assert_idempotent(case, MathContext::Inline);
        }
    }

    #[test]
    fn inline_definition_colon_spacing_respects_the_migration_slice() {
        assert_eq!(fmt("x:=y", MathContext::Inline), "x := y");
        assert_eq!(fmt("x := y", MathContext::Inline), "x := y");
        assert_eq!(fmt("\\mu:=\\nu", MathContext::Inline), "\\mu := \\nu");
        assert_eq!(fmt("x:=-y", MathContext::Inline), "x := -y");
        assert_eq!(fmt("x:y", MathContext::Inline), "x:y");
        assert_eq!(fmt("f: A", MathContext::Inline), "f: A");
        assert_eq!(fmt("x : = y", MathContext::Inline), "x : = y");
        for case in [
            "x:=y",
            "x := y",
            "\\mu:=\\nu",
            "x:=-y",
            "x:y",
            "f: A",
            "x : = y",
        ] {
            assert_idempotent(case, MathContext::Inline);
        }
    }

    #[test]
    fn inline_scripts_preserve_their_base_atom_class() {
        assert_eq!(fmt("a=_ib", MathContext::Inline), "a =_i b");
        assert_eq!(fmt(r"a\gets_ib", MathContext::Inline), r"a \gets_i b");
        assert_eq!(fmt("a:=_ib", MathContext::Inline), "a :=_i b");
        for case in ["a=_ib", r"a\gets_ib", "a:=_ib"] {
            assert_idempotent(case, MathContext::Inline);
        }
    }

    #[test]
    fn colon_runs_in_definition_relations_stay_fused() {
        assert_eq!(fmt("a::=_ib", MathContext::Inline), "a ::=_i b");
        assert_eq!(fmt("a::=b", MathContext::Inline), "a ::= b");
        for case in ["a::=_ib", "a::=b"] {
            assert_idempotent(case, MathContext::Inline);
        }
    }

    #[test]
    fn scripted_composite_relations_stay_fused() {
        assert_eq!(fmt("x<=_iy", MathContext::Inline), "x <=_i y");
        assert_eq!(fmt("x>=_iy", MathContext::Inline), "x >=_i y");
        assert_eq!(fmt("a==_kb", MathContext::Inline), "a ==_k b");
        for case in ["x<=_iy", "x>=_iy", "a==_kb"] {
            assert_idempotent(case, MathContext::Inline);
        }
    }

    #[test]
    fn text_mode_script_argument_keeps_its_group_spaces() {
        assert_eq!(
            fmt(r"x_\text{ max }", MathContext::Inline),
            r"x_\text{ max }"
        );
        assert_eq!(
            fmt(r"x^\mbox{ a b }", MathContext::Inline),
            r"x^\mbox{ a b }"
        );
        assert_eq!(fmt(r"\text{ a }^2", MathContext::Inline), r"\text{ a }^2");
        for case in [r"x_\text{ max }", r"x^\mbox{ a b }", r"\text{ a }^2"] {
            assert_idempotent(case, MathContext::Inline);
        }
    }

    #[test]
    fn dangling_script_marker_bails_to_verbatim() {
        assert_eq!(format_math("x^", &opts(MathContext::Inline)), None);
        assert_eq!(format_math("x^\n\n2", &opts(MathContext::Display)), None);
        assert_eq!(
            format_math(
                "\\begin{align}\nx^\n\n2\n\\end{align}",
                &opts(MathContext::Display)
            ),
            None
        );
    }

    #[test]
    fn overwidth_row_break_keeps_equation_labels_intact() {
        let o = MathFormatOptions {
            line_width: 30,
            bookdown_equation_labels: true,
            ..opts(MathContext::Display)
        };
        let input = "yyyy = aaaaaaaaaa + bbbbbbbbbb + cccccccccc (\\#eq:my-label)";
        let once = fmt_with(input, &o);
        assert!(once.contains("(\\#eq:my-label)"), "got: {once}");
        assert_eq!(fmt_with(&once, &o), once);
    }

    #[test]
    fn scripted_environment_keeps_environment_layout() {
        let input = "\\begin{pmatrix}a \\\\ b\\end{pmatrix}^T";
        let expected = "\\begin{pmatrix}\n  a \\\\\n  b\n\\end{pmatrix}^T";
        assert_eq!(fmt(input, MathContext::Display), expected);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn scripted_closing_delimiter_keeps_delimited_layout() {
        let input = "(\\begin{bmatrix}1 \\\\ 2\\end{bmatrix})^2";
        let expected = "(\n  \\begin{bmatrix}\n    1 \\\\\n    2\n  \\end{bmatrix}\n)^2";
        assert_eq!(fmt(input, MathContext::Display), expected);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn scripted_relation_after_environment_is_spaced() {
        let input = "\\begin{pmatrix}a \\\\ b\\end{pmatrix}=_ic";
        let once = fmt(input, MathContext::Display);
        assert!(once.contains("\\end{pmatrix} =_i c"), "got: {once}");
        assert_idempotent(input, MathContext::Display);
    }

    /// A `*` separated from its command by a script is an operator atom, not a
    /// modifier, so it stays where it was written rather than migrating back
    /// onto the command. It prints tight because `\operatorname` is a large
    /// operator, and TeX coerces a binary atom after one into a unary sign.
    #[test]
    fn star_modifier_does_not_cross_a_script() {
        assert_eq!(
            fmt(r"\operatorname*_i{x}", MathContext::Inline),
            r"\operatorname*_i{x}"
        );
        assert_eq!(
            fmt(r"\operatorname_i*{x}", MathContext::Inline),
            r"\operatorname_i*{x}"
        );
    }

    #[test]
    fn inline_keeps_unary_operators_tight() {
        assert_eq!(fmt("-x", MathContext::Inline), "-x");
        assert_eq!(fmt("- x", MathContext::Inline), "-x");
        assert_eq!(fmt("f(-x)", MathContext::Inline), "f(-x)");
        assert_eq!(fmt("f( - x)", MathContext::Inline), "f(-x)");
        assert_eq!(fmt("x = - y", MathContext::Inline), "x = -y");
        assert_eq!(fmt("e^{- t}", MathContext::Inline), "e^{-t}");
        assert_eq!(fmt("a - -b", MathContext::Inline), "a - -b");
        assert_eq!(fmt("a--b", MathContext::Inline), "a - -b");
        for case in [
            "-x", "- x", "f(-x)", "f( - x)", "x = - y", "e^{- t}", "a - -b", "a--b",
        ] {
            assert_idempotent(case, MathContext::Inline);
        }
    }

    #[test]
    fn inline_spaces_command_operators() {
        assert_eq!(fmt("a\\cdot b", MathContext::Inline), "a \\cdot b");
        assert_eq!(fmt("a\\leq b", MathContext::Inline), "a \\leq b");
        assert_eq!(fmt("x\\leq y", MathContext::Inline), "x \\leq y");
        assert_eq!(fmt("a \\cdot b", MathContext::Inline), "a \\cdot b");
        assert_eq!(
            fmt("\\alpha\\cdot\\beta", MathContext::Inline),
            "\\alpha \\cdot \\beta"
        );
        assert_eq!(fmt("\\sum x", MathContext::Inline), "\\sum x");
        assert_eq!(
            fmt("\\operatorname*{minimize} a", MathContext::Inline),
            "\\operatorname*{minimize} a"
        );
        assert_eq!(
            fmt("\\operatorname * {minimize} a", MathContext::Inline),
            "\\operatorname*{minimize} a"
        );
        assert_eq!(fmt("\\alpha*x", MathContext::Inline), "\\alpha * x");
        assert_eq!(fmt("\\alpha x", MathContext::Inline), "\\alpha x");
        assert_eq!(
            fmt("\\left( x \\right)", MathContext::Inline),
            "\\left( x \\right)"
        );
        for case in [
            "a\\cdot b",
            "a\\leq b",
            "\\alpha\\cdot\\beta",
            "\\sum x",
            "\\operatorname*{minimize} a",
            "\\operatorname * {minimize} a",
            "\\alpha*x",
            "\\alpha x",
            "\\left( x \\right)",
        ] {
            assert_idempotent(case, MathContext::Inline);
        }
    }

    #[test]
    fn environment_body_context_aligns_bare_body() {
        let input = "\nx &= 1 \\\\\ny &= 22\n";
        let expected = "  x & = 1  \\\\\n  y & = 22";
        assert_eq!(fmt(input, MathContext::EnvironmentBody), expected);
        assert_idempotent(
            &fmt(input, MathContext::EnvironmentBody),
            MathContext::EnvironmentBody,
        );
    }

    #[test]
    fn line_break_modifiers_stay_attached() {
        let input = "a \\\\*[2ex]\nb";
        assert_eq!(fmt(input, MathContext::Display), input);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn trailing_line_breaks_align() {
        let input = "\\begin{aligned}\nx &= 1 \\\\\ny &= 22 \\\\\nz &= 333\n\\end{aligned}";
        let expected =
            "\\begin{aligned}\n  x & = 1   \\\\\n  y & = 22  \\\\\n  z & = 333\n\\end{aligned}";
        assert_eq!(fmt(input, MathContext::Display), expected);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn ragged_columns_align_per_column() {
        let input = "\\begin{aligned}\na &= 1 \\\\\nb &= c &= d\n\\end{aligned}";
        let expected = "\\begin{aligned}\n  a & = 1 \\\\\n  b & = c & = d\n\\end{aligned}";
        assert_eq!(fmt(input, MathContext::Display), expected);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn nested_environment_indents_one_more_level() {
        let input = "\\begin{aligned}\nx &= \\begin{cases} a \\\\ b \\end{cases}\n\\end{aligned}";
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn ampersand_inside_group_is_not_a_column() {
        let input = "\\begin{aligned}\nx &= \\text{a & b} \\\\\ny &= 2\n\\end{aligned}";
        let once = fmt(input, MathContext::Display);
        assert!(once.contains("\\text{a & b}"), "got: {once}");
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn free_display_content_keeps_lines() {
        let input = "E = mc^2";
        assert_eq!(fmt(input, MathContext::Display), "E = mc^2");
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn display_breaks_overwidth_relation_chain() {
        let narrow = MathFormatOptions {
            line_width: 20,
            ..opts(MathContext::Display)
        };
        let input = "A = bbbbbbbbbb = cccccccccc";
        let expected = "A = bbbbbbbbbb\n  = cccccccccc";
        assert_eq!(fmt_with(input, &narrow), expected);
        let once = fmt_with(input, &narrow);
        assert_eq!(fmt_with(&once, &narrow), once);
    }

    #[test]
    fn display_leaves_fitting_chain_on_one_line() {
        let wide = opts(MathContext::Display); // line_width 80
        assert_eq!(
            fmt_with("A = bbbbbbbbbb = cccccccccc", &wide),
            "A = bbbbbbbbbb = cccccccccc"
        );
    }

    #[test]
    fn display_nests_binary_operators_under_relations() {
        let narrow = MathFormatOptions {
            line_width: 20,
            ..opts(MathContext::Display)
        };
        let input = "A = aaaaaaaaaa + bbbbbbbbbb = cccccccccc + dddddddddd";
        let expected = "A = aaaaaaaaaa\n    + bbbbbbbbbb\n  = cccccccccc\n    + dddddddddd";
        assert_eq!(fmt_with(input, &narrow), expected);
        let once = fmt_with(input, &narrow);
        assert_eq!(fmt_with(&once, &narrow), once);
    }

    #[test]
    fn display_binary_on_relation_lhs_hangs_flush() {
        let narrow = MathFormatOptions {
            line_width: 20,
            ..opts(MathContext::Display)
        };
        let input = "H(a) - H(b) \\leq \\frac{L}{2n}";
        let expected = "H(a)\n- H(b) \\leq \\frac{L}{2n}";
        assert_eq!(fmt_with(input, &narrow), expected);
        let once = fmt_with(input, &narrow);
        assert_eq!(fmt_with(&once, &narrow), once);
    }

    #[test]
    fn display_comment_terminating_newline_is_not_joined() {
        let wide = opts(MathContext::Display);
        let input = "% leading comment\nx = 1";
        assert_eq!(fmt_with(input, &wide), "% leading comment\nx = 1");
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn display_does_not_break_inside_delimiters_or_groups() {
        let narrow = MathFormatOptions {
            line_width: 12,
            ..opts(MathContext::Display)
        };
        let frac = "\\frac{aaaaaaaa}{bbbbbbbb}";
        assert_eq!(fmt_with(frac, &narrow), frac);
        let paren = "\\left( xxxx = yyyy + wwww \\right)";
        let once = fmt_with(paren, &narrow);
        assert!(!once.contains('\n'), "should not break: {once:?}");
        assert_eq!(fmt_with(&once, &narrow), once);
    }
}
