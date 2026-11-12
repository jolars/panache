//! Math parsing for both inline and display math.
//!
//! This module handles all math-related parsing:
//! - **Inline math**: `$...$`, `$`...`$`, `\(...\)`, `\\(...\\)`. Under the
//!   Pandoc dialect these may span a single newline within a paragraph (a blank
//!   line ends the span); under the CommonMark dialect (GFM, etc.) they are
//!   single line only. Callers pass `allow_multiline` from `dialect == Pandoc`.
//! - **Display math**: `$$...$$`, `\[...\]`, `\\[...\\]` - can span multiple lines
//!
//! Display math can appear both inline (within paragraphs) and as block-level elements.
//! The parsing functions return `Option<(usize, &str)>` tuples containing the length
//! consumed and the math content, allowing calling contexts to emit appropriate nodes.

use super::sink::InlineSink;
use crate::parser::inlines::bookdown::try_parse_bookdown_equation_definition;
use crate::parser::math::{MathParseOptions, parse_math_content};
use crate::parser::utils::tree_copy::copy_green_node;
use crate::syntax::SyntaxKind;

/// Emit structural TeX segments and host-owned Bookdown labels in source order.
///
/// Equation labels are Markdown-flavor syntax, not TeX. They therefore sit
/// beside `MATH_CONTENT` rather than inside it. Splitting at those labels keeps
/// every token's range aligned with the host document while the remaining
/// segments stay ordinary, independently parseable TeX subtrees.
fn emit_math_content(builder: &mut impl InlineSink, content: &str, opts: MathParseOptions) {
    if !opts.bookdown_equation_labels {
        copy_green_node(builder, &parse_math_content(content, opts));
        return;
    }

    let tex_opts = MathParseOptions {
        bookdown_equation_labels: false,
    };
    let bytes = content.as_bytes();
    let mut segment_start = 0;
    let mut pos = 0;
    // Only a label at the top level may split the content: splitting inside a
    // group or an environment would leave each half unbalanced, and a label
    // written inside a `%` comment is prose, not a definition.
    let mut brace_depth = 0usize;
    let mut environment_depth = 0usize;
    while pos < content.len() {
        match bytes[pos] {
            b'%' => {
                pos = content[pos..]
                    .find('\n')
                    .map_or(content.len(), |offset| pos + offset + 1);
                continue;
            }
            b'\\' => {
                pos += match control_word_at(content, pos) {
                    Some(name) => {
                        match name {
                            "begin" => environment_depth += 1,
                            "end" => environment_depth = environment_depth.saturating_sub(1),
                            _ => {}
                        }
                        1 + name.len()
                    }
                    // A control symbol escapes whatever character it carries,
                    // so `\{`, `\}`, and `\%` are literal text.
                    None => 1 + content[pos + 1..].chars().next().map_or(0, char::len_utf8),
                };
                continue;
            }
            b'{' => {
                brace_depth += 1;
                pos += 1;
                continue;
            }
            b'}' => {
                brace_depth = brace_depth.saturating_sub(1);
                pos += 1;
                continue;
            }
            b'(' if brace_depth == 0 && environment_depth == 0 => {
                if let Some((len, _)) = try_parse_bookdown_equation_definition(&content[pos..]) {
                    copy_green_node(
                        builder,
                        &parse_math_content(&content[segment_start..pos], tex_opts),
                    );
                    builder.token(
                        SyntaxKind::MATH_EQUATION_LABEL.into(),
                        &content[pos..pos + len],
                    );
                    pos += len;
                    segment_start = pos;
                    continue;
                }
            }
            _ => {}
        }
        pos += content[pos..]
            .chars()
            .next()
            .expect("position is before the end of content")
            .len_utf8();
    }
    copy_green_node(
        builder,
        &parse_math_content(&content[segment_start..], tex_opts),
    );
}

/// The control-word name at `pos`, using the math parser's `[A-Za-z@]`
/// alphabet. `None` for a control symbol such as `\\` or `\{`.
fn control_word_at(text: &str, pos: usize) -> Option<&str> {
    let after = text[pos..].strip_prefix('\\')?;
    let len = after
        .bytes()
        .take_while(|byte| byte.is_ascii_alphabetic() || *byte == b'@')
        .count();
    (len > 0).then(|| &after[..len])
}

