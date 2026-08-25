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
//! optional arguments, brackets, comments, and whitespace. Everything else is
//! an ordinary-atom run ([`MATH_WORD`]).
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
    /// Recognize Bookdown equation labels `(\#eq:label)` during standalone
    /// math parsing. Host embedding instead emits these labels beside its
    /// `MATH_CONTENT` segments.
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
    /// Inside a `[ ... ]` optional argument; stops at `]` or a recovery
    /// boundary that cannot belong to the optional.
    Optional,
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
                '}' if ctx == Ctx::Optional => break,
                '}' => self.bump_bytes(1, SyntaxKind::MATH_GROUP_CLOSE),
                ']' if ctx == Ctx::Optional => break,
                '[' => self.parse_scripted_atom(ctx),
                ']' => self.parse_scripted_atom(ctx),
                '\\' => {
                    if self.rest().starts_with("\\\\") {
                        self.parse_line_break();
                    } else if let Some(word) = self.peek_control_word() {
                        match word {
                            "begin" | "end" if ctx == Ctx::Optional => break,
                            "end"
                                if ctx == Ctx::Env
                                    && self.peek_environment_name(self.pos).is_some() =>
                            {
                                break;
                            }
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
    ///
    /// The word length is computed once here and shared between the split and
    /// the emission — the word scan (and, with bookdown labels on, its per-`(`
    /// label probe) is the lexer's hot path.
    fn parse_scripted_atom(&mut self, ctx: Ctx) {
        let word_len = self.word_len();
        if word_len > 0 {
            let Some(marker_pos) = self.script_marker_after_layout(self.pos + word_len) else {
                self.bump_bytes(word_len, SyntaxKind::MATH_WORD);
                return;
            };
            let text = &self.input[self.pos..self.pos + word_len];
            let (last_offset, _) = text.char_indices().next_back().expect("non-empty word");
            if last_offset > 0 {
                self.bump_bytes(last_offset, SyntaxKind::MATH_WORD);
            }
            let checkpoint = self.builder.checkpoint();
            self.bump_bytes(word_len - last_offset, SyntaxKind::MATH_WORD);
            self.attach_scripts(checkpoint, ctx, marker_pos);
            return;
        }

        let checkpoint = self.builder.checkpoint();
        self.parse_atom();
        if let Some(marker_pos) = self.script_marker_after_layout(self.pos) {
            self.attach_scripts(checkpoint, ctx, marker_pos);
        }
    }

    /// Wrap the just-emitted atom at `checkpoint` in a `MATH_SCRIPTED` node
    /// and attach every following script. `marker_pos` is the already-scanned
    /// position of the first marker, so the lookahead is not repeated.
    fn attach_scripts(&mut self, checkpoint: rowan::Checkpoint, ctx: Ctx, first_marker: usize) {
        self.builder
            .start_node_at(checkpoint, SyntaxKind::MATH_SCRIPTED.into());
        let mut marker_pos = first_marker;
        loop {
            self.parse_layout_until(marker_pos);
            let (script_kind, marker_kind) = match self.peek_char() {
                Some('^') => (SyntaxKind::MATH_SUPERSCRIPT, SyntaxKind::MATH_CARET),
                Some('_') => (SyntaxKind::MATH_SUBSCRIPT, SyntaxKind::MATH_UNDERSCORE),
                _ => unreachable!("script lookahead must end at a script marker"),
            };
            self.builder.start_node(script_kind.into());
            self.bump_bytes(1, marker_kind);

            let argument_pos = self.layout_end_before_boundary(self.pos);
            self.parse_layout_until(argument_pos);
            if self.can_start_script_argument(ctx) {
                self.parse_script_argument();
            }
            self.builder.finish_node();

            match self.script_marker_after_layout(self.pos) {
                Some(next) => marker_pos = next,
                None => break,
            }
        }
        self.builder.finish_node();
    }

    /// Parse one atom without considering scripts that follow it.
    fn parse_atom(&mut self) {
        match self.peek_char() {
            Some('\\') => match self.peek_control_word() {
                Some("begin") if self.peek_environment_name(self.pos).is_some() => {
                    self.parse_environment();
                }
                Some("left") if self.left_right_closes() => self.parse_delimited(),
                Some("end") if self.peek_environment_name(self.pos).is_some() => {
                    self.parse_environment_end();
                }
                // A matched `\right` is consumed by its delimited parser before
                // reaching here. A stray one stays lexically bare for now.
                Some("right") => self.parse_control_word(),
                Some(_) => self.parse_command(),
                None => self.parse_control_symbol(),
            },
            Some('{') => self.parse_group(),
            Some('[') => self.bump_bytes(1, SyntaxKind::MATH_BRACKET_OPEN),
            Some(']') => self.bump_bytes(1, SyntaxKind::MATH_BRACKET_CLOSE),
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
        let (begin_group_start, begin_name) = self
            .peek_environment_name(self.pos)
            .expect("environment parsing requires a name");
        self.builder.start_node(SyntaxKind::MATH_ENVIRONMENT.into());
        self.builder.start_node(SyntaxKind::MATH_BEGIN.into());
        self.parse_control_word(); // \begin
        self.parse_environment_name(begin_group_start);
        self.builder.finish_node(); // MATH_BEGIN

        self.builder.start_node(SyntaxKind::MATH_CONTENT.into());
        self.parse_elements(Ctx::Env);
        self.builder.finish_node(); // MATH_CONTENT

        if self.peek_control_word() == Some("end")
            && self
                .peek_environment_name(self.pos)
                .is_some_and(|(_, end_name)| end_name == begin_name)
        {
            self.parse_environment_end();
        }
        self.builder.finish_node();
    }

    fn parse_environment_name(&mut self, group_start: usize) {
        self.parse_trivia_until(group_start);
        self.builder.start_node(SyntaxKind::MATH_NAME_GROUP.into());
        self.bump_bytes(1, SyntaxKind::MATH_GROUP_OPEN);
        self.parse_elements(Ctx::Group);
        if self.peek_char() == Some('}') {
            self.bump_bytes(1, SyntaxKind::MATH_GROUP_CLOSE);
        }
        self.builder.finish_node();
    }

    fn parse_environment_end(&mut self) {
        let (group_start, _) = self
            .peek_environment_name(self.pos)
            .expect("environment end parsing requires a name");
        self.builder.start_node(SyntaxKind::MATH_END.into());
        self.parse_control_word();
        self.parse_environment_name(group_start);
        self.builder.finish_node();
    }

    /// Return the opening brace and trimmed name for a delimiter-like
    /// `\begin`/`\end`, or `None` when the following bytes are macro data.
    fn peek_environment_name(&self, command_pos: usize) -> Option<(usize, String)> {
        let mut pos = command_pos + self.control_word_len_at(command_pos)?;
        let mut saw_newline = false;
        loop {
            let c = self.input[pos..].chars().next()?;
            match c {
                ' ' | '\t' => pos += c.len_utf8(),
                '%' => {
                    pos += self.input[pos..]
                        .find(['\n', '\r'])
                        .unwrap_or(self.input.len() - pos);
                }
                '\n' | '\r' if !saw_newline => {
                    saw_newline = true;
                    if c == '\r' && self.input[pos..].starts_with("\r\n") {
                        pos += 2;
                    } else {
                        pos += c.len_utf8();
                    }
                }
                '\n' | '\r' => return None,
                '{' => break,
                _ => return None,
            }
        }

        let group_start = pos;
        pos += 1;
        let name_start = pos;
        while let Some(c) = self.input[pos..].chars().next() {
            match c {
                '}' => return Some((group_start, self.input[name_start..pos].trim().to_owned())),
                '#' | '\\' | '{' => return None,
                '\n' | '\r' => break,
                _ => pos += c.len_utf8(),
            }
        }
        Some((group_start, self.input[name_start..pos].trim().to_owned()))
    }

    fn control_word_len_at(&self, pos: usize) -> Option<usize> {
        let after = self.input[pos..].strip_prefix('\\')?;
        let name_len = after
            .bytes()
            .take_while(|byte| byte.is_ascii_alphabetic() || *byte == b'@')
            .count();
        (name_len > 0).then_some(name_len + 1)
    }

    fn parse_trivia_until(&mut self, end: usize) {
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
                Some('%') => self.parse_comment(),
                _ => unreachable!("environment-name lookahead may only skip trivia"),
            }
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

        self.builder.start_node(SyntaxKind::MATH_CONTENT.into());
        self.parse_elements(Ctx::LeftRight);
        self.builder.finish_node(); // MATH_CONTENT

        if self.peek_control_word() == Some("right") {
            self.parse_control_word(); // \right
            self.consume_delimiter(); // closing delimiter argument
        }
        self.builder.finish_node();
    }

    /// Consume the single delimiter that follows `\left` / `\right`, when it sits
    /// after optional trivia. Character delimiters retain their lexical token
    /// kind; a control-sequence delimiter (`\{`, `\langle`, `\|`, …) remains a
    /// bare control token.
    fn consume_delimiter(&mut self) {
        self.parse_delimiter_trivia();
        match self.peek_char() {
            Some('}') => {}
            Some('[') => self.bump_bytes(1, SyntaxKind::MATH_BRACKET_OPEN),
            Some(']') => self.bump_bytes(1, SyntaxKind::MATH_BRACKET_CLOSE),
            Some('{') => self.bump_bytes(1, SyntaxKind::MATH_GROUP_OPEN),
            Some('&') => self.bump_bytes(1, SyntaxKind::MATH_ALIGN),
            Some('^') => self.bump_bytes(1, SyntaxKind::MATH_CARET),
            Some('_') => self.bump_bytes(1, SyntaxKind::MATH_UNDERSCORE),
            Some('%') => {}
            Some('\\') if matches!(self.peek_control_word(), Some("left" | "right" | "end")) => {}
            Some('\\') if Self::is_host_math_close(self.rest()) => {}
            Some('\\') => {
                if self.peek_control_word().is_some() {
                    self.parse_control_word();
                } else {
                    self.parse_control_symbol();
                }
            }
            Some(_) => {
                let len = self.peek_char().map(char::len_utf8).unwrap_or(0);
                self.bump_bytes(len, SyntaxKind::MATH_WORD);
            }
            None => {}
        }
    }

    fn parse_delimiter_trivia(&mut self) {
        loop {
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
                Some('%') => self.parse_comment(),
                _ => break,
            }
        }
    }

    /// Whether the current `\left` has a same-scope `\right` before a recovery
    /// boundary. This is a read-only gate; the real parse remains single-pass.
    fn left_right_closes(&self) -> bool {
        let command_end = self.pos + self.control_word_len_at(self.pos).unwrap_or(0);
        let Some(mut pos) = self.delimiter_end_after(command_end) else {
            return self.scan_for_right(command_end);
        };
        self.scan_for_right_from(&mut pos)
    }

    fn scan_for_right(&self, mut pos: usize) -> bool {
        self.scan_for_right_from(&mut pos)
    }

    fn scan_for_right_from(&self, pos: &mut usize) -> bool {
        let mut brace_depth = 0usize;
        let mut environment_depth = 0usize;
        let mut nested_pairs = 0usize;
        let mut newlines = 0usize;

        while *pos < self.input.len() {
            let rest = &self.input[*pos..];
            let c = rest.chars().next().expect("position is in bounds");
            match c {
                ' ' | '\t' => *pos += c.len_utf8(),
                '\n' | '\r' => {
                    newlines += 1;
                    if brace_depth == 0
                        && environment_depth == 0
                        && nested_pairs == 0
                        && newlines >= 2
                    {
                        return false;
                    }
                    if c == '\r' && rest.starts_with("\r\n") {
                        *pos += 2;
                    } else {
                        *pos += c.len_utf8();
                    }
                }
                '%' => {
                    *pos += rest.find(['\n', '\r']).unwrap_or(rest.len());
                }
                '{' => {
                    brace_depth += 1;
                    newlines = 0;
                    *pos += 1;
                }
                '}' => {
                    if brace_depth == 0 && environment_depth == 0 && nested_pairs == 0 {
                        return false;
                    }
                    brace_depth = brace_depth.saturating_sub(1);
                    newlines = 0;
                    *pos += 1;
                }
                '\\' => {
                    if Self::is_host_math_close(rest)
                        && brace_depth == 0
                        && environment_depth == 0
                        && nested_pairs == 0
                    {
                        return false;
                    }
                    let Some(word_len) = self.control_word_len_at(*pos) else {
                        *pos += rest.chars().nth(1).map_or(1, |next| 1 + next.len_utf8());
                        newlines = 0;
                        continue;
                    };
                    let word = &self.input[*pos + 1..*pos + word_len];
                    if brace_depth == 0 {
                        match word {
                            "begin" if self.peek_environment_name(*pos).is_some() => {
                                environment_depth += 1;
                            }
                            "end" if self.peek_environment_name(*pos).is_some() => {
                                if environment_depth == 0 && nested_pairs == 0 {
                                    return false;
                                }
                                environment_depth = environment_depth.saturating_sub(1);
                            }
                            "left" if environment_depth == 0 => nested_pairs += 1,
                            "right" if environment_depth == 0 => {
                                if nested_pairs == 0 {
                                    return true;
                                }
                                nested_pairs -= 1;
                            }
                            _ => {}
                        }
                    }
                    newlines = 0;
                    *pos += word_len;
                }
                _ => {
                    newlines = 0;
                    *pos += c.len_utf8();
                }
            }
        }
        false
    }

    fn delimiter_end_after(&self, mut pos: usize) -> Option<usize> {
        let mut newlines = 0usize;
        loop {
            let c = self.input[pos..].chars().next()?;
            match c {
                ' ' | '\t' => pos += c.len_utf8(),
                '\n' | '\r' => {
                    newlines += 1;
                    if newlines >= 2 {
                        return None;
                    }
                    if c == '\r' && self.input[pos..].starts_with("\r\n") {
                        pos += 2;
                    } else {
                        pos += c.len_utf8();
                    }
                }
                '%' => {
                    pos += self.input[pos..]
                        .find(['\n', '\r'])
                        .unwrap_or(self.input.len() - pos)
                }
                '}' => return None,
                '\\' if matches!(self.control_word_at(pos), Some("left" | "right" | "end")) => {
                    return None;
                }
                '\\' if Self::is_host_math_close(&self.input[pos..]) => return None,
                '\\' => {
                    return Some(
                        pos + self.control_word_len_at(pos).unwrap_or_else(|| {
                            self.input[pos..]
                                .chars()
                                .nth(1)
                                .map_or(1, |next| 1 + next.len_utf8())
                        }),
                    );
                }
                _ => return Some(pos + c.len_utf8()),
            }
        }
    }

    fn control_word_at(&self, pos: usize) -> Option<&str> {
        let len = self.control_word_len_at(pos)?;
        Some(&self.input[pos + 1..pos + len])
    }

    fn is_host_math_close(rest: &str) -> bool {
        rest.starts_with(r"\]") || rest.starts_with(r"\)")
    }

    /// Parse a control word as a `MATH_COMMAND` node owning its arguments.
    ///
    /// Argument attachment is arity-blind, matching Badness: every trailing
    /// `{…}` group and tight, closed `[…]` optional is attached, no matter the
    /// command. Signatures decide only
    /// how an argument body is *interpreted*, never how many groups attach, so
    /// unknown commands behave exactly like known zero-argument ones.
    fn parse_command(&mut self) {
        let brackets_forbidden = is_big_delimiter_command(self.peek_control_word());
        self.builder.start_node(SyntaxKind::MATH_COMMAND.into());
        self.parse_control_word();
        if self.pos < self.input.len()
            && self.rest().starts_with('*')
            && self.word_len() == 1
            && self.argument_opener_after_trivia(self.pos + 1).is_some()
        {
            // A star variant folds into the command only when it directly
            // abuts and an argument follows (`\operatorname*{…}`); a bare
            // `\pi*r` keeps its `*` outside as ordinary content.
            self.bump_bytes(1, SyntaxKind::MATH_WORD);
        }
        loop {
            if !brackets_forbidden
                && self.peek_char() == Some('[')
                && self.optional_close_from(self.pos).is_some()
            {
                self.parse_optional();
                continue;
            }
            let Some((argument, '{')) = self.argument_opener_after_trivia(self.pos) else {
                break;
            };
            self.parse_attachment_trivia_until(argument);
            self.parse_group();
        }
        self.builder.finish_node();
    }

    /// Find the start of a directly attachable `{…}` argument: past spaces,
    /// newlines, and comments, but never across a blank line (two newlines with
    /// nothing but layout between them — a comment occupies its line and resets
    /// the newline run without forgiving an earlier blank).
    fn argument_opener_after_trivia(&self, mut pos: usize) -> Option<(usize, char)> {
        let mut newline_run = 0usize;
        let mut saw_blank_line = false;
        loop {
            let rest = &self.input[pos..];
            match rest.chars().next() {
                Some(' ' | '\t') => pos += 1,
                Some('\n') => {
                    pos += 1;
                    newline_run += 1;
                    saw_blank_line |= newline_run >= 2;
                }
                Some('\r') => {
                    pos += if rest.starts_with("\r\n") { 2 } else { 1 };
                    newline_run += 1;
                    saw_blank_line |= newline_run >= 2;
                }
                Some('%') => {
                    pos += rest.find(['\n', '\r']).unwrap_or(rest.len());
                    newline_run = 0;
                }
                Some(opener @ ('{' | '[')) if !saw_blank_line => return Some((pos, opener)),
                _ => return None,
            }
        }
    }

    /// Find the `]` that would close an optional beginning at `open` without
    /// crossing a math/recovery boundary. Braces are opaque, and optionals
    /// tightly attached to nested commands or line breaks claim their own
    /// closing bracket first.
    fn optional_close_from(&self, open: usize) -> Option<usize> {
        debug_assert!(self.input[open..].starts_with('['));
        let mut pos = open + 1;
        let mut brace_depth = 0usize;
        let mut newline_run = 0usize;

        while pos < self.input.len() {
            let rest = &self.input[pos..];
            let c = rest.chars().next()?;
            match c {
                '{' => {
                    brace_depth += 1;
                    newline_run = 0;
                    pos += 1;
                }
                '}' if brace_depth > 0 => {
                    brace_depth -= 1;
                    newline_run = 0;
                    pos += 1;
                }
                '}' => return None,
                ']' if brace_depth == 0 => return Some(pos),
                '%' => {
                    pos += rest.find(['\n', '\r']).unwrap_or(rest.len());
                }
                '\n' | '\r' if brace_depth == 0 => {
                    pos += if c == '\r' && rest.starts_with("\r\n") {
                        2
                    } else {
                        1
                    };
                    newline_run += 1;
                    if newline_run >= 2 {
                        return None;
                    }
                }
                ' ' | '\t' if brace_depth == 0 => pos += 1,
                '\\' => {
                    newline_run = 0;
                    if rest.starts_with("\\\\") {
                        pos += 2;
                        if self.input[pos..].starts_with('*') && self.word_len_at(pos) == 1 {
                            pos += 1;
                        }
                        if self.input[pos..].starts_with('[') {
                            pos = self.optional_close_from(pos)? + 1;
                        }
                        continue;
                    }

                    let Some(after) = rest.strip_prefix('\\') else {
                        unreachable!()
                    };
                    let word_len = after
                        .bytes()
                        .take_while(|b| b.is_ascii_alphabetic() || *b == b'@')
                        .count();
                    if word_len == 0 {
                        pos += 1 + after.chars().next().map(char::len_utf8).unwrap_or(0);
                        continue;
                    }
                    let name = &after[..word_len];
                    if brace_depth == 0 && matches!(name, "begin" | "end") {
                        return None;
                    }
                    pos += 1 + word_len;
                    if brace_depth > 0 {
                        continue;
                    }
                    pos = self.command_tail_end(pos, is_big_delimiter_command(Some(name)))?;
                }
                _ => {
                    newline_run = 0;
                    pos += c.len_utf8();
                }
            }
        }
        None
    }

    /// Scan the arguments greedily owned by a nested command while deciding
    /// which `]` closes an enclosing optional. This mirrors `parse_command`
    /// without emitting or interpreting argument contents.
    fn command_tail_end(&self, mut pos: usize, brackets_forbidden: bool) -> Option<usize> {
        if self.input[pos..].starts_with('*')
            && self.word_len_at(pos) == 1
            && self.argument_opener_after_trivia(pos + 1).is_some()
        {
            pos += 1;
        }
        loop {
            if !brackets_forbidden && self.input[pos..].starts_with('[') {
                pos = self.optional_close_from(pos)? + 1;
                continue;
            }
            let Some((open, '{')) = self.argument_opener_after_trivia(pos) else {
                break;
            };
            pos = self.group_end_from(open)?;
        }
        Some(pos)
    }

    /// Return the byte after a balanced brace group, treating control symbols
    /// and comments as opaque so escaped braces do not affect nesting.
    fn group_end_from(&self, open: usize) -> Option<usize> {
        debug_assert!(self.input[open..].starts_with('{'));
        let mut pos = open + 1;
        let mut depth = 1usize;
        while pos < self.input.len() {
            let rest = &self.input[pos..];
            let c = rest.chars().next()?;
            match c {
                '{' => {
                    depth += 1;
                    pos += 1;
                }
                '}' => {
                    depth -= 1;
                    pos += 1;
                    if depth == 0 {
                        return Some(pos);
                    }
                }
                '%' => pos += rest.find(['\n', '\r']).unwrap_or(rest.len()),
                '\\' => {
                    let after = &rest[1..];
                    let word_len = after
                        .bytes()
                        .take_while(|b| b.is_ascii_alphabetic() || *b == b'@')
                        .count();
                    pos += if word_len > 0 {
                        1 + word_len
                    } else {
                        1 + after.chars().next().map(char::len_utf8).unwrap_or(0)
                    };
                }
                _ => pos += c.len_utf8(),
            }
        }
        None
    }

    /// Emit the trivia between a command and an attached argument inside the
    /// command node.
    fn parse_attachment_trivia_until(&mut self, end: usize) {
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
                Some('%') => self.parse_comment(),
                _ => unreachable!("attachment lookahead may only skip trivia"),
            }
        }
    }

    fn parse_line_break(&mut self) {
        self.builder.start_node(SyntaxKind::MATH_LINE_BREAK.into());
        self.bump_bytes(2, SyntaxKind::MATH_CONTROL_SYMBOL);
        if self.peek_char() == Some('*') && self.word_len() == 1 {
            self.bump_bytes(1, SyntaxKind::MATH_WORD);
        }
        if self.peek_char() == Some('[') {
            self.parse_optional();
        }
        self.builder.finish_node();
    }

    fn parse_optional(&mut self) {
        self.builder.start_node(SyntaxKind::MATH_OPTIONAL.into());
        self.bump_bytes(1, SyntaxKind::MATH_BRACKET_OPEN);
        self.parse_elements(Ctx::Optional);
        if self.peek_char() == Some(']') {
            self.bump_bytes(1, SyntaxKind::MATH_BRACKET_CLOSE);
        }
        self.builder.finish_node();
    }

    fn parse_control_word(&mut self) {
        let word_len = self.peek_control_word().map(str::len).unwrap_or(0);
        self.bump_bytes(1 + word_len, SyntaxKind::MATH_CONTROL_WORD);
    }

    fn parse_control_symbol(&mut self) {
        let after = &self.input[self.pos + 1..];
        let len = 1 + after.chars().next().map(char::len_utf8).unwrap_or(0);
        self.bump_bytes(len, SyntaxKind::MATH_CONTROL_SYMBOL);
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
        self.word_len_at(self.pos)
    }

    fn word_len_at(&self, pos: usize) -> usize {
        self.input[pos..]
            .char_indices()
            .find_map(|(offset, c)| {
                let structural = is_structural(c);
                let host_label = self.opts.bookdown_equation_labels
                    && c == '('
                    && try_parse_bookdown_equation_definition(&self.input[pos + offset..])
                        .is_some();
                (structural || host_label).then_some(offset)
            })
            .unwrap_or_else(|| self.input[pos..].len())
    }

    fn equation_label_len(&self) -> Option<usize> {
        try_parse_bookdown_equation_definition(self.rest()).map(|(len, _)| len)
    }
}

