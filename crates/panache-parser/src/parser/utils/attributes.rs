//! Parsing for Pandoc-style attributes: {#id .class key=value}
//!
//! Attributes can appear after headings, fenced code blocks, fenced divs, etc.
//! Syntax: {#identifier .class1 .class2 key1=val1 key2="val2"}
//!
//! Rules:
//! - Surrounded by { }
//! - Identifier: #id (optional, only first one counts)
//! - Classes: .class (can have multiple)
//! - Key-value pairs: key=value or key="value" or key='value' (can have multiple)
//! - Whitespace flexible between items

use crate::parser::inlines::sink::InlineSink;
use crate::syntax::SyntaxKind;
#[cfg(test)]
use rowan::GreenNodeBuilder;

#[derive(Debug, PartialEq)]
pub struct AttributeBlock {
    pub identifier: Option<String>,
    pub classes: Vec<String>,
    pub key_values: Vec<(String, String)>,
}

/// Try to parse an attribute block from the end of a string
/// Returns: (attribute_block, text_before_attributes)
pub fn try_parse_trailing_attributes(text: &str) -> Option<(AttributeBlock, &str)> {
    let (attrs, before, _) = try_parse_trailing_attributes_with_pos(text)?;
    Some((attrs, before))
}

/// Try to parse an attribute block from the end of a string.
/// Returns: (attribute_block, text_before_attributes, open_brace_position_in_trimmed_text)
pub fn try_parse_trailing_attributes_with_pos(text: &str) -> Option<(AttributeBlock, &str, usize)> {
    let trimmed = text.trim_end();

    if !trimmed.ends_with('}') {
        return None;
    }

    let open_brace = find_matching_open_brace_for_trailing_block(trimmed)?;
    // A shortcode's outer braces enclose arguments, not Markdown attributes.
    if trimmed[open_brace..].starts_with("{{<") || trimmed[open_brace..].starts_with("{{%") {
        return None;
    }

    let before_brace = &trimmed[..open_brace];
    if before_brace.trim_end().ends_with(']') {
        log::trace!("Skipping attribute parsing for bracketed span: {}", text);
        return None;
    }

    let attr_content = &trimmed[open_brace + 1..trimmed.len() - 1];
    let attr_block = parse_attribute_content(attr_content)?;

    let before_attrs = trim_attribute_gap(&trimmed[..open_brace]);

    Some((attr_block, before_attrs, open_brace))
}

/// Trim the whitespace gap in front of a trailing attribute block.
///
/// The gap has to be *unescaped* whitespace. `pandoc -f markdown` reads
/// `# foo\ {#id}` as `Header 1 (id) [Str "foo\160"]`: the `\ ` is a
/// nonbreaking space belonging to the content, and the attribute block comes
/// off on its own --- pandoc asks for no gap at all, so `# foo{#id}` carries
/// the attribute too. Trimming an escaped space would strand its backslash in
/// the content, turning `foo\160` into a literal `foo\`.
///
/// Only the escaped character itself is content; any further whitespace in
/// front of the block is still gap. Whether the last backslash escapes at all
/// depends on the parity of the run behind it: `# foo\\ {#id}` is
/// `[Str "foo\\"]` with an ordinary gap, `# foo\\\ {#id}` is
/// `[Str "foo\\\160"]`.
fn trim_attribute_gap(before: &str) -> &str {
    let trimmed = before.trim_end();
    if trimmed.len() == before.len() {
        return before;
    }

    let backslashes = trimmed.bytes().rev().take_while(|&b| b == b'\\').count();
    if backslashes % 2 == 0 {
        return trimmed;
    }

    match before[trimmed.len()..].chars().next() {
        Some(escaped) => &before[..trimmed.len() + escaped.len_utf8()],
        None => trimmed,
    }
}

fn find_matching_open_brace_for_trailing_block(text: &str) -> Option<usize> {
    if !text.ends_with('}') {
        return None;
    }

    let mut stack: Vec<usize> = Vec::new();
    let mut in_quote: Option<char> = None;
    let mut escaped = false;
    let mut end_brace_open = None;
    let hugo = (text.contains("{{<") || text.contains("{{%"))
        .then(|| crate::parser::inlines::hugo::HugoIndex::new(text));

    let mut chars = text.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if let Some(q) = in_quote {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == q {
                in_quote = None;
            }
            continue;
        }

        if ch == '{'
            && let Some(index) = &hugo
            && let Some(span) = index.scan_at(text, idx)
            && span.opening.complete
        {
            // Braces and quotes in shortcode payloads do not participate in
            // the surrounding heading's trailing attribute syntax.
            while chars.peek().is_some_and(|(pos, _)| *pos < idx + span.end) {
                chars.next();
            }
            continue;
        }

        match ch {
            '\'' | '"' => in_quote = Some(ch),
            '{' => stack.push(idx),
            '}' => {
                let open = stack.pop()?;
                if idx == text.len() - 1 {
                    end_brace_open = Some(open);
                }
            }
            _ => {}
        }
    }

    if in_quote.is_some() || !stack.is_empty() {
        return None;
    }

    end_brace_open
}

/// One recognized component inside an attribute `{...}` body, as byte ranges
/// relative to the `content` slice passed to [`attribute_content_spans`] (the
/// bytes strictly between `{` and `}`). Marker bytes (`#`/`.`/`=`) and value
/// quotes are kept INSIDE the ranges so the emitter can wrap the exact source
/// bytes; the string-deriving helpers strip them.
#[derive(Debug, Clone, PartialEq)]
pub enum AttrComponent {
    /// `#id` — range includes the leading `#`.
    Id(std::ops::Range<usize>),
    /// `.class` or `=format` — range includes the leading `.`/`=` marker.
    Class(std::ops::Range<usize>),
    /// A bare `-` — pandoc's shorthand for `.unnumbered`. Always exactly one
    /// byte, since pandoc consumes the `-` and resumes scanning immediately
    /// after it (`{---}` is three `unnumbered` classes, `{-.a}` is `unnumbered`
    /// plus `a`).
    Unnumbered(std::ops::Range<usize>),
    /// `key=value`: key range, `=` byte index, value range (the value range
    /// includes surrounding quotes when present).
    KeyValue {
        key: std::ops::Range<usize>,
        eq: usize,
        value: std::ops::Range<usize>,
    },
}

/// Recognized components of an attribute `{...}` body, in source order. The
/// single source of truth shared by detection ([`parse_attribute_content`],
/// which derives owned strings) and emission (`emit_attribute_node`, which
/// wraps these byte ranges in ATTR_* CST nodes) — one walk, no detect/emit
/// drift. Bytes the scan skips (duplicate `#id`, malformed tokens, whitespace)
/// are not components; the emitter recovers them from the gaps between ranges.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeSpans {
    pub components: Vec<AttrComponent>,
}

fn attr_value_string(raw: &str) -> String {
    let bytes = raw.as_bytes();
    if let Some(&q) = bytes.first()
        && (q == b'"' || q == b'\'')
    {
        let inner = &raw[1..];
        return inner.strip_suffix(q as char).unwrap_or(inner).to_string();
    }
    raw.to_string()
}

