//! Parsing for links, images, and automatic links.
//!
//! Implements:
//! - Automatic links: `<http://example.com>` and `<user@example.com>`
//! - Inline links: `[text](url)` and `[text](url "title")`
//! - Link attributes: `[text](url){#id .class key=value}`
//! - Inline images: `![alt](url)` and `![alt](url "title")`
//! - Image attributes: `![alt](url){#id .class key=value}`
//! - Reference links: `[text][ref]`, `[text][]`, `[text]`
//! - Reference images: `![alt][ref]`, `![alt][]`, `![alt]`

use super::code_spans::try_parse_code_span;
use super::core::parse_inline_text;
use super::inline_html::try_parse_inline_html;
use crate::options::ParserOptions;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

// Import attribute parsing
use crate::parser::utils::attributes::try_parse_trailing_attributes;

/// Flags that control which inline spans the link-bracket scanner treats as
/// opaque (so a `]` inside them does not terminate the link/image text).
///
/// - `skip_raw_html` is universal across dialects: pandoc-markdown and
///   CommonMark both refuse to close link text inside a raw HTML span (e.g.
///   `[foo <bar attr="](baz)">`), per CommonMark spec example #524 / #536.
/// - `skip_autolinks` is **CommonMark-only**. Pandoc-markdown does *not*
///   treat `<scheme://...>` as opaque inside link text, so the same input
///   produces a different parse under each dialect (CommonMark spec example
///   #526 / #538). Always derive this from
///   `extensions.autolinks && dialect == Dialect::CommonMark`.
/// - `disallow_inner_links` is **CommonMark-only** structural rule (§6.4):
///   "Links may not contain other links, at any level of nesting." When the
///   candidate link/image text contains a valid inline link or image, the
///   outer match is rejected so the inner-most definition is used instead
///   (spec examples #518–#520, #532). Pandoc-markdown allows nested links,
///   so the flag is `false` there.
#[derive(Clone, Copy, Default)]
pub struct LinkScanContext {
    pub skip_raw_html: bool,
    pub skip_autolinks: bool,
    pub disallow_inner_links: bool,
}

impl LinkScanContext {
    pub fn from_options(config: &ParserOptions) -> Self {
        let is_commonmark = config.dialect == crate::options::Dialect::CommonMark;
        Self {
            skip_raw_html: config.extensions.raw_html,
            skip_autolinks: config.extensions.autolinks && is_commonmark,
            disallow_inner_links: is_commonmark,
        }
    }
}

/// Find the closing `]` of a link/image text span, starting from `start`.
///
/// Walks `text[start..]` tracking nested brackets and backslash escapes. When
/// a backtick run starting a valid code span is encountered, the entire span
/// (including any trailing attribute block) is skipped — per CommonMark §6
/// precedence, code spans bind tighter than links/images, so a `]` *inside*
/// a code span cannot terminate the link's text. The same opacity applies to
/// raw HTML and (CommonMark-only) autolink spans gated through `ctx`.
/// Returns the byte offset of the closing `]` within `text`, or `None` if no
/// unmatched `]` is reached.
fn find_link_close_bracket(text: &str, start: usize, ctx: LinkScanContext) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut bracket_depth = 0;
    let mut escape_next = false;
    let mut i = start;

    while i < bytes.len() {
        let b = bytes[i];

        if escape_next {
            escape_next = false;
            i += step(text, i);
            continue;
        }

        match b {
            b'\\' => {
                escape_next = true;
                i += 1;
            }
            b'`' => {
                if let Some((len, _, _, _)) = try_parse_code_span(&text[i..]) {
                    i += len;
                } else {
                    i += 1;
                }
            }
            b'<' => {
                // Order matters: autolinks are the more specific `<...>`
                // shape (URI/email between angle brackets), so try that
                // before falling through to general inline raw HTML which
                // would also match `<bar attr="...">`-style tags.
                if ctx.skip_autolinks
                    && let Some((len, _)) = try_parse_autolink(&text[i..], true)
                {
                    i += len;
                } else if ctx.skip_raw_html
                    && let Some(len) = try_parse_inline_html(&text[i..])
                {
                    i += len;
                } else {
                    i += 1;
                }
            }
            b'[' => {
                bracket_depth += 1;
                i += 1;
            }
            b']' => {
                if bracket_depth == 0 {
                    return Some(i);
                }
                bracket_depth -= 1;
                i += 1;
            }
            _ => i += step(text, i),
        }
    }
    None
}

/// Find the closing `)` of a link/image destination, given the text *after*
/// the opening `(`. Tracks paren nesting, quoted titles, and angle-bracketed
/// destinations (`<...>` may legitimately contain unbalanced parens — see
/// spec example #499). Returns the byte offset of the closing `)` within the
/// passed slice, or `None` if not found.
fn find_dest_close_paren(remaining: &str) -> Option<usize> {
    let bytes = remaining.as_bytes();
    let mut paren_depth = 0;
    let mut escape_next = false;
    let mut in_quotes = false;
    let mut in_angle = false;
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];

        if escape_next {
            escape_next = false;
            i += step(remaining, i);
            continue;
        }

        match b {
            b'\\' => {
                escape_next = true;
                i += 1;
            }
            b'<' if !in_quotes && !in_angle => {
                in_angle = true;
                i += 1;
            }
            b'>' if in_angle => {
                in_angle = false;
                i += 1;
            }
            b'"' if !in_angle => {
                in_quotes = !in_quotes;
                i += 1;
            }
            b'(' if !in_quotes && !in_angle => {
                paren_depth += 1;
                i += 1;
            }
            b')' if !in_quotes && !in_angle => {
                if paren_depth == 0 {
                    return Some(i);
                }
                paren_depth -= 1;
                i += 1;
            }
            _ => i += step(remaining, i),
        }
    }
    None
}

/// Byte length of the UTF-8 character starting at byte index `i` in `s`.
/// Used to advance an index loop char-by-char without incurring `char_indices`
/// overhead and without splitting on a UTF-8 boundary.
fn step(s: &str, i: usize) -> usize {
    s[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1)
}