fn is_structural(c: char) -> bool {
    matches!(
        c,
        '\\' | '{' | '}' | '[' | ']' | '&' | '^' | '_' | '%' | ' ' | '\t' | '\n' | '\r'
    )
}

fn is_big_delimiter_command(name: Option<&str>) -> bool {
    let Some(name) = name else { return false };
    ["bigg", "Bigg", "big", "Big"].iter().any(|prefix| {
        name.strip_prefix(prefix)
            .is_some_and(|suffix| matches!(suffix, "" | "l" | "m" | "r"))
    })
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
        for content in ["P(X", "M(t)", "i=1", "a+b"] {
            assert_eq!(
                token_kinds(content),
                vec![SyntaxKind::MATH_WORD],
                "word run: {content:?}"
            );
            assert_lossless(content);
        }
    }

    #[test]
    fn brackets_have_badness_lexical_grain_while_punctuation_stays_in_words() {
        assert_eq!(
            token_kinds("[a,b);"),
            vec![SyntaxKind::MATH_BRACKET_OPEN, SyntaxKind::MATH_WORD]
        );
        assert_lossless("[a,b);");
        assert_eq!(token_kinds("a|b.c/d"), vec![SyntaxKind::MATH_WORD]);
        assert_lossless("a|b.c/d");
        assert_eq!(
            token_kinds(r"\(\)\[\]"),
            vec![SyntaxKind::MATH_CONTROL_SYMBOL; 4]
        );
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
        assert_eq!(token_kinds(r"\<"), vec![SyntaxKind::MATH_CONTROL_SYMBOL]);
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
            vec![
                SyntaxKind::MATH_CONTROL_WORD,
                SyntaxKind::MATH_CONTROL_SYMBOL
            ]
        );
        assert_lossless(r"\alpha\,");
        assert_eq!(
            token_kinds(r"\&\%\{\}"),
            vec![SyntaxKind::MATH_CONTROL_SYMBOL; 4]
        );
        assert_lossless(r"\&\%\{\}");
    }

    /// A control word is always a `MATH_COMMAND` node wrapping its
    /// `MATH_CONTROL_WORD` token — even with zero arguments — while control
    /// symbols stay bare tokens, matching Badness's lexical model.
    #[test]
    fn control_words_wrap_in_command_nodes_and_symbols_stay_bare() {
        let tree = node(r"\alpha\,");
        let kinds: Vec<_> = tree.children_with_tokens().map(|el| el.kind()).collect();
        assert_eq!(
            kinds,
            vec![SyntaxKind::MATH_COMMAND, SyntaxKind::MATH_CONTROL_SYMBOL]
        );
        let command = tree.children().next().expect("command node");
        assert_eq!(command.text().to_string(), r"\alpha");
    }

    /// Argument attachment is arity-blind: every trailing brace group belongs
    /// to the command, known or unknown, and single-token "arguments" do not.
    #[test]
    fn commands_own_their_trailing_brace_groups() {
        let tree = node(r"\frac{\partial f}{\partial x} = 2x");
        let command = tree.children().next().expect("command node");
        assert_eq!(command.kind(), SyntaxKind::MATH_COMMAND);
        assert_eq!(command.text().to_string(), r"\frac{\partial f}{\partial x}");
        assert_eq!(
            command
                .children()
                .filter(|child| child.kind() == SyntaxKind::MATH_GROUP)
                .count(),
            2
        );
        assert_lossless(r"\frac{\partial f}{\partial x} = 2x");

        let unknown = node(r"\unknowncmd{a}{b}");
        let command = unknown.children().next().expect("command node");
        assert_eq!(command.text().to_string(), r"\unknowncmd{a}{b}");
    }

    #[test]
    fn single_token_arguments_stay_outside_the_command() {
        let tree = node(r"\frac12");
        let command = tree.children().next().expect("command node");
        assert_eq!(command.text().to_string(), r"\frac");
        assert_lossless(r"\frac12");

        let commands: Vec<String> = node(r"\frac\alpha\beta")
            .children()
            .filter(|child| child.kind() == SyntaxKind::MATH_COMMAND)
            .map(|child| child.text().to_string())
            .collect();
        assert_eq!(commands, vec![r"\frac", r"\alpha", r"\beta"]);
        assert_lossless(r"\frac\alpha\beta");
    }

    /// Attachment crosses spaces, newlines, and comments (which ride inside
    /// the command node), but never a blank line (which stays outside).
    #[test]
    fn argument_attachment_crosses_trivia_but_not_blank_lines() {
        for content in ["\\frac {1}{2}", "\\frac\n{1}{2}", "\\frac% c\n{1}{2}"] {
            let tree = node(content);
            let command = tree.children().next().expect("command node");
            assert_eq!(command.text().to_string(), content, "attach: {content:?}");
            assert_lossless(content);
        }

        let tree = node("\\alpha\n\n{a}");
        let command = tree.children().next().expect("command node");
        assert_eq!(command.text().to_string(), r"\alpha");
        assert!(
            tree.children()
                .any(|child| child.kind() == SyntaxKind::MATH_GROUP),
            "the group stays a sibling across a blank line"
        );
        assert_lossless("\\alpha\n\n{a}");
    }

    /// A star variant folds into the command only when it directly abuts and
    /// an argument follows; `\pi*r` keeps its `*` as ordinary content.
    #[test]
    fn star_variants_fold_into_the_command_only_before_arguments() {
        let tree = node(r"\operatorname*{min}");
        let command = tree.children().next().expect("command node");
        assert_eq!(command.text().to_string(), r"\operatorname*{min}");
        assert_lossless(r"\operatorname*{min}");

        let tree = node(r"\pi*r");
        let command = tree.children().next().expect("command node");
        assert_eq!(command.text().to_string(), r"\pi");
        assert_lossless(r"\pi*r");
    }

    #[test]
    fn commands_own_tight_optional_arguments_in_source_order() {
        let tree = node(r"\sqrt[3]{x}");
        let command = tree.children().next().expect("command node");
        assert_eq!(command.text().to_string(), r"\sqrt[3]{x}");
        assert_eq!(
            command
                .children_with_tokens()
                .map(|el| el.kind())
                .collect::<Vec<_>>(),
            vec![
                SyntaxKind::MATH_CONTROL_WORD,
                SyntaxKind::MATH_OPTIONAL,
                SyntaxKind::MATH_GROUP,
            ]
        );
        let optional = command
            .children()
            .find(|child| child.kind() == SyntaxKind::MATH_OPTIONAL)
            .expect("optional argument");
        assert_eq!(
            optional
                .children_with_tokens()
                .map(|el| el.kind())
                .collect::<Vec<_>>(),
            vec![
                SyntaxKind::MATH_BRACKET_OPEN,
                SyntaxKind::MATH_WORD,
                SyntaxKind::MATH_BRACKET_CLOSE,
            ]
        );
        assert_lossless(r"\sqrt[3]{x}");
    }

    #[test]
    fn optional_arguments_are_tight_closed_and_not_big_delimiters() {
        for content in [r"\sqrt [3]{x}", r"\sqrt[3{x}", r"\Big[x\Big]"] {
            let command = node(content).children().next().expect("command node");
            assert!(
                command
                    .children()
                    .all(|child| child.kind() != SyntaxKind::MATH_OPTIONAL),
                "must not attach: {content:?}"
            );
            assert_lossless(content);
        }

        let tree = node(r"\sqrt[n]{x}");
        assert!(
            tree.children()
                .next()
                .expect("command node")
                .children()
                .any(|child| child.kind() == SyntaxKind::MATH_OPTIONAL)
        );
    }

    #[test]
    fn optional_gate_accounts_for_nested_command_arguments() {
        let attached = node(r"\outer[\inner{x}[y] z]");
        let outer = attached.children().next().expect("outer command");
        assert_eq!(outer.text().to_string(), r"\outer[\inner{x}[y] z]");
        assert_lossless(r"\outer[\inner{x}[y] z]");

        let unclosed = node(r"\outer[\inner*[x]");
        let outer = unclosed.children().next().expect("outer command");
        assert!(
            outer
                .children()
                .all(|child| child.kind() != SyntaxKind::MATH_OPTIONAL),
            "the nested optional's `]` must not close the outer optional"
        );
        assert_lossless(r"\outer[\inner*[x]");
    }

    #[test]
    fn star_variants_fold_before_optional_arguments() {
        let tree = node(r"\inferrule*[right]{A}{B}");
        let command = tree.children().next().expect("command node");
        assert_eq!(command.text().to_string(), r"\inferrule*[right]{A}{B}");
        assert_lossless(r"\inferrule*[right]{A}{B}");
    }

    #[test]
    fn line_break_is_a_node_wrapping_its_control_symbol() {
        let tree = node(r"a \\ b");
        let line_break = tree
            .children()
            .find(|child| child.kind() == SyntaxKind::MATH_LINE_BREAK)
            .expect("line break node");
        assert_eq!(
            line_break
                .children_with_tokens()
                .map(|el| el.kind())
                .collect::<Vec<_>>(),
            vec![SyntaxKind::MATH_CONTROL_SYMBOL]
        );
        assert_lossless(r"a \\ b");
    }

    #[test]
    fn line_break_owns_only_tight_star_and_optional_modifiers() {
        for (content, expected) in [
            (r"\\*", r"\\*"),
            (r"\\[2ex]", r"\\[2ex]"),
            (r"\\*[2ex]", r"\\*[2ex]"),
            (r"\\*foo", r"\\"),
            (r"\\ [2ex]", r"\\"),
        ] {
            let tree = node(content);
            let line_break = tree
                .children()
                .find(|child| child.kind() == SyntaxKind::MATH_LINE_BREAK)
                .expect("line break node");
            assert_eq!(
                line_break.text().to_string(),
                expected,
                "shape: {content:?}"
            );
            assert_lossless(content);
        }
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
                SyntaxKind::MATH_WORD,           // x
                SyntaxKind::MATH_SPACE,          // ' '
                SyntaxKind::MATH_ALIGN,          // &
                SyntaxKind::MATH_WORD,           // =
                SyntaxKind::MATH_SPACE,          // ' '
                SyntaxKind::MATH_WORD,           // 1
                SyntaxKind::MATH_SPACE,          // ' '
                SyntaxKind::MATH_CONTROL_SYMBOL, // \\ (inside MATH_LINE_BREAK)
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

    /// Scripts bind to the complete command atom, arguments included, because
    /// the base's extent is only known after greedy attachment (`\mathbb{R}^n`
    /// scripts the whole `\mathbb{R}`, not the trailing group).
    #[test]
    fn scripts_bind_to_the_command_with_its_arguments() {
        for (content, base) in [
            (r"\mathbb{R}^n", r"\mathbb{R}"),
            (r"\frac{a}{b}^2", r"\frac{a}{b}"),
        ] {
            let scripted = node(content)
                .children()
                .find(|child| child.kind() == SyntaxKind::MATH_SCRIPTED)
                .unwrap_or_else(|| panic!("scripted atom: {content:?}"));
            let first = scripted
                .children_with_tokens()
                .next()
                .expect("scripted base");
            assert_eq!(first.kind(), SyntaxKind::MATH_COMMAND, "base: {content:?}");
            assert_eq!(first.to_string(), base, "base extent: {content:?}");
            assert_lossless(content);
        }
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
        assert_eq!(
            env.children().map(|child| child.kind()).collect::<Vec<_>>(),
            vec![
                SyntaxKind::MATH_BEGIN,
                SyntaxKind::MATH_CONTENT,
                SyntaxKind::MATH_END,
            ]
        );
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
    fn mismatched_environment_end_unwinds_to_the_matching_level() {
        let content = r"\begin{a}\begin{b}x\end{a}";
        let tree = node(content);
        let envs = tree
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::MATH_ENVIRONMENT)
            .collect::<Vec<_>>();
        assert_eq!(envs.len(), 2);
        assert!(
            envs[0]
                .children()
                .any(|child| child.kind() == SyntaxKind::MATH_END)
        );
        assert!(
            envs[1]
                .children()
                .all(|child| child.kind() != SyntaxKind::MATH_END)
        );
        assert_lossless(content);
    }

    #[test]
    fn stray_environment_end_is_a_structural_end_node() {
        let content = r"x \end{aligned}";
        assert_eq!(
            node(content)
                .children()
                .filter(|node| node.kind() == SyntaxKind::MATH_END)
                .count(),
            1
        );
        assert_lossless(content);
    }

    #[test]
    fn delimiter_like_environment_commands_require_a_static_name() {
        for content in [r"\begin x", r"\begin{\foo}x", "\\begin\n\n{aligned}x"] {
            assert!(
                node(content)
                    .descendants()
                    .all(|node| node.kind() != SyntaxKind::MATH_ENVIRONMENT)
            );
            assert_lossless(content);
        }
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
            vec![SyntaxKind::MATH_WORD, SyntaxKind::MATH_CONTROL_SYMBOL]
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
            .filter(|t| t.kind() == SyntaxKind::MATH_CONTROL_WORD)
            .map(|t| t.text().to_string())
            .collect();
        assert_eq!(commands, vec![r"\left", r"\right"]);
        assert_eq!(
            delim
                .children()
                .map(|child| child.kind())
                .collect::<Vec<_>>(),
            vec![SyntaxKind::MATH_CONTENT]
        );
        assert_lossless(content);
    }

    #[test]
    fn left_right_delimiters_keep_their_token_kinds() {
        assert_eq!(
            token_kinds(r"\left(x\right)"),
            vec![
                SyntaxKind::MATH_CONTROL_WORD, // \left
                SyntaxKind::MATH_WORD,         // (
                SyntaxKind::MATH_WORD,         // x
                SyntaxKind::MATH_CONTROL_WORD, // \right
                SyntaxKind::MATH_WORD,         // )
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
    fn left_right_gate_observes_nested_pairs_and_scope_boundaries() {
        let nested = r"\left[ \left( a \right) \right]";
        assert_eq!(delimited_count(nested), 2);

        for content in [r"\left( { x \right) }", "\\left(\n\n x \\right)"] {
            assert_eq!(delimited_count(content), 0, "must not pair: {content:?}");
            assert_lossless(content);
        }
    }

    #[test]
    fn unclosed_and_stray_delimiters_stay_lossless() {
        assert_eq!(delimited_count(r"\left( x"), 0);
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
