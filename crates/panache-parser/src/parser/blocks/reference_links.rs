//! Reference definition and footnote parsing functions.
//!
//! Reference definitions have the form:
//! ```markdown
//! [label]: url "optional title"
//! [label]: url 'optional title'
//! [label]: url (optional title)
//! [label]: <url> "title"
//! ```
//!
//! Footnote definitions have the form:
//! ```markdown
//! [^id]: Footnote content here.
//!     Can continue on multiple lines
//!     as long as they're indented.
//! ```

/// Try to parse a reference definition starting at the current position.
/// Returns Some((bytes_consumed, label, url, title)) on success.
///
/// `text` may span multiple lines. The destination and title may each be
/// preceded by at most one newline (per CommonMark §4.7). Blank lines
/// terminate the definition: callers should stop the input at the first
/// blank line so the parser cannot cross one.
///
/// `dialect` controls a CommonMark-only constraint (§4.7): the title, if
/// present on the same line as the destination, must be separated from the
/// destination by at least one space or tab. Pandoc-markdown accepts the
/// title even when it's directly attached (e.g. `[foo]: <bar>(baz)`).
///
/// Syntax:
/// ```markdown
/// [label]: url "title"
/// [label]: <url> 'title'
/// [label]:
///   url
///   "title"
/// ```
pub fn try_parse_reference_definition(
    text: &str,
    dialect: crate::options::Dialect,
) -> Option<(usize, String, String, Option<String>)> {
    try_parse_reference_definition_with_mode(text, true, dialect, false)
}

/// Multimarkdown-flavored variant: tolerates trailing content after the title
/// on the same line (e.g. `[ref]: /url "title" width=20px ...`). Callers in
/// the MMD code path then keep collecting attribute-continuation lines.
pub fn try_parse_reference_definition_lax(
    text: &str,
    dialect: crate::options::Dialect,
) -> Option<(usize, String, String, Option<String>)> {
    try_parse_reference_definition_with_mode(text, false, dialect, false)
}

pub(crate) fn try_parse_reference_definition_with_options(
    text: &str,
    config: &crate::options::ParserOptions,
) -> Option<(usize, String, String, Option<String>)> {
    try_parse_reference_definition_with_mode(
        text,
        !config.extensions.mmd_link_attributes,
        config.dialect,
        config.extensions.hugo_shortcodes,
    )
}

fn try_parse_reference_definition_with_mode(
    text: &str,
    strict_eol: bool,
    dialect: crate::options::Dialect,
    hugo_shortcodes: bool,
) -> Option<(usize, String, String, Option<String>)> {
    let spans = reference_definition_spans(text, strict_eol, dialect, hugo_shortcodes)?;
    let label = text[spans.indent + 1..spans.label_close].to_string();
    let url = if spans.url_is_angle {
        text[spans.url.start + 1..spans.url.end - 1].to_string()
    } else {
        text[spans.url.clone()].to_string()
    };
    let title = spans
        .title
        .as_ref()
        .map(|r| text[r.start + 1..r.end - 1].to_string());
    Some((spans.consumed, label, url, title))
}

/// Byte spans of a recognized reference definition, all relative to the `text`
/// passed to [`reference_definition_spans`].
///
/// This is the single source of truth shared by *detection*
/// (`try_parse_reference_definition`, which extracts the component strings)
/// and *emission* (`emit_reference_definition_lines`, which wraps the same
/// byte ranges in `REFERENCE_URL` / `REFERENCE_TITLE` CST nodes). Keeping both
/// phases on one walker is what prevents the detect/emit drift the dispatcher's
/// doc comment warns about.
#[derive(Debug, Clone)]
pub(crate) struct ReferenceSpans {
    /// Leading-space count before `[` (0..=3); also the byte index of `[`.
    pub indent: usize,
    /// Byte index of the label-closing `]`.
    pub label_close: usize,
    /// Byte index of the `:` after the label.
    pub colon: usize,
    /// Destination byte range, *including* `<>` when angle-bracketed.
    pub url: std::ops::Range<usize>,
    /// Whether the destination is `<…>` angle-bracketed.
    pub url_is_angle: bool,
    /// Title byte range, *including* its quote/paren delimiters, when present.
    pub title: Option<std::ops::Range<usize>>,
    /// Total bytes consumed (matches the legacy `bytes_consumed`).
    pub consumed: usize,
}

