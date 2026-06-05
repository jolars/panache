//! In-tree TeX math content parser.
//!
//! Produces a lossless structural CST for the *content* between math
//! delimiters (the delimiters themselves are owned by the host `INLINE_MATH` /
//! `DISPLAY_MATH` nodes, see `parser/inlines/math.rs`). The returned subtree is
//! rooted at [`SyntaxKind::MATH_CONTENT`] and is spliced directly into the host
//! document tree, replacing the opaque content `TEXT` token.
//!
//! This is a *syntactic* parse, not a semantic one: TeX is a Turing-complete
//! macro language, so we only capture structure that a formatter can safely act
//! on — brace groups, `\begin`/`\end` environments, control sequences,
//! alignment tabs (`&`), line breaks (`\\`), sub/superscript markers, comments,
//! and whitespace. Everything else is an ordinary-atom run ([`MATH_TEXT`]).
//!
//! Two outputs, two channels — the same split YAML uses (see
//! `parser/yaml/model.rs`) and that texlab uses for LaTeX:
//!
//! - the **CST is lossless and never fails** (`node.text() == content` for every
//!   input; worst case is a single `MATH_TEXT` atom), and
//! - **errors ride a side-channel** ([`MathParseReport::diagnostics`]) so the
//!   linter (and by proxy the LSP) can surface unbalanced braces and mismatched
//!   environments without the parser ever rejecting input.
//!
//! [`MATH_TEXT`]: SyntaxKind::MATH_TEXT

use crate::parser::inlines::bookdown::try_parse_bookdown_equation_definition;
use crate::syntax::SyntaxKind;
use rowan::{GreenNode, GreenNodeBuilder};

/// A non-fatal problem found while parsing math content. Byte offsets are
/// relative to the math content string (the caller offsets them into host
/// document coordinates when surfacing through the linter/LSP).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathDiagnostic {
    pub code: &'static str,
    pub message: &'static str,
    pub byte_start: usize,
    pub byte_end: usize,
}

/// The lossless CST plus any diagnostics gathered on the side-channel.
#[derive(Debug, Clone)]
pub struct MathParseReport {
    pub green: GreenNode,
    pub diagnostics: Vec<MathDiagnostic>,
}

/// Stable diagnostic codes for math content. Mirrors `yaml::diagnostic_codes`.
pub mod diagnostic_codes {
    /// A `{` was never closed before the end of the math content.
    pub const UNCLOSED_GROUP: &str = "MATH_UNCLOSED_GROUP";
    /// A `}` appeared with no matching `{`.
    pub const UNEXPECTED_CLOSE_BRACE: &str = "MATH_UNEXPECTED_CLOSE_BRACE";
    /// A `\begin{env}` was never closed by a matching `\end{env}`.
    pub const UNCLOSED_ENVIRONMENT: &str = "MATH_UNCLOSED_ENVIRONMENT";
    /// A `\begin{a}` was closed by `\end{b}` with a different name.
    pub const MISMATCHED_ENVIRONMENT: &str = "MATH_MISMATCHED_ENVIRONMENT";
    /// An `\end` appeared with no open `\begin`.
    pub const UNEXPECTED_END: &str = "MATH_UNEXPECTED_END";
}

/// Flavor-/extension-dependent parsing options for math content. Default is
/// all-off (pure TeX). The math grammar itself is flavor-agnostic; only
/// constructs layered on top of TeX by a Markdown flavor live here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MathParseOptions {
    /// Recognize bookdown equation labels `(\#eq:label)` as a single
    /// [`SyntaxKind::MATH_EQUATION_LABEL`] token (gated on the
    /// `bookdown_equation_references` extension).
    pub bookdown_equation_labels: bool,
}

/// Parse math content into a lossless `MATH_CONTENT` green node, discarding
/// diagnostics. `content` is the raw text between (but excluding) the math
/// delimiters.
pub fn parse_math_content(content: &str, opts: MathParseOptions) -> GreenNode {
    parse_math_report(content, opts).green
}