/// Derive math-content parse options from the parser config. Keeps the
/// flavor/extension → math-grammar mapping in one place.
pub fn math_opts(config: &crate::options::ParserOptions) -> MathParseOptions {
    MathParseOptions {
        bookdown_equation_labels: config.extensions.bookdown_equation_references,
    }
}

/// Whether a newline reached inside an inline math span ends the math.
///
/// When `allow_multiline` is false (CommonMark dialect — GFM, etc.) any newline
/// ends the span: inline math is single line only. When true (Pandoc dialect)
/// the span may fold a single newline within a paragraph and only a blank line
/// — a newline followed by only whitespace and then another line break or end
/// of input — terminates it. `after_nl` is the text immediately following the
/// newline. By the time inline parsing runs the block parser has already split
/// paragraphs at blank lines, so the blank-line arm is a defensive guard.
fn newline_ends_inline_math(after_nl: &str, allow_multiline: bool) -> bool {
    if !allow_multiline {
        return true;
    }
    let next = after_nl.trim_start_matches([' ', '\t']);
    next.is_empty() || next.starts_with('\n') || next.starts_with('\r')
}

/// Try to parse an inline math span starting at the current position.
/// Returns the number of characters consumed if successful, or None if not inline math.
///
/// Per Pandoc spec (tex_math_dollars extension):
/// - Opening $ must have non-space character immediately to its right
/// - Closing $ must have non-space character immediately to its left
/// - Closing $ must not be followed immediately by a digit
pub fn try_parse_inline_math(text: &str, allow_multiline: bool) -> Option<(usize, &str)> {
    if !text.starts_with('$') || text.starts_with("$$") {
        return None;
    }

    let rest = &text[1..];

    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        return None;
    }

    let mut pos = 0;
    while pos < rest.len() {
        let ch = rest[pos..].chars().next()?;

        if ch == '$' {
            if pos > 0 && rest.as_bytes()[pos - 1] == b'\\' {
                pos += 1;
                continue;
            }

            if pos == 0 || rest[..pos].ends_with(char::is_whitespace) {
                pos += 1;
                continue;
            }

            if let Some(next_ch) = rest[pos + 1..].chars().next()
                && next_ch.is_ascii_digit()
            {
                pos += 1;
                continue;
            }

            let math_content = &rest[..pos];
            let total_len = 1 + pos + 1; // opening $ + content + closing $
            return Some((total_len, math_content));
        }

        if ch == '\n' && newline_ends_inline_math(&rest[pos + 1..], allow_multiline) {
            return None;
        }

        pos += ch.len_utf8();
    }

    None
}

/// Try to parse GFM inline math: $`...`$
/// Extension: tex_math_gfm
pub fn try_parse_gfm_inline_math(text: &str, allow_multiline: bool) -> Option<(usize, &str)> {
    if !text.starts_with("$`") {
        return None;
    }

    let rest = &text[2..];
    if rest.is_empty() {
        return None;
    }

    let mut pos = 0;
    while pos < rest.len() {
        let ch = rest[pos..].chars().next()?;
        if ch == '\n' && newline_ends_inline_math(&rest[pos + 1..], allow_multiline) {
            return None;
        }
        if rest[pos..].starts_with("`$") {
            if pos == 0 {
                return None;
            }
            let math_content = &rest[..pos];
            let total_len = 2 + pos + 2; // $` + content + `$
            return Some((total_len, math_content));
        }
        pos += ch.len_utf8();
    }

    None
}

/// Try to parse single backslash inline math: \(...\)
/// Extension: tex_math_single_backslash
pub fn try_parse_single_backslash_inline_math(
    text: &str,
    allow_multiline: bool,
) -> Option<(usize, &str)> {
    if !text.starts_with(r"\(") {
        return None;
    }

    let rest = &text[2..]; // Skip \(

    let mut pos = 0;
    while pos < rest.len() {
        let ch = rest[pos..].chars().next()?;

        if ch == '\\' && rest[pos..].starts_with(r"\)") {
            let math_content = &rest[..pos];
            let total_len = 2 + pos + 2; // \( + content + \)
            return Some((total_len, math_content));
        }

        if ch == '\n' && newline_ends_inline_math(&rest[pos + 1..], allow_multiline) {
            return None;
        }

        pos += ch.len_utf8();
    }

    None
}

