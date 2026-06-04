//! In-tree TeX math **content** formatter (experimental, opt-in).
//!
//! Consumes the lossless structural math CST built by
//! [`panache_parser::parser::math`] and re-emits the content with structurally
//! safe normalizations: inline whitespace collapse, environment-body
//! indentation, `\\` line-break normalization, and `&`-column alignment. The
//! canonical rules live in `STYLE.md` (next to this file).
//!
//! Like the YAML formatter, this **re-parses the clean content string** rather
//! than walking the host-embedded subtree. The host block machinery interleaves
//! container prefixes (a blockquote `>` and its whitespace) into `MATH_CONTENT`
//! on continuation lines; re-parsing the already-prefix-stripped string (from
//! [`panache_parser::syntax::math::math_content_text`]) sidesteps that entirely.
//!
//! **Scope is structural only** — no operator spacing, no `\frac`
//! canonicalization, no auto-`&` insertion. Operators are deliberately not
//! tokenized; alignment keys off the `&` token and treats cell contents as
//! opaque text measured by source-character width.
//!
//! The gate is [`crate::config::Config::experimental_format_math`]. Off (the
//! default) callers emit math verbatim and never reach this module; on, they
//! route content through [`format_math`].

use panache_parser::parser::math::{MathParseOptions, parse_math_report};
use panache_parser::syntax::SyntaxNode;

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
    fn cell_internal_spacing_is_preserved() {
        // The formatter aligns at `&` but never touches operator spacing inside
        // a cell: `&=1` stays `=1`, `&= 22` stays `= 22`.
        let input = "\\begin{aligned}\nx&=1\\\\\ny &= 22\n\\end{aligned}";
        let expected = "\\begin{aligned}\n  x & =1   \\\\\n  y & = 22\n\\end{aligned}";
        assert_eq!(fmt(input, MathContext::Display), expected);
        assert_idempotent(input, MathContext::Display);
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
}
