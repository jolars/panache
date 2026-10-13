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
use std::ops::Range;

use super::sink::InlineSink;
use crate::options::ParserOptions;
use crate::syntax::SyntaxKind;

use crate::parser::utils::attributes::{emit_attribute_node, try_parse_trailing_attributes};

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
#[derive(Clone, Copy)]
pub struct LinkScanContext {
    pub skip_raw_html: bool,
    pub skip_autolinks: bool,
    pub disallow_inner_links: bool,
    /// Dialect controlling which HTML constructs the raw-HTML opacity check
    /// recognizes. Pandoc-markdown excludes bare declarations and CDATA
    /// from its inline raw HTML grammar.
    pub dialect: crate::options::Dialect,
    pub hugo_shortcodes: bool,
}

/// Destination slots captured while the inline-link scanner already has the
/// source split into URL, title, delimiters, and trivia.
#[derive(Debug, Clone)]
pub(super) struct ParsedLinkDestination<'a> {
    raw: &'a str,
    url: Range<usize>,
    url_delimiters: Option<DestinationDelimiters>,
    title: Option<Range<usize>>,
    title_delimiters: Option<DestinationDelimiters>,
}

type DestinationDelimiters = (Range<usize>, Range<usize>);
type ParsedTitle = (Option<Range<usize>>, Option<DestinationDelimiters>);

impl Default for LinkScanContext {
    fn default() -> Self {
        Self {
            skip_raw_html: false,
            skip_autolinks: false,
            disallow_inner_links: false,
            dialect: crate::options::Dialect::Pandoc,
            hugo_shortcodes: false,
        }
    }
}