/// CommonMark §6.4: "Links may not contain other links, at any level of
/// nesting. If multiple otherwise valid link definitions appear nested inside
/// each other, the inner-most definition is used." This helper scans a
/// candidate link text for any `[` that starts a valid inline link; when
/// found, the outer link must be rejected so the inner-most wins (spec
/// examples #518–#519, #532).
///
/// Images themselves do not count as inner links — a link can contain an
/// image (#517, #531). A link *inside* an image's alt text, however, still
/// deactivates outer link openers per CommonMark's bracket-scanner rules, so
/// the helper recurses into image alt text looking for inner links.
///
/// Reference-link nesting (#533, #569, #571) requires resolving labels
/// against the document's reference-definition map, which the parser does
/// not have at this point — those cases remain unhandled and need a later
/// stack-based pass.
fn link_text_contains_inner_link(text: &str, ctx: LinkScanContext, strict_dest: bool) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut escape_next = false;
    while i < bytes.len() {
        let b = bytes[i];
        if escape_next {
            escape_next = false;
            i += step(text, i);
            continue;
        }
        match b {
            b'\\' => {
                escape_next = true;
                i += 1;
            }
            b'`' => {
                if let Some((len, _, _, _)) = try_parse_code_span(&text[i..]) {
                    i += len;
                } else {
                    i += 1;
                }
            }
            b'<' => {
                if ctx.skip_autolinks
                    && let Some((len, _)) = try_parse_autolink(&text[i..], true)
                {
                    i += len;
                } else if ctx.skip_raw_html
                    && let Some(len) = try_parse_inline_html(&text[i..])
                {
                    i += len;
                } else {
                    i += 1;
                }
            }
            b'!' if i + 1 < bytes.len() && bytes[i + 1] == b'[' => {
                if let Some((len, alt, _, _)) = try_parse_inline_image(&text[i..], ctx) {
                    if link_text_contains_inner_link(alt, ctx, strict_dest) {
                        return true;
                    }
                    i += len;
                } else {
                    i += 2;
                }
            }
            b'[' => {
                if try_parse_inline_link(&text[i..], strict_dest, ctx).is_some() {
                    return true;
                }
                i += 1;
            }
            _ => i += step(text, i),
        }
    }
    false
}

/// Try to parse an inline image starting at the current position.
///
/// Inline images have the form `![alt](url)` or `![alt](url "title")`.
/// Can also have trailing attributes: `![alt](url){#id .class}`.
/// Returns Some((length, alt_text, dest_content, raw_attributes)) if a valid image is found.
///
/// `ctx` controls bracket-scanner opacity for raw HTML / autolink spans;
/// see `LinkScanContext`.
pub fn try_parse_inline_image(
    text: &str,
    ctx: LinkScanContext,
) -> Option<(usize, &str, &str, Option<&str>)> {
    if !text.starts_with("![") {
        return None;
    }

    // Find the closing ]
    let close_bracket = find_link_close_bracket(text, 2, ctx)?;
    let alt_text = &text[2..close_bracket];

    // Check for immediate ( after ]
    let after_bracket = close_bracket + 1;
    if text.len() <= after_bracket || !text[after_bracket..].starts_with('(') {
        return None;
    }

    // Find closing ) for destination (reuse same logic as links)
    let dest_start = after_bracket + 1;
    let remaining = &text[dest_start..];

    let close_paren = find_dest_close_paren(remaining)?;
    let dest_content = &remaining[..close_paren];

    // Check for trailing attributes {#id .class key=value}
    let after_paren = dest_start + close_paren + 1;
    let after_close = &text[after_paren..];

    // Attributes must start immediately after closing paren (no whitespace/newlines)
    if after_close.starts_with('{') {
        // Find the closing brace
        if let Some(close_brace_pos) = after_close.find('}') {
            let attr_text = &after_close[..=close_brace_pos];
            // Try to parse as attributes to validate
            if let Some((_attrs, _)) = try_parse_trailing_attributes(attr_text) {
                let total_len = after_paren + close_brace_pos + 1;
                // Return raw attribute string for lossless parsing
                let raw_attrs = attr_text;
                return Some((total_len, alt_text, dest_content, Some(raw_attrs)));
            }
        }
    }

    // No attributes, just return the image
    let total_len = after_paren;
    Some((total_len, alt_text, dest_content, None))
}

/// Emit an inline image node to the builder.
/// Note: alt_text may contain inline elements and should be parsed recursively.
pub fn emit_inline_image(
    builder: &mut GreenNodeBuilder,
    _text: &str,
    alt_text: &str,
    dest: &str,
    raw_attributes: Option<&str>,
    config: &ParserOptions,
) {
    builder.start_node(SyntaxKind::IMAGE_LINK.into());

    // Opening ![
    builder.start_node(SyntaxKind::IMAGE_LINK_START.into());
    builder.token(SyntaxKind::IMAGE_LINK_START.into(), "![");
    builder.finish_node();

    // Alt text (recursively parse inline elements)
    builder.start_node(SyntaxKind::IMAGE_ALT.into());
    // Use the standalone parse_inline_text function for recursive parsing
    // Note: nested contexts don't resolve references
    parse_inline_text(builder, alt_text, config, false);
    builder.finish_node();

    // Closing ]
    builder.token(SyntaxKind::IMAGE_ALT_END.into(), "]");

    // Opening (
    builder.token(SyntaxKind::IMAGE_DEST_START.into(), "(");

    // Destination
    builder.start_node(SyntaxKind::LINK_DEST.into());
    builder.token(SyntaxKind::TEXT.into(), dest);
    builder.finish_node();

    // Closing )
    builder.token(SyntaxKind::IMAGE_DEST_END.into(), ")");

    // Emit raw attributes if present (preserve original formatting)
    if let Some(raw_attrs) = raw_attributes {
        builder.start_node(SyntaxKind::ATTRIBUTE.into());
        builder.token(SyntaxKind::ATTRIBUTE.into(), raw_attrs);
        builder.finish_node();
    }

    builder.finish_node();
}

/// Try to parse an automatic link starting at the current position.
///
/// Automatic links have the form `<url>` (URI autolink) or `<email>`
/// (email autolink) per CommonMark §6.4. Under `Dialect::CommonMark` the
/// scheme/email grammar is enforced strictly (e.g. scheme must be 2-32
/// ASCII chars; email local parts cannot contain backslashes). Pandoc
/// markdown is laxer — it accepts Unicode in email addresses, for
/// example — so non-CommonMark callers fall back to the heuristic
/// "contains `:` or `@`" check that the parser used historically.
pub fn try_parse_autolink(text: &str, is_commonmark: bool) -> Option<(usize, &str)> {
    if !text.starts_with('<') {
        return None;
    }

    let close_pos = text[1..].find('>')?;
    let content = &text[1..1 + close_pos];

    if content.is_empty() {
        return None;
    }
    if content.contains(|c: char| c.is_whitespace()) {
        return None;
    }

    if is_commonmark {
        if !is_valid_uri_autolink(content) && !is_valid_email_autolink(content) {
            return None;
        }
    } else if !content.contains(':') && !content.contains('@') {
        return None;
    }

    Some((close_pos + 2, content))
}

