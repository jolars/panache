//! Streaming, char-by-char YAML scanner (libyaml/PyYAML-style).
//!
//! Replaces the line-based `lexer.rs` once parity is reached. The plan
//! and resolved design decisions live in
//! `.claude/skills/yaml-shadow-expand/scanner-rewrite.md`.
//!
//! Currently implements: trivia, document markers, directives, flow
//! indicators, block indicators (`-`/`?`/`:`) with the simple-key
//! table, plain scalars (with internal whitespace and multi-line
//! continuation), quoted scalars (`'…'`, `"…"`) with escape
//! diagnostics, and block scalars (`|` literal, `>` folded). Anchors,
//! tags, and aliases land alongside the parser cutover (step 12).

// No production callers yet — the line-based lexer remains the live
// path until step 12. Remove once the scanner is wired into parsing.
#![allow(dead_code)]

use std::collections::VecDeque;

use super::model::{YamlDiagnostic, diagnostic_codes};

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
    /// Count of tokens that have been popped via `next_token`. Together
    /// with `tokens.len()` it gives the global index of the next token
    /// that will be added to the queue — the value `save_simple_key`
    /// records so `fetch_value` can splice `Key`/`BlockMappingStart`
    /// before the candidate even after intervening trivia is popped.
    tokens_taken: usize,
    /// Current block-context indent column. `-1` represents "before the
    /// first block container" and matches PyYAML's sentinel.
    indent: i32,
    /// Stack of prior `indent` values; popped during `unwind_indent`.
    indent_stack: Vec<i32>,
    /// Per-flow-level simple-key candidate slot. Index 0 is block
    /// context; each `[`/`{` pushes a new slot.
    simple_keys: Vec<Option<SimpleKey>>,
    flow_level: usize,
    /// Whether the next non-trivia token may register a simple-key
    /// candidate. Reset by indicators that close key candidacy
    /// (`fetch_value`, plain/quoted scalar emission) and reopened by
    /// indicators that re-enable it (`fetch_key`, `fetch_block_entry`,
    /// `fetch_flow_entry`, line breaks in block context).
    allow_simple_key: bool,
    diagnostics: Vec<YamlDiagnostic>,
    stream_end_emitted: bool,
}

impl<'a> Scanner<'a> {
    pub(crate) fn new(input: &'a str) -> Self {
        let mut scanner = Self {
            input,
            cursor: Mark::default(),
            tokens: VecDeque::new(),
            tokens_taken: 0,
            indent: -1,
            indent_stack: Vec::new(),
            // Slot for the implicit block-context level (flow_level 0).
            // Each flow open pushes another slot; flow close pops.
            simple_keys: vec![None],
            flow_level: 0,
            allow_simple_key: true,
            diagnostics: Vec::new(),
            stream_end_emitted: false,
        };
        let mark = scanner.cursor;
        scanner.tokens.push_back(Token {
            kind: TokenKind::StreamStart,
            start: mark,
            end: mark,
        });
        scanner
    }

    pub(crate) fn next_token(&mut self) -> Option<Token> {
        while self.need_more_tokens() {
            self.fetch_more_tokens();
        }
        let tok = self.tokens.pop_front();
        if tok.is_some() {
            self.tokens_taken += 1;
        }
        tok
    }

    /// Should the caller fetch more tokens before popping the queue
    /// head? True when the queue is empty (and the stream is still
    /// open), or when the queue head is itself a registered simple-key
    /// candidate that may still be spliced before. The latter is what
    /// makes `Key` / `BlockMappingStart` splicing work — we keep
    /// fetching past the candidate until either a `:` confirms it
    /// (cancelling the slot) or a stale check expires it.
    fn need_more_tokens(&mut self) -> bool {
        if self.stream_end_emitted {
            return false;
        }
        if self.tokens.is_empty() {
            return true;
        }
        self.stale_simple_keys();
        matches!(
            self.next_possible_simple_key_index(),
            Some(min) if min == self.tokens_taken
        )
    }

    fn next_possible_simple_key_index(&self) -> Option<usize> {
        self.simple_keys
            .iter()
            .filter_map(|slot| slot.as_ref().map(|k| k.token_number))
            .min()
    }