/// Decode HTML character references in an attribute *value*, mirroring what
/// pandoc's TagSoup-based reader does when it lifts `<div>`/`<span>`
/// attributes into a `Div`/`Span` `Attr`.
///
/// Only fully terminated references decode (`&name;`, `&#NN;`, `&#xHH;`);
/// an unknown name (`&notreal;`) and a bare `&` stay verbatim. Decoding is a
/// single pass, so `&amp;amp;` yields `&amp;`, not `&`. Named lookup is
/// case-sensitive against the HTML5 table, so `&AMP;` decodes but `&Amp;`
/// does not.
///
/// Known gap: pandoc also decodes some *semicolon-less* legacy references
/// (`id="a&amp b"` → `a& b`), gated on TagSoup's own name charset. That form
/// is left verbatim here.
///
/// The CST keeps the source bytes untouched — this decodes only at read time,
/// so losslessness is unaffected.
pub fn decode_html_attr_entities(raw: &str) -> std::borrow::Cow<'_, str> {
    if !raw.contains('&') {
        return std::borrow::Cow::Borrowed(raw);
    }
    let bytes = raw.as_bytes();
    let mut out: Option<String> = None;
    let mut copied = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'&' {
            i += 1;
            continue;
        }
        let Some((decoded, end)) = decode_char_reference(raw, i) else {
            i += 1;
            continue;
        };
        let buf = out.get_or_insert_with(String::new);
        buf.push_str(&raw[copied..i]);
        buf.push_str(&decoded);
        i = end;
        copied = end;
    }
    match out {
        Some(mut buf) => {
            buf.push_str(&raw[copied..]);
            std::borrow::Cow::Owned(buf)
        }
        None => std::borrow::Cow::Borrowed(raw),
    }
}

/// Decode the single character reference starting at `start` (which must index
/// an `&` in `raw`), returning the replacement text and the byte offset just
/// past the terminating `;`. `None` means "not a reference" — the caller
/// leaves the bytes alone.
fn decode_char_reference(raw: &str, start: usize) -> Option<(String, usize)> {
    let rest = &raw[start + 1..];
    let semi = rest.find(';')?;
    let body = &rest[..semi];
    if body.is_empty() {
        return None;
    }
    let end = start + 1 + semi + 1;

    if let Some(digits) = body.strip_prefix('#') {
        let code = if let Some(hex) = digits.strip_prefix(['x', 'X']) {
            if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            u32::from_str_radix(hex, 16).ok()
        } else {
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            digits.parse::<u32>().ok()
        };
        return Some((numeric_reference_char(code).to_string(), end));
    }

    if !body.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    let needle = &raw[start..end];
    entities::ENTITIES
        .iter()
        .find(|e| e.entity == needle)
        .map(|e| (e.characters.to_string(), end))
}

/// Map a numeric character reference's codepoint the way pandoc does: a
/// surrogate becomes U+FFFD, anything above U+10FFFF (or too large to parse)
/// becomes `?`, and `&#0;` stays NUL.
fn numeric_reference_char(code: Option<u32>) -> char {
    match code {
        None => '?',
        Some(c) if c > 0x10FFFF => '?',
        Some(c) if (0xD800..=0xDFFF).contains(&c) => '\u{FFFD}',
        Some(c) => char::from_u32(c).unwrap_or('\u{FFFD}'),
    }
}

/// Scan an attribute `{...}` body into [`AttributeSpans`]. Returns `None` when
/// no component is recognized (empty/whitespace-only/`{}` is not a valid
/// attribute block). Offsets are relative to `content`.
pub fn attribute_content_spans(content: &str) -> Option<AttributeSpans> {
    let bytes = content.as_bytes();
    let mut pos = 0;
    let mut components: Vec<AttrComponent> = Vec::new();
    let mut have_id = false;

    while pos < bytes.len() {
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }

        if bytes[pos] == b'=' {
            let start = pos;
            pos += 1; // skip '='
            while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() && bytes[pos] != b'}' {
                pos += 1;
            }
            if pos > start + 1 {
                components.push(AttrComponent::Class(start..pos));
            }
        } else if bytes[pos] == b'#' {
            let start = pos;
            pos += 1; // skip '#'
            while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() && bytes[pos] != b'}' {
                pos += 1;
            }
            if !have_id && pos > start + 1 {
                components.push(AttrComponent::Id(start..pos));
                have_id = true;
            }
        } else if bytes[pos] == b'-' {
            let run_end = pos + bytes[pos..].iter().take_while(|&&b| b == b'-').count();
            let mut word_end = run_end;
            while word_end < bytes.len() && !bytes[word_end].is_ascii_whitespace() {
                word_end += 1;
            }
            let follower = &content[run_end..word_end];
            let key_len = follower.bytes().take_while(|&b| b != b'=').count();
            let starts_new_item = follower.is_empty()
                || matches!(bytes[run_end], b'.' | b'#')
                || (key_len > 0 && key_len < follower.len());
            if starts_new_item {
                components.extend((pos..run_end).map(|i| AttrComponent::Unnumbered(i..i + 1)));
                pos = run_end;
            } else {
                pos = word_end;
            }
        } else if bytes[pos] == b'.' {
            let start = pos;
            pos += 1; // skip '.'
            while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() && bytes[pos] != b'}' {
                pos += 1;
            }
            if pos > start + 1 {
                components.push(AttrComponent::Class(start..pos));
            }
        } else {
            let key_start = pos;
            while pos < bytes.len() && bytes[pos] != b'=' && !bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if pos >= bytes.len() || bytes[pos] != b'=' {
                while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() {
                    pos += 1;
                }
                continue;
            }
            let key_end = pos;
            let eq = pos;
            pos += 1; // skip '='

            let value_start = pos;
            if pos < bytes.len() && (bytes[pos] == b'"' || bytes[pos] == b'\'') {
                let quote = bytes[pos];
                pos += 1; // opening quote
                while pos < bytes.len() && bytes[pos] != quote {
                    pos += 1;
                }
                if pos < bytes.len() {
                    pos += 1; // closing quote
                }
            } else {
                while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() && bytes[pos] != b'}' {
                    pos += 1;
                }
            }
            if key_end > key_start {
                components.push(AttrComponent::KeyValue {
                    key: key_start..key_end,
                    eq,
                    value: value_start..pos,
                });
            }
        }
    }

    if components.is_empty() {
        return None;
    }
    Some(AttributeSpans { components })
}

/// Parse the content inside the attribute braces into owned strings. Thin
/// wrapper over [`attribute_content_spans`] so detection and emission share one
/// walk.
pub fn parse_attribute_content(content: &str) -> Option<AttributeBlock> {
    let spans = attribute_content_spans(content)?;
    let mut identifier = None;
    let mut classes = Vec::new();
    let mut key_values = Vec::new();

    for comp in &spans.components {
        match comp {
            AttrComponent::Id(r) => {
                identifier = Some(content[r.start + 1..r.end].to_string());
            }
            AttrComponent::Class(r) => {
                let raw = &content[r.clone()];
                match raw.strip_prefix('.') {
                    Some(class) => classes.push(class.to_string()),
                    None => classes.push(raw.to_string()),
                }
            }
            AttrComponent::Unnumbered(_) => classes.push("unnumbered".to_string()),
            AttrComponent::KeyValue { key, value, .. } => {
                key_values.push((
                    content[key.clone()].to_string(),
                    attr_value_string(&content[value.clone()]),
                ));
            }
        }
    }

    Some(AttributeBlock {
        identifier,
        classes,
        key_values,
    })
}

