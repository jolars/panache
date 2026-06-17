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
//! neutral `MATH_OPERATOR` tokens and the class/precedence logic lives in
//! [`operators`] (the math analog of YAML scalar cooking), keyed on operator
//! text + command name. Still out of scope: `\frac` canonicalization, auto-`&`
//! insertion, and macro rewriting.
//!
//! The gate is [`crate::config::Config::experimental_format_math`]. Off (the
//! default) callers emit math verbatim and never reach this module; on, they
//! route content through [`format_math`].

use panache_parser::parser::math::{MathParseOptions, parse_math_report};
use panache_parser::syntax::SyntaxNode;

mod linebreak;
pub mod operators;
mod render;

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
#[derive(Debug, Clone, Copy)]
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
        }
    }
}

/// Format clean math content (delimiters excluded, both in and out).
///
/// Returns the input unchanged — never panicking, never erroring — on any bail
/// condition: the gate is off, the content has an unescaped lone `$` (a
/// preservation guard against cross-pass drift), or the structural parse
/// reports a diagnostic (malformed math is never reflowed). Otherwise it
/// re-emits the content per `STYLE.md`.
pub fn format_math(input: &str, opts: &MathFormatOptions) -> String {
    if !opts.enabled {
        return input.to_string();
    }
    // Lone unescaped `$` inside content confuses delimiter handling downstream;
    // mirror the existing `has_unescaped_single_dollar_in_content` guard.
    if has_unescaped_single_dollar(input) {
        return input.to_string();
    }
    let report = parse_math_report(
        input,
        MathParseOptions {
            bookdown_equation_labels: opts.bookdown_equation_labels,
        },
    );
    // Malformed math (unclosed/mismatched braces or environments) has an
    // untrustworthy row/column structure — leave it exactly as written.
    if !report.diagnostics.is_empty() {
        return input.to_string();
    }
    let tree = SyntaxNode::new_root(report.green);
    render::render(&tree, opts)
}