/// Scan a reference definition and record the byte spans of its components.
///
/// The walk is identical to the legacy string-returning parser — it just
/// records offsets instead of allocating component strings, so detection and
/// emission stay byte-for-byte consistent. See [`ReferenceSpans`].
pub(crate) fn reference_definition_spans(
    text: &str,
    strict_eol: bool,
    dialect: crate::options::Dialect,
    hugo_shortcodes: bool,
) -> Option<ReferenceSpans> {
    let leading_spaces = text.chars().take_while(|&c| c == ' ').count();
    if leading_spaces > 3 {
        return None;
    }
    let inner = &text[leading_spaces..];
    let bytes = inner.as_bytes();

    if bytes.is_empty() || bytes[0] != b'[' {
        return None;
    }

    if bytes.len() >= 2 && bytes[1] == b'^' {
        return None;
    }

    let mut pos = 1;
    let mut escape_next = false;

    while pos < bytes.len() {
        if escape_next {
            escape_next = false;
            pos += 1;
            continue;
        }

        match bytes[pos] {
            b'\\' => {
                escape_next = true;
                pos += 1;
            }
            b']' => {
                break;
            }
            b'[' => {
                return None;
            }
            b'\n' | b'\r' => {
                let nl_end =
                    if bytes[pos] == b'\r' && pos + 1 < bytes.len() && bytes[pos + 1] == b'\n' {
                        pos + 2
                    } else {
                        pos + 1
                    };
                let mut probe = nl_end;
                while probe < bytes.len() && matches!(bytes[probe], b' ' | b'\t') {
                    probe += 1;
                }
                if probe >= bytes.len() || bytes[probe] == b'\n' || bytes[probe] == b'\r' {
                    return None;
                }
                pos = nl_end;
            }
            _ => {
                pos += 1;
            }
        }
    }

    if pos >= bytes.len() || bytes[pos] != b']' {
        return None;
    }

    let label = &inner[1..pos];
    if label.trim().is_empty() {
        return None;
    }
    let label_close = leading_spaces + pos;

    pos += 1; // Skip ]

    if pos >= bytes.len() || bytes[pos] != b':' {
        return None;
    }
    let colon = leading_spaces + pos;
    pos += 1;

    pos = skip_ws_one_newline(bytes, pos)?;

    let url_start = pos;
    let url_is_angle = pos < bytes.len() && bytes[pos] == b'<';

    if url_is_angle {
        pos += 1;
        while pos < bytes.len() && bytes[pos] != b'>' && bytes[pos] != b'\n' && bytes[pos] != b'\r'
        {
            if hugo_shortcodes
                && bytes[pos] == b'{'
                && let Some(tag) =
                    crate::parser::inlines::hugo::scan_tag(&inner[pos..]).filter(|tag| tag.complete)
            {
                pos += tag.end;
                continue;
            }
            pos += 1;
        }
        if pos >= bytes.len() || bytes[pos] != b'>' {
            return None;
        }
        pos += 1; // Skip >
    } else {
        while pos < bytes.len() && !matches!(bytes[pos], b' ' | b'\t' | b'\n' | b'\r') {
            if hugo_shortcodes
                && bytes[pos] == b'{'
                && let Some(tag) =
                    crate::parser::inlines::hugo::scan_tag(&inner[pos..]).filter(|tag| tag.complete)
            {
                pos += tag.end;
                continue;
            }
            pos += 1;
        }
        if pos == url_start {
            return None;
        }
    }
    let url = (leading_spaces + url_start)..(leading_spaces + pos);

    let after_url = pos;
    let url_line_end = consume_to_eol(bytes, after_url);
    let url_line_end_lax = if strict_eol {
        url_line_end
    } else {
        Some(consume_to_eol_lax(bytes, after_url))
    };

    let mut title: Option<std::ops::Range<usize>> = None;
    let mut end_pos: Option<usize> = None;

    if let Some(title_start) = skip_ws_one_newline(bytes, after_url) {
        let crossed_newline = bytes[after_url..title_start]
            .iter()
            .any(|&b| b == b'\n' || b == b'\r');
        let cmark_requires_separator = dialect == crate::options::Dialect::CommonMark
            && !crossed_newline
            && title_start == after_url;
        if cmark_requires_separator {
            return Some(ReferenceSpans {
                indent: leading_spaces,
                label_close,
                colon,
                url,
                url_is_angle,
                title: None,
                consumed: leading_spaces + url_line_end_lax?,
            });
        }
        let mut title_pos = title_start;
        match parse_title(bytes, &mut title_pos) {
            Some(Some(range)) => {
                let line_end = if strict_eol {
                    consume_to_eol(bytes, title_pos)
                } else {
                    Some(consume_to_eol_lax(bytes, title_pos))
                };
                if let Some(end) = line_end {
                    title = Some((leading_spaces + range.start)..(leading_spaces + range.end));
                    end_pos = Some(end);
                } else if !crossed_newline {
                    return None;
                }
            }
            None => {
                if !crossed_newline {
                    return None;
                }
            }
            Some(None) => {}
        }
    }

    let end = match end_pos {
        Some(p) => p,
        None => url_line_end_lax?,
    };

    Some(ReferenceSpans {
        indent: leading_spaces,
        label_close,
        colon,
        url,
        url_is_angle,
        title,
        consumed: leading_spaces + end,
    })
}