/// Parse HTML-style attributes from a raw HTML opening tag text such as
/// `<div id="x" class="a b" data-key="v">`, returning the same
/// `AttributeBlock` shape as Pandoc-style brace attributes. Whitespace-
/// separated `class="..."` is split into individual classes; `id="..."`
/// becomes the identifier; everything else becomes a key/value pair.
/// Returns `None` if the tag has no recognized attributes.
///
/// Self-closing slashes (`<div .../>`) and trailing whitespace are tolerated.
/// The leading `<TAG` and trailing `>` are stripped; this routine does not
/// validate the tag name.
pub fn parse_html_tag_attributes(tag_text: &str) -> Option<AttributeBlock> {
    let trimmed = tag_text.trim_start();
    let after_lt = trimmed.strip_prefix('<')?;
    let bytes = after_lt.as_bytes();
    let mut tag_end = None;
    let mut quote: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate() {
        match (quote, b) {
            (None, b'"') | (None, b'\'') => quote = Some(b),
            (Some(q), b2) if b2 == q => quote = None,
            (None, b'>') => {
                tag_end = Some(i);
                break;
            }
            _ => {}
        }
    }
    let tag_end = tag_end?;
    let inner = &after_lt[..tag_end];
    let inner = inner.trim_end().trim_end_matches('/').trim_end();
    let bytes = inner.as_bytes();
    let mut name_end = 0usize;
    while name_end < bytes.len()
        && !bytes[name_end].is_ascii_whitespace()
        && bytes[name_end] != b'/'
    {
        name_end += 1;
    }
    let attrs_text = &inner[name_end..];
    parse_html_attribute_list(attrs_text)
}

/// Parse a raw HTML attribute list (the bytes between a tag name and the
/// closing `>`, exclusive). Accepts inputs like `id="x" class="a b"
/// data-key=v` and produces an [`AttributeBlock`]. Returns `None` if no
/// recognized attributes are present.
///
/// Used by [`parse_html_tag_attributes`] (which strips `<TAG ...>`
/// surrounding chrome before delegating here) and by
/// `AttributeNode::id` for the structural `HTML_ATTRS` CST node, whose
/// text holds JUST the attribute region.
pub fn parse_html_attribute_list(attrs_text: &str) -> Option<AttributeBlock> {
    let comps = html_attribute_spans(attrs_text);
    if comps.is_empty() {
        return None;
    }
    let mut identifier: Option<String> = None;
    let mut classes: Vec<String> = Vec::new();
    let mut key_values: Vec<(String, String)> = Vec::new();
    for comp in &comps {
        match comp {
            HtmlAttrComponent::Id(r) => {
                if identifier.is_none() {
                    identifier =
                        Some(decode_html_attr_entities(&attrs_text[r.clone()]).into_owned());
                }
            }
            HtmlAttrComponent::Class(r) => {
                classes.push(decode_html_attr_entities(&attrs_text[r.clone()]).into_owned())
            }
            HtmlAttrComponent::KeyValue { key, value, .. } => {
                key_values.push((
                    attrs_text[key.clone()].to_string(),
                    decode_html_attr_entities(&attr_value_string(&attrs_text[value.clone()]))
                        .into_owned(),
                ));
            }
            HtmlAttrComponent::Flag(r) => {
                key_values.push((attrs_text[r.clone()].to_string(), String::new()));
            }
        }
    }
    if identifier.is_none() && classes.is_empty() && key_values.is_empty() {
        return None;
    }
    Some(AttributeBlock {
        identifier,
        classes,
        key_values,
    })
}

/// Pandoc's native HTML elements require every attribute name to start with
/// a Unicode letter and continue with letters, numbers, `:`, `-`, or `_`.
/// Values are skipped verbatim: entities there do not invalidate the name.
pub(crate) fn pandoc_html_attribute_names_valid(mut attrs: &str) -> bool {
    while !attrs.is_empty() {
        attrs = attrs.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '/');
        if attrs.is_empty() {
            break;
        }
        let name_end = attrs
            .find(|c: char| c.is_ascii_whitespace() || matches!(c, '=' | '/'))
            .unwrap_or(attrs.len());
        if !pandoc_html_name_valid(&attrs[..name_end]) {
            return false;
        }
        attrs = attrs[name_end..].trim_start_matches(|c: char| c.is_ascii_whitespace());
        if let Some(value) = attrs.strip_prefix('=') {
            attrs = value.trim_start_matches(|c: char| c.is_ascii_whitespace());
            if let Some(quote @ (b'\'' | b'"')) = attrs.bytes().next() {
                let Some(end) = attrs[1..].find(char::from(quote)) else {
                    return false;
                };
                attrs = &attrs[end + 2..];
            } else {
                let end = attrs
                    .find(|c: char| c.is_ascii_whitespace())
                    .unwrap_or(attrs.len());
                attrs = &attrs[end..];
            }
        }
    }
    true
}

fn pandoc_html_name_valid(name: &str) -> bool {
    use unicode_general_category::{GeneralCategory, get_general_category};

    let is_letter = |c| {
        matches!(
            get_general_category(c),
            GeneralCategory::UppercaseLetter
                | GeneralCategory::LowercaseLetter
                | GeneralCategory::TitlecaseLetter
                | GeneralCategory::ModifierLetter
                | GeneralCategory::OtherLetter
        )
    };
    let mut chars = name.chars();
    chars.next().is_some_and(is_letter)
        && chars.all(|c| is_letter(c) || c.is_numeric() || matches!(c, ':' | '-' | '_'))
}

/// One recognized HTML attribute, as byte ranges relative to the attribute
/// body passed to [`html_attribute_spans`] (the bytes between a tag name and
/// the closing `>`, exclusive). Range semantics match the `ATTR_*` token each
/// becomes: `Id`/`Class` wrap the bare value (quotes excluded — the reader uses
/// the text verbatim, since HTML has no `#`/`.` marker), while `KeyValue` keeps
/// the value's quotes (the reader strips them), mirroring the Pandoc
/// convention. The single source of truth shared by [`parse_html_attribute_list`]
/// (string derivation) and [`emit_html_attrs_node`] (CST emission).
#[derive(Debug, Clone, PartialEq)]
enum HtmlAttrComponent {
    /// `id="x"` → range covers the bare id value (`x`); only the first counts.
    Id(std::ops::Range<usize>),
    /// One whitespace-separated word of a `class="a b"` value.
    Class(std::ops::Range<usize>),
    /// `key="v"` / `key=v` → key range, `=` byte index, value range (value
    /// includes surrounding quotes when present).
    KeyValue {
        key: std::ops::Range<usize>,
        eq: usize,
        value: std::ops::Range<usize>,
    },
    /// A valueless attribute (`hidden`) → key range only (projects to `(key,"")`).
    Flag(std::ops::Range<usize>),
}