/// String-level twin of `DisplayMath::has_unescaped_single_dollar_in_content`
/// (`crates/panache-parser/src/syntax/math.rs`): a `$` not preceded by an odd
/// run of backslashes and not part of a `$$` pair.
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
        }
    }

    fn fmt(input: &str, context: MathContext) -> String {
        format_math(input, &opts(context))
    }

    /// Every well-formed case must be a fixed point of itself.
    fn assert_idempotent(input: &str, context: MathContext) {
        let once = fmt(input, context);
        let twice = fmt(&once, context);
        assert_eq!(once, twice, "not idempotent for {input:?}");
    }

    #[test]
    fn gate_off_is_verbatim() {
        let off = MathFormatOptions {
            enabled: false,
            ..opts(MathContext::Display)
        };
        let input = "\\begin{aligned}\nx&=1\\\\\ny &= 22\n\\end{aligned}";
        assert_eq!(format_math(input, &off), input);
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
    fn malformed_math_is_verbatim() {
        // Unclosed group → diagnostic → bail.
        let input = "\\frac{1}{2";
        assert_eq!(fmt(input, MathContext::Inline), input);
    }

    #[test]
    fn lone_dollar_is_verbatim() {
        let input = "a $ b";
        assert_eq!(fmt(input, MathContext::Inline), input);
    }

    #[test]
    fn display_aligns_environment() {
        let input = "\\begin{aligned}\nx &= 1\\\\\ny &= 22\n\\end{aligned}";
        // The trailing `\\` aligns: the last column pads to its widest cell.
        let expected = "\\begin{aligned}\n  x & = 1  \\\\\n  y & = 22\n\\end{aligned}";
        assert_eq!(fmt(input, MathContext::Display), expected);
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn cell_operator_spacing_applied() {
        // Operator spacing runs inside each cell: a tight `&=1` is normalized to
        // `= 1`, so it lines up with an already-spaced `&= 22`.
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
        // Adjacent relation chars stay one spaced unit.
        assert_eq!(fmt("a<=b", MathContext::Inline), "a <= b");
        assert_eq!(fmt("a==b", MathContext::Inline), "a == b");
        // A relation followed by a sign splits: the sign is unary.
        assert_eq!(fmt("x=-y", MathContext::Inline), "x = -y");
        // Commands are ordinary operands → the operator between them is binary.
        assert_eq!(
            fmt("\\alpha+\\beta", MathContext::Inline),
            "\\alpha + \\beta"
        );
        // Operators inside groups are spaced too.
        assert_eq!(fmt("{a+b}", MathContext::Inline), "{a + b}");
        // A superscripted operand is ordinary, so the trailing `-` is binary.
        assert_eq!(fmt("n^2-1", MathContext::Inline), "n^2 - 1");
        for case in ["a+b", "a<=b", "x=-y", "\\alpha+\\beta", "{a+b}", "n^2-1"] {
            assert_idempotent(case, MathContext::Inline);
        }
    }

    #[test]
    fn inline_keeps_unary_operators_tight() {
        // Leading unary minus, and one written with a space, both canonicalize tight.
        assert_eq!(fmt("-x", MathContext::Inline), "-x");
        assert_eq!(fmt("- x", MathContext::Inline), "-x");
        // After an opening delimiter (lumped into the text run) the minus is unary.
        assert_eq!(fmt("f(-x)", MathContext::Inline), "f(-x)");
        assert_eq!(fmt("f( - x)", MathContext::Inline), "f(-x)");
        // After a relation the minus is unary, but the relation keeps its space.
        assert_eq!(fmt("x = - y", MathContext::Inline), "x = -y");
        // Inside a script group, after `{` the minus is unary.
        assert_eq!(fmt("e^{- t}", MathContext::Inline), "e^{-t}");
        // Two minuses (adjacent or spaced) are binary-then-unary: `a - -b`.
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
        // Binary/relation command operators get one space on each side.
        assert_eq!(fmt("a\\cdot b", MathContext::Inline), "a \\cdot b");
        assert_eq!(fmt("a\\leq b", MathContext::Inline), "a \\leq b");
        assert_eq!(fmt("x\\leq y", MathContext::Inline), "x \\leq y");
        // Already-spaced input is a fixed point.
        assert_eq!(fmt("a \\cdot b", MathContext::Inline), "a \\cdot b");
        // No author space (a `\` terminates the prior control word) still spaces.
        assert_eq!(
            fmt("\\alpha\\cdot\\beta", MathContext::Inline),
            "\\alpha \\cdot \\beta"
        );
        // Large operators (Op) are not binary-spaced; ordinary commands keep
        // their terminating space verbatim.
        assert_eq!(fmt("\\sum x", MathContext::Inline), "\\sum x");
        assert_eq!(fmt("\\alpha x", MathContext::Inline), "\\alpha x");
        // Delimiter commands (Open/Close) are not spaced.
        assert_eq!(
            fmt("\\left( x \\right)", MathContext::Inline),
            "\\left( x \\right)"
        );
        for case in [
            "a\\cdot b",
            "a\\leq b",
            "\\alpha\\cdot\\beta",
            "\\sum x",
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
    fn trailing_line_breaks_align() {
        // The last column pads so the `\\` line up; the final (no-`\\`) row's
        // last cell is not padded (no trailing whitespace).
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
        // The nested env's `&`/`\\` are not top-level, so the outer row is not
        // split on them; the cases env renders inline within the cell.
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn ampersand_inside_group_is_not_a_column() {
        let input = "\\begin{aligned}\nx &= \\text{a & b} \\\\\ny &= 2\n\\end{aligned}";
        let once = fmt(input, MathContext::Display);
        // Two columns only: the `&` inside `\text{...}` stays put.
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
        // First `=` stays; the second starts a continuation aligned under it
        // (equality chain ⇒ relations stack under the first relation).
        let expected = "A = bbbbbbbbbb\n  = cccccccccc";
        assert_eq!(format_math(input, &narrow), expected);
        // Re-feeding the broken (multi-line) form recomputes the same layout.
        let once = format_math(input, &narrow);
        assert_eq!(format_math(&once, &narrow), once);
    }

    #[test]
    fn display_leaves_fitting_chain_on_one_line() {
        // The same content under a generous width is untouched (byte-identical
        // to the pre-line-breaking behavior).
        let wide = opts(MathContext::Display); // line_width 80
        assert_eq!(
            format_math("A = bbbbbbbbbb = cccccccccc", &wide),
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
        // Relations break first; each over-width segment nests its `+` term one
        // indent level deeper, under the relation's right-hand side.
        let expected = "A = aaaaaaaaaa\n    + bbbbbbbbbb\n  = cccccccccc\n    + dddddddddd";
        assert_eq!(format_math(input, &narrow), expected);
        let once = format_math(input, &narrow);
        assert_eq!(format_math(&once, &narrow), once);
    }

    #[test]
    fn display_comment_terminating_newline_is_not_joined() {
        // A `%` comment runs to EOL; the soft newline ending it must remain a row
        // boundary, or the next line is absorbed into the comment (and lost from
        // the rendered math). Regression for the logical-row re-join.
        let wide = opts(MathContext::Display);
        let input = "% leading comment\nx = 1";
        assert_eq!(format_math(input, &wide), "% leading comment\nx = 1");
        assert_idempotent(input, MathContext::Display);
    }

    #[test]
    fn display_does_not_break_inside_delimiters_or_groups() {
        let narrow = MathFormatOptions {
            line_width: 12,
            ..opts(MathContext::Display)
        };
        // A single over-width fraction with no top-level operator stays one line.
        let frac = "\\frac{aaaaaaaa}{bbbbbbbb}";
        assert_eq!(format_math(frac, &narrow), frac);
        // Relation *and* binary operators buried inside `\left(…\right)` are not
        // depth-0 break points, so this over-width row has no break candidate and
        // stays on one line (the broader binary-breaking scope must still respect
        // delimiter opacity).
        let paren = "\\left( xxxx = yyyy + wwww \\right)";
        let once = format_math(paren, &narrow);
        assert!(!once.contains('\n'), "should not break: {once:?}");
        assert_eq!(format_math(&once, &narrow), once);
    }
}