/// Parse math content into a lossless CST plus a side-channel of diagnostics.
pub fn parse_math_report(content: &str, opts: MathParseOptions) -> MathParseReport {
    let mut parser = MathParser {
        input: content,
        pos: 0,
        builder: GreenNodeBuilder::new(),
        diagnostics: Vec::new(),
        opts,
    };
    parser.builder.start_node(SyntaxKind::MATH_CONTENT.into());
    parser.parse_elements(Ctx::Top);
    parser.builder.finish_node();
    MathParseReport {
        green: parser.builder.finish(),
        diagnostics: parser.diagnostics,
    }
}

/// Parse context, controlling which delimiter ends the current element run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ctx {
    /// Top level of the math content.
    Top,
    /// Inside a `{ ... }` brace group; stops at the matching `}`.
    Group,
    /// Inside a `\begin{env} ... \end{env}` body; stops at `\end`.
    Env,
}

struct MathParser<'a> {
    input: &'a str,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    diagnostics: Vec<MathDiagnostic>,
    opts: MathParseOptions,
}

impl MathParser<'_> {
    fn rest(&self) -> &str {
        &self.input[self.pos..]
    }

    fn peek_char(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn diagnose(&mut self, code: &'static str, message: &'static str, start: usize, end: usize) {
        self.diagnostics.push(MathDiagnostic {
            code,
            message,
            byte_start: start,
            byte_end: end,
        });
    }

    /// Emit a token of `len` bytes (from the current position) with `kind`.
    fn bump_bytes(&mut self, len: usize, kind: SyntaxKind) {
        let text = &self.input[self.pos..self.pos + len];
        self.builder.token(kind.into(), text);
        self.pos += len;
    }

    /// If the cursor is at a control word (`\` followed by ASCII letters or
    /// `@`, matching TeX/texlab's control-word class), return that word
    /// (without the backslash) without consuming anything.
    fn peek_control_word(&self) -> Option<&str> {
        let after = self.rest().strip_prefix('\\')?;
        let len: usize = after
            .bytes()
            .take_while(|b| b.is_ascii_alphabetic() || *b == b'@')
            .count();
        if len == 0 { None } else { Some(&after[..len]) }
    }

    fn parse_elements(&mut self, ctx: Ctx) {
        while let Some(c) = self.peek_char() {
            match c {
                '}' if ctx == Ctx::Group => break,
                // A `}` outside any group is an unmatched close: keep it as a
                // faithful (stray) close token and flag it on the side-channel.
                '}' => {
                    self.diagnose(
                        diagnostic_codes::UNEXPECTED_CLOSE_BRACE,
                        "unmatched closing brace `}`",
                        self.pos,
                        self.pos + 1,
                    );
                    self.bump_bytes(1, SyntaxKind::MATH_GROUP_CLOSE);
                }
                '\\' => {
                    if self.rest().starts_with("\\\\") {
                        self.bump_bytes(2, SyntaxKind::MATH_LINE_BREAK);
                    } else if let Some(word) = self.peek_control_word() {
                        match word {
                            "begin" => self.parse_environment(),
                            "end" if ctx == Ctx::Env => break,
                            "end" => {
                                // Stray `\end` with no open `\begin` at this level.
                                self.diagnose(
                                    diagnostic_codes::UNEXPECTED_END,
                                    "`\\end` without a matching `\\begin`",
                                    self.pos,
                                    self.pos + 1 + word.len(),
                                );
                                self.parse_control_word();
                            }
                            _ => self.parse_control_word(),
                        }
                    } else {
                        self.parse_control_symbol();
                    }
                }
                '{' => self.parse_group(),
                // Bookdown equation label `(\#eq:label)`, only when enabled.
                // When off, `(` is not intercepted here and flows into an
                // ordinary atom run, so the CST is unchanged for plain math.
                '(' if self.opts.bookdown_equation_labels => match self.equation_label_len() {
                    Some(len) => self.bump_bytes(len, SyntaxKind::MATH_EQUATION_LABEL),
                    // A non-matching `(` is just one ordinary atom (it is a
                    // text-run boundary only while the extension is on).
                    None => self.bump_bytes(1, SyntaxKind::MATH_TEXT),
                },
                '&' => self.bump_bytes(1, SyntaxKind::MATH_ALIGN),
                '^' | '_' => self.bump_bytes(1, SyntaxKind::MATH_SCRIPT),
                // Operator atoms (`+ - * = < >`), one token per char. Class and
                // precedence are *not* assigned here: TeX itself coerces a
                // binary atom to ordinary by its neighbors (unary minus), so the
                // class is a property of list position, owned by the formatter.
                c if is_operator(c) => self.bump_bytes(1, SyntaxKind::MATH_OPERATOR),
                '%' => self.parse_comment(),
                ' ' | '\t' => self.parse_spaces(),
                '\n' => self.bump_bytes(1, SyntaxKind::MATH_NEWLINE),
                '\r' => {
                    let len = if self.rest().starts_with("\r\n") {
                        2
                    } else {
                        1
                    };
                    self.bump_bytes(len, SyntaxKind::MATH_NEWLINE);
                }
                _ => self.parse_text(),
            }
        }
    }

    /// `\begin{env} ... \end{env}`. Matching is done by recursion plus the
    /// `Env` context; name mismatches and missing `\end` are reported on the
    /// side-channel but never abort the parse.
    fn parse_environment(&mut self) {
        let begin_start = self.pos;
        self.builder.start_node(SyntaxKind::MATH_ENVIRONMENT.into());
        self.parse_control_word(); // \begin
        let begin_name = self.parse_environment_name();
        self.parse_elements(Ctx::Env);
        if self.peek_control_word() == Some("end") {
            let end_start = self.pos;
            self.parse_control_word(); // \end
            let end_name = self.parse_environment_name();
            if begin_name != end_name {
                self.diagnose(
                    diagnostic_codes::MISMATCHED_ENVIRONMENT,
                    "`\\end` name does not match the open `\\begin`",
                    end_start,
                    self.pos,
                );
            }
        } else {
            self.diagnose(
                diagnostic_codes::UNCLOSED_ENVIRONMENT,
                "`\\begin` without a matching `\\end`",
                begin_start,
                self.pos,
            );
        }
        self.builder.finish_node();
    }

    /// Parse the `{name}` group following `\begin` / `\end` (if present) and
    /// return the inner name text for matching. Empty when absent.
    fn parse_environment_name(&mut self) -> String {
        if self.peek_char() != Some('{') {
            return String::new();
        }
        let open = self.pos;
        self.parse_group();
        // Inner text = the group span minus its braces.
        self.input[open..self.pos]
            .trim_start_matches('{')
            .trim_end_matches('}')
            .to_string()
    }

    fn parse_group(&mut self) {
        let open = self.pos;
        self.builder.start_node(SyntaxKind::MATH_GROUP.into());
        self.bump_bytes(1, SyntaxKind::MATH_GROUP_OPEN); // {
        self.parse_elements(Ctx::Group);
        if self.peek_char() == Some('}') {
            self.bump_bytes(1, SyntaxKind::MATH_GROUP_CLOSE); // }
        } else {
            self.diagnose(
                diagnostic_codes::UNCLOSED_GROUP,
                "unclosed `{` group",
                open,
                open + 1,
            );
        }
        self.builder.finish_node();
    }

    /// `\` + a run of control-word characters (e.g. `\alpha`, `\frac`, `\begin`).
    fn parse_control_word(&mut self) {
        let word_len = self.peek_control_word().map(str::len).unwrap_or(0);
        self.bump_bytes(1 + word_len, SyntaxKind::MATH_COMMAND);
    }

    /// `\` + exactly one following character (e.g. `\%`, `\{`, `\,`), or a
    /// lone trailing backslash at EOF.
    fn parse_control_symbol(&mut self) {
        let after = &self.input[self.pos + 1..];
        let len = 1 + after.chars().next().map(char::len_utf8).unwrap_or(0);
        self.bump_bytes(len, SyntaxKind::MATH_COMMAND);
    }

    /// `%` to (but not including) the end of the line.
    fn parse_comment(&mut self) {
        let len = self
            .rest()
            .find(['\n', '\r'])
            .unwrap_or_else(|| self.rest().len());
        self.bump_bytes(len, SyntaxKind::MATH_COMMENT);
    }

    fn parse_spaces(&mut self) {
        let len = self
            .rest()
            .bytes()
            .take_while(|&b| b == b' ' || b == b'\t')
            .count();
        self.bump_bytes(len, SyntaxKind::MATH_SPACE);
    }

    /// A run of ordinary atoms, up to the next structural character. While the
    /// bookdown extension is on, `(` also bounds the run so every `(` reaches
    /// the dispatcher's equation-label check.
    fn parse_text(&mut self) {
        let bookdown = self.opts.bookdown_equation_labels;
        let len = self
            .rest()
            .find(|c: char| is_special(c) || (bookdown && c == '('))
            .unwrap_or_else(|| self.rest().len());
        debug_assert!(len > 0, "parse_text on a special char");
        self.bump_bytes(len, SyntaxKind::MATH_TEXT);
    }

    /// If the cursor is at a bookdown equation label `(\#eq:label)`, return its
    /// byte length. Reuses the shared bookdown definition parser so the
    /// recognized span matches the rest of the codebase exactly.
    fn equation_label_len(&self) -> Option<usize> {
        try_parse_bookdown_equation_definition(self.rest()).map(|(len, _)| len)
    }
}