fn html_value_inner_range(content: &str, start: usize, end: usize) -> std::ops::Range<usize> {
    let b = content.as_bytes();
    if end > start && (b[start] == b'"' || b[start] == b'\'') {
        let q = b[start];
        if end > start + 1 && b[end - 1] == q {
            return (start + 1)..(end - 1);
        }
        return (start + 1)..end;
    }
    start..end
}

fn html_word_ranges(content: &str, start: usize, end: usize) -> Vec<std::ops::Range<usize>> {
    let b = content.as_bytes();
    let mut out = Vec::new();
    let mut i = start;
    while i < end {
        while i < end && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= end {
            break;
        }
        let ws = i;
        while i < end && !b[i].is_ascii_whitespace() {
            i += 1;
        }
        out.push(ws..i);
    }
    out
}

fn html_attribute_spans(content: &str) -> Vec<HtmlAttrComponent> {
    let bytes = content.as_bytes();
    let mut i = 0usize;
    let mut comps: Vec<HtmlAttrComponent> = Vec::new();
    let mut have_id = false;

    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' | b'/' => {
                i += 1;
            }
            _ => {
                let key_start = i;
                while i < bytes.len()
                    && !matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | b'=' | b'/')
                {
                    i += 1;
                }
                let key_end = i;
                let key = &content[key_start..key_end];

                if i < bytes.len() && bytes[i] == b'=' {
                    let eq = i;
                    i += 1; // skip '='
                    let value_start = i;
                    if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                        let quote = bytes[i];
                        i += 1; // opening quote
                        while i < bytes.len() && bytes[i] != quote {
                            i += 1;
                        }
                        if i < bytes.len() {
                            i += 1; // closing quote
                        }
                    } else {
                        while i < bytes.len()
                            && !matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | b'/')
                        {
                            i += 1;
                        }
                    }
                    let value_end = i;
                    match key {
                        "id" => {
                            if !have_id {
                                let inner = html_value_inner_range(content, value_start, value_end);
                                if inner.end > inner.start {
                                    comps.push(HtmlAttrComponent::Id(inner));
                                    have_id = true;
                                }
                            }
                        }
                        "class" => {
                            let inner = html_value_inner_range(content, value_start, value_end);
                            for w in html_word_ranges(content, inner.start, inner.end) {
                                comps.push(HtmlAttrComponent::Class(w));
                            }
                        }
                        _ => comps.push(HtmlAttrComponent::KeyValue {
                            key: key_start..key_end,
                            eq,
                            value: value_start..value_end,
                        }),
                    }
                } else if key_end > key_start {
                    comps.push(HtmlAttrComponent::Flag(key_start..key_end));
                }
            }
        }
    }

    comps
}

/// Emit a structural `HTML_ATTRS` node, wrapping the source bytes of each
/// recognized HTML attribute in `ATTR_ID` / `ATTR_CLASS` / `ATTR_KEY_VALUE`
/// children (bare values — HTML has no `#`/`.` marker). Bytes between/around
/// components (names, `=`, quotes, whitespace, `/`) become gap tokens, so
/// `node.text()` is exactly `attrs_text`. An unrecognized/empty body falls back
/// to a single opaque `TEXT` token.
pub fn emit_html_attrs_node(builder: &mut impl InlineSink, attrs_text: &str) {
    emit_html_attrs_with_kind(builder, SyntaxKind::HTML_ATTRS, attrs_text);
}

/// As [`emit_html_attrs_node`] but for the legacy native-span `SPAN_ATTRIBUTES`
/// node, which carries HTML `class="..."` syntax (not Pandoc `{...}`).
pub fn emit_html_span_attributes_node(builder: &mut impl InlineSink, attrs_text: &str) {
    emit_html_attrs_with_kind(builder, SyntaxKind::SPAN_ATTRIBUTES, attrs_text);
}

fn emit_html_attrs_with_kind(
    builder: &mut impl InlineSink,
    node_kind: SyntaxKind,
    attrs_text: &str,
) {
    builder.start_node(node_kind.into());
    let comps = html_attribute_spans(attrs_text);
    if comps.is_empty() {
        builder.token(SyntaxKind::TEXT.into(), attrs_text);
    } else {
        let mut cursor = 0usize;
        for comp in &comps {
            let (start, end) = match comp {
                HtmlAttrComponent::Id(r)
                | HtmlAttrComponent::Class(r)
                | HtmlAttrComponent::Flag(r) => (r.start, r.end),
                HtmlAttrComponent::KeyValue { key, value, .. } => (key.start, value.end),
            };
            emit_attribute_gap(builder, &attrs_text[cursor..start]);
            match comp {
                HtmlAttrComponent::Id(r) => {
                    builder.token(SyntaxKind::ATTR_ID.into(), &attrs_text[r.clone()]);
                }
                HtmlAttrComponent::Class(r) => {
                    builder.token(SyntaxKind::ATTR_CLASS.into(), &attrs_text[r.clone()]);
                }
                HtmlAttrComponent::Flag(r) => {
                    builder.start_node(SyntaxKind::ATTR_KEY_VALUE.into());
                    builder.token(SyntaxKind::ATTR_KEY.into(), &attrs_text[r.clone()]);
                    builder.finish_node();
                }
                HtmlAttrComponent::KeyValue { key, eq, value } => {
                    builder.start_node(SyntaxKind::ATTR_KEY_VALUE.into());
                    builder.token(SyntaxKind::ATTR_KEY.into(), &attrs_text[key.clone()]);
                    builder.token(SyntaxKind::TEXT.into(), &attrs_text[*eq..value.start]);
                    if value.end > value.start {
                        builder.token(SyntaxKind::ATTR_VALUE.into(), &attrs_text[value.clone()]);
                    }
                    builder.finish_node();
                }
            }
            cursor = end;
        }
        emit_attribute_gap(builder, &attrs_text[cursor..]);
    }
    builder.finish_node();
}

/// Emit a Pandoc `{...}` ATTRIBUTE node by STRUCTURING the raw source slice
/// into ATTR_* children that wrap the original bytes (no synthesis). Markers
/// and quotes stay inside their tokens; whitespace/newlines between components,
/// and any bytes the scanner skips (duplicate `#id`, malformed tokens), become
/// standalone WHITESPACE/NEWLINE/TEXT tokens — so `node.text()` is exactly the
/// source slice. Non-`{...}`-shaped or unrecognized input (MMD `[#id]` header
/// brackets, raw-inline `{=format}`, empty `{}`) falls back to a single opaque
/// ATTRIBUTE token, preserving the prior shape.
pub fn emit_attribute_node(builder: &mut impl InlineSink, raw_attr_text: &str) {
    emit_attribute_node_with_kinds(
        builder,
        SyntaxKind::ATTRIBUTE,
        SyntaxKind::ATTRIBUTE,
        raw_attr_text,
    );
}

