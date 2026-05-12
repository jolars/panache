//! Shared utilities for parsing the argument list of a Quarto shortcode
//! (`{{< name arg1 arg2 key=value >}}`).
//!
//! Used by both the file-rename handler (to rewrite paths embedded in
//! `include`-style shortcodes) and the completion handler (to suggest
//! file paths inside the same arguments). Each returned span is a byte
//! range **relative to the SHORTCODE_CONTENT text**, not the document.

/// Tokenize a shortcode content string into byte-range tokens.
///
/// Whitespace separates tokens; single- or double-quoted runs are kept
/// as a single token (quotes included in the range).
pub(crate) fn shortcode_tokens(content: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    let mut in_quotes = false;
    let mut quote_char = '\0';

    for (idx, ch) in content.char_indices() {
        if in_quotes {
            if ch == quote_char {
                in_quotes = false;
            }
            continue;
        }

        if ch == '"' || ch == '\'' {
            if start.is_none() {
                start = Some(idx);
            }
            in_quotes = true;
            quote_char = ch;
            continue;
        }

        if ch.is_whitespace() {
            if let Some(s) = start.take() {
                out.push((s, idx));
            }
            continue;
        }

        if start.is_none() {
            start = Some(idx);
        }
    }

    if let Some(s) = start {
        out.push((s, content.len()));
    }
    out
}

/// Reduce a raw token span to its value span: strips a leading `key=`
/// prefix and surrounding matching quotes. Returns `None` if the result
/// would be empty.
pub(crate) fn shortcode_token_value_span(
    content: &str,
    token: (usize, usize),
) -> Option<(usize, usize)> {
    let raw = content.get(token.0..token.1)?;
    let value = if let Some(eq_idx) = raw.find('=') {
        let after_eq = token.0 + eq_idx + 1;
        (after_eq, token.1)
    } else {
        (token.0, token.1)
    };

    let start_char = content.get(value.0..value.0 + 1)?;
    let end_char = content.get(value.1.saturating_sub(1)..value.1)?;
    if (start_char == "\"" && end_char == "\"") || (start_char == "'" && end_char == "'") {
        if value.1 <= value.0 + 1 {
            return None;
        }
        Some((value.0 + 1, value.1 - 1))
    } else {
        Some(value)
    }
}

/// `true` if the token has a `key=value` shape (named argument).
pub(crate) fn token_is_named(content: &str, token: (usize, usize)) -> bool {
    content
        .get(token.0..token.1)
        .map(|raw| raw.contains('='))
        .unwrap_or(false)
}