impl LinkScanContext {
    pub fn from_options(config: &ParserOptions) -> Self {
        let is_commonmark = config.dialect == crate::options::Dialect::CommonMark;
        Self {
            skip_raw_html: config.extensions.raw_html,
            skip_autolinks: config.extensions.autolinks && is_commonmark,
            disallow_inner_links: is_commonmark,
            dialect: config.dialect,
            hugo_shortcodes: config.extensions.hugo_shortcodes,
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
    let hugo = ctx
        .hugo_shortcodes
        .then(|| super::hugo::HugoIndex::new(text));
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

        if b == b'{'
            && let Some(index) = &hugo
            && let Some(span) = index.scan_at(text, i).filter(|span| span.opening.complete)
        {
            i += span.end;
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
                    && let Some(len) = try_parse_inline_html(&text[i..], ctx.dialect)
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
fn find_dest_close_paren(remaining: &str, hugo_shortcodes: bool) -> Option<usize> {
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

        if hugo_shortcodes
            && b == b'{'
            && let Some(tag) = super::hugo::scan_tag(&remaining[i..]).filter(|tag| tag.complete)
        {
            i += tag.end;
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
    let hugo = ctx
        .hugo_shortcodes
        .then(|| super::hugo::HugoIndex::new(text));
    let mut i = 0;
    let mut escape_next = false;
    while i < bytes.len() {
        let b = bytes[i];
        if escape_next {
            escape_next = false;
            i += step(text, i);
            continue;
        }
        if b == b'{'
            && let Some(index) = &hugo
            && let Some(span) = index.scan_at(text, i).filter(|span| span.opening.complete)
        {
            i += span.end;
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
                    && let Some(len) = try_parse_inline_html(&text[i..], ctx.dialect)
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
    try_parse_inline_image_parts(text, ctx)
        .map(|(len, alt, destination, attrs)| (len, alt, destination.raw, attrs))
}

pub(super) fn try_parse_inline_image_parts(
    text: &str,
    ctx: LinkScanContext,
) -> Option<(usize, &str, ParsedLinkDestination<'_>, Option<&str>)> {
    if !text.starts_with("![") {
        return None;
    }

    let close_bracket = find_link_close_bracket(text, 2, ctx)?;
    let alt_text = &text[2..close_bracket];

    let after_bracket = close_bracket + 1;
    if text.len() <= after_bracket || !text[after_bracket..].starts_with('(') {
        return None;
    }

    let dest_start = after_bracket + 1;
    let remaining = &text[dest_start..];

    let close_paren = find_dest_close_paren(remaining, ctx.hugo_shortcodes)?;
    let dest_content = &remaining[..close_paren];
    let destination = parse_link_destination_parts(
        dest_content,
        ctx.dialect == crate::options::Dialect::CommonMark,
        ctx.hugo_shortcodes,
    )?;

    let after_paren = dest_start + close_paren + 1;
    let after_close = &text[after_paren..];

    if after_close.starts_with('{')
        && let Some(close_brace_pos) = after_close.find('}')
    {
        let attr_text = &after_close[..=close_brace_pos];
        if let Some((_attrs, _)) = try_parse_trailing_attributes(attr_text) {
            let total_len = after_paren + close_brace_pos + 1;
            let raw_attrs = attr_text;
            return Some((total_len, alt_text, destination, Some(raw_attrs)));
        }
    }

    let total_len = after_paren;
    Some((total_len, alt_text, destination, None))
}

/// Emit an inline image node to the builder.
/// Note: alt_text may contain inline elements and should be parsed recursively.
pub fn emit_inline_image(
    builder: &mut impl InlineSink,
    text: &str,
    alt_text: &str,
    dest: &str,
    raw_attributes: Option<&str>,
    config: &ParserOptions,
    suppress_footnote_refs: bool,
) {
    let destination = parse_link_destination_parts(
        dest,
        config.dialect == crate::options::Dialect::CommonMark,
        config.extensions.hugo_shortcodes,
    )
    .unwrap_or_else(|| parse_pandoc_destination_parts(dest));
    emit_inline_image_parts(
        builder,
        text,
        alt_text,
        destination,
        raw_attributes,
        config,
        suppress_footnote_refs,
    );
}

pub(super) fn emit_inline_image_parts(
    builder: &mut impl InlineSink,
    _text: &str,
    alt_text: &str,
    dest: ParsedLinkDestination<'_>,
    raw_attributes: Option<&str>,
    config: &ParserOptions,
    suppress_footnote_refs: bool,
) {
    builder.start_node(SyntaxKind::IMAGE_LINK.into());

    builder.start_node(SyntaxKind::IMAGE_LINK_START.into());
    builder.token(SyntaxKind::IMAGE_LINK_START.into(), "![");
    builder.finish_node();

    builder.start_node(SyntaxKind::IMAGE_ALT.into());
    parse_inline_text(builder, alt_text, config, false, suppress_footnote_refs);
    builder.finish_node();

    builder.token(SyntaxKind::IMAGE_ALT_END.into(), "]");

    builder.token(SyntaxKind::IMAGE_DEST_START.into(), "(");

    emit_link_destination(builder, &dest);

    builder.token(SyntaxKind::IMAGE_DEST_END.into(), ")");

    if let Some(raw_attrs) = raw_attributes {
        emit_attribute_node(builder, raw_attrs);
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
    // A one-character label has no interior (`1..0` is not a valid slice).
    let interior = bytes.get(1..bytes.len() - 1).unwrap_or_default();
    interior
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
}

/// Emit an automatic link node to the builder.
pub fn emit_autolink(builder: &mut impl InlineSink, _text: &str, url: &str) {
    builder.start_node(SyntaxKind::AUTO_LINK.into());

    builder.start_node(SyntaxKind::AUTO_LINK_MARKER.into());
    builder.token(SyntaxKind::AUTO_LINK_MARKER.into(), "<");
    builder.finish_node();

    builder.token(SyntaxKind::TEXT.into(), url);

    builder.start_node(SyntaxKind::AUTO_LINK_MARKER.into());
    builder.token(SyntaxKind::AUTO_LINK_MARKER.into(), ">");
    builder.finish_node();

    builder.finish_node();
}

include!(concat!(env!("OUT_DIR"), "/uri_schemes.rs"));

fn is_known_bare_uri_scheme(scheme: &str) -> bool {
    let lower = scheme.to_ascii_lowercase();
    BARE_URI_SCHEMES.binary_search(&lower.as_str()).is_ok()
}

pub fn try_parse_bare_uri(text: &str) -> Option<(usize, &str)> {
    try_parse_bare_uri_with_hugo(text, false)
}

pub(super) fn try_parse_bare_uri_with_hugo(
    text: &str,
    hugo_shortcodes: bool,
) -> Option<(usize, &str)> {
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

    if !is_known_bare_uri_scheme(&text[..scheme_end]) {
        return None;
    }

    let mut end = scheme_end + 1;
    let bytes = text.as_bytes();
    while end < text.len() {
        let b = bytes[end];
        if hugo_shortcodes
            && b == b'{'
            && (bytes[end..].starts_with(b"{{<") || bytes[end..].starts_with(b"{{%"))
        {
            break;
        }
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
        if matches!(
            ch,
            '.' | ',' | ';' | ':' | '?' | '!' | '*' | '_' | '~' | ')' | ']' | '}'
        ) {
            trimmed -= ch.len_utf8();
        } else {
            break;
        }
    }

    if trimmed <= scheme_end + 1 {
        return None;
    }

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
    try_parse_inline_link_parts(text, strict_dest, ctx)
        .map(|(len, label, destination, attrs)| (len, label, destination.raw, attrs))
}

pub(super) fn try_parse_inline_link_parts(
    text: &str,
    strict_dest: bool,
    ctx: LinkScanContext,
) -> Option<(usize, &str, ParsedLinkDestination<'_>, Option<&str>)> {
    if !text.starts_with('[') {
        return None;
    }

    let close_bracket = find_link_close_bracket(text, 1, ctx)?;
    let link_text = &text[1..close_bracket];

    let after_bracket = close_bracket + 1;
    if text.len() <= after_bracket || !text[after_bracket..].starts_with('(') {
        return None;
    }

    let dest_start = after_bracket + 1;
    let remaining = &text[dest_start..];

    let close_paren = find_dest_close_paren(remaining, ctx.hugo_shortcodes)?;
    let dest_content = &remaining[..close_paren];
    let destination = parse_link_destination_parts(dest_content, strict_dest, ctx.hugo_shortcodes)?;

    if ctx.disallow_inner_links && link_text_contains_inner_link(link_text, ctx, strict_dest) {
        return None;
    }

    let after_paren = dest_start + close_paren + 1;
    let after_close = &text[after_paren..];

    if after_close.starts_with('{')
        && let Some(close_brace_pos) = after_close.find('}')
    {
        let attr_text = &after_close[..=close_brace_pos];
        if let Some((_attrs, _)) = try_parse_trailing_attributes(attr_text) {
            let total_len = after_paren + close_brace_pos + 1;
            let raw_attrs = attr_text;
            return Some((total_len, link_text, destination, Some(raw_attrs)));
        }
    }

    let total_len = after_paren;
    Some((total_len, link_text, destination, None))
}

/// CommonMark §6.4 destination + optional title validation. The text passed
/// in is whatever the parser captured between `(` and `)`. A valid form is:
/// `[ws] destination [ws title [ws]]` where:
/// - bare destination has no spaces, tabs, ASCII control chars, and balanced
///   parentheses (escaped parens permitted);
/// - bracketed destination is `<...>` with no newlines and no unescaped `<>`;
/// - the optional title is delimited by `"..."`, `'...'`, or `(...)`;
/// - any text outside that structure invalidates the link.
fn parse_link_destination_parts(
    content: &str,
    strict: bool,
    hugo_shortcodes: bool,
) -> Option<ParsedLinkDestination<'_>> {
    if strict {
        parse_commonmark_destination_parts(content, hugo_shortcodes)
    } else {
        Some(parse_pandoc_destination_parts(content))
    }
}

fn parse_commonmark_destination_parts(
    content: &str,
    hugo_shortcodes: bool,
) -> Option<ParsedLinkDestination<'_>> {
    let bytes = content.as_bytes();
    let mut p = link_ws_end(bytes, 0);
    if p == bytes.len() {
        return Some(ParsedLinkDestination {
            raw: content,
            url: p..p,
            url_delimiters: None,
            title: None,
            title_delimiters: None,
        });
    }

    let (url, url_delimiters, dest_end) = if bytes[p] == b'<' {
        let open = p..p + 1;
        p += 1;
        let start = p;
        let mut escape = false;
        while p < bytes.len() {
            let byte = bytes[p];
            if escape {
                escape = false;
                p += 1;
                continue;
            }
            if hugo_shortcodes
                && byte == b'{'
                && let Some(tag) = super::hugo::scan_tag(&content[p..]).filter(|tag| tag.complete)
            {
                p += tag.end;
                continue;
            }
            match byte {
                b'\\' => {
                    escape = true;
                    p += 1;
                }
                b'\n' | b'<' => return None,
                b'>' => break,
                _ => p += 1,
            }
        }
        if p >= bytes.len() || bytes[p] != b'>' {
            return None;
        }
        let close = p..p + 1;
        (start..p, Some((open, close)), p + 1)
    } else {
        let start = p;
        let mut escape = false;
        let mut depth = 0i32;
        while p < bytes.len() {
            let byte = bytes[p];
            if escape {
                escape = false;
                p += 1;
                continue;
            }
            if hugo_shortcodes
                && byte == b'{'
                && let Some(tag) = super::hugo::scan_tag(&content[p..]).filter(|tag| tag.complete)
            {
                p += tag.end;
                continue;
            }
            match byte {
                b'\\' => {
                    escape = true;
                    p += 1;
                }
                b' ' | b'\t' | b'\n' => break,
                byte if byte < 0x20 || byte == 0x7f => return None,
                b'(' => {
                    depth += 1;
                    p += 1;
                }
                b')' => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                    p += 1;
                }
                _ => p += 1,
            }
        }
        if p == start || depth != 0 {
            return None;
        }
        (start..p, None, p)
    };

    p = link_ws_end(bytes, dest_end);
    if p == bytes.len() {
        return Some(ParsedLinkDestination {
            raw: content,
            url,
            url_delimiters,
            title: None,
            title_delimiters: None,
        });
    }

    let open = p;
    let close_byte = match bytes[p] {
        b'"' => b'"',
        b'\'' => b'\'',
        b'(' => b')',
        _ => return None,
    };
    let opens_paren = bytes[p] == b'(';
    p += 1;
    let title_start = p;
    let mut escape = false;
    while p < bytes.len() {
        let byte = bytes[p];
        if escape {
            escape = false;
            p += 1;
            continue;
        }
        if byte == b'\\' {
            escape = true;
            p += 1;
            continue;
        }
        if opens_paren && byte == b'(' {
            return None;
        }
        if byte == close_byte {
            break;
        }
        p += 1;
    }
    if p >= bytes.len() {
        return None;
    }
    let title_end = p;
    let close = p..p + 1;
    p = link_ws_end(bytes, p + 1);
    if p != bytes.len() {
        return None;
    }

    Some(ParsedLinkDestination {
        raw: content,
        url,
        url_delimiters,
        title: Some(title_start..title_end),
        title_delimiters: Some((open..open + 1, close)),
    })
}

fn parse_pandoc_destination_parts(content: &str) -> ParsedLinkDestination<'_> {
    let bytes = content.as_bytes();
    let trimmed_start = link_ws_end(bytes, 0);
    let trimmed_end = bytes
        .iter()
        .rposition(|byte| !matches!(byte, b' ' | b'\t' | b'\n'))
        .map_or(trimmed_start, |index| index + 1);

    if trimmed_start >= trimmed_end {
        return ParsedLinkDestination {
            raw: content,
            url: trimmed_start..trimmed_start,
            url_delimiters: None,
            title: None,
            title_delimiters: None,
        };
    }

    if bytes[trimmed_start] == b'<'
        && let Some(relative_end) = content[trimmed_start + 1..trimmed_end].find('>')
    {
        let close_index = trimmed_start + 1 + relative_end;
        let after = link_ws_end(bytes, close_index + 1);
        let (title, title_delimiters) = parse_permissive_title(bytes, after, trimmed_end);
        return ParsedLinkDestination {
            raw: content,
            url: trimmed_start + 1..close_index,
            url_delimiters: Some((
                trimmed_start..trimmed_start + 1,
                close_index..close_index + 1,
            )),
            title,
            title_delimiters,
        };
    }

    let mut url_end = trimmed_end;
    let mut index = trimmed_start;
    while index < trimmed_end {
        if matches!(bytes[index], b' ' | b'\t' | b'\n') {
            let next = link_ws_end(bytes, index);
            if next < trimmed_end && matches!(bytes[next], b'"' | b'\'' | b'(') {
                url_end = index;
                break;
            }
            index = next;
        } else {
            index += 1;
        }
    }
    let title_start = link_ws_end(bytes, url_end);
    let (title, title_delimiters) = parse_permissive_title(bytes, title_start, trimmed_end);
    ParsedLinkDestination {
        raw: content,
        url: trimmed_start..url_end,
        url_delimiters: None,
        title,
        title_delimiters,
    }
}

fn parse_permissive_title(bytes: &[u8], start: usize, end: usize) -> ParsedTitle {
    if start >= end {
        return (None, None);
    }
    let close_byte = match bytes[start] {
        b'"' => b'"',
        b'\'' => b'\'',
        b'(' => b')',
        _ => return (None, None),
    };
    let Some(relative_close) = bytes[start + 1..end]
        .iter()
        .rposition(|byte| *byte == close_byte)
    else {
        return (None, None);
    };
    let close = start + 1 + relative_close;
    (
        Some(start + 1..close),
        Some((start..start + 1, close..close + 1)),
    )
}

fn link_ws_end(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\t' | b'\n') {
        index += 1;
    }
    index
}

fn emit_link_destination(builder: &mut impl InlineSink, destination: &ParsedLinkDestination<'_>) {
    builder.start_node(SyntaxKind::LINK_DEST.into());
    let mut cursor = 0;

    if let Some((open, close)) = &destination.url_delimiters {
        emit_destination_text(builder, &destination.raw[cursor..open.start]);
        builder.start_node(SyntaxKind::LINK_DEST_URL.into());
        builder.token(
            SyntaxKind::LINK_DEST_URL_MARKER.into(),
            &destination.raw[open.clone()],
        );
        builder.token(
            SyntaxKind::TEXT.into(),
            &destination.raw[destination.url.clone()],
        );
        builder.token(
            SyntaxKind::LINK_DEST_URL_MARKER.into(),
            &destination.raw[close.clone()],
        );
        builder.finish_node();
        cursor = close.end;
    } else {
        emit_destination_text(builder, &destination.raw[cursor..destination.url.start]);
        builder.start_node(SyntaxKind::LINK_DEST_URL.into());
        builder.token(
            SyntaxKind::TEXT.into(),
            &destination.raw[destination.url.clone()],
        );
        builder.finish_node();
        cursor = destination.url.end;
    }

    if let (Some(title), Some((open, close))) = (&destination.title, &destination.title_delimiters)
    {
        emit_destination_text(builder, &destination.raw[cursor..open.start]);
        builder.start_node(SyntaxKind::LINK_DEST_TITLE.into());
        builder.token(
            SyntaxKind::LINK_DEST_TITLE_MARKER.into(),
            &destination.raw[open.clone()],
        );
        builder.token(SyntaxKind::TEXT.into(), &destination.raw[title.clone()]);
        builder.token(
            SyntaxKind::LINK_DEST_TITLE_MARKER.into(),
            &destination.raw[close.clone()],
        );
        builder.finish_node();
        cursor = close.end;
    }

    emit_destination_text(builder, &destination.raw[cursor..]);
    builder.finish_node();
}

fn emit_destination_text(builder: &mut impl InlineSink, text: &str) {
    if !text.is_empty() {
        builder.token(SyntaxKind::TEXT.into(), text);
    }
}

/// Emit an inline link node to the builder.
/// Note: link_text may contain inline elements and should be parsed recursively.
pub fn emit_inline_link(
    builder: &mut impl InlineSink,
    text: &str,
    link_text: &str,
    dest: &str,
    raw_attributes: Option<&str>,
    config: &ParserOptions,
    suppress_footnote_refs: bool,
) {
    let destination = parse_link_destination_parts(
        dest,
        config.dialect == crate::options::Dialect::CommonMark,
        config.extensions.hugo_shortcodes,
    )
    .unwrap_or_else(|| parse_pandoc_destination_parts(dest));
    emit_inline_link_parts(
        builder,
        text,
        link_text,
        destination,
        raw_attributes,
        config,
        suppress_footnote_refs,
    );
}

pub(super) fn emit_inline_link_parts(
    builder: &mut impl InlineSink,
    _text: &str,
    link_text: &str,
    dest: ParsedLinkDestination<'_>,
    raw_attributes: Option<&str>,
    config: &ParserOptions,
    suppress_footnote_refs: bool,
) {
    builder.start_node(SyntaxKind::LINK.into());

    builder.start_node(SyntaxKind::LINK_START.into());
    builder.token(SyntaxKind::LINK_START.into(), "[");
    builder.finish_node();

    builder.start_node(SyntaxKind::LINK_TEXT.into());
    parse_inline_text(builder, link_text, config, true, suppress_footnote_refs);
    builder.finish_node();

    builder.token(SyntaxKind::LINK_TEXT_END.into(), "]");

    builder.token(SyntaxKind::LINK_DEST_START.into(), "(");

    emit_link_destination(builder, &dest);

    builder.token(SyntaxKind::LINK_DEST_END.into(), ")");

    if let Some(raw_attrs) = raw_attributes {
        emit_attribute_node(builder, raw_attrs);
    }

    builder.finish_node();
}

/// Emit a bare-URI autolink (pandoc's `autolink_bare_uris`).
///
/// A bare URI like `https://example.com` carries no syntactic markers in the
/// source, so the CST must contain exactly its bytes — nothing else. We emit it
/// as a marker-less [`AUTO_LINK`](SyntaxKind::AUTO_LINK) holding a single `TEXT`
/// token: lossless, and a faithful structural sibling of the angle-bracket
/// autolink (`<url>`), which the same node represents with `AUTO_LINK_MARKER`
/// tokens around the text. Downstream (formatter, pandoc AST, HTML renderer)
/// derives the destination from the `TEXT` token and re-emits markers verbatim,
/// so a bare URI round-trips to `url` while `<url>` round-trips to `<url>`.
///
/// Emitting a `LINK` with fabricated `[`/`]`/`(`/`)` tokens (the previous
/// approach) duplicated the URL and inflated the node's text range, breaking
/// losslessness and desyncing every byte offset after the URI.
pub fn emit_bare_uri_link(builder: &mut impl InlineSink, uri: &str, _config: &ParserOptions) {
    builder.start_node(SyntaxKind::AUTO_LINK.into());
    builder.token(SyntaxKind::TEXT.into(), uri);
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
    allow_spaced: bool,
    ctx: LinkScanContext,
) -> Option<(usize, &str, String, &str, bool)> {
    if !text.starts_with('[') {
        return None;
    }

    if text.len() > 1 {
        let bytes = text.as_bytes();
        if bytes[1] == b'@' {
            return None;
        }
        if bytes[1] == b'-' && text.len() > 2 && bytes[2] == b'@' {
            return None;
        }
    }

    let close_bracket = find_link_close_bracket(text, 1, ctx)?;
    let link_text = &text[1..close_bracket];

    if ctx.disallow_inner_links
        && link_text_contains_inner_link(link_text, ctx, ctx.disallow_inner_links)
    {
        return None;
    }

    let after_bracket = close_bracket + 1;

    if after_bracket < text.len() && text[after_bracket..].starts_with('{') {
        return None;
    }

    if after_bracket < text.len()
        && text[after_bracket..].starts_with('(')
        && (!allow_shortcut || !inline_link_attempted)
    {
        return None;
    }

    let gap_end = if allow_spaced {
        let bytes = text.as_bytes();
        let mut p = after_bracket;
        let mut saw_newline = false;
        while p < bytes.len() {
            match bytes[p] {
                b' ' | b'\t' => p += 1,
                b'\n' if !saw_newline => {
                    saw_newline = true;
                    p += 1;
                }
                _ => break,
            }
        }
        p
    } else {
        after_bracket
    };
    let gap = &text[after_bracket..gap_end];

    if gap_end < text.len() && text[gap_end..].starts_with('[') {
        let label_start = gap_end + 1;
        let mut label_end = None;

        for (i, ch) in text[label_start..].char_indices() {
            if ch == ']' {
                label_end = Some(i + label_start);
                break;
            }
            if ch == '\n' {
                return None;
            }
        }

        let label_end = label_end?;
        let label = &text[label_start..label_end];

        let total_len = label_end + 1;

        if label.is_empty() {
            return Some((total_len, link_text, String::new(), gap, false));
        }

        Some((total_len, link_text, label.to_string(), gap, false))
    } else if allow_shortcut {
        if link_text.is_empty() {
            return None;
        }
        Some((after_bracket, link_text, link_text.to_string(), "", true))
    } else {
        None
    }
}

/// Emit a reference link node to the builder.
/// Preserves the original reference syntax (explicit [text][ref], implicit [text][], or shortcut [text]).
/// `gap` carries any whitespace consumed between the link-text `]` and the
/// label `[` under `spaced_reference_links`; empty otherwise.
pub fn emit_reference_link(
    builder: &mut impl InlineSink,
    link_text: &str,
    label: &str,
    gap: &str,
    is_shortcut: bool,
    config: &ParserOptions,
    suppress_footnote_refs: bool,
) {
    builder.start_node(SyntaxKind::LINK.into());

    builder.start_node(SyntaxKind::LINK_START.into());
    builder.token(SyntaxKind::LINK_START.into(), "[");
    builder.finish_node();

    builder.start_node(SyntaxKind::LINK_TEXT.into());
    parse_inline_text(builder, link_text, config, true, suppress_footnote_refs);
    builder.finish_node();

    builder.token(SyntaxKind::TEXT.into(), "]");

    if !is_shortcut {
        emit_reference_link_gap(builder, gap);
        builder.token(SyntaxKind::TEXT.into(), "[");
        builder.start_node(SyntaxKind::LINK_REF.into());
        if !label.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), label);
        }
        builder.finish_node();
        builder.token(SyntaxKind::TEXT.into(), "]");
    }

