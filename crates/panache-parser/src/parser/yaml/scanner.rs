//! Streaming, char-by-char YAML scanner (libyaml/PyYAML-style).
//!
//! Replaces the line-based `lexer.rs` once parity is reached. The plan
//! and resolved design decisions live in
//! `.claude/skills/yaml-shadow-expand/scanner-rewrite.md`.
//!
//! This is the step-1 scaffold: types and a `next_token` stub that
//! emits `StreamStart` then `StreamEnd`. Real scanning is added in
//! subsequent steps; until cutover, the line-based lexer remains the
//! live path.

// Scaffold for the staged rewrite: variants and fields below are
// consumed as steps 2–9 land. Remove this once the scanner has callers.
#![allow(dead_code)]

use std::collections::VecDeque;

use super::model::YamlDiagnostic;

/// Position in the input stream. Lines and columns are 0-indexed,
/// matching PyYAML / libyaml convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Mark {
    pub index: usize,
    pub line: usize,
    pub column: usize,
}

/// A simple-key candidate awaiting confirmation by a downstream `:`.
///
/// `token_number` records the non-trivia token count at the moment the
/// candidate was registered, so the parser can splice
/// `BlockMappingStart` / `FlowMappingStart` before the candidate when
/// the `:` arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SimpleKey {
    pub token_number: usize,
    pub required: bool,
    pub mark: Mark,
}

/// Scalar source style — folding/escape decoding lives in projection,
/// not here. Scanner emits the raw source span and tags the style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScalarStyle {
    Plain,
    SingleQuoted,
    DoubleQuoted,
    Literal,
    Folded,
}