/// Try to parse double backslash inline math: \\(...\\)
/// Extension: tex_math_double_backslash
pub fn try_parse_double_backslash_inline_math(
    text: &str,
    allow_multiline: bool,
) -> Option<(usize, &str)> {
    if !text.starts_with(r"\\(") {
        return None;
    }

    let rest = &text[3..]; // Skip \\(

    let mut pos = 0;
    while pos < rest.len() {
        let ch = rest[pos..].chars().next()?;

        if ch == '\\' && rest[pos..].starts_with(r"\\)") {
            let math_content = &rest[..pos];
            let total_len = 3 + pos + 3; // \\( + content + \\)
            return Some((total_len, math_content));
        }

        if ch == '\n' && newline_ends_inline_math(&rest[pos + 1..], allow_multiline) {
            return None;
        }

        pos += ch.len_utf8();
    }

    None
}

/// Try to parse display math (`$$...$$`) starting at the current position.
/// Returns the number of bytes consumed and the math content if successful.
/// Display math can span multiple lines in inline contexts.
///
/// Per Pandoc (`tex_math_dollars`, `mathDisplayWith "$$" "$$"`):
/// - Both delimiters are *exactly* two dollars. A longer opening run puts its
///   extra dollars into the content (`$$$x$$` is display math over `$x`); a
///   longer closing run leaves its extra dollars in the document as text
///   (`$$x$$$` is display math over `x` followed by a literal `$`). Consuming
///   more than two closing dollars while emitting a two-dollar marker used to
///   drop those bytes outright.
/// - The content must be non-empty (`many1Till`), so `$$$$` is literal text.
/// - There is no escape handling inside the span: the first `$$` after the
///   first content character closes it.
pub fn try_parse_display_math(text: &str) -> Option<(usize, &str)> {
    const DELIM: &str = "$$";

    let rest = text.strip_prefix(DELIM)?;

    if rest.starts_with(DELIM) {
        return None;
    }
    let first_len = rest.chars().next()?.len_utf8();

    let close = first_len + rest[first_len..].find(DELIM)?;
    let math_content = &rest[..close];
    Some((DELIM.len() + close + DELIM.len(), math_content))
}

/// Try to parse single backslash display math: \[...\]
/// Extension: tex_math_single_backslash
///
/// Per Pandoc spec:
/// - Content can span multiple lines
/// - No escape handling needed (backslash is the delimiter)
pub fn try_parse_single_backslash_display_math(text: &str) -> Option<(usize, &str)> {
    if !text.starts_with(r"\[") {
        return None;
    }

    let rest = &text[2..]; // Skip \[

    let mut pos = 0;
    while pos < rest.len() {
        let ch = rest[pos..].chars().next()?;

        if ch == '\\' && rest[pos..].starts_with(r"\]") {
            let math_content = &rest[..pos];
            let total_len = 2 + pos + 2; // \[ + content + \]
            return Some((total_len, math_content));
        }

        pos += ch.len_utf8();
    }

    None
}

/// Try to parse double backslash display math: \\[...\\]
/// Extension: tex_math_double_backslash
///
/// Per Pandoc spec:
/// - Content can span multiple lines
/// - Double backslash is the delimiter
pub fn try_parse_double_backslash_display_math(text: &str) -> Option<(usize, &str)> {
    if !text.starts_with(r"\\[") {
        return None;
    }

    let rest = &text[3..]; // Skip \\[

    let mut pos = 0;
    while pos < rest.len() {
        let ch = rest[pos..].chars().next()?;

        if ch == '\\' && rest[pos..].starts_with(r"\\]") {
            let math_content = &rest[..pos];
            let total_len = 3 + pos + 3; // \\[ + content + \\]
            return Some((total_len, math_content));
        }

        pos += ch.len_utf8();
    }

    None
}

/// Emit an inline math node to the builder.
pub fn emit_inline_math(builder: &mut impl InlineSink, content: &str, opts: MathParseOptions) {
    builder.start_node(SyntaxKind::INLINE_MATH.into());

    builder.token(SyntaxKind::INLINE_MATH_MARKER.into(), "$");

    emit_math_content(builder, content, opts);

    builder.token(SyntaxKind::INLINE_MATH_MARKER.into(), "$");

    builder.finish_node();
}

