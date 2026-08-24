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
//! and whitespace. Everything else is an ordinary-atom run ([`MATH_WORD`]).
//!
//! The **CST is lossless and never fails** (`node.text() == content` for every
//! input; worst case is a single `MATH_WORD` atom). Structural problems
//! (unbalanced braces, unclosed or mismatched environments) are *not* reported
//! here: they are derived from the realized tree shape by
//! [`crate::syntax::math_diagnostics`], the single source of truth shared by the
//! linter, formatter, and LSP. Keeping the parser diagnostic-free means the
//! host-aligned ranges come for free from the spliced subtree.
//!
//! [`MATH_WORD`]: SyntaxKind::MATH_WORD

use crate::parser::inlines::bookdown::try_parse_bookdown_equation_definition;
use crate::syntax::SyntaxKind;
use rowan::{GreenNode, GreenNodeBuilder};

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

/// Parse math content into a lossless `MATH_CONTENT` green node. `content` is
/// the raw text between (but excluding) the math delimiters. Never fails:
/// `SyntaxNode::new_root(result).text() == content` for every input.
pub fn parse_math_content(content: &str, opts: MathParseOptions) -> GreenNode {
    let mut parser = MathParser {
        input: content,
        pos: 0,
        builder: GreenNodeBuilder::new(),
        opts,
    };
    parser.builder.start_node(SyntaxKind::MATH_CONTENT.into());
    parser.parse_elements(Ctx::Top);
    parser.builder.finish_node();
    parser.builder.finish()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ctx {
    /// Top level of the math content.
    Top,
    /// Inside a `{ ... }` brace group; stops at the matching `}`.
    Group,
    /// Inside a `\begin{env} ... \end{env}` body; stops at `\end`.
    Env,
    /// Inside a `\left<d> ... \right<d>` body; stops at `\right`.
    LeftRight,
}

struct MathParser<'a> {
    input: &'a str,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    opts: MathParseOptions,
}