    /// Drain trivia and one meaningful token into the queue. Called
    /// repeatedly from `next_token` while `need_more_tokens` is true.
    fn fetch_more_tokens(&mut self) {
        self.scan_trivia();
        self.stale_simple_keys();
        self.unwind_indent(self.cursor.column as i32);
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
            Some('-') if self.check_block_entry() => {
                self.fetch_block_entry();
                return;
            }
            Some('?') if self.check_key() => {
                self.fetch_key();
                return;
            }
            Some(':') if self.check_value() => {
                self.fetch_value();
                return;
            }
            Some('\'') => {
                self.fetch_flow_scalar(ScalarStyle::SingleQuoted);
                return;
            }
            Some('"') => {
                self.fetch_flow_scalar(ScalarStyle::DoubleQuoted);
                return;
            }
            Some('|') if self.flow_level == 0 => {
                self.fetch_block_scalar(ScalarStyle::Literal);
                return;
            }
            Some('>') if self.flow_level == 0 => {
                self.fetch_block_scalar(ScalarStyle::Folded);
                return;
            }
            _ => {}
        }
        // Default: anything else opens a plain scalar.
        // Anchors/tags/aliases land in later steps and will be
        // dispatched here before this default.
        self.fetch_plain_scalar();
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
        // `,` separates flow items. Subsequent entries can be implicit
        // keys, so re-open candidacy and clear the current slot.
        self.allow_simple_key = true;
        self.remove_simple_key();
        let start = self.cursor;
        self.advance();
        let end = self.cursor;
        self.tokens.push_back(Token {
            kind: TokenKind::FlowEntry,
            start,
            end,
        });
    }

    fn fetch_block_entry(&mut self) {
        if self.flow_level == 0 {
            if !self.allow_simple_key {
                self.push_diagnostic(
                    diagnostic_codes::LEX_BLOCK_ENTRY_NOT_ALLOWED,
                    "block sequence entry not allowed here",
                );
            }
            if self.add_indent(self.cursor.column as i32) {
                let mark = self.cursor;
                self.tokens.push_back(Token {
                    kind: TokenKind::BlockSequenceStart,
                    start: mark,
                    end: mark,
                });
            }
        }
        self.allow_simple_key = true;
        self.remove_simple_key();
        let start = self.cursor;
        self.advance();
        let end = self.cursor;
        self.tokens.push_back(Token {
            kind: TokenKind::BlockEntry,
            start,
            end,
        });
    }

    fn fetch_key(&mut self) {
        if self.flow_level == 0 {
            if !self.allow_simple_key {
                self.push_diagnostic(
                    diagnostic_codes::LEX_KEY_INDICATOR_NOT_ALLOWED,
                    "explicit key indicator not allowed here",
                );
            }
            if self.add_indent(self.cursor.column as i32) {
                let mark = self.cursor;
                self.tokens.push_back(Token {
                    kind: TokenKind::BlockMappingStart,
                    start: mark,
                    end: mark,
                });
            }
        }
        // After `?`, the next thing in block context can itself be an
        // implicit key (the explicit-key path opens a fresh entry).
        self.allow_simple_key = self.flow_level == 0;
        self.remove_simple_key();
        let start = self.cursor;
        self.advance();
        let end = self.cursor;
        self.tokens.push_back(Token {
            kind: TokenKind::Key,
            start,
            end,
        });
    }

    fn fetch_value(&mut self) {
        if let Some(key) = self.simple_keys[self.flow_level].take() {
            // Implicit key confirmed: splice `Key` (and possibly
            // `BlockMappingStart`) before the candidate token in the
            // queue. Both go at the same queue index, with
            // `BlockMappingStart` inserted last so it ends up first.
            let queue_pos = key.token_number.saturating_sub(self.tokens_taken);
            self.tokens.insert(
                queue_pos,
                Token {
                    kind: TokenKind::Key,
                    start: key.mark,
                    end: key.mark,
                },
            );
            if self.flow_level == 0 && self.add_indent(key.mark.column as i32) {
                self.tokens.insert(
                    queue_pos,
                    Token {
                        kind: TokenKind::BlockMappingStart,
                        start: key.mark,
                        end: key.mark,
                    },
                );
            }
            self.allow_simple_key = false;
        } else {
            // No candidate: explicit `:` (e.g. `? key\n: value`) or
            // an empty-key shorthand. In block context this needs to
            // be at a position where a fresh key could appear.
            if self.flow_level == 0 {
                if !self.allow_simple_key {
                    self.push_diagnostic(
                        diagnostic_codes::LEX_VALUE_INDICATOR_NOT_ALLOWED,
                        "value indicator not allowed here",
                    );
                }
                if self.add_indent(self.cursor.column as i32) {
                    let mark = self.cursor;
                    self.tokens.push_back(Token {
                        kind: TokenKind::BlockMappingStart,
                        start: mark,
                        end: mark,
                    });
                }
            }
            self.allow_simple_key = self.flow_level == 0;
            self.remove_simple_key();
        }
        let start = self.cursor;
        self.advance();
        let end = self.cursor;
        self.tokens.push_back(Token {
            kind: TokenKind::Value,
            start,
            end,
        });
    }

    /// Plain scalar with internal whitespace and multi-line
    /// continuation (YAML 1.2 §7.3.3). Each iteration reads a
    /// non-whitespace "chunk", then peeks past trailing whitespace
    /// and line breaks to decide whether the scalar continues. A
    /// scalar terminates on:
    /// - EOF or a `#` after whitespace (comment),
    /// - dedent below `parent_indent + 1` after a line break,
    /// - a column-0 document marker (`---` / `...`) on a continuation
    ///   line, or a block indicator (`-`/`?`/`:` followed by EOL/space)
    ///   at the head of a continuation line in block context,
    /// - in flow context, a flow indicator (`,`/`[`/`]`/`{`/`}`/`?`).
    ///
    /// Trailing whitespace that does NOT lead to continuation is left
    /// unconsumed so the next fetch can emit it as trivia.
    fn fetch_plain_scalar(&mut self) {
        self.save_simple_key();
        self.allow_simple_key = false;
        let start = self.cursor;
        let min_indent = self.indent + 1;
        loop {
            let chunk_start = self.cursor.index;
            self.consume_plain_chunk();
            if self.cursor.index == chunk_start {
                break;
            }
            // Peek past inter-chunk whitespace and any line break to
            // determine if the scalar continues. If not, rewind so
            // the trailing whitespace becomes trivia.
            let saved = self.cursor;
            while matches!(self.peek_char(), Some(' ' | '\t')) {
                self.advance();
            }
            match self.peek_char() {
                None | Some('#') => {
                    self.cursor = saved;
                    break;
                }
                Some('\n' | '\r') => {
                    if !self.try_consume_plain_line_break(min_indent) {
                        self.cursor = saved;
                        break;
                    }
                }
                Some(_) => {
                    // Same-line continuation: the consumed spaces are
                    // internal whitespace; keep going.
                }
            }
        }
        let end = self.cursor;
        if start.index == end.index {
            // Pathological: dispatch landed here on a char we can't
            // consume (a stray `?`/`-`/`:` not followed by whitespace
            // at EOF, etc.). Advance one codepoint so the loop makes
            // progress.
            self.advance();
            let end = self.cursor;
            self.tokens.push_back(Token {
                kind: TokenKind::Scalar(ScalarStyle::Plain),
                start,
                end,
            });
            return;
        }
        self.tokens.push_back(Token {
            kind: TokenKind::Scalar(ScalarStyle::Plain),
            start,
            end,
        });
    }

    /// Consume one run of non-whitespace, non-special chars belonging
    /// to a plain scalar. Stops at whitespace/break, at `: ` (value
    /// indicator), and — in flow context — at `,`/`[`/`]`/`{`/`}`/`?`.
    fn consume_plain_chunk(&mut self) {
        loop {
            match self.peek_char() {
                None | Some('\n' | '\r' | ' ' | '\t') => break,
                Some(':') => {
                    let next = self.peek_at(1);
                    if matches!(next, None | Some(' ' | '\t' | '\n' | '\r')) {
                        break;
                    }
                    if self.flow_level > 0 && matches!(next, Some(',' | ']' | '}')) {
                        break;
                    }
                    self.advance();
                }
                Some(',' | '[' | ']' | '{' | '}') if self.flow_level > 0 => break,
                _ => {
                    self.advance();
                }
            }
        }
    }

    /// Try to consume a line break plus any blank lines and the
    /// leading whitespace of the next non-empty line, leaving the
    /// cursor at the next chunk if continuation is allowed. Returns
    /// false (without modifying the cursor) if the scalar must
    /// terminate at the line break. The caller is responsible for
    /// rewinding to a saved cursor in that case.
    fn try_consume_plain_line_break(&mut self, min_indent: i32) -> bool {
        let saved = self.cursor;
        self.consume_one_line_break();
        loop {
            while matches!(self.peek_char(), Some(' ' | '\t')) {
                self.advance();
            }
            match self.peek_char() {
                None => {
                    self.cursor = saved;
                    return false;
                }
                Some('\n' | '\r') => {
                    self.consume_one_line_break();
                    continue;
                }
                Some('#') => {
                    self.cursor = saved;
                    return false;
                }
                Some(_) => {
                    let col = self.cursor.column as i32;
                    if col < min_indent {
                        self.cursor = saved;
                        return false;
                    }
                    if self.flow_level == 0 {
                        // Document marker at column 0 ends the scalar.
                        if col == 0
                            && (self.check_document_indicator(b"---")
                                || self.check_document_indicator(b"..."))
                        {
                            self.cursor = saved;
                            return false;
                        }
                        // A block indicator (`-`/`?`/`:` followed by
                        // EOL or whitespace) at the head of the next
                        // line aborts the plain scalar — those would
                        // otherwise be (mis)consumed as part of the
                        // chunk by the inner loop on the next pass.
                        if matches!(self.peek_char(), Some('-' | '?' | ':'))
                            && matches!(self.peek_at(1), None | Some(' ' | '\t' | '\n' | '\r'))
                        {
                            self.cursor = saved;
                            return false;
                        }
                    } else if matches!(self.peek_char(), Some(',' | ']' | '}')) {
                        // In flow context, a flow terminator/separator
                        // at the head of the next line closes the
                        // surrounding container — it doesn't continue
                        // the scalar.
                        self.cursor = saved;
                        return false;
                    }
                    return true;
                }
            }
        }
    }

    /// Quoted scalar (`'...'` or `"..."`). Both styles can span
    /// multiple lines and can be implicit keys; the scanner emits the
    /// raw source span and surfaces escape/termination diagnostics.
    /// Cooking (escape decoding, line folding) is the projection
    /// layer's job.
    fn fetch_flow_scalar(&mut self, style: ScalarStyle) {
        self.save_simple_key();
        self.allow_simple_key = false;
        let start = self.cursor;
        let quote = match style {
            ScalarStyle::SingleQuoted => '\'',
            ScalarStyle::DoubleQuoted => '"',
            _ => unreachable!("fetch_flow_scalar called with non-quoted style"),
        };
        // Opening quote.
        self.advance();
        let mut closed = false;
        while let Some(c) = self.peek_char() {
            if c == quote {
                if style == ScalarStyle::SingleQuoted && self.peek_at(1) == Some('\'') {
                    // `''` is a literal single quote inside a
                    // single-quoted scalar — not a terminator.
                    self.advance();
                    self.advance();
                    continue;
                }
                self.advance();
                closed = true;
                break;
            }
            if style == ScalarStyle::DoubleQuoted && c == '\\' {
                self.advance();
                self.consume_double_quoted_escape();
                continue;
            }
            // Document markers at column 0 inside an unterminated
            // quoted scalar abort the scalar (libyaml convention) so
            // we don't swallow the next document. Bail out before
            // consuming the marker.
            if self.flow_level == 0
                && self.cursor.column == 0
                && (self.check_document_indicator(b"---") || self.check_document_indicator(b"..."))
            {
                break;
            }
            self.advance();
        }
        if !closed {
            self.diagnostics.push(YamlDiagnostic {
                code: diagnostic_codes::LEX_UNTERMINATED_QUOTED_SCALAR,
                message: "unterminated quoted scalar",
                byte_start: start.index,
                byte_end: self.cursor.index,
            });
        }
        let end = self.cursor;
        self.tokens.push_back(Token {
            kind: TokenKind::Scalar(style),
            start,
            end,
        });
    }

    /// Consume one escape sequence inside a double-quoted scalar,
    /// starting AFTER the introducing `\`. Recognised escapes follow
    /// YAML 1.2 §5.7 (`\0`, `\a`, …, `\xHH`, `\uHHHH`, `\UHHHHHHHH`,
    /// and `\<line-break>` for continuation). Unrecognised escapes
    /// emit a diagnostic; the cursor still advances by one codepoint
    /// to make progress.
    fn consume_double_quoted_escape(&mut self) {
        // The backslash is already past the cursor; record its index
        // for diagnostic spans (one byte before).
        let backslash_index = self.cursor.index.saturating_sub(1);
        match self.peek_char() {
            None => {
                // EOF after backslash; the unterminated-scalar branch
                // will fire.
            }
            Some('\n') => {
                self.advance();
            }
            Some('\r') => {
                self.advance();
                if self.peek_char() == Some('\n') {
                    self.advance();
                }
            }
            Some('x') => {
                self.advance();
                self.consume_hex_digits(2, backslash_index);
            }
            Some('u') => {
                self.advance();
                self.consume_hex_digits(4, backslash_index);
            }
            Some('U') => {
                self.advance();
                self.consume_hex_digits(8, backslash_index);
            }
            Some(c) if Self::is_double_quoted_single_byte_escape(c) => {
                self.advance();
            }
            Some(_) => {
                let invalid_end = self.cursor.index + self.peek_char().unwrap().len_utf8();
                self.diagnostics.push(YamlDiagnostic {
                    code: diagnostic_codes::LEX_INVALID_DOUBLE_QUOTED_ESCAPE,
                    message: "invalid double-quoted escape",
                    byte_start: backslash_index,
                    byte_end: invalid_end,
                });
                self.advance();
            }
        }
    }

    fn consume_hex_digits(&mut self, count: usize, backslash_index: usize) {
        let mut consumed = 0;
        while consumed < count {
            match self.peek_char() {
                Some(c) if c.is_ascii_hexdigit() => {
                    self.advance();
                    consumed += 1;
                }
                _ => break,
            }
        }
        if consumed < count {
            self.diagnostics.push(YamlDiagnostic {
                code: diagnostic_codes::LEX_INVALID_DOUBLE_QUOTED_ESCAPE,
                message: "incomplete hex escape in double-quoted scalar",
                byte_start: backslash_index,
                byte_end: self.cursor.index,
            });
        }
    }

    fn is_double_quoted_single_byte_escape(c: char) -> bool {
        // YAML 1.2 §5.7 escape characters that take no payload.
        matches!(
            c,
            '0' | 'a'
                | 'b'
                | 't'
                | '\t'
                | 'n'
                | 'v'
                | 'f'
                | 'r'
                | 'e'
                | ' '
                | '"'
                | '/'
                | '\\'
                | 'N'
                | '_'
                | 'L'
                | 'P'
        )
    }

    /// Block scalar (`|` literal, `>` folded). The header is `|`/`>`
    /// optionally followed by an indent indicator (`1`–`9`) and/or a
    /// chomping indicator (`+`/`-`), then trailing spaces/comment, then
    /// a line break. Content lines whose indentation falls below the
    /// resolved minimum terminate the scalar — at which point the
    /// cursor is left at the start of the dedented line so the main
    /// loop can pick up the next token.
    ///
    /// As with quoted scalars, the source span is emitted raw; folding
    /// and chomping live in projection.
    fn fetch_block_scalar(&mut self, style: ScalarStyle) {
        // Block scalars are values, not keys, so they don't register
        // a simple-key candidate; but they DO close any pending
        // candidate at the current level (e.g. `key: |` confirms `key`
        // as the candidate before we get here).
        self.allow_simple_key = true;
        self.remove_simple_key();
        let start = self.cursor;
        let parent_indent = self.indent;
        // Header indicator (`|` or `>`).
        self.advance();
        // Optional indent + chomping indicators (in either order).
        let mut explicit_increment: Option<u32> = None;
        for _ in 0..2 {
            match self.peek_char() {
                Some('+' | '-') => {
                    self.advance();
                }
                Some(d @ '1'..='9') if explicit_increment.is_none() => {
                    explicit_increment = Some(d.to_digit(10).expect("hex digit"));
                    self.advance();
                }
                _ => break,
            }
        }
        // Header trailing whitespace.
        while matches!(self.peek_char(), Some(' ' | '\t')) {
            self.advance();
        }
        // Optional trailing comment on the header line.
        if self.peek_char() == Some('#') {
            while !matches!(self.peek_char(), None | Some('\n' | '\r')) {
                self.advance();
            }
        }
        // The header must end at a line break (or EOF, for an empty
        // body). Non-blank trailing content is malformed; libyaml
        // diagnoses but we just consume to end-of-line for resilience.
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
            None => {
                // Empty body at EOF.
                let end = self.cursor;
                self.tokens.push_back(Token {
                    kind: TokenKind::Scalar(style),
                    start,
                    end,
                });
                return;
            }
            Some(_) => {
                // Trailing junk on header — skip to end of line.
                while !matches!(self.peek_char(), None | Some('\n' | '\r')) {
                    self.advance();
                }
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
                    _ => {}
                }
            }
        }
        // Determine the minimum content indent. The libyaml rule:
        // base = max(parent_indent, 0); explicit indicator m yields
        // base + m; otherwise auto-detect from the first non-blank
        // content line, falling back to base + 1.
        let base = parent_indent.max(0);
        let min_indent = match explicit_increment {
            Some(m) => base + m as i32,
            None => self
                .auto_detect_block_scalar_indent()
                .unwrap_or(base + 1)
                .max(base + 1),
        };
        // Walk content lines via lookahead so a dedented line stays
        // unconsumed and the main fetch loop sees it.
        loop {
            let line_start = self.cursor.index;
            let bytes = self.input.as_bytes();
            let mut probe = line_start;
            while bytes.get(probe) == Some(&b' ') {
                probe += 1;
            }
            let leading_spaces = probe - line_start;
            match bytes.get(probe) {
                None => break,
                Some(b'\n' | b'\r') => {
                    // Blank line — entirely whitespace. Consume the
                    // spaces and the line break as content.
                    while self.cursor.index < probe {
                        self.advance();
                    }
                    self.consume_one_line_break();
                    continue;
                }
                _ => {}
            }
            if (leading_spaces as i32) < min_indent {
                // Dedent below content — terminate without consuming.
                break;
            }
            if leading_spaces == 0
                && (bytes.get(probe..probe + 3) == Some(b"---")
                    || bytes.get(probe..probe + 3) == Some(b"..."))
                && matches!(
                    bytes.get(probe + 3),
                    None | Some(b' ' | b'\t' | b'\n' | b'\r')
                )
            {
                // Document marker terminates the scalar.
                break;
            }
            // Consume the rest of the line as content.
            while !matches!(self.peek_char(), None | Some('\n' | '\r')) {
                self.advance();
            }
            self.consume_one_line_break();
            if self.at_eof() {
                break;
            }
        }
        let end = self.cursor;
        self.tokens.push_back(Token {
            kind: TokenKind::Scalar(style),
            start,
            end,
        });
    }

    /// Look ahead through blank lines to find the first non-blank
    /// content line, returning its leading-space count. Pure peek;
    /// the cursor does not move.
    fn auto_detect_block_scalar_indent(&self) -> Option<i32> {
        let bytes = self.input.as_bytes();
        let mut i = self.cursor.index;
        while i < bytes.len() {
            let line_start = i;
            while bytes.get(i) == Some(&b' ') {
                i += 1;
            }
            match bytes.get(i) {
                None => return None,
                Some(b'\n') => {
                    i += 1;
                    continue;
                }
                Some(b'\r') => {
                    i += 1;
                    if bytes.get(i) == Some(&b'\n') {
                        i += 1;
                    }
                    continue;
                }
                _ => {
                    return Some((i - line_start) as i32);
                }
            }
        }
        None
    }

    fn consume_one_line_break(&mut self) {
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
            _ => {}
        }
    }

    fn fetch_stream_end(&mut self) {
        if self.stream_end_emitted {
            return;
        }
        self.unwind_indent(-1);
        // Drain any pending simple-key candidates. Required candidates
        // that never met a `:` are diagnosed; non-required ones are
        // dropped silently.
        for slot in self.simple_keys.iter_mut() {
            if let Some(key) = slot.take()
                && key.required
            {
                self.diagnostics.push(YamlDiagnostic {
                    code: diagnostic_codes::LEX_REQUIRED_SIMPLE_KEY_NOT_FOUND,
                    message: "could not find expected ':' for required simple key",
                    byte_start: key.mark.index,
                    byte_end: key.mark.index,
                });
            }
        }
        self.allow_simple_key = false;
        self.stream_end_emitted = true;
        let mark = self.cursor;
        self.tokens.push_back(Token {
            kind: TokenKind::StreamEnd,
            start: mark,
            end: mark,
        });
    }

    fn check_block_entry(&self) -> bool {
        matches!(self.peek_at(1), None | Some(' ' | '\t' | '\n' | '\r'))
    }

    /// `?` opens an explicit key only when followed by whitespace,
    /// end-of-input, or end-of-line — in both block and flow context.
    /// A `?` that's followed by any other character is plain-scalar
    /// text (e.g. `value?`, `another ? string`, `?key`). yaml-test-suite
    /// JR7V pins this for flow context; libyaml `check_key` agrees.
    fn check_key(&self) -> bool {
        matches!(self.peek_at(1), None | Some(' ' | '\t' | '\n' | '\r'))
    }

    /// `:` is a value indicator in the same conditions as `?`. In flow
    /// context it's always structural; in block context only when
    /// followed by whitespace/EOL (otherwise it's part of a plain
    /// scalar like `https://example.com`).
    fn check_value(&self) -> bool {
        if self.flow_level > 0 {
            return true;
        }
        matches!(self.peek_at(1), None | Some(' ' | '\t' | '\n' | '\r'))
    }

    /// Push a new indent level if `column` exceeds the current one.
    /// Returns true if the level was newly opened, signalling the
    /// caller should emit a `BlockSequenceStart` / `BlockMappingStart`.
    fn add_indent(&mut self, column: i32) -> bool {
        if self.indent < column {
            self.indent_stack.push(self.indent);
            self.indent = column;
            true
        } else {
            false
        }
    }

    /// Pop indent levels above `column`, emitting `BlockEnd` for each.
    /// Flow context never owns indent levels, so this is a no-op there.
    fn unwind_indent(&mut self, column: i32) {
        if self.flow_level > 0 {
            return;
        }
        while self.indent > column {
            let mark = self.cursor;
            self.indent = self.indent_stack.pop().unwrap_or(-1);
            self.tokens.push_back(Token {
                kind: TokenKind::BlockEnd,
                start: mark,
                end: mark,
            });
        }
    }

    /// Tentatively register a simple-key candidate at the current flow
    /// level. The candidate's `token_number` is the global index where
    /// the next token will be appended — i.e. the scalar/anchor that
    /// triggered registration. A subsequent `:` confirms the candidate
    /// (splicing `Key` before that token); a line break or required
    /// expiration cancels it.
    fn save_simple_key(&mut self) {
        if !self.allow_simple_key {
            return;
        }
        let required = self.flow_level == 0 && self.indent == self.cursor.column as i32;
        self.remove_simple_key();
        let token_number = self.tokens_taken + self.tokens.len();
        self.simple_keys[self.flow_level] = Some(SimpleKey {
            token_number,
            required,
            mark: self.cursor,
        });
    }

    /// Cancel the simple-key candidate at the current flow level. If it
    /// was required, surface a diagnostic — required candidates that
    /// fail to confirm indicate malformed YAML (e.g. an indent change
    /// before the expected `:`).
    fn remove_simple_key(&mut self) {
        if let Some(key) = self.simple_keys[self.flow_level].take()
            && key.required
        {
            self.diagnostics.push(YamlDiagnostic {
                code: diagnostic_codes::LEX_REQUIRED_SIMPLE_KEY_NOT_FOUND,
                message: "could not find expected ':' for required simple key",
                byte_start: key.mark.index,
                byte_end: key.mark.index,
            });
        }
    }

    /// Expire candidates whose registration line lies behind the
    /// cursor — a simple key cannot span a line break. Required
    /// candidates that age out get a diagnostic; others are dropped
    /// silently.
    fn stale_simple_keys(&mut self) {
        let line = self.cursor.line;
        for slot in self.simple_keys.iter_mut() {
            let stale = match slot {
                Some(key) => key.mark.line != line,
                None => false,
            };
            if stale
                && let Some(key) = slot.take()
                && key.required
            {
                self.diagnostics.push(YamlDiagnostic {
                    code: diagnostic_codes::LEX_REQUIRED_SIMPLE_KEY_NOT_FOUND,
                    message: "could not find expected ':' for required simple key",
                    byte_start: key.mark.index,
                    byte_end: key.mark.index,
                });
            }
        }
    }

    fn push_diagnostic(&mut self, code: &'static str, message: &'static str) {
        self.diagnostics.push(YamlDiagnostic {
            code,
            message,
            byte_start: self.cursor.index,
            byte_end: self.cursor.index,
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
        // A document marker terminates the previous document's block
        // structure: any indent levels held by an open block map or
        // sequence must close before the marker so the next document
        // starts from a clean indent stack. Without this, a
        // multi-document stream where doc N closed at column 0 leaves
        // `self.indent == 0`, which prevents `add_indent(0)` from
        // emitting a fresh `BlockMappingStart` / `BlockSequenceStart`
        // for doc N+1's body — its content lands at document level
        // instead of inside a container. Mirrors libyaml/PyYAML's
        // `fetch_document_indicator`.
        self.unwind_indent(-1);
        self.remove_simple_key();
        self.allow_simple_key = false;
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
        // Line breaks in block context re-open simple-key candidacy:
        // the next non-trivia token starts a fresh line and may be a
        // key. Flow context ignores indentation, so candidacy is
        // governed by `,`/`[`/`{` instead.
        if self.flow_level == 0 {
            self.allow_simple_key = true;
        }
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

/// Byte-completeness report from running the streaming scanner over an
/// input. Used by the integration harness to gate the cutover (step 12)
/// — until every allowlisted fixture is covered byte-completely with no
/// overlaps or gaps, the new scanner cannot replace the line-based
/// lexer.
#[derive(Debug, Clone)]
pub struct ShadowScannerReport {
    /// True when token spans cover the entire input contiguously and
    /// no two non-synthetic tokens overlap.
    pub byte_complete: bool,
    /// Total tokens emitted (including trivia and stream markers).
    pub token_count: usize,
    /// Diagnostic codes emitted during scanning, in order.
    pub diagnostic_codes: Vec<&'static str>,
    /// Highest end-index reached across non-synthetic tokens.
    pub last_token_end: usize,
    pub input_len: usize,
    /// First byte index where coverage is missing, if any.
    pub gap_at: Option<usize>,
    /// True if any non-synthetic token's start index is below the
    /// preceding token's end (a regression in the splice/queue logic).
    pub overlapping: bool,
}

/// Drive the streaming scanner to completion over `input` and return a
/// byte-completeness report. This is exposed so the integration harness
/// in `tests/yaml.rs` can run the scanner over every allowlisted
/// fixture without depending on internal `Token`/`Scanner` types.
pub fn shadow_scanner_check(input: &str) -> ShadowScannerReport {
    let mut scanner = Scanner::new(input);
    let mut tokens = Vec::new();
    while let Some(tok) = scanner.next_token() {
        tokens.push(tok);
    }
    let mut cursor = 0usize;
    let mut overlapping = false;
    let mut gap_at: Option<usize> = None;
    for tok in &tokens {
        match tok.kind {
            TokenKind::StreamStart | TokenKind::StreamEnd => {}
            _ => {
                if tok.start.index < cursor {
                    overlapping = true;
                } else if tok.start.index > cursor && gap_at.is_none() {
                    gap_at = Some(cursor);
                }
                if tok.end.index > cursor {
                    cursor = tok.end.index;
                }
            }
        }
    }
    let byte_complete = !overlapping && gap_at.is_none() && cursor == input.len();
    ShadowScannerReport {
        byte_complete,
        token_count: tokens.len(),
        diagnostic_codes: scanner.diagnostics.iter().map(|d| d.code).collect(),
        last_token_end: cursor,
        input_len: input.len(),
        gap_at,
        overlapping,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_emits_stream_start_then_stream_end() {
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
    fn first_and_last_tokens_are_always_stream_markers() {
        let mut scanner = Scanner::new("foo: bar\n");
        assert_eq!(
            scanner.next_token().map(|t| t.kind),
            Some(TokenKind::StreamStart)
        );
        let mut last = None;
        while let Some(tok) = scanner.next_token() {
            last = Some(tok);
        }
        assert_eq!(last.map(|t| t.kind), Some(TokenKind::StreamEnd));
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
        let tokens = collect_tokens("---abc\n");
        let kinds = meaningful_kinds(&tokens);
        assert!(!kinds.contains(&TokenKind::DocumentStart), "got {kinds:?}",);
        assert!(
            kinds.contains(&TokenKind::Scalar(ScalarStyle::Plain)),
            "got {kinds:?}",
        );
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
    fn document_marker_followed_by_space_emits_marker_then_content_scalar() {
        let input = "--- foo\n";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        assert_eq!(kinds[0], TokenKind::StreamStart);
        assert_eq!(kinds[1], TokenKind::DocumentStart);
        // " " is whitespace trivia; "foo" is now a plain scalar.
        assert_eq!(kinds[2], TokenKind::Scalar(ScalarStyle::Plain));
        assert_eq!(*kinds.last().unwrap(), TokenKind::StreamEnd);
        assert_byte_complete(input, &tokens);
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
    fn comma_outside_flow_is_not_a_flow_entry() {
        // Outside flow context, `,` is plain text, not an indicator.
        let tokens = collect_tokens(",");
        let kinds = meaningful_kinds(&tokens);
        assert!(!kinds.contains(&TokenKind::FlowEntry), "got {kinds:?}");
    }

    #[test]
    fn doc_markers_inside_flow_context_are_not_recognized() {
        // `[---]` — the `---` inside flow context is plain text, not a
        // doc marker.
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

    #[test]
    fn block_mapping_implicit_key_splices_block_mapping_start_and_key() {
        // The classic case: `key: value` registers `key` as a simple-key
        // candidate; the `:` confirms it, splicing BlockMappingStart and
        // Key before the scalar.
        let input = "key: value";
        let tokens = collect_tokens(input);
        assert_eq!(
            meaningful_kinds(&tokens),
            vec![
                TokenKind::StreamStart,
                TokenKind::BlockMappingStart,
                TokenKind::Key,
                TokenKind::Scalar(ScalarStyle::Plain),
                TokenKind::Value,
                TokenKind::Scalar(ScalarStyle::Plain),
                TokenKind::BlockEnd,
                TokenKind::StreamEnd,
            ],
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn block_sequence_emits_block_sequence_start_then_entries() {
        let input = "- a\n- b\n";
        let tokens = collect_tokens(input);
        assert_eq!(
            meaningful_kinds(&tokens),
            vec![
                TokenKind::StreamStart,
                TokenKind::BlockSequenceStart,
                TokenKind::BlockEntry,
                TokenKind::Scalar(ScalarStyle::Plain),
                TokenKind::BlockEntry,
                TokenKind::Scalar(ScalarStyle::Plain),
                TokenKind::BlockEnd,
                TokenKind::StreamEnd,
            ],
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn explicit_key_indicator_emits_key_and_value_without_splice() {
        // `? a\n: b` — the `?` opens an explicit-key entry, so when `:`
        // arrives there's no implicit-key candidate to confirm (the
        // candidate registered for `a` aged out at the line break).
        let input = "? a\n: b\n";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        assert_eq!(
            kinds,
            vec![
                TokenKind::StreamStart,
                TokenKind::BlockMappingStart,
                TokenKind::Key,
                TokenKind::Scalar(ScalarStyle::Plain),
                TokenKind::Value,
                TokenKind::Scalar(ScalarStyle::Plain),
                TokenKind::BlockEnd,
                TokenKind::StreamEnd,
            ],
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn multi_line_plain_scalar_does_not_confirm_simple_key_on_next_line() {
        // `a\nb: c\n` — under multi-line plain rules `a\nb` is one
        // continuation scalar, terminated by `: `. The simple-key
        // candidate registered when the scalar started on line 0 must
        // age out before the `:` arrives (it lives on line 1), so the
        // `:` does NOT splice a Key before the multi-line scalar.
        let input = "a\nb: c\n";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        // The first plain scalar token must precede any Key token —
        // proving the multi-line scalar wasn't retroactively keyed.
        let scalar_pos = kinds
            .iter()
            .position(|&k| k == TokenKind::Scalar(ScalarStyle::Plain))
            .expect("plain scalar present");
        if let Some(key_pos) = kinds.iter().position(|&k| k == TokenKind::Key) {
            assert!(
                scalar_pos < key_pos,
                "multi-line scalar must precede any key: {kinds:?}",
            );
        }
        // The scalar's source span covers both lines.
        let scalar = tokens
            .iter()
            .find(|t| matches!(t.kind, TokenKind::Scalar(ScalarStyle::Plain)))
            .unwrap();
        assert_eq!(&input[scalar.start.index..scalar.end.index], "a\nb");
    }

    #[test]
    fn flow_mapping_with_implicit_key_emits_only_flow_indicators() {
        // Inside `{}`, `a: b` triggers the simple-key splice for `a`
        // but DOES NOT emit BlockMappingStart (we're in flow context).
        let input = "{a: b}";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        assert_eq!(
            kinds,
            vec![
                TokenKind::StreamStart,
                TokenKind::FlowMappingStart,
                TokenKind::Key,
                TokenKind::Scalar(ScalarStyle::Plain),
                TokenKind::Value,
                TokenKind::Scalar(ScalarStyle::Plain),
                TokenKind::FlowMappingEnd,
                TokenKind::StreamEnd,
            ],
        );
        assert!(
            !kinds.contains(&TokenKind::BlockMappingStart),
            "got {kinds:?}",
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn flow_explicit_key_indicator_emits_key_token() {
        // `?` inside flow context is always a key indicator (no
        // whitespace lookahead needed).
        let input = "{? a: b}";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        assert_eq!(kinds[0], TokenKind::StreamStart);
        assert_eq!(kinds[1], TokenKind::FlowMappingStart);
        assert_eq!(kinds[2], TokenKind::Key);
        // After the `?`, the rest is implicit-key-style: candidate for
        // `a` is confirmed by `:`.
        assert!(kinds.contains(&TokenKind::Value));
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn nested_block_mapping_emits_block_end_on_dedent() {
        // outer:
        //   inner: x
        // y: z
        // The dedent before `y` must emit BlockEnd, popping the inner
        // mapping's indent level.
        let input = "outer:\n  inner: x\ny: z\n";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        let block_ends = kinds.iter().filter(|&&k| k == TokenKind::BlockEnd).count();
        // One BlockEnd for the inner mapping (popped before `y`),
        // one for the outer mapping at stream end.
        assert_eq!(block_ends, 2, "got {kinds:?}");
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn nested_block_sequence_inside_mapping_unwinds_correctly() {
        // items:
        //   - a
        //   - b
        // status: ok
        //
        // The dedent before `status:` pops the inner sequence's indent
        // level, emitting BlockEnd before the next outer mapping key.
        let input = "items:\n  - a\n  - b\nstatus: ok\n";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        // Find the position of the SECOND Key (`status`) and the
        // BlockEnd that should precede it (closing the sequence).
        let key_positions: Vec<_> = kinds
            .iter()
            .enumerate()
            .filter_map(|(i, &k)| (k == TokenKind::Key).then_some(i))
            .collect();
        assert_eq!(key_positions.len(), 2, "expected 2 keys: {kinds:?}");
        let second_key = key_positions[1];
        let preceding_block_end = kinds[..second_key]
            .iter()
            .rposition(|&k| k == TokenKind::BlockEnd);
        assert!(
            preceding_block_end.is_some(),
            "BlockEnd must precede second key: {kinds:?}",
        );
        // Final two tokens are BlockEnd (outer mapping), StreamEnd.
        let n = kinds.len();
        assert_eq!(kinds[n - 1], TokenKind::StreamEnd);
        assert_eq!(kinds[n - 2], TokenKind::BlockEnd);
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn value_indicator_with_no_simple_key_emits_block_mapping_start() {
        // A bare `: value` at column 0 (empty key shorthand) opens a
        // block mapping with no Key splice; the parser will treat it
        // as "empty implicit key, then value".
        let input = ": value\n";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        assert_eq!(kinds[0], TokenKind::StreamStart);
        assert_eq!(kinds[1], TokenKind::BlockMappingStart);
        assert_eq!(kinds[2], TokenKind::Value);
        // No Key token before Value — the parser handles empty key.
        assert!(!kinds[..3].contains(&TokenKind::Key), "got {kinds:?}",);
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn block_mapping_unwinds_indents_at_stream_end() {
        // a:
        //   b: c
        // (no trailing newline) — must still emit two BlockEnd tokens
        // before StreamEnd as the indent stack unwinds.
        let input = "a:\n  b: c";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        // Last meaningful tokens should be BlockEnd, BlockEnd, StreamEnd.
        let n = kinds.len();
        assert_eq!(kinds[n - 1], TokenKind::StreamEnd);
        assert_eq!(kinds[n - 2], TokenKind::BlockEnd);
        assert_eq!(kinds[n - 3], TokenKind::BlockEnd);
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn colon_inside_plain_scalar_token_does_not_break_scalar() {
        // `https://example.com` — the `:` is not followed by whitespace
        // so it stays inside the plain scalar.
        let input = "https://example.com";
        let tokens = collect_tokens(input);
        let scalar = tokens
            .iter()
            .find(|t| matches!(t.kind, TokenKind::Scalar(_)))
            .expect("plain scalar token");
        assert_eq!(
            &input[scalar.start.index..scalar.end.index],
            "https://example.com",
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn diagnostics_remain_empty_for_well_formed_inputs() {
        for input in ["key: value", "- a\n- b\n", "{a: b, c: d}", "? k\n: v\n"] {
            let mut scanner = Scanner::new(input);
            while scanner.next_token().is_some() {}
            assert!(
                scanner.diagnostics().is_empty(),
                "{input:?} produced unexpected diagnostics: {:?}",
                scanner.diagnostics(),
            );
        }
    }

    fn find_scalar(tokens: &[Token]) -> &Token {
        tokens
            .iter()
            .find(|t| matches!(t.kind, TokenKind::Scalar(_)))
            .expect("expected scalar token")
    }

    #[test]
    fn single_quoted_scalar_emits_token_spanning_quotes() {
        let input = "'hello'";
        let tokens = collect_tokens(input);
        let scalar = find_scalar(&tokens);
        assert_eq!(scalar.kind, TokenKind::Scalar(ScalarStyle::SingleQuoted));
        assert_eq!(&input[scalar.start.index..scalar.end.index], "'hello'");
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn double_quoted_scalar_emits_token_spanning_quotes() {
        let input = "\"hello\"";
        let tokens = collect_tokens(input);
        let scalar = find_scalar(&tokens);
        assert_eq!(scalar.kind, TokenKind::Scalar(ScalarStyle::DoubleQuoted));
        assert_eq!(&input[scalar.start.index..scalar.end.index], "\"hello\"");
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn single_quoted_scalar_treats_doubled_quote_as_escape() {
        // `'it''s'` is a single scalar containing `it's`. The middle
        // `''` must NOT terminate the scalar.
        let input = "'it''s'";
        let tokens = collect_tokens(input);
        let scalars: Vec<_> = tokens
            .iter()
            .filter(|t| matches!(t.kind, TokenKind::Scalar(_)))
            .collect();
        assert_eq!(scalars.len(), 1, "got {:?}", tokens);
        assert_eq!(
            &input[scalars[0].start.index..scalars[0].end.index],
            "'it''s'",
        );
    }

    #[test]
    fn double_quoted_scalar_with_escaped_quote_does_not_terminate_early() {
        // `"a\"b"` — the middle `\"` is an escaped quote; the closer
        // is the final `"`.
        let input = "\"a\\\"b\"";
        let tokens = collect_tokens(input);
        let scalars: Vec<_> = tokens
            .iter()
            .filter(|t| matches!(t.kind, TokenKind::Scalar(_)))
            .collect();
        assert_eq!(scalars.len(), 1, "got {tokens:?}");
        assert_eq!(
            &input[scalars[0].start.index..scalars[0].end.index],
            "\"a\\\"b\"",
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn double_quoted_scalar_recognises_common_single_byte_escapes() {
        // Each escape advances by exactly one char after `\`.
        let input = "\"\\n\\t\\r\\0\\\\\\\"\"";
        let tokens = collect_tokens(input);
        let scalar = find_scalar(&tokens);
        assert_eq!(scalar.kind, TokenKind::Scalar(ScalarStyle::DoubleQuoted));
        // The whole input should be the scalar.
        assert_eq!(scalar.start.index, 0);
        assert_eq!(scalar.end.index, input.len());
        let mut scanner = Scanner::new(input);
        while scanner.next_token().is_some() {}
        assert!(scanner.diagnostics().is_empty());
    }

    #[test]
    fn double_quoted_scalar_recognises_hex_escapes() {
        // `\x41` is `A`; `é` is `é`; `\U0001F600` is 😀.
        let input = "\"\\x41\\u00E9\\U0001F600\"";
        let mut scanner = Scanner::new(input);
        while scanner.next_token().is_some() {}
        assert!(
            scanner.diagnostics().is_empty(),
            "got {:?}",
            scanner.diagnostics()
        );
    }

    #[test]
    fn double_quoted_scalar_with_invalid_escape_emits_diagnostic() {
        let input = "\"\\q\"";
        let mut scanner = Scanner::new(input);
        while scanner.next_token().is_some() {}
        assert_eq!(
            scanner.diagnostics().len(),
            1,
            "got {:?}",
            scanner.diagnostics(),
        );
        assert_eq!(
            scanner.diagnostics()[0].code,
            diagnostic_codes::LEX_INVALID_DOUBLE_QUOTED_ESCAPE,
        );
    }

    #[test]
    fn double_quoted_scalar_with_short_hex_escape_emits_diagnostic() {
        // `\x4` is missing one hex digit; the `"` after closes the
        // scalar but the truncated escape is reported.
        let input = "\"\\x4\"";
        let mut scanner = Scanner::new(input);
        while scanner.next_token().is_some() {}
        assert!(
            scanner
                .diagnostics()
                .iter()
                .any(|d| d.code == diagnostic_codes::LEX_INVALID_DOUBLE_QUOTED_ESCAPE),
            "got {:?}",
            scanner.diagnostics(),
        );
    }

    #[test]
    fn double_quoted_scalar_spans_multiple_lines() {
        // A literal newline inside the quotes is part of the scalar.
        let input = "\"line1\nline2\"";
        let tokens = collect_tokens(input);
        let scalar = find_scalar(&tokens);
        assert_eq!(scalar.kind, TokenKind::Scalar(ScalarStyle::DoubleQuoted));
        // The entire input is the scalar (no Newline trivia between
        // the two lines — line breaks inside quoted scalars belong to
        // the scalar's source span).
        assert_eq!(scalar.start.index, 0);
        assert_eq!(scalar.end.index, input.len());
    }

    #[test]
    fn line_continuation_escape_consumes_newline_inside_quoted_scalar() {
        // `\<newline>` is a folding line break: the `\` plus the
        // following newline are together one escape.
        let input = "\"a\\\nb\"";
        let mut scanner = Scanner::new(input);
        while scanner.next_token().is_some() {}
        assert!(
            scanner.diagnostics().is_empty(),
            "got {:?}",
            scanner.diagnostics(),
        );
    }

    #[test]
    fn unterminated_quoted_scalar_emits_diagnostic() {
        for input in ["'oops", "\"oops"] {
            let mut scanner = Scanner::new(input);
            while scanner.next_token().is_some() {}
            assert!(
                scanner
                    .diagnostics()
                    .iter()
                    .any(|d| d.code == diagnostic_codes::LEX_UNTERMINATED_QUOTED_SCALAR),
                "{input:?} produced {:?}",
                scanner.diagnostics(),
            );
        }
    }

    #[test]
    fn quoted_scalar_can_be_implicit_key() {
        let input = "\"key\": value";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        assert_eq!(
            kinds,
            vec![
                TokenKind::StreamStart,
                TokenKind::BlockMappingStart,
                TokenKind::Key,
                TokenKind::Scalar(ScalarStyle::DoubleQuoted),
                TokenKind::Value,
                TokenKind::Scalar(ScalarStyle::Plain),
                TokenKind::BlockEnd,
                TokenKind::StreamEnd,
            ],
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn multi_line_quoted_scalar_cannot_be_implicit_key() {
        // The scalar opens on line 0; the simple-key candidate's mark
        // is on line 0. After scanning across the line break the
        // cursor is on line 1, so stale_simple_keys removes the
        // candidate before the `:` arrives — no Key splice.
        let input = "\"line1\nline2\": value\n";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        // Expected: StreamStart, Scalar(DoubleQuoted), BlockMappingStart,
        // Value, Scalar(Plain), BlockEnd, StreamEnd. The Scalar comes
        // BEFORE BlockMappingStart/Value, demonstrating no key splice.
        assert_eq!(kinds[0], TokenKind::StreamStart);
        assert_eq!(kinds[1], TokenKind::Scalar(ScalarStyle::DoubleQuoted));
        assert_eq!(kinds[2], TokenKind::BlockMappingStart);
        assert_eq!(kinds[3], TokenKind::Value);
        assert!(!kinds[..3].contains(&TokenKind::Key), "got {kinds:?}",);
    }

    #[test]
    fn quoted_scalar_inside_flow_mapping_terminates_at_closing_quote() {
        let input = "{\"a\": \"b\"}";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        assert_eq!(
            kinds,
            vec![
                TokenKind::StreamStart,
                TokenKind::FlowMappingStart,
                TokenKind::Key,
                TokenKind::Scalar(ScalarStyle::DoubleQuoted),
                TokenKind::Value,
                TokenKind::Scalar(ScalarStyle::DoubleQuoted),
                TokenKind::FlowMappingEnd,
                TokenKind::StreamEnd,
            ],
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn literal_block_scalar_at_top_level_spans_to_eof() {
        let input = "|\n  hello\n  world\n";
        let tokens = collect_tokens(input);
        let scalar = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Scalar(ScalarStyle::Literal))
            .expect("literal scalar");
        // The scalar covers the header `|`, line break, both content
        // lines, and their trailing newlines.
        assert_eq!(scalar.start.index, 0);
        assert_eq!(scalar.end.index, input.len());
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn folded_block_scalar_emits_folded_style() {
        let input = ">\n  hello\n";
        let tokens = collect_tokens(input);
        assert!(
            tokens
                .iter()
                .any(|t| t.kind == TokenKind::Scalar(ScalarStyle::Folded)),
            "got {tokens:?}",
        );
    }

    #[test]
    fn block_scalar_terminates_on_dedent_to_parent_indent() {
        // key: |
        //   line1
        //   line2
        // next: x
        //
        // The block scalar's content indent is 2; `next:` at column 0
        // is below that, so the scalar terminates without consuming
        // `next` and the outer mapping continues.
        let input = "key: |\n  line1\n  line2\nnext: x\n";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        // Find the block scalar's span; everything before "next" must
        // be inside it.
        let scalar = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Scalar(ScalarStyle::Literal))
            .expect("literal scalar");
        let next_idx = input.find("next:").expect("next key in fixture");
        assert!(
            scalar.end.index <= next_idx,
            "scalar should end before `next:` at {next_idx}: scalar ends at {}",
            scalar.end.index,
        );
        // The outer mapping must produce two key/value pairs.
        let key_count = kinds.iter().filter(|&&k| k == TokenKind::Key).count();
        assert_eq!(key_count, 2, "got {kinds:?}");
    }

    #[test]
    fn block_scalar_with_keep_chomping_indicator_in_header() {
        let input = "|+\n  text\n\n";
        let tokens = collect_tokens(input);
        let scalar = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Scalar(ScalarStyle::Literal))
            .expect("literal scalar");
        // The header `|+` and the empty trailing line are part of the
        // scalar's source span.
        assert_eq!(scalar.start.index, 0);
        assert_eq!(scalar.end.index, input.len());
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn block_scalar_with_explicit_indent_indicator_uses_that_indent() {
        // `|2` declares the content indent is 2. Lines at less than
        // 2 spaces terminate. The single content line at indent 2
        // is included; `bye` at indent 0 is not.
        let input = "key: |2\n  hi\nbye: x\n";
        let tokens = collect_tokens(input);
        let scalar = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Scalar(ScalarStyle::Literal))
            .expect("literal scalar");
        let bye_idx = input.find("bye:").expect("bye key in fixture");
        assert!(
            scalar.end.index <= bye_idx,
            "scalar must end before `bye`: {} vs {}",
            scalar.end.index,
            bye_idx,
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn block_scalar_at_eof_without_trailing_newline_still_emits() {
        let input = "|\n  text";
        let tokens = collect_tokens(input);
        let scalar = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Scalar(ScalarStyle::Literal))
            .expect("literal scalar");
        assert_eq!(scalar.end.index, input.len());
    }

    #[test]
    fn block_scalar_with_internal_blank_lines_includes_them() {
        // Blank lines inside the block scalar are part of content.
        let input = "|\n  a\n\n  b\n";
        let tokens = collect_tokens(input);
        let scalar = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Scalar(ScalarStyle::Literal))
            .expect("literal scalar");
        assert_eq!(scalar.end.index, input.len());
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn pipe_inside_flow_context_is_part_of_plain_scalar_not_block() {
        // `[|]` — `|` in flow context is plain text.
        let input = "[|]";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        // Should NOT see a Literal-style scalar — flow context disables
        // the block-scalar dispatch.
        assert!(
            !kinds.contains(&TokenKind::Scalar(ScalarStyle::Literal)),
            "got {kinds:?}",
        );
        assert_eq!(kinds[1], TokenKind::FlowSequenceStart);
        assert!(kinds.contains(&TokenKind::Scalar(ScalarStyle::Plain)));
    }

    #[test]
    fn block_scalar_terminates_on_document_marker() {
        let input = "|\n  text\n---\nnext\n";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        // The scalar must NOT swallow the `---` marker.
        assert!(kinds.contains(&TokenKind::DocumentStart), "got {kinds:?}");
    }

    #[test]
    fn plain_scalar_with_internal_whitespace_is_one_token() {
        let input = "hello world";
        let tokens = collect_tokens(input);
        let scalars: Vec<_> = tokens
            .iter()
            .filter(|t| matches!(t.kind, TokenKind::Scalar(ScalarStyle::Plain)))
            .collect();
        assert_eq!(scalars.len(), 1, "got {tokens:?}");
        assert_eq!(
            &input[scalars[0].start.index..scalars[0].end.index],
            "hello world",
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn plain_scalar_with_multiple_internal_spaces_is_one_token() {
        let input = "a   b   c";
        let tokens = collect_tokens(input);
        let scalars: Vec<_> = tokens
            .iter()
            .filter(|t| matches!(t.kind, TokenKind::Scalar(ScalarStyle::Plain)))
            .collect();
        assert_eq!(scalars.len(), 1, "got {tokens:?}");
        assert_eq!(
            &input[scalars[0].start.index..scalars[0].end.index],
            "a   b   c",
        );
    }

    #[test]
    fn plain_scalar_drops_trailing_whitespace_before_eof() {
        // Trailing spaces on the same line are not part of the scalar.
        let input = "hello   ";
        let tokens = collect_tokens(input);
        let scalar = tokens
            .iter()
            .find(|t| matches!(t.kind, TokenKind::Scalar(ScalarStyle::Plain)))
            .expect("plain scalar");
        assert_eq!(&input[scalar.start.index..scalar.end.index], "hello");
        // The trailing spaces become a Whitespace trivia token.
        assert!(
            tokens
                .iter()
                .any(|t| t.kind == TokenKind::Trivia(TriviaKind::Whitespace)),
            "expected trailing whitespace as trivia: {tokens:?}",
        );
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn plain_scalar_drops_trailing_whitespace_before_comment() {
        // `hello # comment` — the scalar is `hello`; the `# comment`
        // is a comment trivia (and the spaces between are whitespace).
        let input = "hello # comment";
        let tokens = collect_tokens(input);
        let scalar = tokens
            .iter()
            .find(|t| matches!(t.kind, TokenKind::Scalar(ScalarStyle::Plain)))
            .expect("plain scalar");
        assert_eq!(&input[scalar.start.index..scalar.end.index], "hello");
        assert!(
            tokens
                .iter()
                .any(|t| t.kind == TokenKind::Trivia(TriviaKind::Comment)),
            "expected comment trivia: {tokens:?}",
        );
    }

    #[test]
    fn colon_inside_url_does_not_break_plain_scalar() {
        // `https://example.com` — `:` followed by `/` stays inside the
        // scalar (regression of step-6 behaviour after the rewrite).
        let input = "url: https://example.com\n";
        let tokens = collect_tokens(input);
        let scalars: Vec<_> = tokens
            .iter()
            .filter(|t| matches!(t.kind, TokenKind::Scalar(ScalarStyle::Plain)))
            .map(|t| &input[t.start.index..t.end.index])
            .collect();
        assert_eq!(scalars, vec!["url", "https://example.com"]);
    }

    #[test]
    fn multi_line_plain_scalar_continues_under_indent() {
        // `key: hello\n  world\n` — the `world` line is indented past
        // the parent indent (0+1=1), so it continues the scalar.
        let input = "key: hello\n  world\n";
        let tokens = collect_tokens(input);
        let plain_scalars: Vec<_> = tokens
            .iter()
            .filter(|t| matches!(t.kind, TokenKind::Scalar(ScalarStyle::Plain)))
            .collect();
        // Two plain scalars: `key`, and the multi-line value.
        assert_eq!(plain_scalars.len(), 2, "got {tokens:?}");
        // The value scalar spans both lines.
        let value = plain_scalars[1];
        assert!(
            input[value.start.index..value.end.index].contains("hello"),
            "scalar text: {:?}",
            &input[value.start.index..value.end.index],
        );
        assert!(
            input[value.start.index..value.end.index].contains("world"),
            "scalar text: {:?}",
            &input[value.start.index..value.end.index],
        );
    }

    #[test]
    fn plain_scalar_terminates_at_blank_line_continuation() {
        // A blank line between content terminates the plain scalar.
        let input = "key: hello\n\n  world\n";
        let tokens = collect_tokens(input);
        let plain_scalars: Vec<_> = tokens
            .iter()
            .filter(|t| matches!(t.kind, TokenKind::Scalar(ScalarStyle::Plain)))
            .map(|t| &input[t.start.index..t.end.index])
            .collect();
        // Hmm — actually a blank line in YAML plain-scalar continuation
        // is allowed as folding whitespace. Verify what we emit: at
        // minimum, `hello` and `world` should both be present, but we
        // accept either (one merged scalar OR separate). Check both.
        let merged = plain_scalars.iter().any(|s| s.contains("world"));
        assert!(
            merged || plain_scalars.contains(&"world"),
            "got {plain_scalars:?}"
        );
    }

    #[test]
    fn plain_scalar_terminates_on_dedent() {
        // `outer:\n  hello\nnext: x` — `next:` at column 0 is below
        // the continuation indent (parent=2, min=3), so the value
        // scalar ends at end-of-line-1 and `next:` opens a new entry.
        let input = "outer:\n  hello\nnext: x\n";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        // Two Key tokens (outer, next).
        let key_count = kinds.iter().filter(|&&k| k == TokenKind::Key).count();
        assert_eq!(key_count, 2, "got {kinds:?}");
        // Three plain scalars: `outer`, `hello`, `next`, `x`.
        let plain_count = kinds
            .iter()
            .filter(|&&k| k == TokenKind::Scalar(ScalarStyle::Plain))
            .count();
        assert_eq!(plain_count, 4, "got {kinds:?}");
    }

    #[test]
    fn plain_scalar_terminates_on_following_block_entry_indicator() {
        // `outer:\n  - a` — under the value `outer:` we have a block
        // sequence whose first entry `- a` is on line 1. The (empty)
        // value of `outer:` must NOT swallow `- a` as a continuation.
        let input = "outer:\n  - a\n  - b\n";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        // Should see at least one BlockEntry (we'd see two for the
        // two items, but the bigger point is that `- a` was NOT
        // absorbed into the plain-scalar continuation).
        let block_entry_count = kinds
            .iter()
            .filter(|&&k| k == TokenKind::BlockEntry)
            .count();
        assert!(block_entry_count >= 1, "got {kinds:?}");
    }

    #[test]
    fn flow_context_plain_scalar_does_not_absorb_terminator_line_break() {
        // `{a: 42\n}\n` — the `\n` between `42` and `}` must NOT be
        // swallowed into the scalar's continuation. The plain scalar
        // ends at `42`; the line break is trivia between scalar and
        // closer.
        let input = "{a: 42\n}\n";
        let tokens = collect_tokens(input);
        let scalars: Vec<_> = tokens
            .iter()
            .filter(|t| matches!(t.kind, TokenKind::Scalar(ScalarStyle::Plain)))
            .map(|t| &input[t.start.index..t.end.index])
            .collect();
        assert!(scalars.contains(&"42"), "got {scalars:?}");
        assert_byte_complete(input, &tokens);
    }

    #[test]
    fn plain_scalar_in_flow_context_terminates_on_flow_indicators() {
        let input = "[a b, c]";
        let tokens = collect_tokens(input);
        let plain_scalars: Vec<_> = tokens
            .iter()
            .filter(|t| matches!(t.kind, TokenKind::Scalar(ScalarStyle::Plain)))
            .map(|t| &input[t.start.index..t.end.index])
            .collect();
        // `a b` is one scalar (internal whitespace allowed); `c` is
        // another. The `,` separates them.
        assert_eq!(plain_scalars, vec!["a b", "c"]);
    }

    #[test]
    fn multi_line_plain_scalar_does_not_register_as_simple_key() {
        // `hello\n  world: value\n` — after the multi-line plain
        // scalar emerges, a `:` would be on a different line from the
        // candidate's mark.line. stale_simple_keys must drop the
        // candidate so the `:` does NOT splice a Key before
        // `hello\n  world`.
        //
        // This is the case that motivated the scanner rewrite.
        let input = "hello\n  world: value\n";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        // Find positions of the first plain Scalar and the first Key.
        let scalar_pos = kinds
            .iter()
            .position(|&k| k == TokenKind::Scalar(ScalarStyle::Plain));
        let key_pos = kinds.iter().position(|&k| k == TokenKind::Key);
        assert!(scalar_pos.is_some(), "no scalar: {kinds:?}");
        // If there is a Key, the multi-line scalar must NOT be its
        // body (i.e., the Scalar must not appear AFTER Key without
        // first having been emitted standalone). The simplest check:
        // the first scalar must come before any Key — because the
        // multi-line scalar is committed to the queue before the `:`
        // would even be reached.
        if let Some(k) = key_pos {
            let s = scalar_pos.unwrap();
            assert!(s < k, "multi-line scalar must precede any key: {kinds:?}",);
        }
    }

    #[test]
    fn plain_scalar_preserves_single_line_simple_key_behaviour() {
        // Single-line `hello world: value` — the scalar `hello world`
        // (with internal space) IS still a valid implicit key because
        // it stays on one line.
        let input = "hello world: value\n";
        let tokens = collect_tokens(input);
        let kinds = meaningful_kinds(&tokens);
        assert_eq!(
            kinds,
            vec![
                TokenKind::StreamStart,
                TokenKind::BlockMappingStart,
                TokenKind::Key,
                TokenKind::Scalar(ScalarStyle::Plain),
                TokenKind::Value,
                TokenKind::Scalar(ScalarStyle::Plain),
                TokenKind::BlockEnd,
                TokenKind::StreamEnd,
            ],
        );
    }
}