/// Emit a GFM inline math node: $`...`$
pub fn emit_gfm_inline_math(builder: &mut impl InlineSink, content: &str, opts: MathParseOptions) {
    builder.start_node(SyntaxKind::INLINE_MATH.into());
    builder.token(SyntaxKind::INLINE_MATH_MARKER.into(), "$`");
    emit_math_content(builder, content, opts);
    builder.token(SyntaxKind::INLINE_MATH_MARKER.into(), "`$");
    builder.finish_node();
}

/// Emit a single backslash inline math node: \(...\)
pub fn emit_single_backslash_inline_math(
    builder: &mut impl InlineSink,
    content: &str,
    opts: MathParseOptions,
) {
    builder.start_node(SyntaxKind::INLINE_MATH.into());

    builder.token(SyntaxKind::INLINE_MATH_MARKER.into(), r"\(");
    emit_math_content(builder, content, opts);
    builder.token(SyntaxKind::INLINE_MATH_MARKER.into(), r"\)");

    builder.finish_node();
}

/// Emit a double backslash inline math node: \\(...\\)
pub fn emit_double_backslash_inline_math(
    builder: &mut impl InlineSink,
    content: &str,
    opts: MathParseOptions,
) {
    builder.start_node(SyntaxKind::INLINE_MATH.into());

    builder.token(SyntaxKind::INLINE_MATH_MARKER.into(), r"\\(");
    emit_math_content(builder, content, opts);
    builder.token(SyntaxKind::INLINE_MATH_MARKER.into(), r"\\)");

    builder.finish_node();
}

/// Emit a display math node to the builder (when occurring inline in paragraph).
///
/// The markers are always `$$` because [`try_parse_display_math`] consumes
/// exactly two dollars on each side; any surplus dollars are content or
/// trailing text and must be emitted by the caller.
pub fn emit_display_math(builder: &mut impl InlineSink, content: &str, opts: MathParseOptions) {
    builder.start_node(SyntaxKind::DISPLAY_MATH.into());

    builder.token(SyntaxKind::DISPLAY_MATH_MARKER.into(), "$$");

    emit_math_content(builder, content, opts);

    builder.token(SyntaxKind::DISPLAY_MATH_MARKER.into(), "$$");

    builder.finish_node();
}

/// Emit a single backslash display math node: \[...\]
pub fn emit_single_backslash_display_math(
    builder: &mut impl InlineSink,
    content: &str,
    opts: MathParseOptions,
) {
    builder.start_node(SyntaxKind::DISPLAY_MATH.into());

    builder.token(SyntaxKind::DISPLAY_MATH_MARKER.into(), r"\[");
    emit_math_content(builder, content, opts);
    builder.token(SyntaxKind::DISPLAY_MATH_MARKER.into(), r"\]");

    builder.finish_node();
}

/// Emit a double backslash display math node: \\[...\\]
pub fn emit_double_backslash_display_math(
    builder: &mut impl InlineSink,
    content: &str,
    opts: MathParseOptions,
) {
    builder.start_node(SyntaxKind::DISPLAY_MATH.into());

    builder.token(SyntaxKind::DISPLAY_MATH_MARKER.into(), r"\\[");
    emit_math_content(builder, content, opts);
    builder.token(SyntaxKind::DISPLAY_MATH_MARKER.into(), r"\\]");

    builder.finish_node();
}

/// Cheap candidate gate for block lookahead. Only the full matcher can verify
/// whether an opener actually matches, including its brace and escape rules.
fn may_open_jupyter_math(text: &str) -> bool {
    text.contains('$')
        || text.contains(r"\\(")
        || text.contains(r"\\[")
        || text.contains(r"\begin{")
}

/// Note openers that might continue onto another line without treating a
/// brace-depth fallback as a definite closer. Complete single-line math and
/// code spans need no block lookahead.
pub(crate) fn pending_jupyter_math(text: &str) -> bool {
    if !may_open_jupyter_math(text) {
        return false;
    }
    let mut pos = 0;
    while pos < text.len() {
        let rest = &text[pos..];
        if rest.starts_with('$')
            || rest.starts_with(r"\\(")
            || rest.starts_with(r"\\[")
            || rest.starts_with(r"\begin{")
        {
            if let Some((len, ..)) = match_jupyter_math(rest, false) {
                pos += len;
                continue;
            }
            return true;
        }
        if rest.starts_with('`')
            && let Some((len, ..)) = super::code_spans::try_parse_code_span(rest)
        {
            pos += len;
            continue;
        }
        pos += if let Some(after_escape) = rest.strip_prefix('\\') {
            1 + after_escape.chars().next().map_or(0, char::len_utf8)
        } else {
            rest.chars().next().expect("character boundary").len_utf8()
        };
    }
    false
}