/// CommonMark §6.4 URI autolink:
/// scheme = 2-32 chars, ASCII letter then `[a-zA-Z0-9+.-]`, followed by `:`,
/// followed by URI body (any char except control, space, `<`, `>`).
fn is_valid_uri_autolink(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    let mut i = 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.' {
            i += 1;
        } else {
            break;
        }
    }
    if !(2..=32).contains(&i) {
        return false;
    }
    if i >= bytes.len() || bytes[i] != b':' {
        return false;
    }
    for &b in &bytes[i + 1..] {
        if b < 0x20 || b == 0x7f || b == b'<' || b == b'>' {
            return false;
        }
    }
    true
}

/// CommonMark §6.4 email autolink, matching the HTML5 non-normative regex:
/// `^[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?
///  (?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*$`.
fn is_valid_email_autolink(s: &str) -> bool {
    let Some(at) = s.find('@') else {
        return false;
    };
    let local = &s[..at];
    let domain = &s[at + 1..];
    if local.is_empty() || !local.bytes().all(is_email_local_byte) {
        return false;
    }
    if domain.is_empty() {
        return false;
    }
    domain.split('.').all(is_valid_email_label)
}

fn is_email_local_byte(b: u8) -> bool {
    matches!(
        b,
        b'a'..=b'z'
            | b'A'..=b'Z'
            | b'0'..=b'9'
            | b'.'
            | b'!'
            | b'#'
            | b'$'
            | b'%'
            | b'&'
            | b'\''
            | b'*'
            | b'+'
            | b'/'
            | b'='
            | b'?'
            | b'^'
            | b'_'
            | b'`'
            | b'{'
            | b'|'
            | b'}'
            | b'~'
            | b'-'
    )
}

fn is_valid_email_label(label: &str) -> bool {
    let bytes = label.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    if !bytes[0].is_ascii_alphanumeric() {
        return false;
    }
    if !bytes[bytes.len() - 1].is_ascii_alphanumeric() {
        return false;
    }
    bytes[1..bytes.len() - 1]
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
}

/// Emit an automatic link node to the builder.
pub fn emit_autolink(builder: &mut GreenNodeBuilder, _text: &str, url: &str) {
    builder.start_node(SyntaxKind::AUTO_LINK.into());

    // Opening <
    builder.start_node(SyntaxKind::AUTO_LINK_MARKER.into());
    builder.token(SyntaxKind::AUTO_LINK_MARKER.into(), "<");
    builder.finish_node();

    // URL content
    builder.token(SyntaxKind::TEXT.into(), url);

    // Closing >
    builder.start_node(SyntaxKind::AUTO_LINK_MARKER.into());
    builder.token(SyntaxKind::AUTO_LINK_MARKER.into(), ">");
    builder.finish_node();

    builder.finish_node();
}

pub fn try_parse_bare_uri(text: &str) -> Option<(usize, &str)> {
    let mut chars = text.char_indices();
    let (_, first) = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }

    let mut scheme_end = None;
    for (idx, ch) in text.char_indices() {
        if ch == ':' {
            scheme_end = Some(idx);
            break;
        }
        if !ch.is_ascii_alphanumeric() && ch != '+' && ch != '-' && ch != '.' {
            return None;
        }
    }
    let scheme_end = scheme_end?;
    if scheme_end == 0 {
        return None;
    }

    let mut end = scheme_end + 1;
    let bytes = text.as_bytes();
    while end < text.len() {
        let b = bytes[end];
        if b.is_ascii_whitespace() {
            break;
        }
        if matches!(b, b'<' | b'>' | b'`' | b'"' | b'\'') {
            break;
        }
        end += 1;
    }

    if end == scheme_end + 1 {
        return None;
    }

    let mut trimmed = end;
    while trimmed > scheme_end + 1 {
        let ch = text[..trimmed].chars().last().unwrap();
        if matches!(ch, '.' | ',' | ';' | ':' | ')' | ']' | '}') {
            trimmed -= ch.len_utf8();
        } else {
            break;
        }
    }

    if trimmed <= scheme_end + 1 {
        return None;
    }

    // If trimming terminal punctuation leaves a dangling backslash, the match
    // came from escaped punctuation (e.g., `a:\]`) and should stay literal.
    if text[..trimmed].ends_with('\\') {
        return None;
    }

    Some((trimmed, &text[..trimmed]))
}

/// Try to parse an inline link starting at the current position.
///
/// Inline links have the form `[text](url)` or `[text](url "title")`.
/// Can also have trailing attributes: `[text](url){#id .class}`.
/// Returns Some((length, text_content, dest_content, raw_attributes)) if a valid link is found.
///
/// `strict_dest` enables CommonMark §6.4 destination-and-title validation:
/// the bare destination form may not contain spaces or ASCII control
/// characters and must have balanced parentheses; if a title follows it
/// must be properly delimited; only whitespace is allowed before/after.
/// Pandoc-markdown is more permissive, so leave this off for that dialect.
pub fn try_parse_inline_link(
    text: &str,
    strict_dest: bool,
    ctx: LinkScanContext,
) -> Option<(usize, &str, &str, Option<&str>)> {
    if !text.starts_with('[') {
        return None;
    }

    // Find the closing ]
    let close_bracket = find_link_close_bracket(text, 1, ctx)?;
    let link_text = &text[1..close_bracket];

    // Check for immediate ( after ]
    let after_bracket = close_bracket + 1;
    if text.len() <= after_bracket || !text[after_bracket..].starts_with('(') {
        return None;
    }

    // Find closing ) for destination
    let dest_start = after_bracket + 1;
    let remaining = &text[dest_start..];

    let close_paren = find_dest_close_paren(remaining)?;
    let dest_content = &remaining[..close_paren];

    if strict_dest && !dest_and_title_ok_commonmark(dest_content) {
        return None;
    }

    // CommonMark §6.4: outer link is rejected when its text contains a valid
    // inner inline link or image, so the inner-most definition wins.
    if ctx.disallow_inner_links && link_text_contains_inner_link(link_text, ctx, strict_dest) {
        return None;
    }

    // Check for trailing attributes {#id .class key=value}
    let after_paren = dest_start + close_paren + 1;
    let after_close = &text[after_paren..];

    // Attributes must start immediately after closing paren (no whitespace/newlines)
    if after_close.starts_with('{') {
        // Find the closing brace
        if let Some(close_brace_pos) = after_close.find('}') {
            let attr_text = &after_close[..=close_brace_pos];
            // Try to parse as attributes to validate
            if let Some((_attrs, _)) = try_parse_trailing_attributes(attr_text) {
                let total_len = after_paren + close_brace_pos + 1;
                // Return raw attribute string for lossless parsing
                let raw_attrs = attr_text;
                return Some((total_len, link_text, dest_content, Some(raw_attrs)));
            }
        }
    }

    // No attributes, just return the link
    let total_len = after_paren;
    Some((total_len, link_text, dest_content, None))
}