    builder.finish_node();
}

fn emit_reference_link_gap(builder: &mut impl InlineSink, gap: &str) {
    if gap.is_empty() {
        return;
    }
    let bytes = gap.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        match bytes[start] {
            b'\r' => {
                let end = if start + 1 < bytes.len() && bytes[start + 1] == b'\n' {
                    start + 2
                } else {
                    start + 1
                };
                builder.token(SyntaxKind::NEWLINE.into(), &gap[start..end]);
                start = end;
            }
            b'\n' => {
                builder.token(SyntaxKind::NEWLINE.into(), &gap[start..start + 1]);
                start += 1;
            }
            _ => {
                let mut end = start + 1;
                while end < bytes.len() && !matches!(bytes[end], b'\r' | b'\n') {
                    end += 1;
                }
                builder.token(SyntaxKind::WHITESPACE.into(), &gap[start..end]);
                start = end;
            }
        }
    }
}

/// Try to parse a reference-style image: `![alt][ref]`, `![alt][]`, or `![alt]`
/// Returns (total_len, alt_text, label, gap, is_shortcut) if successful. `gap`
/// is the whitespace between `]` and `[` consumed under
/// `spaced_reference_links`; empty otherwise (and always empty for shortcuts).
pub fn try_parse_reference_image(
    text: &str,
    allow_shortcut: bool,
    allow_spaced: bool,
) -> Option<(usize, &str, String, &str, bool)> {
    let bytes = text.as_bytes();
    if bytes.len() < 4 || bytes[0] != b'!' || bytes[1] != b'[' {
        return None;
    }

    let mut pos = 2;
    let mut bracket_depth = 1;
    let alt_start = pos;

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
    let after_alt_close = pos;

    if allow_spaced {
        let mut saw_newline = false;
        while pos < bytes.len() {
            match bytes[pos] {
                b' ' | b'\t' => pos += 1,
                b'\n' if !saw_newline => {
                    saw_newline = true;
                    pos += 1;
                }
                _ => break,
            }
        }
    }
    let gap = &text[after_alt_close..pos];

    if pos >= bytes.len() {
        if allow_shortcut && gap.is_empty() {
            let label = alt_text.to_string();
            return Some((pos, alt_text, label, "", true));
        }
        return None;
    }

    if bytes[pos] == b'[' {
        pos += 1;
        let label_start = pos;

        while pos < bytes.len() && bytes[pos] != b']' && bytes[pos] != b'\n' && bytes[pos] != b'\r'
        {
            pos += 1;
        }

        if pos >= bytes.len() || bytes[pos] != b']' {
            return None;
        }

        let label_text = &text[label_start..pos];
        pos += 1;

        let label = if label_text.is_empty() {
            alt_text.to_string() // For implicit references, use alt text as label for equality check
        } else {
            label_text.to_string() // Preserve original case
        };

        return Some((pos, alt_text, label, gap, false));
    }

    if allow_shortcut {
        if bytes[after_alt_close] == b'(' {
            return None;
        }

        let label = alt_text.to_string();
        return Some((after_alt_close, alt_text, label, "", true));
    }

    None
}