/// JupyterLab's math boundaries (rendermime/src/latex.ts). Unlike Pandoc,
/// dollar delimiters allow adjacent whitespace/digits and single line breaks.
/// Markdown source uses doubled backslashes for bracket delimiters.
pub(crate) fn jupyter_math(text: &str) -> Option<(usize, &str, &str, &str)> {
    match_jupyter_math(text, true)
}

/// The physical lines occupied by one logical line after Jupyter extracts math.
/// A closer can share a line with another opener, so scan through the end of
/// each claimed line before deciding where a heading or table row ends.
pub(crate) fn jupyter_logical_line_len(text: &str) -> Option<usize> {
    let line_end = |pos: usize| text[pos..].find('\n').map_or(text.len(), |i| pos + i + 1);
    let first_end = line_end(0);
    if !pending_jupyter_math(&text[..first_end]) {
        return None;
    }
    let mut end = first_end;
    let mut pos = 0;
    while pos < end {
        let rest = &text[pos..];
        if matches!(rest.as_bytes()[0], b'$' | b'\\')
            && let Some((len, ..)) = jupyter_math(rest)
        {
            pos += len;
            if pos > end {
                end = line_end(pos);
            }
            continue;
        }
        if rest.starts_with("$$") {
            pos += 2;
            continue;
        }
        if rest.starts_with('`')
            && let Some((len, ..)) = super::code_spans::try_parse_code_span(rest)
        {
            pos += len;
            continue;
        }
        pos += if let Some(after_escape) = rest.strip_prefix('\\') {
            1 + after_escape.chars().next().map_or(0, char::len_utf8)
        } else {
            rest.chars().next()?.len_utf8()
        };
    }
    (end > first_end).then_some(end)
}

fn match_jupyter_math(text: &str, allow_fallback: bool) -> Option<(usize, &str, &str, &str)> {
    let (open, close) = if text.starts_with("$$") {
        ("$$", "$$")
    } else if text.starts_with('$') {
        ("$", "$")
    } else if text.starts_with(r"\\(") {
        (r"\\(", r"\\)")
    } else if text.starts_with(r"\\[") {
        (r"\\[", r"\\]")
    } else {
        let rest = text.strip_prefix(r"\begin{")?;
        let end = rest.find('}')?;
        let name = &rest[..end];
        if name.is_empty()
            || !name
                .trim_end_matches('*')
                .bytes()
                .all(|b| b.is_ascii_alphabetic())
        {
            return None;
        }
        let opening = &text[..7 + end + 1];
        let closing = format!(r"\end{{{name}}}");
        let body = &text[opening.len()..];
        let at = body.find(&closing)?;
        if body[..at]
            .split_inclusive('\n')
            .skip(1)
            .any(|line| line.ends_with('\n') && line.trim().is_empty())
        {
            return None;
        }
        return Some((
            opening.len() + at + closing.len(),
            &body[..at],
            opening,
            &text[opening.len() + at..opening.len() + at + closing.len()],
        ));
    };
    let mut pos = open.len();
    let mut depth = 0usize;
    let mut fallback = None;
    while pos < text.len() {
        let rest = &text[pos..];
        if close == "$" && rest.starts_with("$$") {
            pos += 2;
            continue;
        }
        if rest.starts_with(close) {
            let result = (pos + close.len(), &text[open.len()..pos], open, close);
            if depth == 0 {
                return Some(result);
            }
            fallback = Some(result);
        }
        let ch = rest.chars().next()?;
        if ch == '\n'
            && (rest[1..]
                .trim_start_matches([' ', '\t', '\r'])
                .starts_with('\n'))
        {
            break;
        }
        if matches!(ch, '`' | '~') {
            let run = rest.bytes().take_while(|b| *b == ch as u8).count();
            if run >= 3
                && text[..pos]
                    .rsplit('\n')
                    .next()
                    .is_some_and(|line| line.trim().is_empty())
            {
                let marker = &rest[..run];
                if let Some((end, _)) = rest[run..].match_indices(marker).find(|(at, _)| {
                    rest[..run + at]
                        .rsplit('\n')
                        .next()
                        .is_some_and(|line| line.trim().is_empty())
                }) {
                    pos += run + end + run;
                    continue;
                }
            }
        }
        if ch == '`' {
            let run = rest.bytes().take_while(|b| *b == b'`').count();
            let marker = &rest[..run];
            if let Some(end) = rest[run..].find(marker)
                && !rest[run..run + end].contains('\n')
            {
                pos += run + end + run;
                continue;
            }
        }
        if ch == '\\' {
            let next = rest[1..].chars().next();
            if next.is_some_and(|c| matches!(c, '{' | '}' | '$' | '\\')) {
                pos += 1 + next?.len_utf8();
                continue;
            }
        }
        if ch == '{' {
            depth += 1;
        }
        if ch == '}' {
            depth = depth.saturating_sub(1);
        }
        pos += ch.len_utf8();
    }
    if allow_fallback { fallback } else { None }
}

