use super::model::{YamlDiagnostic, YamlToken, YamlTokenSpan};

struct YamlLine<'a> {
    line: &'a str,
    newline: &'a str,
}

struct YamlLexer<'a> {
    input: &'a str,
    cursor: usize,
}

impl<'a> YamlLexer<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, cursor: 0 }
    }

    fn next_line(&mut self) -> Option<YamlLine<'a>> {
        if self.cursor >= self.input.len() {
            return None;
        }

        let remaining = &self.input[self.cursor..];
        if let Some(rel_lf_idx) = remaining.find('\n') {
            let lf_idx = self.cursor + rel_lf_idx;
            let newline_start =
                if lf_idx > self.cursor && self.input.as_bytes()[lf_idx - 1] == b'\r' {
                    lf_idx - 1
                } else {
                    lf_idx
                };

            let line = &self.input[self.cursor..newline_start];
            let newline = &self.input[newline_start..=lf_idx];
            self.cursor = lf_idx + 1;
            return Some(YamlLine { line, newline });
        }

        let line = &self.input[self.cursor..];
        self.cursor = self.input.len();
        Some(YamlLine { line, newline: "" })
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum QuoteMode {
    #[default]
    Plain,
    Single,
    Double,
}

#[derive(Default)]
struct LexerState {
    quote_mode: QuoteMode,
    escaped_in_double: bool,
    flow_depth: usize,
}

fn leading_indent(text: &str) -> usize {
    text.bytes()
        .take_while(|b| *b == b' ' || *b == b'\t')
        .count()
}

fn split_once_unquoted(text: &str, separator: char) -> Option<(&str, &str)> {
    let mut state = LexerState::default();
    let idx = find_unquoted_char_with_state(text, separator, &mut state)?;
    let rhs_start = idx + separator.len_utf8();
    Some((&text[..idx], &text[rhs_start..]))
}

fn parse_raw_mapping_line(line: &str) -> Option<(&str, &str)> {
    let (raw_key, raw_value) = split_once_unquoted(line, ':')?;
    if raw_key.trim().is_empty() || raw_value.trim().is_empty() {
        return None;
    }
    Some((raw_key, raw_value))
}

fn split_value_and_comment(raw_value: &str) -> (&str, Option<&str>) {
    let mut state = LexerState::default();
    if let Some(idx) = find_unquoted_char_with_state(raw_value, '#', &mut state) {
        let (before, after) = raw_value.split_at(idx);
        let starts_comment = before.chars().next_back().is_none_or(char::is_whitespace);
        if starts_comment {
            let trimmed_before = before.trim_end_matches([' ', '\t']);
            return (trimmed_before, Some(after));
        }
    }
    (raw_value, None)
}

fn find_unquoted_char_with_state(
    text: &str,
    target: char,
    state: &mut LexerState,
) -> Option<usize> {
    let mut chars = text.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        let next_char = chars.peek().map(|(_, next)| *next);

        if state.quote_mode == QuoteMode::Double {
            if state.escaped_in_double {
                state.escaped_in_double = false;
                continue;
            }
            match ch {
                '\\' => {
                    state.escaped_in_double = true;
                    continue;
                }
                '"' => {
                    state.quote_mode = QuoteMode::Plain;
                    continue;
                }
                _ => continue,
            }
        }

        if state.quote_mode == QuoteMode::Single {
            if ch == '\'' {
                if next_char == Some('\'') {
                    chars.next();
                    continue;
                }
                state.quote_mode = QuoteMode::Plain;
            }
            continue;
        }

        match ch {
            '\'' => state.quote_mode = QuoteMode::Single,
            '"' => state.quote_mode = QuoteMode::Double,
            '{' | '[' => {
                state.flow_depth = state.flow_depth.saturating_add(1);
            }
            '}' | ']' => {
                state.flow_depth = state.flow_depth.saturating_sub(1);
            }
            _ if ch == target && (target != ':' || state.flow_depth == 0) => return Some(idx),
            _ => {}
        }
    }

    None
}