/// CommonMark §6.4 destination + optional title validation. The text passed
/// in is whatever the parser captured between `(` and `)`. A valid form is:
/// `[ws] destination [ws title [ws]]` where:
/// - bare destination has no spaces, tabs, ASCII control chars, and balanced
///   parentheses (escaped parens permitted);
/// - bracketed destination is `<...>` with no newlines and no unescaped `<>`;
/// - the optional title is delimited by `"..."`, `'...'`, or `(...)`;
/// - any text outside that structure invalidates the link.
fn dest_and_title_ok_commonmark(content: &str) -> bool {
    let trimmed = content.trim_start_matches([' ', '\t', '\n']);
    if trimmed.is_empty() {
        return true;
    }

    let after_dest = if let Some(rest) = trimmed.strip_prefix('<') {
        let mut escape = false;
        let mut end_byte = None;
        for (i, c) in rest.char_indices() {
            if escape {
                escape = false;
                continue;
            }
            match c {
                '\\' => escape = true,
                '\n' | '<' => return false,
                '>' => {
                    end_byte = Some(i);
                    break;
                }
                _ => {}
            }
        }
        match end_byte {
            Some(e) => &rest[e + 1..],
            None => return false,
        }
    } else {
        let mut escape = false;
        let mut depth: i32 = 0;
        let mut end = trimmed.len();
        for (i, c) in trimmed.char_indices() {
            if escape {
                escape = false;
                continue;
            }
            match c {
                '\\' => escape = true,
                ' ' | '\t' | '\n' => {
                    end = i;
                    break;
                }
                _ if c.is_ascii_control() => return false,
                '(' => depth += 1,
                ')' => {
                    if depth == 0 {
                        end = i;
                        break;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        if depth != 0 {
            return false;
        }
        if end == 0 {
            // bare destination must be nonempty if the field is non-blank
            return false;
        }
        &trimmed[end..]
    };

    let after_dest = after_dest.trim_start_matches([' ', '\t', '\n']);
    if after_dest.is_empty() {
        return true;
    }

    let bytes = after_dest.as_bytes();
    let close = match bytes[0] {
        b'"' => b'"',
        b'\'' => b'\'',
        b'(' => b')',
        _ => return false,
    };
    let opens_paren = bytes[0] == b'(';
    let mut escape = false;
    let mut title_close_pos = None;
    for (i, &b) in after_dest.as_bytes().iter().enumerate().skip(1) {
        if escape {
            escape = false;
            continue;
        }
        if b == b'\\' {
            escape = true;
            continue;
        }
        if opens_paren && b == b'(' {
            return false;
        }
        if b == close {
            title_close_pos = Some(i);
            break;
        }
    }
    let close_idx = match title_close_pos {
        Some(p) => p,
        None => return false,
    };

    let after_title = &after_dest[close_idx + 1..];
    after_title.trim_matches([' ', '\t', '\n']).is_empty()
}

/// Emit an inline link node to the builder.
/// Note: link_text may contain inline elements and should be parsed recursively.
pub fn emit_inline_link(
    builder: &mut GreenNodeBuilder,
    _text: &str,
    link_text: &str,
    dest: &str,
    raw_attributes: Option<&str>,
    config: &ParserOptions,
) {
    builder.start_node(SyntaxKind::LINK.into());

    // Opening [
    builder.start_node(SyntaxKind::LINK_START.into());
    builder.token(SyntaxKind::LINK_START.into(), "[");
    builder.finish_node();

    // Link text (recursively parse inline elements)
    builder.start_node(SyntaxKind::LINK_TEXT.into());
    // Use the standalone parse_inline_text function for recursive parsing
    parse_inline_text(builder, link_text, config, false);
    builder.finish_node();

    // Closing ]
    builder.token(SyntaxKind::LINK_TEXT_END.into(), "]");

    // Opening (
    builder.token(SyntaxKind::LINK_DEST_START.into(), "(");

    // Destination
    builder.start_node(SyntaxKind::LINK_DEST.into());
    builder.token(SyntaxKind::TEXT.into(), dest);
    builder.finish_node();

    // Closing )
    builder.token(SyntaxKind::LINK_DEST_END.into(), ")");

    // Emit raw attributes if present (preserve original formatting)
    if let Some(raw_attrs) = raw_attributes {
        builder.start_node(SyntaxKind::ATTRIBUTE.into());
        builder.token(SyntaxKind::ATTRIBUTE.into(), raw_attrs);
        builder.finish_node();
    }

    builder.finish_node();
}

pub fn emit_bare_uri_link(builder: &mut GreenNodeBuilder, uri: &str, _config: &ParserOptions) {
    builder.start_node(SyntaxKind::LINK.into());

    builder.start_node(SyntaxKind::LINK_START.into());
    builder.token(SyntaxKind::LINK_START.into(), "[");
    builder.finish_node();

    builder.start_node(SyntaxKind::LINK_TEXT.into());
    builder.token(SyntaxKind::TEXT.into(), uri);
    builder.finish_node();

    builder.token(SyntaxKind::LINK_TEXT_END.into(), "]");
    builder.token(SyntaxKind::LINK_DEST_START.into(), "(");

    builder.start_node(SyntaxKind::LINK_DEST.into());
    builder.token(SyntaxKind::TEXT.into(), uri);
    builder.finish_node();

    builder.token(SyntaxKind::LINK_DEST_END.into(), ")");

    builder.finish_node();
}

/// Try to parse a reference link starting at the current position.
///
/// Reference links have three forms:
/// - Explicit: `[text][label]`
/// - Implicit: `[text][]` (label = text)
/// - Shortcut: `[text]` (if shortcut_reference_links enabled)
///
/// Returns Some((length, text_content, label, is_shortcut)) if a valid reference link is found.
/// The label is what should be looked up in the registry.
pub fn try_parse_reference_link(
    text: &str,
    allow_shortcut: bool,
    inline_link_attempted: bool,
    ctx: LinkScanContext,
) -> Option<(usize, &str, String, bool)> {
    if !text.starts_with('[') {
        return None;
    }

    // Don't match citations (which start with [@) or suppress-author citations (which start with [-@)
    if text.len() > 1 {
        let bytes = text.as_bytes();
        if bytes[1] == b'@' {
            return None;
        }
        if bytes[1] == b'-' && text.len() > 2 && bytes[2] == b'@' {
            return None;
        }
    }

    // Find the closing ] for the text. Uses the shared helper so that a
    // `]` inside a code span doesn't terminate the link text (CommonMark
    // §6 — code spans bind tighter than links). See spec examples #342
    // and #525. Raw HTML and (CommonMark-only) autolink spans are also
    // opaque per `ctx`.
    let close_bracket = find_link_close_bracket(text, 1, ctx)?;
    let link_text = &text[1..close_bracket];

    // CommonMark §6.4: outer reference link is rejected when its text contains
    // a valid inner inline link/image (spec example #532). Reference-link
    // nesting (#533/#569/#571) is not handled here; it requires resolving
    // labels against the document refdef map.
    if ctx.disallow_inner_links
        && link_text_contains_inner_link(link_text, ctx, ctx.disallow_inner_links)
    {
        return None;
    }

    // Check what follows the ]
    let after_bracket = close_bracket + 1;

    // `[content]{...}` is reserved for bracketed spans / attribute
    // trailers, never a shortcut.
    if after_bracket < text.len() && text[after_bracket..].starts_with('{') {
        return None;
    }

    // `[text](...)` is the inline-link shape. CommonMark spec example
    // #568 (`[foo](not a link)` with `[foo]: /url`) requires the shortcut
    // to succeed for `[foo]`, leaving `(not a link)` as literal text when
    // the upstream inline-link parse was rejected by `strict_dest`. We
    // only fall through to shortcut here when the caller has already
    // tried the inline-link form (`inline_link_attempted`) — otherwise
    // disabling the `inline_links` extension would silently let
    // `[text](url)` become a shortcut + literal text, which the
    // `inline_links_disabled_keeps_inline_link_literal` test guards
    // against.
    if after_bracket < text.len()
        && text[after_bracket..].starts_with('(')
        && (!allow_shortcut || !inline_link_attempted)
    {
        return None;
    }

    // Check for explicit reference [text][label] or implicit [text][]
    if after_bracket < text.len() && text[after_bracket..].starts_with('[') {
        // Find the closing ] for the label
        let label_start = after_bracket + 1;
        let mut label_end = None;

        for (i, ch) in text[label_start..].char_indices() {
            if ch == ']' {
                label_end = Some(i + label_start);
                break;
            }
            // Labels can't contain newlines
            if ch == '\n' {
                return None;
            }
        }

        let label_end = label_end?;
        let label = &text[label_start..label_end];

        // Total length includes both bracket pairs
        let total_len = label_end + 1;

        // Implicit reference: empty label means emit [text][]
        if label.is_empty() {
            return Some((total_len, link_text, String::new(), false));
        }

        // Explicit reference: use the provided label
        Some((total_len, link_text, label.to_string(), false))
    } else if allow_shortcut {
        // Shortcut reference: [text] with no second bracket pair
        // The text is both the display text and the label
        if link_text.is_empty() {
            return None;
        }
        Some((after_bracket, link_text, link_text.to_string(), true))
    } else {
        // No second bracket pair and shortcut not allowed - not a reference link
        None
    }
}

/// Emit a reference link node to the builder.
/// Preserves the original reference syntax (explicit [text][ref], implicit [text][], or shortcut [text]).
pub fn emit_reference_link(
    builder: &mut GreenNodeBuilder,
    link_text: &str,
    label: &str,
    is_shortcut: bool,
    config: &ParserOptions,
) {
    builder.start_node(SyntaxKind::LINK.into());

    // Opening [
    builder.start_node(SyntaxKind::LINK_START.into());
    builder.token(SyntaxKind::LINK_START.into(), "[");
    builder.finish_node();

    // Link text (recursively parse inline elements)
    builder.start_node(SyntaxKind::LINK_TEXT.into());
    parse_inline_text(builder, link_text, config, false);
    builder.finish_node();

    // Closing ] and reference label
    builder.token(SyntaxKind::TEXT.into(), "]");

    if !is_shortcut {
        // Explicit or implicit reference: [text][label] or [text][]
        builder.token(SyntaxKind::TEXT.into(), "[");
        builder.start_node(SyntaxKind::LINK_REF.into());
        // For implicit references, label is empty and we emit [text][]
        // For explicit references, emit the label to get [text][label]
        if !label.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), label);
        }
        builder.finish_node();
        builder.token(SyntaxKind::TEXT.into(), "]");
    }
    // For shortcut references, just [text] - no second bracket pair

    builder.finish_node();
}

/// Try to parse a reference-style image: `![alt][ref]`, `![alt][]`, or `![alt]`
/// Returns (total_len, alt_text, label, is_shortcut) if successful.
pub fn try_parse_reference_image(
    text: &str,
    allow_shortcut: bool,
) -> Option<(usize, &str, String, bool)> {
    let bytes = text.as_bytes();
    if bytes.len() < 4 || bytes[0] != b'!' || bytes[1] != b'[' {
        return None;
    }

    let mut pos = 2;
    let mut bracket_depth = 1;
    let alt_start = pos;

    // Find the end of the alt text (allowing nested brackets)
    while pos < bytes.len() && bracket_depth > 0 {
        match bytes[pos] {
            b'[' => bracket_depth += 1,
            b']' => bracket_depth -= 1,
            b'\\' if pos + 1 < bytes.len() => pos += 1, // skip escaped char
            _ => {}
        }
        pos += 1;
    }

    if bracket_depth > 0 {
        return None; // Unclosed brackets
    }

    let alt_text = &text[alt_start..pos - 1];

    // Now check for the label part
    if pos >= bytes.len() {
        return None;
    }

    // Explicit reference: `![alt][label]`
    if bytes[pos] == b'[' {
        pos += 1;
        let label_start = pos;

        // Find the end of the label (no nested brackets, no newlines)
        while pos < bytes.len() && bytes[pos] != b']' && bytes[pos] != b'\n' && bytes[pos] != b'\r'
        {
            pos += 1;
        }

        if pos >= bytes.len() || bytes[pos] != b']' {
            return None;
        }

        let label_text = &text[label_start..pos];
        pos += 1;

        // Return the original label text for formatting preservation
        // Empty label means implicit reference
        let label = if label_text.is_empty() {
            alt_text.to_string() // For implicit references, use alt text as label for equality check
        } else {
            label_text.to_string() // Preserve original case
        };

        return Some((pos, alt_text, label, false));
    }

    // Shortcut reference: `![alt]` (only if enabled)
    // BUT not if followed by (url) - that's an inline image
    if allow_shortcut {
        // Check if next char is ( - if so, not a reference
        if pos < bytes.len() && bytes[pos] == b'(' {
            return None;
        }

        // For shortcut references, use alt text as label for equality check
        let label = alt_text.to_string();
        return Some((pos, alt_text, label, true));
    }

    None
}

/// Emit a reference image node with registry lookup.
pub fn emit_reference_image(
    builder: &mut GreenNodeBuilder,
    alt_text: &str,
    label: &str,
    is_shortcut: bool,
    config: &ParserOptions,
) {
    builder.start_node(SyntaxKind::IMAGE_LINK.into());

    // Emit as reference image (preserve original syntax)
    builder.start_node(SyntaxKind::IMAGE_LINK_START.into());
    builder.token(SyntaxKind::IMAGE_LINK_START.into(), "![");
    builder.finish_node();

    // Alt text (recursively parse inline elements)
    builder.start_node(SyntaxKind::IMAGE_ALT.into());
    parse_inline_text(builder, alt_text, config, false);
    builder.finish_node();

    // Closing ] and reference label
    builder.token(SyntaxKind::TEXT.into(), "]");

    if !is_shortcut {
        // Explicit or implicit reference: ![alt][label] or ![alt][]
        builder.token(SyntaxKind::TEXT.into(), "[");
        builder.start_node(SyntaxKind::LINK_REF.into());
        // For implicit references, emit empty label (label == alt means implicit from parser)
        if label != alt_text {
            builder.token(SyntaxKind::TEXT.into(), label);
        }
        builder.finish_node();
        builder.token(SyntaxKind::TEXT.into(), "]");
    }
    // For shortcut references, just ![alt] - no second bracket pair

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_autolink_url() {
        let input = "<https://example.com>";
        assert_eq!(
            try_parse_autolink(input, false),
            Some((21, "https://example.com"))
        );
        assert_eq!(
            try_parse_autolink(input, true),
            Some((21, "https://example.com"))
        );
    }

    #[test]
    fn test_parse_autolink_email() {
        let input = "<user@example.com>";
        assert_eq!(
            try_parse_autolink(input, false),
            Some((18, "user@example.com"))
        );
        assert_eq!(
            try_parse_autolink(input, true),
            Some((18, "user@example.com"))
        );
    }

    #[test]
    fn test_parse_autolink_no_close() {
        let input = "<https://example.com";
        assert_eq!(try_parse_autolink(input, false), None);
        assert_eq!(try_parse_autolink(input, true), None);
    }

    #[test]
    fn test_parse_autolink_with_space() {
        let input = "<https://example.com >";
        assert_eq!(try_parse_autolink(input, false), None);
        assert_eq!(try_parse_autolink(input, true), None);
    }

    #[test]
    fn test_parse_autolink_not_url_or_email() {
        let input = "<notaurl>";
        assert_eq!(try_parse_autolink(input, false), None);
        assert_eq!(try_parse_autolink(input, true), None);
    }

    #[test]
    fn test_parse_autolink_commonmark_strict_scheme() {
        // Scheme too short (1 char) — invalid under CommonMark, lax-accepted
        // under Pandoc dialect (matches historical behavior).
        let input = "<m:abc>";
        assert_eq!(try_parse_autolink(input, true), None);
        assert_eq!(try_parse_autolink(input, false), Some((7, "m:abc")));
    }

    #[test]
    fn test_parse_autolink_commonmark_email_disallows_backslash() {
        let input = "<foo\\+@bar.example.com>";
        assert_eq!(try_parse_autolink(input, true), None);
        assert_eq!(
            try_parse_autolink(input, false),
            Some((23, "foo\\+@bar.example.com"))
        );
    }

    #[test]
    fn test_parse_inline_link_simple() {
        let input = "[text](url)";
        let result = try_parse_inline_link(input, false, LinkScanContext::default());
        assert_eq!(result, Some((11, "text", "url", None)));
    }

    #[test]
    fn test_parse_inline_link_with_title() {
        let input = r#"[text](url "title")"#;
        let result = try_parse_inline_link(input, false, LinkScanContext::default());
        assert_eq!(result, Some((19, "text", r#"url "title""#, None)));
    }

    #[test]
    fn test_parse_inline_link_with_nested_brackets() {
        let input = "[outer [inner] text](url)";
        let result = try_parse_inline_link(input, false, LinkScanContext::default());
        assert_eq!(result, Some((25, "outer [inner] text", "url", None)));
    }

    #[test]
    fn test_parse_inline_link_no_space_between_brackets_and_parens() {
        let input = "[text] (url)";
        let result = try_parse_inline_link(input, false, LinkScanContext::default());
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_inline_link_no_closing_bracket() {
        let input = "[text(url)";
        let result = try_parse_inline_link(input, false, LinkScanContext::default());
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_inline_link_no_closing_paren() {
        let input = "[text](url";
        let result = try_parse_inline_link(input, false, LinkScanContext::default());
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_inline_link_escaped_bracket() {
        let input = r"[text\]more](url)";
        let result = try_parse_inline_link(input, false, LinkScanContext::default());
        assert_eq!(result, Some((17, r"text\]more", "url", None)));
    }

    #[test]
    fn test_parse_inline_link_parens_in_url() {
        let input = "[text](url(with)parens)";
        let result = try_parse_inline_link(input, false, LinkScanContext::default());
        assert_eq!(result, Some((23, "text", "url(with)parens", None)));
    }

    #[test]
    fn test_parse_inline_image_simple() {
        let input = "![alt](image.jpg)";
        let result = try_parse_inline_image(input, LinkScanContext::default());
        assert_eq!(result, Some((17, "alt", "image.jpg", None)));
    }

    #[test]
    fn test_parse_inline_image_with_title() {
        let input = r#"![alt](image.jpg "A title")"#;
        let result = try_parse_inline_image(input, LinkScanContext::default());
        assert_eq!(result, Some((27, "alt", r#"image.jpg "A title""#, None)));
    }

    #[test]
    fn test_parse_inline_image_with_nested_brackets() {
        let input = "![outer [inner] alt](image.jpg)";
        let result = try_parse_inline_image(input, LinkScanContext::default());
        assert_eq!(result, Some((31, "outer [inner] alt", "image.jpg", None)));
    }

    #[test]
    fn test_parse_bare_uri_rejects_dangling_backslash_after_trim() {
        let input = r"a:\]";
        let result = try_parse_bare_uri(input);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_inline_image_no_space_between_brackets_and_parens() {
        let input = "![alt] (image.jpg)";
        let result = try_parse_inline_image(input, LinkScanContext::default());
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_inline_image_no_closing_bracket() {
        let input = "![alt(image.jpg)";
        let result = try_parse_inline_image(input, LinkScanContext::default());
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_inline_image_no_closing_paren() {
        let input = "![alt](image.jpg";
        let result = try_parse_inline_image(input, LinkScanContext::default());
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_inline_image_with_simple_class() {
        let input = "![alt](img.png){.large}";
        let result = try_parse_inline_image(input, LinkScanContext::default());
        let (len, alt, dest, attrs) = result.unwrap();
        assert_eq!(len, 23);
        assert_eq!(alt, "alt");
        assert_eq!(dest, "img.png");
        assert!(attrs.is_some());
        let attrs = attrs.unwrap();
        assert_eq!(attrs, "{.large}");
    }

    #[test]
    fn test_parse_inline_image_with_id() {
        let input = "![Figure 1](fig1.png){#fig-1}";
        let result = try_parse_inline_image(input, LinkScanContext::default());
        let (len, alt, dest, attrs) = result.unwrap();
        assert_eq!(len, 29);
        assert_eq!(alt, "Figure 1");
        assert_eq!(dest, "fig1.png");
        assert!(attrs.is_some());
        let attrs = attrs.unwrap();
        assert_eq!(attrs, "{#fig-1}");
    }

    #[test]
    fn test_parse_inline_image_with_full_attributes() {
        let input = "![alt](img.png){#fig .large width=\"80%\"}";
        let result = try_parse_inline_image(input, LinkScanContext::default());
        let (len, alt, dest, attrs) = result.unwrap();
        assert_eq!(len, 40);
        assert_eq!(alt, "alt");
        assert_eq!(dest, "img.png");
        assert!(attrs.is_some());
        let attrs = attrs.unwrap();
        assert_eq!(attrs, "{#fig .large width=\"80%\"}");
    }

    #[test]
    fn test_parse_inline_image_attributes_must_be_adjacent() {
        // Space between ) and { should not parse as attributes
        let input = "![alt](img.png) {.large}";
        let result = try_parse_inline_image(input, LinkScanContext::default());
        assert_eq!(result, Some((15, "alt", "img.png", None)));
    }

    // Link attribute tests
    #[test]
    fn test_parse_inline_link_with_id() {
        let input = "[text](url){#link-1}";
        let result = try_parse_inline_link(input, false, LinkScanContext::default());
        let (len, text, dest, attrs) = result.unwrap();
        assert_eq!(len, 20);
        assert_eq!(text, "text");
        assert_eq!(dest, "url");
        assert!(attrs.is_some());
        let attrs = attrs.unwrap();
        assert_eq!(attrs, "{#link-1}");
    }

    #[test]
    fn test_parse_inline_link_with_full_attributes() {
        let input = "[text](url){#link .external target=\"_blank\"}";
        let result = try_parse_inline_link(input, false, LinkScanContext::default());
        let (len, text, dest, attrs) = result.unwrap();
        assert_eq!(len, 44);
        assert_eq!(text, "text");
        assert_eq!(dest, "url");
        assert!(attrs.is_some());
        let attrs = attrs.unwrap();
        assert_eq!(attrs, "{#link .external target=\"_blank\"}");
    }

    #[test]
    fn test_parse_inline_link_attributes_must_be_adjacent() {
        // Space between ) and { should not parse as attributes
        let input = "[text](url) {.class}";
        let result = try_parse_inline_link(input, false, LinkScanContext::default());
        assert_eq!(result, Some((11, "text", "url", None)));
    }

    #[test]
    fn test_parse_inline_link_with_title_and_attributes() {
        let input = r#"[text](url "title"){.external}"#;
        let result = try_parse_inline_link(input, false, LinkScanContext::default());
        let (len, text, dest, attrs) = result.unwrap();
        assert_eq!(len, 30);
        assert_eq!(text, "text");
        assert_eq!(dest, r#"url "title""#);
        assert!(attrs.is_some());
        let attrs = attrs.unwrap();
        assert_eq!(attrs, "{.external}");
    }

    // Reference link tests
    #[test]
    fn test_parse_reference_link_explicit() {
        let input = "[link text][label]";
        let result = try_parse_reference_link(input, false, true, LinkScanContext::default());
        assert_eq!(result, Some((18, "link text", "label".to_string(), false)));
    }

    #[test]
    fn test_parse_reference_link_implicit() {
        let input = "[link text][]";
        let result = try_parse_reference_link(input, false, true, LinkScanContext::default());
        assert_eq!(result, Some((13, "link text", String::new(), false)));
    }

    #[test]
    fn test_parse_reference_link_explicit_same_label_as_text() {
        let input = "[stack][stack]";
        let result = try_parse_reference_link(input, false, true, LinkScanContext::default());
        assert_eq!(result, Some((14, "stack", "stack".to_string(), false)));
    }

    #[test]
    fn test_parse_reference_link_shortcut() {
        let input = "[link text] rest";
        let result = try_parse_reference_link(input, true, true, LinkScanContext::default());
        assert_eq!(
            result,
            Some((11, "link text", "link text".to_string(), true))
        );
    }

    #[test]
    fn test_parse_reference_link_shortcut_rejects_empty_label() {
        let input = "[] rest";
        let result = try_parse_reference_link(input, true, true, LinkScanContext::default());
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_reference_link_shortcut_disabled() {
        let input = "[link text] rest";
        let result = try_parse_reference_link(input, false, true, LinkScanContext::default());
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_reference_link_not_inline_link() {
        // With shortcut disabled, `[text](url)` is rejected so the inline
        // link form upstream gets exclusive ownership.
        let input = "[text](url)";
        let result = try_parse_reference_link(input, false, true, LinkScanContext::default());
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_reference_link_shortcut_falls_through_inline_link() {
        // CommonMark spec example #568: when an inline-link attempt would
        // fail (here we model the reachability — the caller tries inline
        // link first; if that returns None, we should still see `[text]`
        // as a shortcut and leave `(url)` to be parsed as following text).
        let input = "[text](url)";
        let result = try_parse_reference_link(input, true, true, LinkScanContext::default());
        assert_eq!(result, Some((6, "text", "text".to_string(), true)));
    }

    #[test]
    fn test_parse_reference_link_with_nested_brackets() {
        let input = "[outer [inner] text][ref]";
        let result = try_parse_reference_link(input, false, true, LinkScanContext::default());
        assert_eq!(
            result,
            Some((25, "outer [inner] text", "ref".to_string(), false))
        );
    }

    #[test]
    fn test_parse_reference_link_label_no_newline() {
        let input = "[text][label\nmore]";
        let result = try_parse_reference_link(input, false, true, LinkScanContext::default());
        assert_eq!(result, None);
    }

    // Reference image tests
    #[test]
    fn test_parse_reference_image_explicit() {
        let input = "![alt text][label]";
        let result = try_parse_reference_image(input, false);
        assert_eq!(result, Some((18, "alt text", "label".to_string(), false)));
    }

    #[test]
    fn test_parse_reference_image_implicit() {
        let input = "![alt text][]";
        let result = try_parse_reference_image(input, false);
        assert_eq!(
            result,
            Some((13, "alt text", "alt text".to_string(), false))
        );
    }

    #[test]
    fn test_parse_reference_image_shortcut() {
        let input = "![alt text] rest";
        let result = try_parse_reference_image(input, true);
        assert_eq!(result, Some((11, "alt text", "alt text".to_string(), true)));
    }

    #[test]
    fn test_parse_reference_image_shortcut_disabled() {
        let input = "![alt text] rest";
        let result = try_parse_reference_image(input, false);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_reference_image_not_inline() {
        // Should not match inline images with (url)
        let input = "![alt](url)";
        let result = try_parse_reference_image(input, true);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_reference_image_with_nested_brackets() {
        let input = "![alt [nested] text][ref]";
        let result = try_parse_reference_image(input, false);
        assert_eq!(
            result,
            Some((25, "alt [nested] text", "ref".to_string(), false))
        );
    }

    #[test]
    fn test_reference_link_label_with_crlf() {
        // Reference link labels should not span lines with CRLF
        let input = "[foo\r\nbar]";
        let result = try_parse_reference_link(input, false, true, LinkScanContext::default());

        // Should fail to parse because label contains line break
        assert_eq!(
            result, None,
            "Should not parse reference link with CRLF in label"
        );
    }

    #[test]
    fn test_reference_link_label_with_lf() {
        // Reference link labels should not span lines with LF either
        let input = "[foo\nbar]";
        let result = try_parse_reference_link(input, false, true, LinkScanContext::default());

        // Should fail to parse because label contains line break
        assert_eq!(
            result, None,
            "Should not parse reference link with LF in label"
        );
    }

    // Multiline link text tests
    #[test]
    fn test_parse_inline_link_multiline_text() {
        // Per Pandoc spec, link text CAN contain newlines (soft breaks)
        let input = "[text on\nline two](url)";
        let result = try_parse_inline_link(input, false, LinkScanContext::default());
        assert_eq!(
            result,
            Some((23, "text on\nline two", "url", None)),
            "Link text should allow newlines"
        );
    }

    #[test]
    fn test_parse_inline_link_multiline_with_formatting() {
        // Link text with newlines and other inline elements
        let input =
            "[A network graph. Different edges\nwith probability](../images/networkfig.png)";
        let result = try_parse_inline_link(input, false, LinkScanContext::default());
        assert!(result.is_some(), "Link text with newlines should parse");
        let (len, text, _dest, _attrs) = result.unwrap();
        assert!(text.contains('\n'), "Link text should preserve newline");
        assert_eq!(len, input.len());
    }

    #[test]
    fn test_parse_inline_image_multiline_alt() {
        // Per Pandoc spec, image alt text CAN contain newlines
        let input = "![alt on\nline two](img.png)";
        let result = try_parse_inline_image(input, LinkScanContext::default());
        assert_eq!(
            result,
            Some((27, "alt on\nline two", "img.png", None)),
            "Image alt text should allow newlines"
        );
    }

    #[test]
    fn test_parse_inline_image_multiline_with_attributes() {
        // Image with multiline alt text and attributes
        let input = "![network graph\ndiagram](../images/fig.png){width=70%}";
        let result = try_parse_inline_image(input, LinkScanContext::default());
        assert!(
            result.is_some(),
            "Image alt with newlines and attributes should parse"
        );
        let (len, alt, dest, attrs) = result.unwrap();
        assert!(alt.contains('\n'), "Alt text should preserve newline");
        assert_eq!(dest, "../images/fig.png");
        assert_eq!(attrs, Some("{width=70%}"));
        assert_eq!(len, input.len());
    }

    #[test]
    fn test_parse_inline_link_with_attributes_after_newline() {
        // Test for regression: when text is concatenated with newlines,
        // attributes after ) should still be recognized
        let input = "[A network graph.](../images/networkfig.png){width=70%}\nA word\n";
        let result = try_parse_inline_link(input, false, LinkScanContext::default());
        assert!(
            result.is_some(),
            "Link with attributes should parse even with following text"
        );
        let (len, text, dest, attrs) = result.unwrap();
        assert_eq!(text, "A network graph.");
        assert_eq!(dest, "../images/networkfig.png");
        assert_eq!(attrs, Some("{width=70%}"), "Attributes should be captured");
        assert_eq!(
            len, 55,
            "Length should include attributes (up to closing brace)"
        );
    }
}