/// Emit a fenced-div `DIV_INFO` node, structuring the Pandoc `{...}` body the
/// same way [`emit_attribute_node`] does. Bare-word shorthand (`::: Warning`)
/// and malformed/empty bodies fall back to a single opaque `TEXT` token,
/// preserving the prior `DIV_INFO { TEXT(...) }` shape (and the bare-word
/// class semantics the projector reads via `parse_div_info`).
pub fn emit_div_info_node(builder: &mut impl InlineSink, raw_attr_text: &str) {
    emit_attribute_node_with_kinds(
        builder,
        SyntaxKind::DIV_INFO,
        SyntaxKind::TEXT,
        raw_attr_text,
    );
}

/// Emit a bracketed-span `SPAN_ATTRIBUTES` node, structuring the Pandoc `{...}`
/// body the same way [`emit_attribute_node`] does. Malformed/empty bodies fall
/// back to a single opaque `TEXT` token, preserving the prior
/// `SPAN_ATTRIBUTES { TEXT(...) }` shape.
pub fn emit_span_attributes_node(builder: &mut impl InlineSink, raw_attr_text: &str) {
    emit_attribute_node_with_kinds(
        builder,
        SyntaxKind::SPAN_ATTRIBUTES,
        SyntaxKind::TEXT,
        raw_attr_text,
    );
}

/// Structure a code-block info-string region containing a `{...}` attribute
/// block into `ATTR_*` children wrapping the source bytes, the same way
/// [`emit_attribute_node`] does — but with a language carve-out: when
/// `carve_first_class_as_language` is set, the first `.class` component is
/// emitted as `TEXT "."` + `CODE_LANGUAGE <lang>` (Pandoc's `{.python …}`
/// language-first shape) instead of an `ATTR_CLASS`.
///
/// `text` is the full region that may surround the braces with gap bytes (e.g.
/// `" {.numberLines}"` after a shortcut language). The `{`/`}` and any
/// surrounding/interior gap bytes are emitted as gap tokens so `node.text()`
/// reconstructs `text` exactly. Returns `true` when it structured the block, and
/// `false` — having emitted **nothing** — when `text` has no `{...}` or its body
/// is unrecognized (empty/whitespace-only), so callers fall back to a single
/// opaque token and preserve the prior shape.
///
/// A leading `=format` raw marker is never carved as a language; the raw case is
/// handled before this is reached, but the carve defensively skips `=`-prefixed
/// components.
pub fn emit_code_info_attrs(
    builder: &mut impl InlineSink,
    text: &str,
    carve_first_class_as_language: bool,
) -> bool {
    let Some(open) = text.find('{') else {
        return false;
    };
    let Some(close) = text.rfind('}') else {
        return false;
    };
    if close < open {
        return false;
    }
    let body = &text[open + 1..close];
    let Some(spans) = attribute_content_spans(body) else {
        return false;
    };

    emit_attribute_gap(builder, &text[..open]);
    builder.token(SyntaxKind::TEXT.into(), "{");

    let mut carved = false;
    let mut cursor = 0usize;
    for comp in &spans.components {
        let (start, end) = match comp {
            AttrComponent::Id(r) | AttrComponent::Class(r) | AttrComponent::Unnumbered(r) => {
                (r.start, r.end)
            }
            AttrComponent::KeyValue { key, value, .. } => (key.start, value.end),
        };
        emit_attribute_gap(builder, &body[cursor..start]);
        match comp {
            AttrComponent::Id(r) => {
                builder.token(SyntaxKind::ATTR_ID.into(), &body[r.clone()]);
            }
            AttrComponent::Unnumbered(r) => {
                builder.token(SyntaxKind::ATTR_UNNUMBERED.into(), &body[r.clone()]);
            }
            AttrComponent::Class(r) => {
                let is_dot_class = body.as_bytes().get(r.start) == Some(&b'.');
                if carve_first_class_as_language && !carved && is_dot_class {
                    builder.token(SyntaxKind::TEXT.into(), &body[r.start..r.start + 1]);
                    builder.token(SyntaxKind::CODE_LANGUAGE.into(), &body[r.start + 1..r.end]);
                    carved = true;
                } else {
                    builder.token(SyntaxKind::ATTR_CLASS.into(), &body[r.clone()]);
                }
            }
            AttrComponent::KeyValue { key, eq, value } => {
                builder.start_node(SyntaxKind::ATTR_KEY_VALUE.into());
                builder.token(SyntaxKind::ATTR_KEY.into(), &body[key.clone()]);
                builder.token(SyntaxKind::TEXT.into(), &body[*eq..*eq + 1]);
                if value.end > value.start {
                    builder.token(SyntaxKind::ATTR_VALUE.into(), &body[value.clone()]);
                }
                builder.finish_node();
            }
        }
        cursor = end;
    }
    emit_attribute_gap(builder, &body[cursor..]);
    builder.token(SyntaxKind::TEXT.into(), "}");
    emit_attribute_gap(builder, &text[close + 1..]);
    true
}

fn emit_attribute_node_with_kinds(
    builder: &mut impl InlineSink,
    node_kind: SyntaxKind,
    opaque_token_kind: SyntaxKind,
    raw_attr_text: &str,
) {
    builder.start_node(node_kind.into());

    let body = raw_attr_text
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'));
    let spans = body.and_then(attribute_content_spans);

    match (body, spans) {
        (Some(body), Some(spans)) => {
            builder.token(SyntaxKind::TEXT.into(), "{");
            let mut cursor = 0usize;
            for comp in &spans.components {
                let (start, end) = match comp {
                    AttrComponent::Id(r)
                    | AttrComponent::Class(r)
                    | AttrComponent::Unnumbered(r) => (r.start, r.end),
                    AttrComponent::KeyValue { key, value, .. } => (key.start, value.end),
                };
                emit_attribute_gap(builder, &body[cursor..start]);
                match comp {
                    AttrComponent::Id(r) => {
                        builder.token(SyntaxKind::ATTR_ID.into(), &body[r.clone()]);
                    }
                    AttrComponent::Unnumbered(r) => {
                        builder.token(SyntaxKind::ATTR_UNNUMBERED.into(), &body[r.clone()]);
                    }
                    AttrComponent::Class(r) => {
                        builder.token(SyntaxKind::ATTR_CLASS.into(), &body[r.clone()]);
                    }
                    AttrComponent::KeyValue { key, eq, value } => {
                        builder.start_node(SyntaxKind::ATTR_KEY_VALUE.into());
                        builder.token(SyntaxKind::ATTR_KEY.into(), &body[key.clone()]);
                        builder.token(SyntaxKind::TEXT.into(), &body[*eq..*eq + 1]);
                        if value.end > value.start {
                            builder.token(SyntaxKind::ATTR_VALUE.into(), &body[value.clone()]);
                        }
                        builder.finish_node();
                    }
                }
                cursor = end;
            }
            emit_attribute_gap(builder, &body[cursor..]);
            builder.token(SyntaxKind::TEXT.into(), "}");
        }
        _ => {
            builder.token(opaque_token_kind.into(), raw_attr_text);
        }
    }

    builder.finish_node();
}