fn split_tag_prefix(text: &str) -> (Option<&str>, &str) {
    let trimmed = text.trim_start_matches([' ', '\t']);
    if !trimmed.starts_with("!!") {
        return (None, text);
    }

    let rel_start = text.len() - trimmed.len();
    let rest = &text[rel_start + 2..];
    let end_rel = rest
        .char_indices()
        .find_map(|(i, ch)| (ch == ' ' || ch == '\t').then_some(i))
        .unwrap_or(rest.len());
    if end_rel == 0 {
        return (None, text);
    }

    let tag_end = rel_start + 2 + end_rel;
    let tag = &text[rel_start..tag_end];
    let value = &text[tag_end..];
    (Some(tag), value)
}

fn contains_unquoted_mapping_indicator(text: &str) -> bool {
    let mut chars = text.char_indices().peekable();
    let mut state = LexerState::default();

    while let Some((_, ch)) = chars.next() {
        let next_char = chars.peek().map(|(_, next)| *next);

        if state.quote_mode == QuoteMode::Double {
            if state.escaped_in_double {
                state.escaped_in_double = false;
                continue;
            }
            match ch {
                '\\' => {
                    state.escaped_in_double = true;
                    continue;
                }
                '"' => {
                    state.quote_mode = QuoteMode::Plain;
                    continue;
                }
                _ => continue,
            }
        }

        if state.quote_mode == QuoteMode::Single {
            if ch == '\'' {
                if next_char == Some('\'') {
                    chars.next();
                    continue;
                }
                state.quote_mode = QuoteMode::Plain;
            }
            continue;
        }

        match ch {
            '\'' => state.quote_mode = QuoteMode::Single,
            '"' => state.quote_mode = QuoteMode::Double,
            '{' | '[' => state.flow_depth = state.flow_depth.saturating_add(1),
            '}' | ']' => state.flow_depth = state.flow_depth.saturating_sub(1),
            ':' if state.flow_depth == 0 && next_char.is_some_and(char::is_whitespace) => {
                return true;
            }
            _ => {}
        }
    }

    false
}

fn is_valid_double_quote_escape(ch: char) -> bool {
    matches!(
        ch,
        '0' | 'a'
            | 'b'
            | 't'
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
            | 'x'
            | 'u'
            | 'U'
    )
}

fn invalid_double_quote_escape_offset(text: &str) -> Option<usize> {
    let mut chars = text.char_indices().peekable();
    let mut in_double = false;
    let mut escape_start: Option<usize> = None;

    while let Some((idx, ch)) = chars.next() {
        if !in_double {
            if ch == '"' {
                in_double = true;
            }
            continue;
        }

        if let Some(start) = escape_start.take() {
            if !is_valid_double_quote_escape(ch) {
                return Some(start);
            }
            continue;
        }

        match ch {
            '\\' => {
                if chars.peek().is_none() {
                    return Some(idx);
                }
                escape_start = Some(idx);
            }
            '"' => in_double = false,
            _ => {}
        }
    }

    None
}

fn flow_delimiter_delta(text: &str) -> isize {
    let mut chars = text.char_indices().peekable();
    let mut state = LexerState::default();
    let mut delta = 0isize;

    while let Some((_, ch)) = chars.next() {
        let next_char = chars.peek().map(|(_, next)| *next);

        if state.quote_mode == QuoteMode::Double {
            if state.escaped_in_double {
                state.escaped_in_double = false;
                continue;
            }
            match ch {
                '\\' => {
                    state.escaped_in_double = true;
                }
                '"' => state.quote_mode = QuoteMode::Plain,
                _ => {}
            }
            continue;
        }

        if state.quote_mode == QuoteMode::Single {
            if ch == '\'' {
                if next_char == Some('\'') {
                    chars.next();
                    continue;
                }
                state.quote_mode = QuoteMode::Plain;
            }
            continue;
        }

        match ch {
            '\'' => state.quote_mode = QuoteMode::Single,
            '"' => state.quote_mode = QuoteMode::Double,
            '#' => break,
            '[' | '{' => delta += 1,
            ']' | '}' => delta -= 1,
            _ => {}
        }
    }

    delta
}