fn consume_to_eol_lax(bytes: &[u8], mut pos: usize) -> usize {
    while pos < bytes.len() && bytes[pos] != b'\n' && bytes[pos] != b'\r' {
        pos += 1;
    }
    if pos < bytes.len() {
        if bytes[pos] == b'\r' && pos + 1 < bytes.len() && bytes[pos + 1] == b'\n' {
            pos += 2;
        } else {
            pos += 1;
        }
    }
    pos
}

fn consume_to_eol(bytes: &[u8], mut pos: usize) -> Option<usize> {
    while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t') {
        pos += 1;
    }
    if pos >= bytes.len() {
        return Some(pos);
    }
    match bytes[pos] {
        b'\n' => Some(pos + 1),
        b'\r' => {
            if pos + 1 < bytes.len() && bytes[pos + 1] == b'\n' {
                Some(pos + 2)
            } else {
                Some(pos + 1)
            }
        }
        _ => None,
    }
}

/// Skip space/tab and optionally one line ending followed by more space/tab,
/// per the "optional spaces or tabs (including up to one [line ending])" rule
/// in CommonMark §4.7. Returns `None` if a *second* line ending is encountered
/// (i.e. a blank line), which terminates the definition.
fn skip_ws_one_newline(bytes: &[u8], mut pos: usize) -> Option<usize> {
    while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t') {
        pos += 1;
    }
    if pos < bytes.len() && (bytes[pos] == b'\n' || bytes[pos] == b'\r') {
        if bytes[pos] == b'\r' && pos + 1 < bytes.len() && bytes[pos + 1] == b'\n' {
            pos += 2;
        } else {
            pos += 1;
        }
        while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t') {
            pos += 1;
        }
        if pos < bytes.len() && (bytes[pos] == b'\n' || bytes[pos] == b'\r') {
            return None;
        }
    }
    Some(pos)
}