pub(crate) fn emit_jupyter_math(
    builder: &mut impl InlineSink,
    text: &str,
    len: usize,
    content: &str,
    open: &str,
    close: &str,
    opts: MathParseOptions,
) {
    if open.starts_with(r"\begin{") {
        super::latex::parse_latex_command(builder, text, len);
    } else {
        let kind = if open == "$$" || open == r"\\[" {
            SyntaxKind::DISPLAY_MATH
        } else {
            SyntaxKind::INLINE_MATH
        };
        let marker = if kind == SyntaxKind::DISPLAY_MATH {
            SyntaxKind::DISPLAY_MATH_MARKER
        } else {
            SyntaxKind::INLINE_MATH_MARKER
        };
        builder.start_node(kind.into());
        builder.token(marker.into(), open);
        emit_math_content(builder, content, opts);
        builder.token(marker.into(), close);
        builder.finish_node();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::SyntaxNode;

    const BOOKDOWN_MATH: MathParseOptions = MathParseOptions {
        bookdown_equation_labels: true,
    };

    fn emitted_inline_math(content: &str) -> SyntaxNode {
        let mut builder = rowan::GreenNodeBuilder::new();
        emit_inline_math(&mut builder, content, BOOKDOWN_MATH);
        SyntaxNode::new_root(builder.finish())
    }

    #[test]
    fn jupyter_environment_closer_indentation_is_not_a_blank_line() {
        for indent in ["  ", "\t"] {
            let text = format!("\\begin{{align}}\nx &= y\n{indent}\\end{{align}}");
            assert_eq!(jupyter_math(&text).map(|(len, ..)| len), Some(text.len()));
        }
        for newline in ["\n", "\r\n"] {
            let text = format!("\\begin{{align}}{newline}x{newline}  {newline}y\\end{{align}}");
            assert!(jupyter_math(&text).is_none());
        }
    }

    #[test]
    fn jupyter_math_only_masks_complete_fences() {
        for marker in ["~~~", "```"] {
            let text = format!("$$\n\\text{{a\n{marker}b}}\n$$");
            assert_eq!(jupyter_math(&text).map(|(len, ..)| len), Some(text.len()));

            let text = format!("$$\na\n{marker}\n$$\n{marker}\nb\n$$");
            assert_eq!(jupyter_math(&text).map(|(len, ..)| len), Some(text.len()));
        }
    }

    #[test]
    fn bookdown_equation_labels_are_host_children_of_math() {
        let math = emitted_inline_math(r"x (\#eq:inline) + y");
        let label = math
            .descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| token.kind() == SyntaxKind::MATH_EQUATION_LABEL)
            .expect("equation label");

        assert_eq!(label.text(), r"(\#eq:inline)");
        assert_eq!(usize::from(label.text_range().start()), 3);
        assert!(
            label
                .parent_ancestors()
                .all(|ancestor| ancestor.kind() != SyntaxKind::MATH_CONTENT)
        );
        assert_eq!(math.text().to_string(), r"$x (\#eq:inline) + y$");
    }

    #[test]
    fn test_parse_simple_inline_math() {
        let result = try_parse_inline_math("$x = y$", true);
        assert_eq!(result, Some((7, "x = y")));
    }

    #[test]
    fn test_parse_inline_math_with_spaces_inside() {
        let result = try_parse_inline_math("$a + b$", true);
        assert_eq!(result, Some((7, "a + b")));
    }

    #[test]
    fn test_parse_inline_math_complex() {
        let result = try_parse_inline_math(r"$\frac{1}{2}$", true);
        assert_eq!(result, Some((13, r"\frac{1}{2}")));
    }

    #[test]
    fn test_not_inline_math_display() {
        let result = try_parse_inline_math("$$x = y$$", true);
        assert_eq!(result, None);
    }

    #[test]
    fn test_inline_math_no_close() {
        let result = try_parse_inline_math("$no close", true);
        assert_eq!(result, None);
    }

    #[test]
    fn test_inline_math_spans_single_newline_pandoc() {
        let result = try_parse_inline_math("$x =\ny$", true);
        assert_eq!(result, Some((7, "x =\ny")));
    }

    #[test]
    fn test_inline_math_single_line_only_commonmark() {
        let result = try_parse_inline_math("$x =\ny$", false);
        assert_eq!(result, None);
    }

    #[test]
    fn test_inline_math_stops_at_blank_line() {
        let result = try_parse_inline_math("$x =\n\ny$", true);
        assert_eq!(result, None);
    }

    #[test]
    fn test_not_inline_math() {
        let result = try_parse_inline_math("no dollar", true);
        assert_eq!(result, None);
    }

    #[test]
    fn test_inline_math_with_trailing_text() {
        let result = try_parse_inline_math("$x$ and more", true);
        assert_eq!(result, Some((3, "x")));
    }

    #[test]
    fn test_spec_opening_must_have_non_space_right() {
        let result = try_parse_inline_math("$ x$", true);
        assert_eq!(result, None, "Opening $ with space should not parse");
    }

    #[test]
    fn test_spec_closing_must_have_non_space_left() {
        let result = try_parse_inline_math("$x $", true);
        assert_eq!(result, None, "Closing $ with space should not parse");
    }

    #[test]
    fn test_spec_closing_not_followed_by_digit() {
        let result = try_parse_inline_math("$x$5", true);
        assert_eq!(result, None, "Closing $ followed by digit should not parse");
    }

    #[test]
    fn test_spec_dollar_amounts() {
        let result = try_parse_inline_math("$20,000", true);
        assert_eq!(result, None, "Dollar amounts should not parse as math");
    }

    #[test]
    fn test_valid_math_after_spec_checks() {
        let result = try_parse_inline_math("$x$", true);
        assert_eq!(result, Some((3, "x")), "Valid math should parse");
    }

    #[test]
    fn test_math_followed_by_non_digit() {
        let result = try_parse_inline_math("$x$a", true);
        assert_eq!(
            result,
            Some((3, "x")),
            "Math followed by non-digit should parse"
        );
    }

    #[test]
    fn test_parse_display_math_simple() {
        let result = try_parse_display_math("$$x = y$$");
        assert_eq!(result, Some((9, "x = y")));
    }

    #[test]
    fn test_parse_display_math_multiline() {
        let result = try_parse_display_math("$$\nx = y\n$$");
        assert_eq!(result, Some((11, "\nx = y\n")));
    }

    #[test]
    fn test_parse_display_math_triple_dollars() {
        let result = try_parse_display_math("$$$x = y$$$");
        assert_eq!(result, Some((10, "$x = y")));
    }

    #[test]
    fn test_parse_display_math_long_closing_run_is_not_consumed() {
        let result = try_parse_display_math("$$\nx^2 + y\n$$$");
        assert_eq!(result, Some((13, "\nx^2 + y\n")));

        let result = try_parse_display_math("$$\nx^2 +$$$$");
        assert_eq!(result, Some((10, "\nx^2 +")));
    }

    #[test]
    fn test_parse_display_math_empty_content_is_literal() {
        assert_eq!(try_parse_display_math("$$$$"), None);
        assert_eq!(try_parse_display_math("$$$$$"), None);
    }

    #[test]
    fn test_parse_display_math_no_close() {
        let result = try_parse_display_math("$$no close");
        assert_eq!(result, None);
    }

    #[test]
    fn test_not_display_math() {
        let result = try_parse_display_math("$single dollar");
        assert_eq!(result, None);
    }

    #[test]
    fn test_display_math_with_trailing_text() {
        let result = try_parse_display_math("$$x = y$$ and more");
        assert_eq!(result, Some((9, "x = y")));
    }

    #[test]
    fn test_single_backslash_inline_math() {
        let result = try_parse_single_backslash_inline_math(r"\(x^2\)", true);
        assert_eq!(result, Some((7, "x^2")));
    }

    #[test]
    fn test_single_backslash_inline_math_complex() {
        let result = try_parse_single_backslash_inline_math(r"\(\frac{a}{b}\)", true);
        assert_eq!(result, Some((15, r"\frac{a}{b}")));
    }

    #[test]
    fn test_single_backslash_inline_math_no_close() {
        let result = try_parse_single_backslash_inline_math(r"\(no close", true);
        assert_eq!(result, None);
    }

    #[test]
    fn test_single_backslash_inline_math_spans_single_newline() {
        let result = try_parse_single_backslash_inline_math("\\(x =\ny\\)", true);
        assert_eq!(result, Some((9, "x =\ny")));
        let blank = try_parse_single_backslash_inline_math("\\(x =\n\ny\\)", true);
        assert_eq!(blank, None);
        let cm = try_parse_single_backslash_inline_math("\\(x =\ny\\)", false);
        assert_eq!(cm, None);
    }

    #[test]
    fn test_single_backslash_display_math() {
        let result = try_parse_single_backslash_display_math(r"\[E = mc^2\]");
        assert_eq!(result, Some((12, "E = mc^2")));
    }

    #[test]
    fn test_single_backslash_display_math_multiline() {
        let result = try_parse_single_backslash_display_math("\\[\nx = y\n\\]");
        assert_eq!(result, Some((11, "\nx = y\n")));
    }

    #[test]
    fn test_single_backslash_display_math_no_close() {
        let result = try_parse_single_backslash_display_math(r"\[no close");
        assert_eq!(result, None);
    }

    #[test]
    fn test_double_backslash_inline_math() {
        let result = try_parse_double_backslash_inline_math(r"\\(x^2\\)", true);
        assert_eq!(result, Some((9, "x^2")));
    }

    #[test]
    fn test_double_backslash_inline_math_complex() {
        let result = try_parse_double_backslash_inline_math(r"\\(\alpha + \beta\\)", true);
        assert_eq!(result, Some((20, r"\alpha + \beta")));
    }

    #[test]
    fn test_double_backslash_inline_math_no_close() {
        let result = try_parse_double_backslash_inline_math(r"\\(no close", true);
        assert_eq!(result, None);
    }

    #[test]
    fn test_double_backslash_inline_math_spans_single_newline() {
        let result = try_parse_double_backslash_inline_math("\\\\(x =\ny\\\\)", true);
        assert_eq!(result, Some((11, "x =\ny")));
        let blank = try_parse_double_backslash_inline_math("\\\\(x =\n\ny\\\\)", true);
        assert_eq!(blank, None);
        let cm = try_parse_double_backslash_inline_math("\\\\(x =\ny\\\\)", false);
        assert_eq!(cm, None);
    }

    #[test]
    fn test_double_backslash_display_math() {
        let result = try_parse_double_backslash_display_math(r"\\[E = mc^2\\]");
        assert_eq!(result, Some((14, "E = mc^2")));
    }

    #[test]
    fn test_double_backslash_display_math_multiline() {
        let result = try_parse_double_backslash_display_math("\\\\[\nx = y\n\\\\]");
        assert_eq!(result, Some((13, "\nx = y\n")));
    }

    #[test]
    fn test_double_backslash_display_math_no_close() {
        let result = try_parse_double_backslash_display_math(r"\\[no close");
        assert_eq!(result, None);
    }

    #[test]
    fn test_display_math_escaped_dollar() {
        let result = try_parse_display_math(r"$$a = \$100$$");
        assert_eq!(result, Some((13, r"a = \$100")));
    }

    #[test]
    fn test_display_math_with_content_on_fence_line() {
        let result = try_parse_display_math("$$x = y\n$$");
        assert_eq!(result, Some((10, "x = y\n")));
    }
}