/// Emit a reference image node with registry lookup. `gap` carries whitespace
/// consumed between `]` and `[` under `spaced_reference_links`; empty otherwise.
pub fn emit_reference_image(
    builder: &mut impl InlineSink,
    alt_text: &str,
    label: &str,
    gap: &str,
    is_shortcut: bool,
    config: &ParserOptions,
    suppress_footnote_refs: bool,
) {
    builder.start_node(SyntaxKind::IMAGE_LINK.into());

    builder.start_node(SyntaxKind::IMAGE_LINK_START.into());
    builder.token(SyntaxKind::IMAGE_LINK_START.into(), "![");
    builder.finish_node();

    builder.start_node(SyntaxKind::IMAGE_ALT.into());
    parse_inline_text(builder, alt_text, config, false, suppress_footnote_refs);
    builder.finish_node();

    builder.token(SyntaxKind::TEXT.into(), "]");

    if !is_shortcut {
        emit_reference_link_gap(builder, gap);
        builder.token(SyntaxKind::TEXT.into(), "[");
        builder.start_node(SyntaxKind::LINK_REF.into());
        if label != alt_text {
            builder.token(SyntaxKind::TEXT.into(), label);
        }
        builder.finish_node();
        builder.token(SyntaxKind::TEXT.into(), "]");
    }

    builder.finish_node();
}