pub fn line_is_mmd_link_attribute_continuation(line: &str) -> bool {
    if !(line.starts_with(' ') || line.starts_with('\t')) {
        return false;
    }

    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }

    let bytes = trimmed.as_bytes();
    let mut pos = 0usize;
    let len = bytes.len();
    let mut saw_pair = false;

    while pos < len {
        while pos < len && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
            pos += 1;
        }
        if pos >= len {
            break;
        }

        let key_start = pos;
        while pos < len && bytes[pos] != b'=' && bytes[pos] != b' ' && bytes[pos] != b'\t' {
            pos += 1;
        }
        if pos == key_start || pos >= len || bytes[pos] != b'=' {
            return false;
        }
        pos += 1; // skip '='

        if pos >= len {
            return false;
        }
        if bytes[pos] == b'"' || bytes[pos] == b'\'' {
            let quote = bytes[pos];
            pos += 1;
            let value_start = pos;
            while pos < len && bytes[pos] != quote {
                pos += 1;
            }
            if pos == value_start || pos >= len {
                return false;
            }
            pos += 1; // skip closing quote
        } else {
            let value_start = pos;
            while pos < len && bytes[pos] != b' ' && bytes[pos] != b'\t' {
                pos += 1;
            }
            if pos == value_start {
                return false;
            }
        }

        saw_pair = true;
    }

    saw_pair
}

fn parse_title(bytes: &[u8], pos: &mut usize) -> Option<Option<std::ops::Range<usize>>> {
    let base_pos = *pos;

    while *pos < bytes.len() && matches!(bytes[*pos], b' ' | b'\t' | b'\n' | b'\r') {
        *pos += 1;
    }

    if *pos >= bytes.len() {
        return Some(None);
    }

    let quote_char = bytes[*pos];
    if !matches!(quote_char, b'"' | b'\'' | b'(') {
        *pos = base_pos; // Reset position
        return Some(None);
    }

    let closing_char = if quote_char == b'(' { b')' } else { quote_char };

    let open = *pos;
    *pos += 1; // Skip opening quote

    let mut escape_next = false;
    while *pos < bytes.len() {
        if escape_next {
            escape_next = false;
            *pos += 1;
            continue;
        }

        match bytes[*pos] {
            b'\\' => {
                escape_next = true;
                *pos += 1;
            }
            c if c == closing_char => {
                *pos += 1; // Skip closing quote
                let close_end = *pos;

                while *pos < bytes.len() && matches!(bytes[*pos], b' ' | b'\t') {
                    *pos += 1;
                }

                return Some(Some(open..close_end));
            }
            b'\n' if quote_char == b'(' => {
                *pos += 1;
            }
            _ => {
                *pos += 1;
            }
        }
    }

    None
}

/// Try to parse just the footnote marker [^id]: from a line.
/// Returns Some((id, content_start_col)) if the line starts with a footnote marker.
///
/// Syntax:
/// ```markdown
/// [^id]: Footnote content.
/// ```
pub fn try_parse_footnote_marker(line: &str) -> Option<(String, usize)> {
    let bytes = line.as_bytes();

    if bytes.len() < 4 || bytes[0] != b'[' || bytes[1] != b'^' {
        return None;
    }

    let mut pos = 2;
    while pos < bytes.len() && bytes[pos] != b']' && bytes[pos] != b'\n' && bytes[pos] != b'\r' {
        pos += 1;
    }

    if pos >= bytes.len() || bytes[pos] != b']' {
        return None;
    }

    let id = &line[2..pos];
    if id.is_empty() {
        return None;
    }

    pos += 1; // Skip ]

    if pos >= bytes.len() || bytes[pos] != b':' {
        return None;
    }
    pos += 1;

    while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t') {
        pos += 1;
    }

    Some((id.to_string(), pos))
}

#[cfg(test)]
mod tests {
    use super::{line_is_mmd_link_attribute_continuation, try_parse_reference_definition};
    use crate::syntax::SyntaxKind;