impl MathParser<'_> {
    fn rest(&self) -> &str {
        &self.input[self.pos..]
    }

    fn peek_char(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump_bytes(&mut self, len: usize, kind: SyntaxKind) {
        let text = &self.input[self.pos..self.pos + len];
        self.builder.token(kind.into(), text);
        self.pos += len;
    }

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
                '}' => self.bump_bytes(1, SyntaxKind::MATH_GROUP_CLOSE),
                '\\' => {
                    if self.rest().starts_with("\\\\") {
                        self.bump_bytes(2, SyntaxKind::MATH_LINE_BREAK);
                    } else if let Some(word) = self.peek_control_word() {
                        match word {
                            "end" if ctx == Ctx::Env => break,
                            "right" if ctx == Ctx::LeftRight => break,
                            _ => self.parse_scripted_atom(ctx),
                        }
                    } else {
                        self.parse_scripted_atom(ctx);
                    }
                }
                '&' => self.bump_bytes(1, SyntaxKind::MATH_ALIGN),
                '^' => self.bump_bytes(1, SyntaxKind::MATH_CARET),
                '_' => self.bump_bytes(1, SyntaxKind::MATH_UNDERSCORE),
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
                _ => self.parse_scripted_atom(ctx),
            }
        }
    }

    /// Parse one atom and attach any immediately following TeX scripts to it.
    ///
    /// Rowan checkpoints let the parser wrap the base after it has been emitted,
    /// which keeps script attachment in this single pass. An ordinary-text run
    /// is split before its final Unicode scalar when a script follows because
    /// TeX attaches an unbraced script to one token, not to the whole run.
    fn parse_scripted_atom(&mut self, ctx: Ctx) {
        self.split_word_prefix_before_script();

        let checkpoint = self.builder.checkpoint();
        self.parse_atom();

        if self.script_marker_after_layout(self.pos).is_none() {
            return;
        }

        self.builder
            .start_node_at(checkpoint, SyntaxKind::MATH_SCRIPTED.into());
        while let Some(marker_pos) = self.script_marker_after_layout(self.pos) {
            self.parse_layout_until(marker_pos);
            let script_kind = match self.peek_char() {
                Some('^') => SyntaxKind::MATH_SUPERSCRIPT,
                Some('_') => SyntaxKind::MATH_SUBSCRIPT,
                _ => unreachable!("script lookahead must end at a script marker"),
            };
            self.builder.start_node(script_kind.into());
            let marker_kind = match self.peek_char() {
                Some('^') => SyntaxKind::MATH_CARET,
                Some('_') => SyntaxKind::MATH_UNDERSCORE,
                _ => unreachable!("script parser must be at a script marker"),
            };
            self.bump_bytes(1, marker_kind);

            let argument_pos = self.layout_end_before_boundary(self.pos);
            self.parse_layout_until(argument_pos);
            if self.can_start_script_argument(ctx) {
                self.parse_script_argument();
            }
            self.builder.finish_node();
        }
        self.builder.finish_node();
    }

    /// Emit all but the final Unicode scalar in an ordinary-text run when the
    /// run is followed by a script. The final scalar is then parsed as the base.
    fn split_word_prefix_before_script(&mut self) {
        let len = self.word_len();
        if len == 0 || self.script_marker_after_layout(self.pos + len).is_none() {
            return;
        }

        let text = &self.input[self.pos..self.pos + len];
        let Some((last_offset, _)) = text.char_indices().next_back() else {
            return;
        };
        if last_offset > 0 {
            self.bump_bytes(last_offset, SyntaxKind::MATH_WORD);
        }
    }

    /// Parse one atom without considering scripts that follow it.
    fn parse_atom(&mut self) {
        match self.peek_char() {
            Some('\\') if self.peek_control_word() == Some("begin") => self.parse_environment(),
            Some('\\') if self.peek_control_word() == Some("left") => self.parse_delimited(),
            Some('\\') if self.peek_control_word().is_some() => self.parse_control_word(),
            Some('\\') => self.parse_control_symbol(),
            Some('{') => self.parse_group(),
            Some('(') if self.opts.bookdown_equation_labels => match self.equation_label_len() {
                Some(len) => self.bump_bytes(len, SyntaxKind::MATH_EQUATION_LABEL),
                None => self.parse_word(),
            },
            Some(_) => self.parse_word(),
            None => {}
        }
    }

    /// Parse the single TeX token that forms an unbraced script argument.
    fn parse_script_argument(&mut self) {
        if self.word_len() > 0 {
            let len = self.peek_char().map(char::len_utf8).unwrap_or(0);
            self.bump_bytes(len, SyntaxKind::MATH_WORD);
        } else {
            self.parse_atom();
        }
    }

    fn can_start_script_argument(&self, ctx: Ctx) -> bool {
        match self.peek_char() {
            None | Some('}' | '&' | '%' | '^' | '_') => false,
            Some('\\') if self.rest().starts_with("\\\\") => false,
            Some('\\') if ctx == Ctx::Env && self.peek_control_word() == Some("end") => false,
            Some('\\') if ctx == Ctx::LeftRight && self.peek_control_word() == Some("right") => {
                false
            }
            Some(' ' | '\t' | '\n' | '\r') => false,
            Some(_) => true,
        }
    }

    /// Find a script marker after horizontal trivia and at most one physical
    /// newline. Comments and blank lines deliberately stop attachment.
    fn script_marker_after_layout(&self, pos: usize) -> Option<usize> {
        let end = self.layout_end_before_boundary(pos);
        self.input[end..].starts_with(['^', '_']).then_some(end)
    }

    /// Return the end of attachable layout trivia. The second physical newline
    /// is a boundary and remains outside the scripted node.
    fn layout_end_before_boundary(&self, mut pos: usize) -> usize {
        let mut saw_newline = false;
        while let Some(c) = self.input[pos..].chars().next() {
            match c {
                ' ' | '\t' => pos += c.len_utf8(),
                '\n' | '\r' if !saw_newline => {
                    saw_newline = true;
                    if c == '\r' && self.input[pos..].starts_with("\r\n") {
                        pos += 2;
                    } else {
                        pos += c.len_utf8();
                    }
                }
                _ => break,
            }
        }
        pos
    }

    fn parse_layout_until(&mut self, end: usize) {
        while self.pos < end {
            match self.peek_char() {
                Some(' ' | '\t') => self.parse_spaces(),
                Some('\n') => self.bump_bytes(1, SyntaxKind::MATH_NEWLINE),
                Some('\r') => {
                    let len = if self.rest().starts_with("\r\n") {
                        2
                    } else {
                        1
                    };
                    self.bump_bytes(len, SyntaxKind::MATH_NEWLINE);
                }
                _ => unreachable!("layout lookahead may only skip layout trivia"),
            }
        }
    }

    fn parse_environment(&mut self) {
        self.builder.start_node(SyntaxKind::MATH_ENVIRONMENT.into());
        self.parse_control_word(); // \begin
        self.parse_environment_name(); // {name} group, if present
        self.parse_elements(Ctx::Env);
        if self.peek_control_word() == Some("end") {
            self.parse_control_word(); // \end
            self.parse_environment_name(); // {name} group, if present
        }
        self.builder.finish_node();
    }

    fn parse_environment_name(&mut self) {
        if self.peek_char() == Some('{') {
            self.parse_group();
        }
    }

    fn parse_group(&mut self) {
        self.builder.start_node(SyntaxKind::MATH_GROUP.into());
        self.bump_bytes(1, SyntaxKind::MATH_GROUP_OPEN); // {
        self.parse_elements(Ctx::Group);
        if self.peek_char() == Some('}') {
            self.bump_bytes(1, SyntaxKind::MATH_GROUP_CLOSE); // }
        }
        self.builder.finish_node();
    }

    fn parse_delimited(&mut self) {
        self.builder.start_node(SyntaxKind::MATH_DELIMITED.into());
        self.parse_control_word(); // \left
        self.consume_delimiter(); // opening delimiter argument
        self.parse_elements(Ctx::LeftRight);
        if self.peek_control_word() == Some("right") {
            self.parse_control_word(); // \right
            self.consume_delimiter(); // closing delimiter argument
        }
        self.builder.finish_node();
    }

    /// Consume the single delimiter that follows `\left` / `\right`, when it sits
    /// immediately at the cursor. Character delimiters stay lexically neutral
    /// `MATH_WORD` tokens;
    /// a control-sequence delimiter (`\{`, `\langle`, `\|`, …) is a `MATH_COMMAND`.
    /// If a space or anything else intervenes, nothing is consumed here and the
    /// surrounding element loop tokenizes it normally — losslessness holds either
    /// way; only the token's node membership shifts.
    fn consume_delimiter(&mut self) {
        match self.peek_char() {
            Some('(' | '[' | ')' | ']' | '.' | '|' | '/') => {
                let len = self.peek_char().map(char::len_utf8).unwrap_or(0);
                self.bump_bytes(len, SyntaxKind::MATH_WORD);
            }
            Some('\\') => {
                if self.peek_control_word().is_some() {
                    self.parse_control_word();
                } else {
                    self.parse_control_symbol();
                }
            }
            _ => {}
        }
    }

    fn parse_control_word(&mut self) {
        let word_len = self.peek_control_word().map(str::len).unwrap_or(0);
        self.bump_bytes(1 + word_len, SyntaxKind::MATH_COMMAND);
    }

    fn parse_control_symbol(&mut self) {
        let after = &self.input[self.pos + 1..];
        let len = 1 + after.chars().next().map(char::len_utf8).unwrap_or(0);
        self.bump_bytes(len, SyntaxKind::MATH_COMMAND);
    }

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

    fn parse_word(&mut self) {
        let len = self.word_len();
        debug_assert!(len > 0, "parse_word on a structural character");
        self.bump_bytes(len, SyntaxKind::MATH_WORD);
    }

    fn word_len(&self) -> usize {
        self.rest()
            .char_indices()
            .find_map(|(offset, c)| {
                let structural = is_structural(c);
                let host_label = self.opts.bookdown_equation_labels
                    && c == '('
                    && self.equation_label_len_at(offset).is_some();
                (structural || host_label).then_some(offset)
            })
            .unwrap_or_else(|| self.rest().len())
    }

    fn equation_label_len(&self) -> Option<usize> {
        try_parse_bookdown_equation_definition(self.rest()).map(|(len, _)| len)
    }

    fn equation_label_len_at(&self, offset: usize) -> Option<usize> {
        try_parse_bookdown_equation_definition(&self.rest()[offset..]).map(|(len, _)| len)
    }
}