/// Trivia preserved in the queue so the parser walks a single stream
/// rather than re-scanning the input for inter-token bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TriviaKind {
    Whitespace,
    Newline,
    Comment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenKind {
    StreamStart,
    StreamEnd,
    DocumentStart,
    DocumentEnd,
    Directive,
    BlockSequenceStart,
    BlockMappingStart,
    BlockEnd,
    FlowSequenceStart,
    FlowSequenceEnd,
    FlowMappingStart,
    FlowMappingEnd,
    BlockEntry,
    FlowEntry,
    Key,
    Value,
    Alias,
    Anchor,
    Tag,
    Scalar(ScalarStyle),
    Trivia(TriviaKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Token {
    pub kind: TokenKind,
    pub start: Mark,
    pub end: Mark,
}

#[derive(Debug)]
pub(crate) struct Scanner<'a> {
    input: &'a str,
    cursor: Mark,
    tokens: VecDeque<Token>,
    tokens_taken: usize,
    indent: i32,
    indent_stack: Vec<i32>,
    simple_keys: Vec<Option<SimpleKey>>,
    flow_level: usize,
    allow_simple_key: bool,
    diagnostics: Vec<YamlDiagnostic>,
    stream_start_emitted: bool,
    stream_end_emitted: bool,
}

impl<'a> Scanner<'a> {
    pub(crate) fn new(input: &'a str) -> Self {
        Self {
            input,
            cursor: Mark::default(),
            tokens: VecDeque::new(),
            tokens_taken: 0,
            indent: -1,
            indent_stack: Vec::new(),
            simple_keys: Vec::new(),
            flow_level: 0,
            allow_simple_key: true,
            diagnostics: Vec::new(),
            stream_start_emitted: false,
            stream_end_emitted: false,
        }
    }

    pub(crate) fn next_token(&mut self) -> Option<Token> {
        if !self.stream_start_emitted {
            self.stream_start_emitted = true;
            return Some(Token {
                kind: TokenKind::StreamStart,
                start: self.cursor,
                end: self.cursor,
            });
        }
        if self.tokens.is_empty() && !self.stream_end_emitted {
            self.fetch_more_tokens();
        }
        self.tokens.pop_front()
    }

    /// Drain any pending trivia and meaningful tokens into the queue.
    /// Steps 5+ extend this with flow/block indicators, anchors/tags,
    /// and scalars; today it handles trivia plus the column-0
    /// directives and document markers.
    fn fetch_more_tokens(&mut self) {
        self.scan_trivia();
        if self.at_eof() {
            self.fetch_stream_end();
            return;
        }
        // Document markers and directives only apply at column 0 in
        // block context. Flow context (inside `[]` / `{}`) ignores them.
        if self.flow_level == 0 && self.cursor.column == 0 {
            if self.check_document_indicator(b"---") {
                self.fetch_document_marker(TokenKind::DocumentStart);
                return;
            }
            if self.check_document_indicator(b"...") {
                self.fetch_document_marker(TokenKind::DocumentEnd);
                return;
            }
            if self.peek_char() == Some('%') {
                self.fetch_directive();
                return;
            }
        }
        match self.peek_char() {
            Some('[') => {
                self.fetch_flow_collection_start(TokenKind::FlowSequenceStart);
                return;
            }
            Some('{') => {
                self.fetch_flow_collection_start(TokenKind::FlowMappingStart);
                return;
            }
            Some(']') => {
                self.fetch_flow_collection_end(TokenKind::FlowSequenceEnd);
                return;
            }
            Some('}') => {
                self.fetch_flow_collection_end(TokenKind::FlowMappingEnd);
                return;
            }
            Some(',') if self.flow_level > 0 => {
                self.fetch_flow_entry();
                return;
            }
            _ => {}
        }
        // Step 5 placeholder: scalars, anchors/tags, and block
        // indicators land in steps 6–9. For now, terminate the stream
        // when we hit an unhandled meaningful char.
        self.fetch_stream_end();
    }

    fn fetch_flow_collection_start(&mut self, kind: TokenKind) {
        let start = self.cursor;
        self.advance();
        let end = self.cursor;
        self.flow_level += 1;
        // Reserve a simple-key slot for this flow nest. Step 6 wires
        // candidate registration; for now the slot stays None.
        self.simple_keys.push(None);
        self.tokens.push_back(Token { kind, start, end });
    }

    fn fetch_flow_collection_end(&mut self, kind: TokenKind) {
        let start = self.cursor;
        self.advance();
        let end = self.cursor;
        if self.flow_level > 0 {
            self.flow_level -= 1;
            self.simple_keys.pop();
        }
        self.tokens.push_back(Token { kind, start, end });
    }

    fn fetch_flow_entry(&mut self) {
        let start = self.cursor;
        self.advance();
        let end = self.cursor;
        self.tokens.push_back(Token {
            kind: TokenKind::FlowEntry,
            start,
            end,
        });
    }

    fn fetch_stream_end(&mut self) {
        if self.stream_end_emitted {
            return;
        }
        self.stream_end_emitted = true;
        let mark = self.cursor;
        self.tokens.push_back(Token {
            kind: TokenKind::StreamEnd,
            start: mark,
            end: mark,
        });
    }

    /// `---` / `...` are document markers only at column 0 followed by
    /// whitespace, newline, or end-of-input. `---abc` is a plain
    /// scalar, not a marker.
    fn check_document_indicator(&self, marker: &[u8; 3]) -> bool {
        let bytes = self.input.as_bytes();
        let i = self.cursor.index;
        if bytes.get(i..i + 3) != Some(marker.as_slice()) {
            return false;
        }
        matches!(bytes.get(i + 3), None | Some(b' ' | b'\t' | b'\n' | b'\r'))
    }

    fn fetch_document_marker(&mut self, kind: TokenKind) {
        let start = self.cursor;
        self.advance();
        self.advance();
        self.advance();
        let end = self.cursor;
        self.tokens.push_back(Token { kind, start, end });
    }

    /// A directive is `%name args` running to end-of-line. Trailing
    /// whitespace/comment/newline emit as separate trivia on the next
    /// fetch.
    fn fetch_directive(&mut self) {
        let start = self.cursor;
        debug_assert_eq!(self.peek_char(), Some('%'));
        self.advance();
        while let Some(c) = self.peek_char() {
            if c == '\n' || c == '\r' {
                break;
            }
            self.advance();
        }
        let end = self.cursor;
        self.tokens.push_back(Token {
            kind: TokenKind::Directive,
            start,
            end,
        });
    }

    /// Consume runs of whitespace, newlines, and comments, emitting
    /// one `Trivia` token per run. Stops at the first meaningful char
    /// or EOF.
    fn scan_trivia(&mut self) {
        while !self.at_eof() {
            match self.peek_char() {
                Some(' ' | '\t') => self.scan_whitespace_run(),
                Some('\n' | '\r') => self.scan_newline(),
                Some('#') => self.scan_comment(),
                _ => break,
            }
        }
    }

    fn scan_whitespace_run(&mut self) {
        let start = self.cursor;
        while matches!(self.peek_char(), Some(' ' | '\t')) {
            self.advance();
        }
        let end = self.cursor;
        self.tokens.push_back(Token {
            kind: TokenKind::Trivia(TriviaKind::Whitespace),
            start,
            end,
        });
    }

    fn scan_newline(&mut self) {
        let start = self.cursor;
        match self.peek_char() {
            Some('\n') => {
                self.advance();
            }
            Some('\r') => {
                self.advance();
                if self.peek_char() == Some('\n') {
                    self.advance();
                }
            }
            _ => unreachable!("scan_newline called on non-newline char"),
        }
        let end = self.cursor;
        self.tokens.push_back(Token {
            kind: TokenKind::Trivia(TriviaKind::Newline),
            start,
            end,
        });
    }

    fn scan_comment(&mut self) {
        let start = self.cursor;
        debug_assert_eq!(self.peek_char(), Some('#'));
        self.advance();
        while let Some(c) = self.peek_char() {
            if c == '\n' || c == '\r' {
                break;
            }
            self.advance();
        }
        let end = self.cursor;
        self.tokens.push_back(Token {
            kind: TokenKind::Trivia(TriviaKind::Comment),
            start,
            end,
        });
    }

    pub(crate) fn diagnostics(&self) -> &[YamlDiagnostic] {
        &self.diagnostics
    }

    pub(crate) fn cursor(&self) -> Mark {
        self.cursor
    }

    pub(crate) fn at_eof(&self) -> bool {
        self.cursor.index >= self.input.len()
    }

    fn remaining(&self) -> &str {
        &self.input[self.cursor.index..]
    }

    pub(crate) fn peek_char(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    /// Look ahead `offset` codepoints from the cursor. `offset == 0`
    /// returns the same as `peek_char`.
    pub(crate) fn peek_at(&self, offset: usize) -> Option<char> {
        self.remaining().chars().nth(offset)
    }

    /// Consume one codepoint and advance the cursor. Line/column
    /// tracking treats `\n`, `\r\n`, and lone `\r` each as one logical
    /// line break (YAML 1.2 §5.4).
    pub(crate) fn advance(&mut self) -> Option<char> {
        let c = self.peek_char()?;
        self.cursor.index += c.len_utf8();
        match c {
            '\n' => {
                self.cursor.line += 1;
                self.cursor.column = 0;
            }
            '\r' => {
                // CRLF: defer the line break to the following '\n' so
                // each byte updates the cursor exactly once. Lone '\r'
                // takes the line break itself.
                if self.peek_char() != Some('\n') {
                    self.cursor.line += 1;
                    self.cursor.column = 0;
                }
            }
            _ => {
                self.cursor.column += 1;
            }
        }
        Some(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_emits_stream_start_then_stream_end_on_empty_input() {
        let mut scanner = Scanner::new("");
        assert_eq!(
            scanner.next_token().map(|t| t.kind),
            Some(TokenKind::StreamStart)
        );
        assert_eq!(
            scanner.next_token().map(|t| t.kind),
            Some(TokenKind::StreamEnd)
        );
        assert_eq!(scanner.next_token(), None);
    }

    #[test]
    fn stub_emits_stream_markers_regardless_of_input_content() {
        let mut scanner = Scanner::new("foo: bar\n");
        assert_eq!(
            scanner.next_token().map(|t| t.kind),
            Some(TokenKind::StreamStart)
        );
        assert_eq!(
            scanner.next_token().map(|t| t.kind),
            Some(TokenKind::StreamEnd)
        );
        assert_eq!(scanner.next_token(), None);
    }

    #[test]
    fn stream_end_marks_cursor_position_after_trivia_only_input() {
        let input = "   \n";
        let mut scanner = Scanner::new(input);
        // StreamStart, Whitespace, Newline, StreamEnd
        let mut last = None;
        while let Some(tok) = scanner.next_token() {
            last = Some(tok);
        }
        let end = last.expect("stream end");
        assert_eq!(end.kind, TokenKind::StreamEnd);
        assert_eq!(end.start.index, input.len());
        assert_eq!(end.end.index, input.len());
    }

    #[test]
    fn diagnostics_start_empty() {
        let scanner = Scanner::new("");
        assert!(scanner.diagnostics().is_empty());
    }

    #[test]
    fn cursor_starts_at_origin() {
        let scanner = Scanner::new("anything");
        assert_eq!(
            scanner.cursor(),
            Mark {
                index: 0,
                line: 0,
                column: 0
            }
        );
    }

    #[test]
    fn at_eof_is_true_for_empty_input() {
        let scanner = Scanner::new("");
        assert!(scanner.at_eof());
        assert_eq!(scanner.peek_char(), None);
    }

    #[test]
    fn peek_does_not_advance_cursor() {
        let scanner = Scanner::new("abc");
        assert_eq!(scanner.peek_char(), Some('a'));
        assert_eq!(scanner.peek_at(1), Some('b'));
        assert_eq!(scanner.peek_at(2), Some('c'));
        assert_eq!(scanner.peek_at(3), None);
        assert_eq!(scanner.cursor().index, 0);
    }

    #[test]
    fn advance_moves_through_ascii_one_column_per_char() {
        let mut scanner = Scanner::new("abc");
        assert_eq!(scanner.advance(), Some('a'));
        assert_eq!(
            scanner.cursor(),
            Mark {
                index: 1,
                line: 0,
                column: 1
            }
        );
        assert_eq!(scanner.advance(), Some('b'));
        assert_eq!(
            scanner.cursor(),
            Mark {
                index: 2,
                line: 0,
                column: 2
            }
        );
        assert_eq!(scanner.advance(), Some('c'));
        assert_eq!(
            scanner.cursor(),
            Mark {
                index: 3,
                line: 0,
                column: 3
            }
        );
        assert_eq!(scanner.advance(), None);
        assert!(scanner.at_eof());
    }

    #[test]
    fn lf_increments_line_and_resets_column() {
        let mut scanner = Scanner::new("a\nb");
        scanner.advance(); // 'a'
        scanner.advance(); // '\n'
        assert_eq!(
            scanner.cursor(),
            Mark {
                index: 2,
                line: 1,
                column: 0
            }
        );
        scanner.advance(); // 'b'
        assert_eq!(
            scanner.cursor(),
            Mark {
                index: 3,
                line: 1,
                column: 1
            }
        );
    }

    #[test]
    fn crlf_counts_as_one_line_break() {
        let mut scanner = Scanner::new("a\r\nb");
        scanner.advance(); // 'a' → line 0, col 1
        scanner.advance(); // '\r' → line 0 (deferred), col 1, index 2
        assert_eq!(scanner.cursor().line, 0);
        assert_eq!(scanner.cursor().index, 2);
        scanner.advance(); // '\n' → line 1, col 0
        assert_eq!(
            scanner.cursor(),
            Mark {
                index: 3,
                line: 1,
                column: 0
            }
        );
        scanner.advance(); // 'b'
        assert_eq!(
            scanner.cursor(),
            Mark {
                index: 4,
                line: 1,
                column: 1
            }
        );
    }

    #[test]
    fn lone_cr_takes_its_own_line_break() {
        let mut scanner = Scanner::new("a\rb");
        scanner.advance(); // 'a'
        scanner.advance(); // '\r' (no following '\n')
        assert_eq!(
            scanner.cursor(),
            Mark {
                index: 2,
                line: 1,
                column: 0
            }
        );
        scanner.advance(); // 'b'
        assert_eq!(
            scanner.cursor(),
            Mark {
                index: 3,
                line: 1,
                column: 1
            }
        );
    }

    #[test]
    fn multibyte_utf8_advances_index_by_byte_length_and_column_by_one() {
        // 'é' is 2 bytes in UTF-8 (0xC3 0xA9), one codepoint.
        let mut scanner = Scanner::new("é!");
        scanner.advance();
        assert_eq!(
            scanner.cursor(),
            Mark {
                index: 2,
                line: 0,
                column: 1
            }
        );
        scanner.advance();
        assert_eq!(
            scanner.cursor(),
            Mark {
                index: 3,
                line: 0,
                column: 2
            }
        );
    }

    #[test]
    fn mixed_line_endings_track_correctly() {
        // LF, CRLF, lone CR — three logical breaks.
        let mut scanner = Scanner::new("a\nb\r\nc\rd");
        while scanner.advance().is_some() {}
        assert_eq!(scanner.cursor().line, 3);
        assert_eq!(scanner.cursor().column, 1);
        assert_eq!(scanner.cursor().index, 8);
    }

    fn collect_tokens(input: &str) -> Vec<Token> {
        let mut scanner = Scanner::new(input);
        let mut out = Vec::new();
        while let Some(tok) = scanner.next_token() {
            out.push(tok);
        }
        out
    }

    fn trivia_kinds(tokens: &[Token]) -> Vec<TriviaKind> {
        tokens
            .iter()
            .filter_map(|t| match t.kind {
                TokenKind::Trivia(k) => Some(k),
                _ => None,
            })
            .collect()
    }

    fn assert_byte_complete(input: &str, tokens: &[Token]) {
        // Synthetic StreamStart/StreamEnd carry zero-width spans; trivia
        // tokens between them must cover the full input contiguously.
        let mut cursor = 0usize;
        for tok in tokens {
            match tok.kind {
                TokenKind::StreamStart | TokenKind::StreamEnd => {
                    assert_eq!(tok.start.index, tok.end.index, "synthetic token has extent");
                }
                _ => {
                    assert_eq!(tok.start.index, cursor, "token starts at expected position");
                    assert!(tok.end.index >= tok.start.index);
                    cursor = tok.end.index;
                }
            }
        }
        assert_eq!(cursor, input.len(), "all bytes covered");
    }

    #[test]
    fn pure_whitespace_yields_one_whitespace_trivia_token() {
        let tokens = collect_tokens("   \t  ");
        assert_eq!(
            trivia_kinds(&tokens),
            vec![TriviaKind::Whitespace],
            "whitespace coalesces into a single run"
        );
        assert_byte_complete("   \t  ", &tokens);
    }

    #[test]
    fn newline_emits_one_newline_per_logical_break() {
        let input = "\n\r\n\r";
        let tokens = collect_tokens(input);
        assert_eq!(
            trivia_kinds(&tokens),
            vec![
                TriviaKind::Newline,
                TriviaKind::Newline,
                TriviaKind::Newline
            ],
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn comment_runs_to_end_of_line_excluding_break() {
        let input = "# hello\n# next\n";
        let tokens = collect_tokens(input);
        assert_eq!(
            trivia_kinds(&tokens),
            vec![
                TriviaKind::Comment,
                TriviaKind::Newline,
                TriviaKind::Comment,
                TriviaKind::Newline,
            ],
        );
        // First comment span equals "# hello".
        let comment_tok = tokens
            .iter()
            .find(|t| matches!(t.kind, TokenKind::Trivia(TriviaKind::Comment)))
            .unwrap();
        assert_eq!(
            &input[comment_tok.start.index..comment_tok.end.index],
            "# hello"
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn whitespace_then_comment_then_newline_separates_into_three_tokens() {
        let input = "   # comment\n";
        let tokens = collect_tokens(input);
        assert_eq!(
            trivia_kinds(&tokens),
            vec![
                TriviaKind::Whitespace,
                TriviaKind::Comment,
                TriviaKind::Newline
            ],
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn pure_trivia_input_round_trips_byte_complete() {
        // Mixed whitespace/newlines/comments with CRLF — the kind of
        // input we'll hit between meaningful tokens once the scanner
        // is wired up.
        let input = " \t# c1\r\n\n  # c2\n\r";
        let tokens = collect_tokens(input);
        assert_byte_complete(input, &tokens);
        assert!(matches!(
            tokens.last().map(|t| t.kind),
            Some(TokenKind::StreamEnd),
        ));
    }

    #[test]
    fn empty_input_emits_only_stream_markers() {
        let tokens = collect_tokens("");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::StreamStart);
        assert_eq!(tokens[1].kind, TokenKind::StreamEnd);
    }

    fn meaningful_kinds(tokens: &[Token]) -> Vec<TokenKind> {
        tokens
            .iter()
            .map(|t| t.kind)
            .filter(|k| !matches!(k, TokenKind::Trivia(_)))
            .collect()
    }

    #[test]
    fn document_start_marker_at_column_zero_emits_token() {
        let input = "---\n";
        let tokens = collect_tokens(input);
        assert_eq!(
            meaningful_kinds(&tokens),
            vec![
                TokenKind::StreamStart,
                TokenKind::DocumentStart,
                TokenKind::StreamEnd
            ],
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn document_end_marker_at_column_zero_emits_token() {
        let input = "...\n";
        let tokens = collect_tokens(input);
        assert_eq!(
            meaningful_kinds(&tokens),
            vec![
                TokenKind::StreamStart,
                TokenKind::DocumentEnd,
                TokenKind::StreamEnd
            ],
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn document_marker_at_eof_without_trailing_break_still_emits() {
        let input = "---";
        let tokens = collect_tokens(input);
        assert_eq!(
            meaningful_kinds(&tokens),
            vec![
                TokenKind::StreamStart,
                TokenKind::DocumentStart,
                TokenKind::StreamEnd
            ],
        );
    }

    #[test]
    fn three_dashes_followed_by_non_break_is_not_a_marker() {
        // `---abc` at col 0 is a plain scalar starter, not a marker.
        // Step 4 doesn't yet emit scalars, so this terminates after
        // StreamStart with no DocumentStart token.
        let tokens = collect_tokens("---abc\n");
        let kinds = meaningful_kinds(&tokens);
        assert!(!kinds.contains(&TokenKind::DocumentStart), "got {kinds:?}",);
    }

    #[test]
    fn three_dashes_indented_is_not_a_marker() {
        // ` ---` at col 1 is not a doc marker.
        let tokens = collect_tokens(" ---\n");
        let kinds = meaningful_kinds(&tokens);
        assert!(!kinds.contains(&TokenKind::DocumentStart), "got {kinds:?}",);
    }

    #[test]
    fn directive_at_column_zero_emits_directive_token() {
        let input = "%YAML 1.2\n";
        let tokens = collect_tokens(input);
        let directive = tokens
            .iter()
            .find(|t| matches!(t.kind, TokenKind::Directive))
            .expect("directive token");
        assert_eq!(
            &input[directive.start.index..directive.end.index],
            "%YAML 1.2",
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn directive_indented_is_not_recognized() {
        // Directives MUST be at column 0; ` %YAML 1.2` is not a directive.
        let tokens = collect_tokens(" %YAML 1.2\n");
        let kinds = meaningful_kinds(&tokens);
        assert!(!kinds.contains(&TokenKind::Directive), "got {kinds:?}",);
    }

    #[test]
    fn document_start_then_marker_on_new_line() {
        // Two markers separated by a newline: both detected.
        let input = "---\n...\n";
        let tokens = collect_tokens(input);
        assert_eq!(
            meaningful_kinds(&tokens),
            vec![
                TokenKind::StreamStart,
                TokenKind::DocumentStart,
                TokenKind::DocumentEnd,
                TokenKind::StreamEnd,
            ],
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn directive_followed_by_doc_start_emits_both_in_order() {
        let input = "%YAML 1.2\n---\n";
        let tokens = collect_tokens(input);
        assert_eq!(
            meaningful_kinds(&tokens),
            vec![
                TokenKind::StreamStart,
                TokenKind::Directive,
                TokenKind::DocumentStart,
                TokenKind::StreamEnd,
            ],
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn document_marker_followed_by_space_terminates_at_step_4_placeholder() {
        // `--- ` is a valid DocumentStart followed by content; content
        // tokens land in steps 5+. For now verify the marker is
        // recognized (and that " " is consumed as trivia).
        let input = "--- foo\n";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        assert_eq!(kinds[0], TokenKind::StreamStart);
        assert_eq!(kinds[1], TokenKind::DocumentStart);
        // After DocumentStart, " " is whitespace trivia; "foo" is
        // unsupported in step 4, so the scanner terminates with
        // StreamEnd before consuming it.
        assert_eq!(*kinds.last().unwrap(), TokenKind::StreamEnd);
    }

    #[test]
    fn empty_flow_sequence_emits_start_then_end() {
        let input = "[]";
        let tokens = collect_tokens(input);
        assert_eq!(
            meaningful_kinds(&tokens),
            vec![
                TokenKind::StreamStart,
                TokenKind::FlowSequenceStart,
                TokenKind::FlowSequenceEnd,
                TokenKind::StreamEnd,
            ],
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn empty_flow_mapping_emits_start_then_end() {
        let input = "{}";
        let tokens = collect_tokens(input);
        assert_eq!(
            meaningful_kinds(&tokens),
            vec![
                TokenKind::StreamStart,
                TokenKind::FlowMappingStart,
                TokenKind::FlowMappingEnd,
                TokenKind::StreamEnd,
            ],
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn nested_flow_sequence_brackets_emit_in_order() {
        let input = "[[]]";
        let tokens = collect_tokens(input);
        assert_eq!(
            meaningful_kinds(&tokens),
            vec![
                TokenKind::StreamStart,
                TokenKind::FlowSequenceStart,
                TokenKind::FlowSequenceStart,
                TokenKind::FlowSequenceEnd,
                TokenKind::FlowSequenceEnd,
                TokenKind::StreamEnd,
            ],
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn nested_flow_mixed_brackets_emit_in_order() {
        let input = "[{}]";
        let tokens = collect_tokens(input);
        assert_eq!(
            meaningful_kinds(&tokens),
            vec![
                TokenKind::StreamStart,
                TokenKind::FlowSequenceStart,
                TokenKind::FlowMappingStart,
                TokenKind::FlowMappingEnd,
                TokenKind::FlowSequenceEnd,
                TokenKind::StreamEnd,
            ],
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn comma_inside_flow_emits_flow_entry() {
        let input = "[,,]";
        let tokens = collect_tokens(input);
        assert_eq!(
            meaningful_kinds(&tokens),
            vec![
                TokenKind::StreamStart,
                TokenKind::FlowSequenceStart,
                TokenKind::FlowEntry,
                TokenKind::FlowEntry,
                TokenKind::FlowSequenceEnd,
                TokenKind::StreamEnd,
            ],
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn comma_outside_flow_terminates_at_placeholder() {
        // Outside flow context, `,` isn't a recognized indicator. The
        // step-5 placeholder stops at it.
        let tokens = collect_tokens(",");
        let kinds = meaningful_kinds(&tokens);
        assert!(!kinds.contains(&TokenKind::FlowEntry), "got {kinds:?}");
    }

    #[test]
    fn doc_markers_inside_flow_context_are_not_recognized() {
        // `[---]` — the `---` inside flow context is plain text, not a
        // doc marker. Even though step 5 doesn't yet emit scalars, we
        // can verify no DocumentStart was produced before the placeholder.
        let tokens = collect_tokens("[---]");
        let kinds = meaningful_kinds(&tokens);
        assert!(!kinds.contains(&TokenKind::DocumentStart), "got {kinds:?}");
        assert_eq!(kinds[1], TokenKind::FlowSequenceStart);
    }

    #[test]
    fn flow_brackets_with_whitespace_emit_trivia_between() {
        let input = "[ , ]";
        let tokens = collect_tokens(input);
        // FlowSequenceStart, Whitespace, FlowEntry, Whitespace, FlowSequenceEnd.
        assert_eq!(
            tokens
                .iter()
                .map(|t| t.kind)
                .filter(|k| !matches!(k, TokenKind::StreamStart | TokenKind::StreamEnd))
                .collect::<Vec<_>>(),
            vec![
                TokenKind::FlowSequenceStart,
                TokenKind::Trivia(TriviaKind::Whitespace),
                TokenKind::FlowEntry,
                TokenKind::Trivia(TriviaKind::Whitespace),
                TokenKind::FlowSequenceEnd,
            ],
        );
        assert_byte_complete(input, &tokens);
    }
}