/// Emit an `UNRESOLVED_REFERENCE` node for a Pandoc bracket-shape
/// pattern whose label didn't resolve. The wrapper covers the original
/// bracket bytes; the inner text recurses through normal inline
/// parsing (with inner-link suppression so a stray inner inline link
/// doesn't reorder semantics relative to pandoc-native).
///
/// `source` is `text[start..end]` — the full bracket-shape pattern.
/// `text_content` is the inner text between the outer `[` and `]`
/// (the bytes used for inline recursion). `label_suffix` carries the
/// `[label]` / `[]` suffix bytes, including any preceding whitespace gap,
/// verbatim, or `None` for shortcut form.
pub fn emit_unresolved_reference(
    builder: &mut impl InlineSink,
    is_image: bool,
    text_content: &str,
    label_suffix: Option<&str>,
    config: &ParserOptions,
    suppress_footnote_refs: bool,
) {
    builder.start_node(SyntaxKind::UNRESOLVED_REFERENCE.into());

    if is_image {
        builder.start_node(SyntaxKind::IMAGE_LINK_START.into());
        builder.token(SyntaxKind::IMAGE_LINK_START.into(), "![");
        builder.finish_node();
        builder.start_node(SyntaxKind::IMAGE_ALT.into());
        parse_inline_text(builder, text_content, config, false, suppress_footnote_refs);
        builder.finish_node();
    } else {
        builder.start_node(SyntaxKind::LINK_START.into());
        builder.token(SyntaxKind::LINK_START.into(), "[");
        builder.finish_node();
        builder.start_node(SyntaxKind::LINK_TEXT.into());
        parse_inline_text(builder, text_content, config, true, suppress_footnote_refs);
        builder.finish_node();
    }

    builder.token(SyntaxKind::TEXT.into(), "]");

    if let Some(suffix) = label_suffix {
        let label_suffix = suffix.trim_start_matches([' ', '\t', '\r', '\n']);
        emit_reference_link_gap(builder, &suffix[..suffix.len() - label_suffix.len()]);
        let suffix = label_suffix;
        debug_assert!(suffix.starts_with('[') && suffix.ends_with(']'));
        builder.token(SyntaxKind::TEXT.into(), "[");
        let label = &suffix[1..suffix.len() - 1];
        builder.start_node(SyntaxKind::LINK_REF.into());
        if !label.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), label);
        }
        builder.finish_node();
        builder.token(SyntaxKind::TEXT.into(), "]");
    }

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
        let input = "<m:abc>";
        assert_eq!(try_parse_autolink(input, true), None);
        assert_eq!(try_parse_autolink(input, false), Some((7, "m:abc")));
    }

    #[test]
    fn test_parse_autolink_email_single_char_domain_label() {
        for (input, email) in [
            ("<a@b.co>", "a@b.co"),
            ("<ab@c.de>", "ab@c.de"),
            ("<ab@cd.e>", "ab@cd.e"),
            ("<a@b>", "a@b"),
        ] {
            let expected = Some((input.len(), email));
            assert_eq!(try_parse_autolink(input, true), expected, "{input}");
            assert_eq!(try_parse_autolink(input, false), expected, "{input}");
        }
        assert_eq!(try_parse_autolink("<a@->", true), None);
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
    fn test_parse_bare_uri_rejects_unknown_scheme() {
        assert_eq!(try_parse_bare_uri("Note:**"), None);
        assert_eq!(try_parse_bare_uri("Note:foo"), None);
        assert_eq!(try_parse_bare_uri("foo:bar"), None);
    }

    #[test]
    fn test_parse_bare_uri_accepts_known_schemes() {
        assert_eq!(
            try_parse_bare_uri("http://example.com"),
            Some((18, "http://example.com"))
        );
        assert_eq!(
            try_parse_bare_uri("HTTPS://EXAMPLE.COM"),
            Some((19, "HTTPS://EXAMPLE.COM"))
        );
        assert_eq!(
            try_parse_bare_uri("mailto:a@b.com"),
            Some((14, "mailto:a@b.com"))
        );
        assert_eq!(try_parse_bare_uri("doi:10.1/x"), Some((10, "doi:10.1/x")));
    }

    #[test]
    fn test_parse_bare_uri_trims_gfm_trailing_punctuation() {
        for punctuation in ['?', '!', '*', '_', '~'] {
            let input = format!("http://example.com/path{punctuation}");
            assert_eq!(
                try_parse_bare_uri(&input),
                Some((23, "http://example.com/path")),
                "trailing {punctuation:?} must stay outside the URI"
            );
        }

        assert_eq!(try_parse_bare_uri("Tool:****"), None);
    }

    #[test]
    fn bare_uri_scheme_table_is_well_formed() {
        assert!(
            BARE_URI_SCHEMES.len() > 300,
            "only {} schemes",
            BARE_URI_SCHEMES.len()
        );
        assert!(BARE_URI_SCHEMES.windows(2).all(|w| w[0] < w[1]));
        for known in ["http", "https", "mailto", "ftp", "mongodb", "shttp"] {
            assert!(is_known_bare_uri_scheme(known), "missing scheme {known}");
        }
        for extra in ["doi", "gemini", "isbn", "pmid"] {
            assert!(is_known_bare_uri_scheme(extra), "missing scheme {extra}");
        }
        assert!(!is_known_bare_uri_scheme("note"));
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
        let input = "![alt](img.png) {.large}";
        let result = try_parse_inline_image(input, LinkScanContext::default());
        assert_eq!(result, Some((15, "alt", "img.png", None)));
    }

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

    #[test]
    fn test_parse_reference_link_explicit() {
        let input = "[link text][label]";
        let result =
            try_parse_reference_link(input, false, true, false, LinkScanContext::default());
        assert_eq!(
            result,
            Some((18, "link text", "label".to_string(), "", false))
        );
    }

    #[test]
    fn test_parse_reference_link_implicit() {
        let input = "[link text][]";
        let result =
            try_parse_reference_link(input, false, true, false, LinkScanContext::default());
        assert_eq!(result, Some((13, "link text", String::new(), "", false)));
    }

    #[test]
    fn test_parse_reference_link_explicit_same_label_as_text() {
        let input = "[stack][stack]";
        let result =
            try_parse_reference_link(input, false, true, false, LinkScanContext::default());
        assert_eq!(result, Some((14, "stack", "stack".to_string(), "", false)));
    }

    #[test]
    fn test_parse_reference_link_shortcut() {
        let input = "[link text] rest";
        let result = try_parse_reference_link(input, true, true, false, LinkScanContext::default());
        assert_eq!(
            result,
            Some((11, "link text", "link text".to_string(), "", true))
        );
    }

    #[test]
    fn test_parse_reference_link_shortcut_rejects_empty_label() {
        let input = "[] rest";
        let result = try_parse_reference_link(input, true, true, false, LinkScanContext::default());
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_reference_link_shortcut_disabled() {
        let input = "[link text] rest";
        let result =
            try_parse_reference_link(input, false, true, false, LinkScanContext::default());
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_reference_link_not_inline_link() {
        let input = "[text](url)";
        let result =
            try_parse_reference_link(input, false, true, false, LinkScanContext::default());
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_reference_link_shortcut_falls_through_inline_link() {
        let input = "[text](url)";
        let result = try_parse_reference_link(input, true, true, false, LinkScanContext::default());
        assert_eq!(result, Some((6, "text", "text".to_string(), "", true)));
    }

    #[test]
    fn test_parse_reference_link_with_nested_brackets() {
        let input = "[outer [inner] text][ref]";
        let result =
            try_parse_reference_link(input, false, true, false, LinkScanContext::default());
        assert_eq!(
            result,
            Some((25, "outer [inner] text", "ref".to_string(), "", false))
        );
    }

    #[test]
    fn test_parse_reference_link_label_no_newline() {
        let input = "[text][label\nmore]";
        let result =
            try_parse_reference_link(input, false, true, false, LinkScanContext::default());
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_reference_link_spaced_disabled() {
        let input = "[foo] [bar]";
        let result = try_parse_reference_link(input, true, true, false, LinkScanContext::default());
        assert_eq!(result, Some((5, "foo", "foo".to_string(), "", true)));
    }

    #[test]
    fn test_parse_reference_link_spaced_single_space() {
        let input = "[foo] [bar]";
        let result = try_parse_reference_link(input, true, true, true, LinkScanContext::default());
        assert_eq!(result, Some((11, "foo", "bar".to_string(), " ", false)));
    }

    #[test]
    fn test_parse_reference_link_spaced_multiple_spaces_and_tab() {
        let input = "[foo]  \t[bar]";
        let result = try_parse_reference_link(input, true, true, true, LinkScanContext::default());
        assert_eq!(result, Some((13, "foo", "bar".to_string(), "  \t", false)));
    }

    #[test]
    fn test_parse_reference_link_spaced_newline() {
        let input = "[foo]\n[bar]";
        let result = try_parse_reference_link(input, true, true, true, LinkScanContext::default());
        assert_eq!(result, Some((11, "foo", "bar".to_string(), "\n", false)));
    }

    #[test]
    fn test_parse_reference_link_spaced_implicit() {
        let input = "[foo] []";
        let result = try_parse_reference_link(input, true, true, true, LinkScanContext::default());
        assert_eq!(result, Some((8, "foo", String::new(), " ", false)));
    }

    #[test]
    fn test_parse_reference_image_explicit() {
        let input = "![alt text][label]";
        let result = try_parse_reference_image(input, false, false);
        assert_eq!(
            result,
            Some((18, "alt text", "label".to_string(), "", false))
        );
    }

    #[test]
    fn test_parse_reference_image_implicit() {
        let input = "![alt text][]";
        let result = try_parse_reference_image(input, false, false);
        assert_eq!(
            result,
            Some((13, "alt text", "alt text".to_string(), "", false))
        );
    }

    #[test]
    fn test_parse_reference_image_shortcut() {
        let input = "![alt text] rest";
        let result = try_parse_reference_image(input, true, false);
        assert_eq!(
            result,
            Some((11, "alt text", "alt text".to_string(), "", true))
        );
    }

    #[test]
    fn test_parse_reference_image_shortcut_disabled() {
        let input = "![alt text] rest";
        let result = try_parse_reference_image(input, false, false);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_reference_image_not_inline() {
        let input = "![alt](url)";
        let result = try_parse_reference_image(input, true, false);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_reference_image_with_nested_brackets() {
        let input = "![alt [nested] text][ref]";
        let result = try_parse_reference_image(input, false, false);
        assert_eq!(
            result,
            Some((25, "alt [nested] text", "ref".to_string(), "", false))
        );
    }

    #[test]
    fn test_parse_reference_image_spaced() {
        let input = "![alt] [ref]";
        let result = try_parse_reference_image(input, true, true);
        assert_eq!(result, Some((12, "alt", "ref".to_string(), " ", false)));
    }

    #[test]
    fn test_reference_link_label_with_crlf() {
        let input = "[foo\r\nbar]";
        let result =
            try_parse_reference_link(input, false, true, false, LinkScanContext::default());

        assert_eq!(
            result, None,
            "Should not parse reference link with CRLF in label"
        );
    }

    #[test]
    fn test_reference_link_label_with_lf() {
        let input = "[foo\nbar]";
        let result =
            try_parse_reference_link(input, false, true, false, LinkScanContext::default());

        assert_eq!(
            result, None,
            "Should not parse reference link with LF in label"
        );
    }

    #[test]
    fn test_parse_inline_link_multiline_text() {
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