fn is_structural(c: char) -> bool {
    matches!(
        c,
        '\\' | '{' | '}' | '&' | '^' | '_' | '%' | ' ' | '\t' | '\n' | '\r'
    )
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
        assert_eq!(token_kinds("abc"), vec![SyntaxKind::MATH_WORD]);
        assert_lossless("abc");
        assert_eq!(token_kinds("f(x)/2.5"), vec![SyntaxKind::MATH_WORD]);
        assert_lossless("f(x)/2.5");
    }

    #[test]
    fn badness_word_grain_keeps_semantic_characters_lexically_neutral() {
        for content in ["P(X", "M(t)", "i=1", "a+b", "[a,b);"] {
            assert_eq!(
                token_kinds(content),
                vec![SyntaxKind::MATH_WORD],
                "word run: {content:?}"
            );
            assert_lossless(content);
        }
    }

    #[test]
    fn delimiters_and_punctuation_remain_in_word_runs() {
        assert_eq!(token_kinds("[a,b);"), vec![SyntaxKind::MATH_WORD]);
        assert_lossless("[a,b);");
        assert_eq!(token_kinds("a|b.c/d"), vec![SyntaxKind::MATH_WORD]);
        assert_lossless("a|b.c/d");
        assert_eq!(token_kinds(r"\(\)\[\]"), vec![SyntaxKind::MATH_COMMAND; 4]);
        assert_lossless(r"\(\)\[\]");
    }

    #[test]
    fn operators_remain_in_word_runs() {
        assert_eq!(token_kinds("a+b=c"), vec![SyntaxKind::MATH_WORD]);
        assert_lossless("a+b=c");
    }

    #[test]
    fn operator_runs_are_lexically_neutral() {
        for op in ["+", "-", "*", "=", "<", ">"] {
            assert_eq!(
                token_kinds(op),
                vec![SyntaxKind::MATH_WORD],
                "operator {op:?}"
            );
            assert_lossless(op);
        }
        assert_eq!(token_kinds("a<=b"), vec![SyntaxKind::MATH_WORD]);
        assert_eq!(token_kinds("-x"), vec![SyntaxKind::MATH_WORD]);
        assert_lossless("-x");
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
                SyntaxKind::MATH_WORD,
                SyntaxKind::MATH_GROUP_CLOSE
            ]
        );
        assert_lossless(r"x^{2}");
    }

    #[test]
    fn line_break_and_alignment_tokens() {
        assert_eq!(
            token_kinds(r"x &= 1 \\"),
            vec![
                SyntaxKind::MATH_WORD,       // x
                SyntaxKind::MATH_SPACE,      // ' '
                SyntaxKind::MATH_ALIGN,      // &
                SyntaxKind::MATH_WORD,       // =
                SyntaxKind::MATH_SPACE,      // ' '
                SyntaxKind::MATH_WORD,       // 1
                SyntaxKind::MATH_SPACE,      // ' '
                SyntaxKind::MATH_LINE_BREAK, // \\
            ]
        );
        assert_lossless(r"x &= 1 \\");
    }

    #[test]
    fn scripts_attach_to_their_base_and_one_atom_arguments() {
        let tree = node("x^2_i");
        let scripted = tree
            .children()
            .find(|child| child.kind() == SyntaxKind::MATH_SCRIPTED)
            .expect("scripted atom");
        assert_eq!(scripted.text().to_string(), "x^2_i");
        assert_eq!(
            scripted
                .children_with_tokens()
                .map(|el| el.kind())
                .collect::<Vec<_>>(),
            vec![
                SyntaxKind::MATH_WORD,
                SyntaxKind::MATH_SUPERSCRIPT,
                SyntaxKind::MATH_SUBSCRIPT,
            ]
        );
        let superscript = scripted
            .children()
            .find(|child| child.kind() == SyntaxKind::MATH_SUPERSCRIPT)
            .expect("superscript");
        assert_eq!(
            superscript
                .children_with_tokens()
                .map(|el| el.kind())
                .collect::<Vec<_>>(),
            vec![SyntaxKind::MATH_CARET, SyntaxKind::MATH_WORD]
        );
        assert_lossless("x^2_i");
    }

    #[test]
    fn scripts_follow_tex_one_token_attachment() {
        let tree = node("ab^23_i");
        let children: Vec<_> = tree.children_with_tokens().collect();
        assert_eq!(
            children.iter().map(|el| el.kind()).collect::<Vec<_>>(),
            vec![
                SyntaxKind::MATH_WORD,
                SyntaxKind::MATH_SCRIPTED,
                SyntaxKind::MATH_SCRIPTED,
            ]
        );
        assert_eq!(children[0].to_string(), "a");
        assert_eq!(children[1].to_string(), "b^2");
        assert_eq!(children[2].to_string(), "3_i");
        assert_lossless("ab^23_i");
    }

    #[test]
    fn structured_atoms_and_unicode_scalars_can_be_script_bases() {
        for (content, base_kind) in [
            (r"\alpha_i", SyntaxKind::MATH_COMMAND),
            (r"{ab}^2", SyntaxKind::MATH_GROUP),
            (r"\left(x\right)_i", SyntaxKind::MATH_DELIMITED),
        ] {
            let scripted = node(content)
                .children()
                .find(|child| child.kind() == SyntaxKind::MATH_SCRIPTED)
                .unwrap_or_else(|| panic!("scripted atom: {content:?}"));
            assert_eq!(
                scripted.children_with_tokens().next().map(|el| el.kind()),
                Some(base_kind),
                "base: {content:?}"
            );
            assert_lossless(content);
        }

        let tree = node("αβ_γ");
        let children: Vec<_> = tree.children_with_tokens().collect();
        assert_eq!(children[0].to_string(), "α");
        assert_eq!(children[1].kind(), SyntaxKind::MATH_SCRIPTED);
        assert_eq!(children[1].to_string(), "β_γ");
        assert_lossless("αβ_γ");
    }

    #[test]
    fn script_attachment_skips_layout_trivia_but_not_comments_or_blank_lines() {
        let attached = node("x \n ^ 2");
        let scripted = attached
            .children()
            .find(|child| child.kind() == SyntaxKind::MATH_SCRIPTED)
            .expect("script across one newline");
        assert_eq!(scripted.text().to_string(), "x \n ^ 2");
        assert_lossless("x \n ^ 2");

        for content in ["x% stop\n^2", "x\n\n^2"] {
            assert!(
                node(content)
                    .children()
                    .all(|child| child.kind() != SyntaxKind::MATH_SCRIPTED),
                "must not attach: {content:?}"
            );
            assert_lossless(content);
        }
    }

    #[test]
    fn missing_and_stray_scripts_recover_losslessly() {
        let missing = node("x^");
        let superscript = missing
            .descendants()
            .find(|child| child.kind() == SyntaxKind::MATH_SUPERSCRIPT)
            .expect("missing superscript argument remains structured");
        assert_eq!(superscript.text().to_string(), "^");
        assert_lossless("x^");

        assert!(
            node("^2")
                .children()
                .all(|child| child.kind() != SyntaxKind::MATH_SCRIPTED)
        );
        assert_lossless("^2");
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
    }

    #[test]
    fn comment_runs_to_end_of_line() {
        assert_eq!(
            token_kinds("a % tail\nb"),
            vec![
                SyntaxKind::MATH_WORD,
                SyntaxKind::MATH_SPACE,
                SyntaxKind::MATH_COMMENT,
                SyntaxKind::MATH_NEWLINE,
                SyntaxKind::MATH_WORD,
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
            vec![SyntaxKind::MATH_WORD, SyntaxKind::MATH_COMMAND]
        );
        assert_lossless("a\\");
    }

    #[test]
    fn malformed_math_is_still_lossless() {
        for content in [
            "{a",
            "a}b",
            r"\begin{aligned} x &= 1",
            r"\begin{aligned}x\end{matrix}",
            r"x \end{aligned}",
        ] {
            assert_lossless(content);
        }
    }

    fn delimited_count(content: &str) -> usize {
        node(content)
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::MATH_DELIMITED)
            .count()
    }

    #[test]
    fn left_right_wraps_a_delimited_node() {
        let content = r"\left( x + y \right)";
        let tree = node(content);
        let delim = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::MATH_DELIMITED)
            .expect("delimited node");
        assert_eq!(delim.text().to_string(), content);
        let commands: Vec<String> = delim
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|t| t.kind() == SyntaxKind::MATH_COMMAND)
            .map(|t| t.text().to_string())
            .collect();
        assert_eq!(commands, vec![r"\left", r"\right"]);
        assert_lossless(content);
    }

    #[test]
    fn left_right_delimiters_keep_their_token_kinds() {
        assert_eq!(
            token_kinds(r"\left(x\right)"),
            vec![
                SyntaxKind::MATH_COMMAND, // \left
                SyntaxKind::MATH_WORD,    // (
                SyntaxKind::MATH_WORD,    // x
                SyntaxKind::MATH_COMMAND, // \right
                SyntaxKind::MATH_WORD,    // )
            ]
        );
    }

    #[test]
    fn null_delimiter_and_asymmetric_pairs_are_lossless() {
        for content in [
            r"\left. x \right|",
            r"\left( x \right]",
            r"\left\{ x \right\}",
        ] {
            assert_eq!(delimited_count(content), 1, "one node: {content:?}");
            assert_lossless(content);
        }
    }

    #[test]
    fn nested_delimited_is_lossless() {
        let content = r"\left[ \left( a \right) \right]";
        assert_eq!(delimited_count(content), 2);
        assert_lossless(content);
    }

    #[test]
    fn unclosed_and_stray_delimiters_stay_lossless() {
        assert_eq!(delimited_count(r"\left( x"), 1);
        assert_lossless(r"\left( x");
        assert_eq!(delimited_count(r"x \right)"), 0);
        assert_lossless(r"x \right)");
    }

    #[test]
    fn leftarrow_and_rightarrow_are_not_delimiters() {
        let content = r"a \leftarrow b \rightarrow c";
        assert_eq!(delimited_count(content), 0);
        assert_lossless(content);
    }

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
        let label = node_with(r"a (\#eq:foo)", BOOKDOWN)
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|t| t.kind() == SyntaxKind::MATH_EQUATION_LABEL)
            .expect("label token");
        assert_eq!(label.text(), r"(\#eq:foo)");
    }

    #[test]
    fn equation_label_ignored_when_disabled() {
        let kinds = label_kinds(r"a (\#eq:foo)", MathParseOptions::default());
        assert!(!kinds.contains(&SyntaxKind::MATH_EQUATION_LABEL));
    }

    #[test]
    fn plain_parens_tokenize_the_same_with_or_without_bookdown() {
        let expected = vec![SyntaxKind::MATH_WORD];
        assert_eq!(token_kinds("f(x)"), expected);
        assert_eq!(label_kinds("f(x)", BOOKDOWN), expected);
    }

    #[test]
    fn label_parsing_is_lossless() {
        let content = "\\begin{align}\n  a (\\#eq:solveG)\n\\end{align}";
        assert_eq!(node_with(content, BOOKDOWN).text().to_string(), content);
    }
}