/// Characters that terminate a [`SyntaxKind::MATH_TEXT`] run.
fn is_special(c: char) -> bool {
    is_operator(c)
        || matches!(
            c,
            '\\' | '{' | '}' | '&' | '^' | '_' | '%' | ' ' | '\t' | '\n' | '\r'
        )
}

/// Operator atoms split out of ordinary text into their own
/// [`SyntaxKind::MATH_OPERATOR`] token. The TeX mathbin (`+ - *`) and mathrel
/// (`= < >`) core; the formatter assigns class/precedence/spacing downstream.
fn is_operator(c: char) -> bool {
    matches!(c, '+' | '-' | '*' | '=' | '<' | '>')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::SyntaxNode;

    fn node(content: &str) -> SyntaxNode {
        SyntaxNode::new_root(parse_math_content(content, MathParseOptions::default()))
    }

    fn node_with(content: &str, opts: MathParseOptions) -> SyntaxNode {
        SyntaxNode::new_root(parse_math_content(content, opts))
    }

    fn token_kinds(content: &str) -> Vec<SyntaxKind> {
        node(content)
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .map(|tok| tok.kind())
            .collect()
    }

    fn codes(content: &str) -> Vec<&'static str> {
        parse_math_report(content, MathParseOptions::default())
            .diagnostics
            .into_iter()
            .map(|d| d.code)
            .collect()
    }

    /// Losslessness is the hard invariant for every input.
    fn assert_lossless(content: &str) {
        assert_eq!(
            node(content).text().to_string(),
            content,
            "roundtrip: {content:?}"
        );
    }

    #[test]
    fn root_is_math_content() {
        assert_eq!(node("x").kind(), SyntaxKind::MATH_CONTENT);
    }

    #[test]
    fn plain_text_is_one_atom_run() {
        // A run with no structural or operator chars stays a single atom.
        assert_eq!(token_kinds("abc"), vec![SyntaxKind::MATH_TEXT]);
        assert_lossless("abc");
        // `/`, `.`, and parens are ordinary atoms, not operators.
        assert_eq!(token_kinds("f(x)/2.5"), vec![SyntaxKind::MATH_TEXT]);
        assert_lossless("f(x)/2.5");
    }

    #[test]
    fn operators_split_atom_runs() {
        // `+ - * = < >` each break the surrounding text into their own
        // MATH_OPERATOR token. Class/precedence is deferred to the formatter.
        assert_eq!(
            token_kinds("a+b=c"),
            vec![
                SyntaxKind::MATH_TEXT,     // a
                SyntaxKind::MATH_OPERATOR, // +
                SyntaxKind::MATH_TEXT,     // b
                SyntaxKind::MATH_OPERATOR, // =
                SyntaxKind::MATH_TEXT,     // c
            ]
        );
        assert_lossless("a+b=c");
    }

    #[test]
    fn each_operator_char_is_its_own_token() {
        for op in ["+", "-", "*", "=", "<", ">"] {
            assert_eq!(
                token_kinds(op),
                vec![SyntaxKind::MATH_OPERATOR],
                "operator {op:?}"
            );
            assert_lossless(op);
        }
        // Adjacent operators do not coalesce — one token per char.
        assert_eq!(
            token_kinds("a<=b"),
            vec![
                SyntaxKind::MATH_TEXT,
                SyntaxKind::MATH_OPERATOR, // <
                SyntaxKind::MATH_OPERATOR, // =
                SyntaxKind::MATH_TEXT,
            ]
        );
        // Unary vs binary minus is NOT distinguished here — both are operators.
        assert_eq!(
            token_kinds("-x"),
            vec![SyntaxKind::MATH_OPERATOR, SyntaxKind::MATH_TEXT]
        );
        assert_lossless("-x");
        // An escaped special stays a control symbol, never an operator.
        assert_eq!(token_kinds(r"\<"), vec![SyntaxKind::MATH_COMMAND]);
        assert_lossless(r"\<");
    }

    #[test]
    fn operators_inside_groups_and_scripts_are_lossless() {
        for content in [r"e^{-x}", r"10^{-3}", r"\frac{a+b}{c-d}", r"x_{i+1}"] {
            assert_lossless(content);
        }
    }

    #[test]
    fn control_word_and_symbol() {
        assert_eq!(
            token_kinds(r"\alpha\,"),
            vec![SyntaxKind::MATH_COMMAND, SyntaxKind::MATH_COMMAND]
        );
        assert_lossless(r"\alpha\,");
        // Escaped specials are control symbols, not structural markers.
        assert_eq!(token_kinds(r"\&\%\{\}"), vec![SyntaxKind::MATH_COMMAND; 4]);
        assert_lossless(r"\&\%\{\}");
    }

    #[test]
    fn brace_group_nests() {
        let tree = node(r"x^{2}");
        let group = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::MATH_GROUP)
            .expect("group");
        let kinds: Vec<_> = group.children_with_tokens().map(|el| el.kind()).collect();
        assert_eq!(
            kinds,
            vec![
                SyntaxKind::MATH_GROUP_OPEN,
                SyntaxKind::MATH_TEXT,
                SyntaxKind::MATH_GROUP_CLOSE
            ]
        );
        assert_lossless(r"x^{2}");
    }

    #[test]
    fn line_break_alignment_and_scripts() {
        assert_eq!(
            token_kinds(r"x &= 1 \\"),
            vec![
                SyntaxKind::MATH_TEXT,       // x
                SyntaxKind::MATH_SPACE,      // ' '
                SyntaxKind::MATH_ALIGN,      // &
                SyntaxKind::MATH_OPERATOR,   // =
                SyntaxKind::MATH_SPACE,      // ' '
                SyntaxKind::MATH_TEXT,       // 1
                SyntaxKind::MATH_SPACE,      // ' '
                SyntaxKind::MATH_LINE_BREAK, // \\
            ]
        );
        assert_lossless(r"x &= 1 \\");
        assert_eq!(
            token_kinds("x^2_i"),
            vec![
                SyntaxKind::MATH_TEXT,
                SyntaxKind::MATH_SCRIPT,
                SyntaxKind::MATH_TEXT,
                SyntaxKind::MATH_SCRIPT,
                SyntaxKind::MATH_TEXT,
            ]
        );
    }

    #[test]
    fn environment_wraps_body() {
        let content = "\\begin{aligned}\nx &= 1\n\\end{aligned}";
        let tree = node(content);
        let env = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::MATH_ENVIRONMENT)
            .expect("environment");
        assert_eq!(env.text().to_string(), content);
        let commands = env
            .children_with_tokens()
            .filter(|el| el.kind() == SyntaxKind::MATH_COMMAND)
            .count();
        assert_eq!(commands, 2);
        assert_lossless(content);
        assert!(
            codes(content).is_empty(),
            "well-formed env has no diagnostics"
        );
    }

    #[test]
    fn nested_environments() {
        let content = r"\begin{a}\begin{b}x\end{b}\end{a}";
        let envs = node(content)
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::MATH_ENVIRONMENT)
            .count();
        assert_eq!(envs, 2);
        assert_lossless(content);
        assert!(codes(content).is_empty());
    }

    #[test]
    fn comment_runs_to_end_of_line() {
        assert_eq!(
            token_kinds("a % tail\nb"),
            vec![
                SyntaxKind::MATH_TEXT,
                SyntaxKind::MATH_SPACE,
                SyntaxKind::MATH_COMMENT,
                SyntaxKind::MATH_NEWLINE,
                SyntaxKind::MATH_TEXT,
            ]
        );
        assert_lossless("a % tail\nb");
    }

    #[test]
    fn crlf_and_unicode_are_lossless() {
        assert_lossless("x &= 1\r\ny &= 2\r\n");
        assert_lossless(r"\alpha + \beta \neq \gamma_{\text{αβγ}}");
    }

    #[test]
    fn empty_content() {
        assert_eq!(node("").text().to_string(), "");
        assert!(token_kinds("").is_empty());
    }

    #[test]
    fn trailing_backslash() {
        assert_eq!(
            token_kinds("a\\"),
            vec![SyntaxKind::MATH_TEXT, SyntaxKind::MATH_COMMAND]
        );
        assert_lossless("a\\");
    }

    // --- Diagnostics side-channel (lossless even when malformed) ---

    #[test]
    fn unclosed_group_is_lossless_and_diagnosed() {
        assert_lossless("{a");
        assert_eq!(codes("{a"), vec![diagnostic_codes::UNCLOSED_GROUP]);
    }

    #[test]
    fn stray_close_brace_is_lossless_and_diagnosed() {
        assert_lossless("a}b");
        assert_eq!(codes("a}b"), vec![diagnostic_codes::UNEXPECTED_CLOSE_BRACE]);
    }

    #[test]
    fn unclosed_environment_is_diagnosed() {
        let content = r"\begin{aligned} x &= 1";
        assert_lossless(content);
        assert_eq!(codes(content), vec![diagnostic_codes::UNCLOSED_ENVIRONMENT]);
    }

    #[test]
    fn mismatched_environment_is_diagnosed() {
        let content = r"\begin{aligned}x\end{matrix}";
        assert_lossless(content);
        assert_eq!(
            codes(content),
            vec![diagnostic_codes::MISMATCHED_ENVIRONMENT]
        );
    }

    #[test]
    fn stray_end_is_diagnosed() {
        let content = r"x \end{aligned}";
        assert_lossless(content);
        assert_eq!(codes(content), vec![diagnostic_codes::UNEXPECTED_END]);
    }

    #[test]
    fn well_formed_math_has_no_diagnostics() {
        assert!(codes(r"\frac{1}{2} + x^{2}").is_empty());
    }

    // --- Bookdown equation labels (gated on the extension) ---

    const BOOKDOWN: MathParseOptions = MathParseOptions {
        bookdown_equation_labels: true,
    };

    fn label_kinds(content: &str, opts: MathParseOptions) -> Vec<SyntaxKind> {
        node_with(content, opts)
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .map(|tok| tok.kind())
            .collect()
    }

    #[test]
    fn equation_label_recognized_when_enabled() {
        let kinds = label_kinds(r"a (\#eq:foo)", BOOKDOWN);
        assert!(kinds.contains(&SyntaxKind::MATH_EQUATION_LABEL));
        // The label is a single token spanning the whole `(\#eq:foo)`.
        let label = node_with(r"a (\#eq:foo)", BOOKDOWN)
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|t| t.kind() == SyntaxKind::MATH_EQUATION_LABEL)
            .expect("label token");
        assert_eq!(label.text(), r"(\#eq:foo)");
    }

    #[test]
    fn equation_label_ignored_when_disabled() {
        // Default options: no label token, and plain math is byte-identical.
        let kinds = label_kinds(r"a (\#eq:foo)", MathParseOptions::default());
        assert!(!kinds.contains(&SyntaxKind::MATH_EQUATION_LABEL));
    }

    #[test]
    fn plain_parens_unchanged_when_disabled() {
        // `(` must not fragment ordinary atom runs while the extension is off.
        assert_eq!(token_kinds("f(x)"), vec![SyntaxKind::MATH_TEXT]);
    }

    #[test]
    fn label_parsing_is_lossless() {
        let content = "\\begin{align}\n  a (\\#eq:solveG)\n\\end{align}";
        assert_eq!(node_with(content, BOOKDOWN).text().to_string(), content);
    }
}