fn flow_token_kind(ch: char) -> Option<YamlToken> {
    match ch {
        '{' => Some(YamlToken::FlowMapStart),
        '}' => Some(YamlToken::FlowMapEnd),
        '[' => Some(YamlToken::FlowSeqStart),
        ']' => Some(YamlToken::FlowSeqEnd),
        ',' => Some(YamlToken::Comma),
        _ => None,
    }
}

fn push_token<'a>(out: &mut Vec<YamlTokenSpan<'a>>, kind: YamlToken, text: &'a str) {
    out.push(YamlTokenSpan::new(kind, text));
}

fn assign_token_byte_ranges(
    input: &str,
    tokens: &mut [YamlTokenSpan<'_>],
) -> Result<(), YamlDiagnostic> {
    let mut offset = 0usize;
    for token in tokens {
        if token.text.is_empty() {
            token.byte_start = offset;
            token.byte_end = offset;
            continue;
        }

        if !input[offset..].starts_with(token.text) {
            return Err(YamlDiagnostic {
                code: "YAML_LEX_ERROR",
                message: "internal token range reconstruction mismatch",
                byte_start: offset,
                byte_end: offset,
            });
        }

        token.byte_start = offset;
        offset += token.text.len();
        token.byte_end = offset;
    }

    if offset == input.len() {
        Ok(())
    } else {
        Err(YamlDiagnostic {
            code: "YAML_LEX_ERROR",
            message: "token stream did not cover full input",
            byte_start: offset,
            byte_end: input.len(),
        })
    }
}

fn emit_scalar_like_tokens<'a>(text: &'a str, out: &mut Vec<YamlTokenSpan<'a>>) {
    if text.is_empty() {
        return;
    }

    let mut state = LexerState::default();
    let mut chunk_start = 0usize;
    for (idx, ch) in text.char_indices() {
        if state.quote_mode == QuoteMode::Double {
            if state.escaped_in_double {
                state.escaped_in_double = false;
                continue;
            }
            match ch {
                '\\' => state.escaped_in_double = true,
                '"' => state.quote_mode = QuoteMode::Plain,
                _ => {}
            }
            continue;
        }

        if state.quote_mode == QuoteMode::Single {
            if ch == '\'' {
                state.quote_mode = QuoteMode::Plain;
            }
            continue;
        }

        match ch {
            '\'' => state.quote_mode = QuoteMode::Single,
            '"' => state.quote_mode = QuoteMode::Double,
            _ => {
                if let Some(kind) = flow_token_kind(ch) {
                    if chunk_start < idx {
                        push_token(out, YamlToken::Scalar, &text[chunk_start..idx]);
                    }
                    push_token(out, kind, &text[idx..idx + ch.len_utf8()]);
                    chunk_start = idx + ch.len_utf8();
                }
            }
        }
    }

    if chunk_start < text.len() {
        push_token(out, YamlToken::Scalar, &text[chunk_start..]);
    }
}