    #[test]
    fn test_footnote_definition_body_layout_is_lossless() {
        let input = "[^note-on-refs]:\n    Note that if `--file-scope` is used,\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn test_footnote_definition_marker_emits_structural_tokens() {
        let input = "[^note-on-refs]: body\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        let def = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FOOTNOTE_DEFINITION)
            .expect("footnote definition");
        let token_kinds: Vec<_> = def
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .map(|t| t.kind())
            .collect();
        assert!(token_kinds.contains(&SyntaxKind::FOOTNOTE_LABEL_START));
        assert!(token_kinds.contains(&SyntaxKind::FOOTNOTE_LABEL_ID));
        assert!(token_kinds.contains(&SyntaxKind::FOOTNOTE_LABEL_END));
        assert!(token_kinds.contains(&SyntaxKind::FOOTNOTE_LABEL_COLON));
    }

    /// Pandoc reads a list item's contents from the item's content column, so
    /// a marker sitting there opens a definition: `- x[^1]` / `  [^1]: d` is
    /// `Plain [Str "x", Note ...]`, not literal `[^1]:` text.
    #[test]
    fn footnote_definition_opens_at_a_list_item_content_column() {
        let input = "- x[^1]\n\n  [^1]: d\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        assert_eq!(tree.text().to_string(), input);

        let def = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FOOTNOTE_DEFINITION)
            .expect("footnote definition inside the list item");
        assert_eq!(def.text().to_string(), "  [^1]: d\n");
        assert!(
            def.ancestors().any(|n| n.kind() == SyntaxKind::LIST_ITEM),
            "definition should be nested in the list item, got:\n{tree}"
        );
    }

    #[test]
    fn footnote_definition_accepts_up_to_three_leading_spaces() {
        for (indent, prefix) in [(0, ""), (1, " "), (2, "  "), (3, "   ")] {
            let input = format!("{prefix}[^1]: d\n");
            let tree = crate::parse(&input, Some(crate::ParserOptions::default()));
            assert_eq!(tree.text().to_string(), input);
            assert!(
                tree.descendants()
                    .any(|n| n.kind() == SyntaxKind::FOOTNOTE_DEFINITION),
                "{indent} leading spaces should still open a definition, got:\n{tree}"
            );
        }

        let input = "    [^1]: d\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        assert_eq!(tree.text().to_string(), input);
        assert!(
            !tree
                .descendants()
                .any(|n| n.kind() == SyntaxKind::FOOTNOTE_DEFINITION),
            "four leading spaces is an indented code block, got:\n{tree}"
        );
    }

    #[test]
    fn footnote_definition_indent_window_is_relative_to_the_list_item() {
        for extra in 0..=3 {
            let input = format!("- xxxxx[^1]\n\n{:width$}[^1]: d\n", "", width = 2 + extra);
            let tree = crate::parse(&input, Some(crate::ParserOptions::default()));
            assert_eq!(tree.text().to_string(), input);
            assert!(
                tree.descendants()
                    .any(|n| n.kind() == SyntaxKind::FOOTNOTE_DEFINITION),
                "{extra} spaces past the content column should open a definition, got:\n{tree}"
            );
        }

        let input = "- xxxxx[^1]\n\n      [^1]: d\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        assert_eq!(tree.text().to_string(), input);
        assert!(
            !tree
                .descendants()
                .any(|n| n.kind() == SyntaxKind::FOOTNOTE_DEFINITION),
            "four spaces past the content column is indented code, got:\n{tree}"
        );
    }

    #[test]
    fn footnote_multiline_dollar_math_parses_as_display_math_not_tex_block() {
        let input = "[^note]: Intro line before math:\n    $$\n    \\begin{aligned} a &= b \\\\ c &= d \\end{aligned}\n    $$\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));

        let def = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FOOTNOTE_DEFINITION)
            .expect("footnote definition");

        let has_display_math = def
            .descendants()
            .any(|n| n.kind() == SyntaxKind::DISPLAY_MATH);
        let has_tex_block = def.descendants().any(|n| n.kind() == SyntaxKind::TEX_BLOCK);

        assert!(
            has_display_math,
            "Expected DISPLAY_MATH in footnote definition, got:\n{}",
            tree
        );
        assert!(
            !has_tex_block,
            "Did not expect TEX_BLOCK in footnote definition for $$...$$ math, got:\n{}",
            tree
        );
    }

    #[test]
    fn test_reference_definition_with_up_to_three_leading_spaces() {
        let d = crate::options::Dialect::Pandoc;
        assert!(try_parse_reference_definition("   [foo]: #bar", d).is_some());
        assert!(try_parse_reference_definition("    [foo]: #bar", d).is_none());
    }

    #[test]
    fn test_reference_definition_commonmark_requires_separator_before_title() {
        let pandoc =
            try_parse_reference_definition("[foo]: <bar>(baz)\n", crate::options::Dialect::Pandoc);
        assert_eq!(
            pandoc
                .as_ref()
                .map(|(_, _, url, title)| (url.as_str(), title.as_deref())),
            Some(("bar", Some("baz")))
        );

        let cmark = try_parse_reference_definition(
            "[foo]: <bar>(baz)\n",
            crate::options::Dialect::CommonMark,
        );
        assert!(cmark.is_none());

        let cmark_ok = try_parse_reference_definition(
            "[foo]: <bar> (baz)\n",
            crate::options::Dialect::CommonMark,
        );
        assert_eq!(
            cmark_ok
                .as_ref()
                .map(|(_, _, url, title)| (url.as_str(), title.as_deref())),
            Some(("bar", Some("baz")))
        );
    }

    #[test]
    fn test_reference_definition_emits_structured_url_and_title() {
        let input = "[ref]: <https://example.com> \"The Title\"\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        assert_eq!(tree.text().to_string(), input, "must stay lossless");

        let def = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::REFERENCE_DEFINITION)
            .expect("reference definition");

        let url = def
            .children()
            .find(|n| n.kind() == SyntaxKind::REFERENCE_URL)
            .expect("REFERENCE_URL node");
        assert_eq!(url.text().to_string(), "<https://example.com>");
        assert!(
            url.children_with_tokens()
                .any(|e| e.kind() == SyntaxKind::LINK_DEST_START)
        );
        assert!(
            url.children_with_tokens()
                .any(|e| e.kind() == SyntaxKind::LINK_DEST_END)
        );

        let title = def
            .children()
            .find(|n| n.kind() == SyntaxKind::REFERENCE_TITLE)
            .expect("REFERENCE_TITLE node");
        assert_eq!(title.text().to_string(), "\"The Title\"");
    }

    #[test]
    fn test_reference_definition_without_title_omits_title_node() {
        let input = "[ref]: /url\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        assert_eq!(tree.text().to_string(), input, "must stay lossless");

        let def = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::REFERENCE_DEFINITION)
            .expect("reference definition");

        let url = def
            .children()
            .find(|n| n.kind() == SyntaxKind::REFERENCE_URL)
            .expect("REFERENCE_URL node");
        assert_eq!(url.text().to_string(), "/url");
        assert!(
            !def.children()
                .any(|n| n.kind() == SyntaxKind::REFERENCE_TITLE),
            "no title => no REFERENCE_TITLE node"
        );
    }

    #[test]
    fn mmd_link_attribute_continuation_detects_valid_tokens() {
        assert!(line_is_mmd_link_attribute_continuation(
            "    width=20px height=30px id=myId"
        ));
        assert!(line_is_mmd_link_attribute_continuation(
            "\tclass=\"myClass1 myClass2\""
        ));
    }

    #[test]
    fn mmd_link_attribute_continuation_rejects_non_attribute_lines() {
        assert!(!line_is_mmd_link_attribute_continuation(
            "not-indented width=20px"
        ));
        assert!(!line_is_mmd_link_attribute_continuation(
            "    not-an-attr token"
        ));
    }
}