fn emit_attribute_gap(builder: &mut impl InlineSink, gap: &str) {
    let bytes = gap.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                builder.token(SyntaxKind::NEWLINE.into(), "\n");
                i += 1;
            }
            b'\r' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    builder.token(SyntaxKind::NEWLINE.into(), "\r\n");
                    i += 2;
                } else {
                    builder.token(SyntaxKind::NEWLINE.into(), "\r");
                    i += 1;
                }
            }
            b if b.is_ascii_whitespace() => {
                let start = i;
                while i < bytes.len()
                    && bytes[i].is_ascii_whitespace()
                    && bytes[i] != b'\n'
                    && bytes[i] != b'\r'
                {
                    i += 1;
                }
                builder.token(SyntaxKind::WHITESPACE.into(), &gap[start..i]);
            }
            _ => {
                let start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                builder.token(SyntaxKind::TEXT.into(), &gap[start..i]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_id() {
        let result = try_parse_trailing_attributes("Heading {#my-id}");
        assert!(result.is_some());
        let (attrs, before) = result.unwrap();
        assert_eq!(before, "Heading");
        assert_eq!(attrs.identifier, Some("my-id".to_string()));
        assert!(attrs.classes.is_empty());
        assert!(attrs.key_values.is_empty());
    }

    #[test]
    fn test_single_class() {
        let result = try_parse_trailing_attributes("Text {.myclass}");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(attrs.classes, vec!["myclass"]);
    }

    #[test]
    fn test_multiple_classes() {
        let result = try_parse_trailing_attributes("Text {.class1 .class2 .class3}");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(attrs.classes, vec!["class1", "class2", "class3"]);
    }

    #[test]
    fn test_key_value_unquoted() {
        let result = try_parse_trailing_attributes("Text {key=value}");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(
            attrs.key_values,
            vec![("key".to_string(), "value".to_string())]
        );
    }

    #[test]
    fn test_key_value_quoted() {
        let result = try_parse_trailing_attributes("Text {key=\"value with spaces\"}");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(
            attrs.key_values,
            vec![("key".to_string(), "value with spaces".to_string())]
        );
    }

    #[test]
    fn test_full_attributes() {
        let result =
            try_parse_trailing_attributes("Heading {#id .class1 .class2 key1=val1 key2=\"val 2\"}");
        assert!(result.is_some());
        let (attrs, before) = result.unwrap();
        assert_eq!(before, "Heading");
        assert_eq!(attrs.identifier, Some("id".to_string()));
        assert_eq!(attrs.classes, vec!["class1", "class2"]);
        assert_eq!(attrs.key_values.len(), 2);
        assert_eq!(
            attrs.key_values[0],
            ("key1".to_string(), "val1".to_string())
        );
        assert_eq!(
            attrs.key_values[1],
            ("key2".to_string(), "val 2".to_string())
        );
    }

    #[test]
    fn test_trailing_attributes_with_shortcode_in_quoted_value() {
        let text = "Slide Title {background-image='{{< placeholder 100 100 >}}' background-size=\"100px\"}";
        let result = try_parse_trailing_attributes(text);
        assert!(result.is_some());
        let (attrs, before) = result.unwrap();
        assert_eq!(before, "Slide Title");
        assert_eq!(attrs.key_values.len(), 2);
        assert_eq!(
            attrs.key_values[0],
            (
                "background-image".to_string(),
                "{{< placeholder 100 100 >}}".to_string()
            )
        );
        assert_eq!(
            attrs.key_values[1],
            ("background-size".to_string(), "100px".to_string())
        );
    }

    /// A bare `-` is pandoc's shorthand for `.unnumbered` (manual: "A single
    /// hyphen (-) in an attribute context is equivalent to .unnumbered, and
    /// preferable in non-English documents"). Every expectation below is
    /// pinned against `pandoc -f markdown -t native`.
    #[test]
    fn test_bare_hyphen_is_unnumbered_shorthand() {
        let (attrs, before) = try_parse_trailing_attributes("Heading {-}").unwrap();
        assert_eq!(before, "Heading");
        assert_eq!(attrs.identifier, None);
        assert_eq!(attrs.classes, vec!["unnumbered"]);

        let (attrs, _) = try_parse_trailing_attributes("Heading {#hbdl -}").unwrap();
        assert_eq!(attrs.identifier, Some("hbdl".to_string()));
        assert_eq!(attrs.classes, vec!["unnumbered"]);

        let (attrs, _) = try_parse_trailing_attributes("Text {.a - .b}").unwrap();
        assert_eq!(attrs.classes, vec!["a", "unnumbered", "b"]);
    }

    #[test]
    fn test_bare_hyphen_is_single_byte() {
        let (attrs, _) = try_parse_trailing_attributes("Text {---}").unwrap();
        assert_eq!(
            attrs.classes,
            vec!["unnumbered", "unnumbered", "unnumbered"]
        );

        let (attrs, _) = try_parse_trailing_attributes("Text {-.a}").unwrap();
        assert_eq!(attrs.classes, vec!["unnumbered", "a"]);

        let (attrs, _) = try_parse_trailing_attributes("Text {-#id}").unwrap();
        assert_eq!(attrs.identifier, Some("id".to_string()));
        assert_eq!(attrs.classes, vec!["unnumbered"]);

        let (attrs, _) = try_parse_trailing_attributes("Text {-foo=bar}").unwrap();
        assert_eq!(attrs.classes, vec!["unnumbered"]);
        assert_eq!(
            attrs.key_values,
            vec![("foo".to_string(), "bar".to_string())]
        );
    }

    /// Pandoc accepts the shorthand only when a valid attribute item follows
    /// the run of hyphens, and otherwise rejects the whole block. Without this
    /// boundary a heading documenting a CLI flag (`## Using {--verbose}`) would
    /// be swallowed as attributes and rewritten away by the formatter.
    #[test]
    fn test_bare_hyphen_needs_a_following_item() {
        assert!(try_parse_trailing_attributes("Using {--verbose}").is_none());
        assert!(try_parse_trailing_attributes("Text {-x}").is_none());
        assert!(try_parse_trailing_attributes("Text {-=html}").is_none());
    }

    #[test]
    fn test_hyphen_inside_id_or_class_is_not_shorthand() {
        let (attrs, _) = try_parse_trailing_attributes("Text {#id- .x}").unwrap();
        assert_eq!(attrs.identifier, Some("id-".to_string()));
        assert_eq!(attrs.classes, vec!["x"]);

        let (attrs, _) = try_parse_trailing_attributes("Text {.a-}").unwrap();
        assert_eq!(attrs.classes, vec!["a-"]);
    }

    #[test]
    fn test_no_attributes() {
        let result = try_parse_trailing_attributes("Heading with no attributes");
        assert!(result.is_none());
    }

    #[test]
    fn test_empty_braces() {
        let result = try_parse_trailing_attributes("Heading {}");
        assert!(result.is_none());
    }

    #[test]
    fn test_only_first_id_counts() {
        let result = try_parse_trailing_attributes("Text {#id1 #id2}");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(attrs.identifier, Some("id1".to_string()));
    }

    #[test]
    fn test_whitespace_handling() {
        let result = try_parse_trailing_attributes("Text {  #id   .class   key=val  }");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(attrs.identifier, Some("id".to_string()));
        assert_eq!(attrs.classes, vec!["class"]);
        assert_eq!(
            attrs.key_values,
            vec![("key".to_string(), "val".to_string())]
        );
    }

    #[test]
    fn test_parse_html_tag_attributes_id_only() {
        let attrs = parse_html_tag_attributes(r#"<div id="anchor-c">"#).unwrap();
        assert_eq!(attrs.identifier.as_deref(), Some("anchor-c"));
        assert!(attrs.classes.is_empty());
        assert!(attrs.key_values.is_empty());
    }

    #[test]
    fn test_parse_html_tag_attributes_inline_content_after_open() {
        let attrs = parse_html_tag_attributes(r#"<div id="anchor-c">Content.</div>"#).unwrap();
        assert_eq!(attrs.identifier.as_deref(), Some("anchor-c"));
    }

    #[test]
    fn test_parse_html_tag_attributes_class_and_kv() {
        let attrs = parse_html_tag_attributes(r#"<div id="x" class="a b" data-key="v">"#).unwrap();
        assert_eq!(attrs.identifier.as_deref(), Some("x"));
        assert_eq!(attrs.classes, vec!["a", "b"]);
        assert_eq!(
            attrs.key_values,
            vec![("data-key".to_string(), "v".to_string())]
        );
    }

    #[test]
    fn test_parse_html_tag_attributes_no_attrs() {
        assert!(parse_html_tag_attributes("<div>").is_none());
    }

    #[test]
    fn test_trailing_whitespace_before_attrs() {
        let result = try_parse_trailing_attributes("Heading   {#id}");
        assert!(result.is_some());
        let (_, before) = result.unwrap();
        assert_eq!(before, "Heading");
    }

    /// An escaped space is content, not gap. `pandoc -f markdown` reads
    /// `# foo\ {#id}` as `Header 1 (id) [Str "foo\160"]`, so the backslash
    /// keeps the space it escapes and only the rest of the run is trimmed.
    #[test]
    fn escaped_space_before_attrs_is_not_part_of_the_gap() {
        let cases = [
            ("Heading\\ {#id}", "Heading\\ "),
            ("Heading\\   {#id}", "Heading\\ "),
            ("Heading\\\t{#id}", "Heading\\\t"),
            ("Heading\\\\ {#id}", "Heading\\\\"),
            ("Heading\\\\\\ {#id}", "Heading\\\\\\ "),
            ("Heading\\{#id}", "Heading\\"),
            ("Heading {#id}", "Heading"),
        ];
        for (input, expected) in cases {
            let (_, before) = try_parse_trailing_attributes(input).expect(input);
            assert_eq!(before, expected, "{input:?}");
        }
    }

    /// Regression: the inline-code attribute path used to reconstruct a
    /// normalized `{...}` string (reordering id-first, force-quoting values),
    /// which inflated the CST past the input and broke losslessness. The
    /// structured emitter must wrap the original bytes verbatim.
    #[test]
    fn inline_code_attribute_is_lossless() {
        let input = "`code`{.r #x key=v}\n";
        let tree = crate::parse(input, None);
        assert_eq!(tree.text().to_string(), input);
    }

    fn structured_attr(raw: &str) -> crate::syntax::SyntaxNode {
        let mut builder = GreenNodeBuilder::new();
        emit_attribute_node(&mut builder, raw);
        crate::syntax::SyntaxNode::new_root(builder.finish())
    }

    #[test]
    fn emit_attribute_node_is_lossless_over_shapes() {
        for raw in [
            "{#id}",
            "{.a .b}",
            "{key=\"v w\"}",
            "{ #id  .c }",
            "{#id1 #id2}",
            "{key}",
            "{=html}",
            "{#id .a key=v key2='x'}",
            "{key=}",
            "{}",
            "{   }",
            "{-}",
            "{#id -}",
            "{---}",
            "{-.a}",
        ] {
            let node = structured_attr(raw);
            assert_eq!(node.text().to_string(), raw, "lossless emit for {raw:?}");
            assert_eq!(node.kind(), SyntaxKind::ATTRIBUTE);
        }
    }

    #[test]
    fn emit_attribute_node_structures_children() {
        let node = structured_attr("{#x .a .b k=v}");
        let kinds: Vec<_> = node.children_with_tokens().map(|c| c.kind()).collect();
        assert_eq!(
            kinds.iter().filter(|k| **k == SyntaxKind::ATTR_ID).count(),
            1
        );
        assert_eq!(
            kinds
                .iter()
                .filter(|k| **k == SyntaxKind::ATTR_CLASS)
                .count(),
            2
        );
        assert_eq!(
            kinds
                .iter()
                .filter(|k| **k == SyntaxKind::ATTR_KEY_VALUE)
                .count(),
            1
        );
    }

    #[test]
    fn emit_attribute_node_tags_bare_hyphen() {
        let node = structured_attr("{#x - .a}");
        let kinds: Vec<_> = node.children_with_tokens().map(|c| c.kind()).collect();
        assert_eq!(
            kinds
                .iter()
                .filter(|k| **k == SyntaxKind::ATTR_UNNUMBERED)
                .count(),
            1
        );
        assert_eq!(
            kinds
                .iter()
                .filter(|k| **k == SyntaxKind::ATTR_CLASS)
                .count(),
            1,
            "the `-` must not be tagged as an ordinary class"
        );
    }

    fn structured_html_attrs(raw: &str) -> crate::syntax::SyntaxNode {
        let mut builder = GreenNodeBuilder::new();
        emit_html_attrs_node(&mut builder, raw);
        crate::syntax::SyntaxNode::new_root(builder.finish())
    }

    #[test]
    fn emit_html_attrs_node_is_lossless_over_shapes() {
        for raw in [
            r#"id="x""#,
            r#"id="x" class="a b" data-key="v""#,
            r#"class='a  b'"#,
            r#"id=bare class=one"#,
            "hidden",
            r#"id="x" hidden data-n="1""#,
            r#"  id="x"  /"#,
            r#"id="""#,
            "",
            "   ",
        ] {
            let node = structured_html_attrs(raw);
            assert_eq!(node.text().to_string(), raw, "lossless emit for {raw:?}");
            assert_eq!(node.kind(), SyntaxKind::HTML_ATTRS);
        }
    }

    #[test]
    fn emit_html_attrs_node_structures_children() {
        let node = structured_html_attrs(r#"id="x" class="a b" data-key="v" hidden"#);
        let kinds: Vec<_> = node.children_with_tokens().map(|c| c.kind()).collect();
        assert_eq!(
            kinds.iter().filter(|k| **k == SyntaxKind::ATTR_ID).count(),
            1
        );
        assert_eq!(
            kinds
                .iter()
                .filter(|k| **k == SyntaxKind::ATTR_CLASS)
                .count(),
            2,
            "class=\"a b\" splits into two ATTR_CLASS tokens"
        );
        assert_eq!(
            node.children()
                .filter(|n| n.kind() == SyntaxKind::ATTR_KEY_VALUE)
                .count(),
            2
        );
    }

    /// The structured walker and the string-deriving parser must agree.
    #[test]
    fn html_attribute_list_parse_parity() {
        let attrs =
            parse_html_attribute_list(r#"id="x" class="a b" data-key='v w' hidden"#).unwrap();
        assert_eq!(attrs.identifier.as_deref(), Some("x"));
        assert_eq!(attrs.classes, vec!["a", "b"]);
        assert_eq!(
            attrs.key_values,
            vec![
                ("data-key".to_string(), "v w".to_string()),
                ("hidden".to_string(), String::new()),
            ]
        );
        assert!(parse_html_attribute_list("   ").is_none());
        assert!(parse_html_attribute_list(r#"id="""#).is_none());
    }

    /// Terminated named and numeric references decode; everything else is
    /// left verbatim. Pinned against `pandoc -f markdown -t native`.
    #[test]
    fn decode_html_attr_entities_named_and_numeric() {
        assert_eq!(decode_html_attr_entities("a&amp;b"), "a&b");
        assert_eq!(decode_html_attr_entities("a&AMP;b"), "a&b");
        assert_eq!(decode_html_attr_entities("a&lt;b"), "a<b");
        assert_eq!(decode_html_attr_entities("a&gt;b"), "a>b");
        assert_eq!(decode_html_attr_entities("a&quot;b"), "a\"b");
        assert_eq!(decode_html_attr_entities("a&copy;b"), "a\u{a9}b");
        assert_eq!(decode_html_attr_entities("a&nbsp;b"), "a\u{a0}b");
        // Numeric, decimal and hex (either case of the `x` marker).
        assert_eq!(decode_html_attr_entities("a&#65;b"), "aAb");
        assert_eq!(decode_html_attr_entities("a&#x41;b"), "aAb");
        assert_eq!(decode_html_attr_entities("a&#X41;b"), "aAb");
        assert_eq!(decode_html_attr_entities("a&#9;b"), "a\tb");
        // Several references in one value.
        assert_eq!(decode_html_attr_entities("&amp;&lt;&#66;"), "&<B");
    }

    /// A single decoding pass — the output is not rescanned, so an encoded
    /// entity stays encoded (pandoc behaves the same).
    #[test]
    fn decode_html_attr_entities_is_single_pass() {
        assert_eq!(decode_html_attr_entities("a&amp;amp;b"), "a&amp;b");
    }

    #[test]
    fn decode_html_attr_entities_leaves_non_references() {
        for raw in [
            "plain",
            "a&b",         // bare ampersand
            "a&notreal;b", // unknown name
            "a&;b",        // empty name
            "a&#;b",       // no digits
            "a&#x;b",      // no hex digits
            "a&#zz;b",     // non-digits after `#`
            "a&amp b",     // semicolon-less: known gap, left verbatim
            "a&ampb",      // name runs into following text
        ] {
            assert!(
                matches!(
                    decode_html_attr_entities(raw),
                    std::borrow::Cow::Borrowed(_)
                ),
                "{raw:?} should not allocate"
            );
            assert_eq!(decode_html_attr_entities(raw), raw);
        }
    }

    /// Numeric edge cases follow pandoc: surrogates become U+FFFD, values past
    /// U+10FFFF (or too large to parse) become `?`, and `&#0;` stays NUL.
    #[test]
    fn decode_html_attr_entities_numeric_edges() {
        assert_eq!(decode_html_attr_entities("a&#xD800;b"), "a\u{fffd}b");
        assert_eq!(decode_html_attr_entities("a&#x10FFFF;b"), "a\u{10ffff}b");
        assert_eq!(decode_html_attr_entities("a&#x110000;b"), "a?b");
        assert_eq!(decode_html_attr_entities("a&#99999999999999;b"), "a?b");
        assert_eq!(decode_html_attr_entities("a&#0;b"), "a\0b");
    }

    #[test]
    fn pandoc_html_attribute_name_character_rules() {
        for name in [
            "id", "data-id", "xml:lang", "a_b", "a1", "é", "名字", "x²", "xⅧ",
        ] {
            assert!(pandoc_html_attribute_names_valid(name), "{name:?}");
        }
        for name in [
            "a&amp;b", "a&#38;b", "a&b", "1a", "_a", ":a", "a.b", "Ⅷ", "a\u{345}",
        ] {
            assert!(!pandoc_html_attribute_names_valid(name), "{name:?}");
        }
    }

    #[test]
    fn pandoc_html_attribute_names_skip_quoted_and_unquoted_values() {
        for attrs in [
            "id=\"a&amp;b\" data-key='> & x' hidden",
            "id=a&amp;b data-key = \"a&amp;b\" /",
            "id=\"line one\n& line two\"\nclass='x'",
            "id='x\\' data-key='a&amp;b'",
        ] {
            assert!(pandoc_html_attribute_names_valid(attrs), "{attrs:?}");
        }
        assert!(!pandoc_html_attribute_names_valid(
            "id=\"a&amp;b\"\ninvalid&amp;name='x'"
        ));
    }

    #[test]
    fn html_attribute_list_decodes_entities() {
        let attrs =
            parse_html_attribute_list(r#"id="a&amp;b" class="c&lt;d e&#70;f" k="g&gt;h""#).unwrap();
        assert_eq!(attrs.identifier.as_deref(), Some("a&b"));
        assert_eq!(attrs.classes, vec!["c<d", "eFf"]);
        assert_eq!(attrs.key_values, vec![("k".to_string(), "g>h".to_string())]);
        // Unquoted values decode too.
        let unquoted = parse_html_attribute_list("id=a&amp;b").unwrap();
        assert_eq!(unquoted.identifier.as_deref(), Some("a&b"));
    }

    fn structured_div_info(raw: &str) -> crate::syntax::SyntaxNode {
        let mut builder = GreenNodeBuilder::new();
        emit_div_info_node(&mut builder, raw);
        crate::syntax::SyntaxNode::new_root(builder.finish())
    }

    #[test]
    fn emit_div_info_node_is_lossless_and_structures_brace_body() {
        // `{...}` bodies structure into ATTR_* children; bare-word shorthand
        // and malformed/empty bodies stay one opaque TEXT token. All round-trip.
        for raw in ["{#id .a .b key=val key2=\"v w\"}", "Warning", "{}", "{   }"] {
            let node = structured_div_info(raw);
            assert_eq!(node.text().to_string(), raw, "lossless emit for {raw:?}");
            assert_eq!(node.kind(), SyntaxKind::DIV_INFO);
        }

        let structured = structured_div_info("{#id .a .b key=val key2=\"v w\"}");
        let kinds: Vec<_> = structured
            .children_with_tokens()
            .map(|c| c.kind())
            .collect();
        assert_eq!(
            kinds.iter().filter(|k| **k == SyntaxKind::ATTR_ID).count(),
            1
        );
        assert_eq!(
            kinds
                .iter()
                .filter(|k| **k == SyntaxKind::ATTR_CLASS)
                .count(),
            2
        );
        assert_eq!(
            kinds
                .iter()
                .filter(|k| **k == SyntaxKind::ATTR_KEY_VALUE)
                .count(),
            2
        );

        // Bare-word fallback: a single opaque TEXT token, no ATTR_* children.
        let bare = structured_div_info("Warning");
        let bare_kinds: Vec<_> = bare.children_with_tokens().map(|c| c.kind()).collect();
        assert_eq!(bare_kinds, vec![SyntaxKind::TEXT]);
    }
}