fn lex_mapping_line_tokens<'a>(
    line: &'a str,
    newline: &'a str,
    line_start: usize,
    current_indent: usize,
    indent_stack: &mut Vec<usize>,
    out: &mut Vec<YamlTokenSpan<'a>>,
) -> Result<(), YamlDiagnostic> {
    let line_indent = leading_indent(line);
    let content = &line[line_indent..];

    if content.trim().is_empty() {
        if !newline.is_empty() {
            push_token(out, YamlToken::Newline, newline);
        }
        return Ok(());
    }

    if line_indent > current_indent {
        indent_stack.push(line_indent);
        push_token(out, YamlToken::Indent, "");
    } else if line_indent < current_indent {
        while let Some(last) = indent_stack.last().copied() {
            if line_indent < last {
                indent_stack.pop();
                push_token(out, YamlToken::Dedent, "");
            } else {
                break;
            }
        }
        if indent_stack.last().copied().unwrap_or(0) != line_indent {
            return Err(YamlDiagnostic {
                code: "YAML_LEX_ERROR",
                message: "invalid indentation level for YAML block mapping",
                byte_start: line_start,
                byte_end: line_start + line.len(),
            });
        }
    }

    if line_indent > 0 {
        push_token(out, YamlToken::Whitespace, &line[..line_indent]);
    }

    let trimmed = content.trim();
    if trimmed == "---" {
        push_token(out, YamlToken::DocumentStart, content);
        if !newline.is_empty() {
            push_token(out, YamlToken::Newline, newline);
        }
        return Ok(());
    }
    if trimmed.starts_with("---") {
        return Err(YamlDiagnostic {
            code: "YAML_LEX_TRAILING_CONTENT_AFTER_DOCUMENT_START",
            message: "trailing content after document start marker",
            byte_start: line_start + line_indent,
            byte_end: line_start + line.len(),
        });
    }
    if trimmed == "..." {
        push_token(out, YamlToken::DocumentEnd, content);
        if !newline.is_empty() {
            push_token(out, YamlToken::Newline, newline);
        }
        return Ok(());
    }
    if trimmed.starts_with("...") {
        return Err(YamlDiagnostic {
            code: "YAML_LEX_TRAILING_CONTENT_AFTER_DOCUMENT_END",
            message: "trailing content after document end marker",
            byte_start: line_start + line_indent,
            byte_end: line_start + line.len(),
        });
    }
    if trimmed.starts_with('%') {
        push_token(out, YamlToken::Directive, content);
        if !newline.is_empty() {
            push_token(out, YamlToken::Newline, newline);
        }
        return Ok(());
    }

    let Some((raw_key, raw_value)) = parse_raw_mapping_line(content) else {
        if split_once_unquoted(content, ':').is_some() {
            return Err(YamlDiagnostic {
                code: "YAML_LEX_ERROR",
                message: "invalid plain mapping line",
                byte_start: line_start + line_indent,
                byte_end: line_start + line.len(),
            });
        }
        if let Some(rel_idx) = invalid_double_quote_escape_offset(content) {
            return Err(YamlDiagnostic {
                code: "YAML_LEX_INVALID_DOUBLE_QUOTED_ESCAPE",
                message: "invalid escape in double quoted scalar",
                byte_start: line_start + line_indent + rel_idx,
                byte_end: line_start + line_indent + rel_idx + 1,
            });
        }
        emit_scalar_like_tokens(content, out);
        if !newline.is_empty() {
            push_token(out, YamlToken::Newline, newline);
        }
        return Ok(());
    };

    let (key_tag, key_text) = split_tag_prefix(raw_key);
    if let Some(tag) = key_tag {
        push_token(out, YamlToken::Tag, tag);
        let ws_len = leading_indent(key_text);
        if ws_len > 0 {
            push_token(out, YamlToken::Whitespace, &key_text[..ws_len]);
        }
        push_token(out, YamlToken::Key, &key_text[ws_len..]);
    } else {
        push_token(out, YamlToken::Key, raw_key);
    }

    push_token(out, YamlToken::Colon, ":");

    let (value_part, comment_part) = split_value_and_comment(raw_value);
    let leading_ws_len = leading_indent(value_part);
    if leading_ws_len > 0 {
        push_token(out, YamlToken::Whitespace, &value_part[..leading_ws_len]);
    }

    let scalar_part = &value_part[leading_ws_len..];
    let (value_tag, value_text) = split_tag_prefix(scalar_part);
    if let Some(tag) = value_tag {
        push_token(out, YamlToken::Tag, tag);
        let ws_len = leading_indent(value_text);
        if ws_len > 0 {
            push_token(out, YamlToken::Whitespace, &value_text[..ws_len]);
        }
        let tagged_scalar = &value_text[ws_len..];
        if let Some(rel_idx) = invalid_double_quote_escape_offset(tagged_scalar) {
            return Err(YamlDiagnostic {
                code: "YAML_LEX_INVALID_DOUBLE_QUOTED_ESCAPE",
                message: "invalid escape in double quoted scalar",
                byte_start: line_start
                    + line_indent
                    + raw_key.len()
                    + 1
                    + leading_ws_len
                    + ws_len
                    + rel_idx,
                byte_end: line_start
                    + line_indent
                    + raw_key.len()
                    + 1
                    + leading_ws_len
                    + ws_len
                    + rel_idx
                    + 1,
            });
        }
        if contains_unquoted_mapping_indicator(tagged_scalar) {
            return Err(YamlDiagnostic {
                code: "YAML_LEX_ERROR",
                message: "invalid plain scalar containing mapping indicator sequence",
                byte_start: line_start + line_indent + raw_key.len() + 1,
                byte_end: line_start + line.len(),
            });
        }
        push_token(out, YamlToken::Scalar, tagged_scalar);
    } else {
        if let Some(rel_idx) = invalid_double_quote_escape_offset(scalar_part) {
            return Err(YamlDiagnostic {
                code: "YAML_LEX_INVALID_DOUBLE_QUOTED_ESCAPE",
                message: "invalid escape in double quoted scalar",
                byte_start: line_start + line_indent + raw_key.len() + 1 + leading_ws_len + rel_idx,
                byte_end: line_start
                    + line_indent
                    + raw_key.len()
                    + 1
                    + leading_ws_len
                    + rel_idx
                    + 1,
            });
        }
        if contains_unquoted_mapping_indicator(scalar_part) {
            return Err(YamlDiagnostic {
                code: "YAML_LEX_ERROR",
                message: "invalid plain scalar containing mapping indicator sequence",
                byte_start: line_start + line_indent + raw_key.len() + 1,
                byte_end: line_start + line.len(),
            });
        }
        emit_scalar_like_tokens(scalar_part, out);
    }

    if let Some(comment) = comment_part {
        let leading_comment_ws_len = raw_value.len() - comment.len() - value_part.len();
        if leading_comment_ws_len > 0 {
            let start = value_part.len();
            let end = start + leading_comment_ws_len;
            push_token(out, YamlToken::Whitespace, &raw_value[start..end]);
        }
        push_token(out, YamlToken::Comment, comment);
    }

    if !newline.is_empty() {
        push_token(out, YamlToken::Newline, newline);
    }

    Ok(())
}

pub fn lex_mapping_tokens_with_diagnostic(
    input: &str,
) -> Result<Vec<YamlTokenSpan<'_>>, YamlDiagnostic> {
    if input.is_empty() {
        return Err(YamlDiagnostic {
            code: "YAML_LEX_ERROR",
            message: "empty YAML input",
            byte_start: 0,
            byte_end: 0,
        });
    }

    let mut tokens = Vec::new();
    let mut indent_stack = vec![0usize];
    let mut lexer = YamlLexer::new(input);
    let mut line_start = 0usize;
    let mut flow_depth: isize = 0;
    let mut flow_base_indent: usize = 0;
    let mut flow_requires_indent = false;

    while let Some(yaml_line) = lexer.next_line() {
        let line_indent = leading_indent(yaml_line.line);
        let content = &yaml_line.line[line_indent..];
        if flow_depth > 0
            && flow_requires_indent
            && !content.trim().is_empty()
            && line_indent <= flow_base_indent
        {
            return Err(YamlDiagnostic {
                code: "YAML_LEX_WRONG_INDENTED_FLOW",
                message: "wrong indentation for continued flow collection",
                byte_start: line_start,
                byte_end: line_start + yaml_line.line.len(),
            });
        }

        let current_indent = indent_stack.last().copied().unwrap_or(0);
        lex_mapping_line_tokens(
            yaml_line.line,
            yaml_line.newline,
            line_start,
            current_indent,
            &mut indent_stack,
            &mut tokens,
        )?;

        let delta = flow_delimiter_delta(content);
        if flow_depth == 0 && delta > 0 {
            flow_base_indent = line_indent;
            flow_requires_indent = parse_raw_mapping_line(content)
                .map(|(_, raw_value)| flow_delimiter_delta(raw_value) > 0)
                .unwrap_or(false);
        }
        flow_depth += delta;
        if flow_depth < 0 {
            flow_depth = 0;
        }
        if flow_depth == 0 {
            flow_requires_indent = false;
        }

        line_start += yaml_line.line.len() + yaml_line.newline.len();
    }

    while indent_stack.len() > 1 {
        indent_stack.pop();
        push_token(&mut tokens, YamlToken::Dedent, "");
    }

    assign_token_byte_ranges(input, &mut tokens)?;

    Ok(tokens)
}

pub fn lex_mapping_tokens(input: &str) -> Option<Vec<YamlTokenSpan<'_>>> {
    lex_mapping_tokens_with_diagnostic(input).ok()
}
