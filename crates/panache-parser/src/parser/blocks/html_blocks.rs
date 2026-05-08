//! HTML block parsing utilities.

use crate::parser::inlines::inline_html::{parse_close_tag, parse_open_tag};
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::blockquotes::{count_blockquote_markers, strip_n_blockquote_markers};
use crate::parser::utils::helpers::{strip_leading_spaces, strip_newline};

/// HTML block-level tags as defined by CommonMark spec.
/// These tags start an HTML block when found at the start of a line.
const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "section",
    "source",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

/// Tags that contain raw/verbatim content (no Markdown processing inside).
const VERBATIM_TAGS: &[&str] = &["script", "style", "pre", "textarea"];

/// Whether `name` (case-insensitive) is one of the HTML block-level tags
/// recognized by CommonMark §4.6 type-6 (and pandoc's `markdown_in_html_blocks`
/// splitter). Used by the pandoc-native projector to decide whether a complete
/// HTML tag inside an `HTML_BLOCK` should split the block — block-level tags
/// emit as separate `RawBlock` entries; inline tags (e.g. `<em>`, `<a>`,
/// `<input>`, `<br>`) stay inline in the surrounding `Plain` content.
pub fn is_html_block_tag_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    BLOCK_TAGS.contains(&lower.as_str())
}

/// Information about a detected HTML block opening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HtmlBlockType {
    /// HTML comment: <!-- ... -->
    Comment,
    /// Processing instruction: <? ... ?>
    ProcessingInstruction,
    /// Declaration: <!...>
    Declaration,
    /// CDATA section: <![CDATA[ ... ]]>
    CData,
    /// Block-level tag (CommonMark types 6/1 — `tag_name` is one of
    /// `BLOCK_TAGS` or `VERBATIM_TAGS`). Set `closed_by_blank_line` to use
    /// CommonMark §4.6 type-6 end semantics (block ends at blank line);
    /// otherwise the legacy "ends at matching `</tag>`" semantics apply.
    BlockTag {
        tag_name: String,
        is_verbatim: bool,
        closed_by_blank_line: bool,
    },
    /// CommonMark §4.6 type 7: complete open or close tag on a line by
    /// itself, tag name not in the type-1 verbatim list. Block ends at
    /// blank line. Cannot interrupt a paragraph.
    Type7,
}

/// Try to detect an HTML block opening from content.
/// Returns block type if this is a valid HTML block start.
///
/// `is_commonmark` enables CommonMark §4.6 semantics: type-6 starts also
/// accept closing tags (`</div>`), type-6 blocks end at the next blank
/// line (rather than a matching close tag), and type 7 is recognized.
pub(crate) fn try_parse_html_block_start(
    content: &str,
    is_commonmark: bool,
) -> Option<HtmlBlockType> {
    let trimmed = strip_leading_spaces(content);

    // Must start with <
    if !trimmed.starts_with('<') {
        return None;
    }

    // HTML comment
    if trimmed.starts_with("<!--") {
        return Some(HtmlBlockType::Comment);
    }

    // Processing instruction
    if trimmed.starts_with("<?") {
        return Some(HtmlBlockType::ProcessingInstruction);
    }

    // CDATA section — CommonMark dialect only. Pandoc-markdown does not
    // recognize bare CDATA as a raw HTML block; the literal bytes fall
    // through to paragraph parsing (`<![CDATA[` becomes Str, the inner
    // text is parsed as inline markdown, etc).
    if is_commonmark && trimmed.starts_with("<![CDATA[") {
        return Some(HtmlBlockType::CData);
    }

    // Declaration (DOCTYPE, etc.) — CommonMark dialect only. Pandoc-markdown
    // does not recognize bare declarations as raw HTML blocks (its
    // `htmlBlock` reader uses `htmlTag isBlockTag`, which only matches
    // tag-shaped blocks); the bytes fall through to paragraph parsing.
    if is_commonmark && trimmed.starts_with("<!") && trimmed.len() > 2 {
        let after_bang = &trimmed[2..];
        if after_bang.chars().next()?.is_ascii_alphabetic() {
            return Some(HtmlBlockType::Declaration);
        }
    }

    // Try to parse as opening tag (or closing tag, under CommonMark)
    if let Some(tag_name) = extract_block_tag_name(trimmed, is_commonmark) {
        let tag_lower = tag_name.to_lowercase();
        let is_closing = trimmed.starts_with("</");

        // Check if it's a block-level tag
        if BLOCK_TAGS.contains(&tag_lower.as_str()) {
            let is_verbatim = VERBATIM_TAGS.contains(&tag_lower.as_str());
            return Some(HtmlBlockType::BlockTag {
                tag_name: tag_lower,
                is_verbatim,
                closed_by_blank_line: is_commonmark && !is_verbatim,
            });
        }

        // Also accept verbatim tags even if not in BLOCK_TAGS list — but
        // only as opening tags. CommonMark §4.6 type 1 starts with `<pre`,
        // `<script`, `<style`, or `<textarea`; closing forms like `</pre>`
        // do not start a type-1 block. Letting `</pre>` through here would
        // wrongly interrupt a paragraph.
        if !is_closing && VERBATIM_TAGS.contains(&tag_lower.as_str()) {
            return Some(HtmlBlockType::BlockTag {
                tag_name: tag_lower,
                is_verbatim: true,
                closed_by_blank_line: false,
            });
        }
    }

    // Type 7 (CommonMark only): complete open or close tag on a line by
    // itself, tag name not in the type-1 verbatim list.
    if is_commonmark && let Some(end) = parse_open_tag(trimmed).or_else(|| parse_close_tag(trimmed))
    {
        let rest = &trimmed[end..];
        let only_ws = rest
            .bytes()
            .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'));
        if only_ws {
            // Reject if the tag name belongs to the type-1 verbatim set
            // (`<pre>`, `<script>`, `<style>`, `<textarea>`) — those are
            // type-1 starts above, so seeing one here means the opener
            // had a different shape (e.g. `<pre/>` self-closing) that
            // shouldn't trigger type 7 either. Conservatively skip.
            let leading = trimmed.strip_prefix("</").unwrap_or_else(|| &trimmed[1..]);
            let name_end = leading
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                .unwrap_or(leading.len());
            let name = leading[..name_end].to_ascii_lowercase();
            if !VERBATIM_TAGS.contains(&name.as_str()) {
                return Some(HtmlBlockType::Type7);
            }
        }
    }

    None
}

/// Extract the tag name for HTML-block-start detection.
///
/// Accepts both opening (`<tag>`) and closing (`</tag>`) forms when
/// `accept_closing` is true (CommonMark §4.6 type 6 allows either). The
/// tag must be followed by a space, tab, line ending, `>`, or `/>` per
/// the spec — we approximate that with the space/`>`/`/` boundary check.
fn extract_block_tag_name(text: &str, accept_closing: bool) -> Option<String> {
    if !text.starts_with('<') {
        return None;
    }

    let after_bracket = &text[1..];

    let after_slash = if let Some(stripped) = after_bracket.strip_prefix('/') {
        if !accept_closing {
            return None;
        }
        stripped
    } else {
        after_bracket
    };

    // Extract tag name (alphanumeric, ends at space, >, or /)
    let tag_end = after_slash
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .unwrap_or(after_slash.len());

    if tag_end == 0 {
        return None;
    }

    let tag_name = &after_slash[..tag_end];

    // Tag name must be valid (ASCII alphabetic start, alphanumeric)
    if !tag_name.chars().next()?.is_ascii_alphabetic() {
        return None;
    }

    if !tag_name.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }

    Some(tag_name.to_string())
}

/// Whether this block type ends at a blank line (CommonMark types 6 & 7
/// in CommonMark dialect). Such blocks do NOT close on a matching tag /
/// marker — only at end of input or the next blank line.
fn ends_at_blank_line(block_type: &HtmlBlockType) -> bool {
    matches!(
        block_type,
        HtmlBlockType::Type7
            | HtmlBlockType::BlockTag {
                closed_by_blank_line: true,
                ..
            }
    )
}

/// Check if a line contains the closing marker for the given HTML block type.
/// Only meaningful for types 1–5 and the legacy "type 6 closed by tag" path;
/// blank-line-terminated types (6 in CommonMark, 7) never match here.
fn is_closing_marker(line: &str, block_type: &HtmlBlockType) -> bool {
    match block_type {
        HtmlBlockType::Comment => line.contains("-->"),
        HtmlBlockType::ProcessingInstruction => line.contains("?>"),
        HtmlBlockType::Declaration => line.contains('>'),
        HtmlBlockType::CData => line.contains("]]>"),
        HtmlBlockType::BlockTag {
            tag_name,
            closed_by_blank_line: false,
            ..
        } => {
            // Look for closing tag </tagname>
            let closing_tag = format!("</{}>", tag_name);
            line.to_lowercase().contains(&closing_tag)
        }
        HtmlBlockType::BlockTag {
            closed_by_blank_line: true,
            ..
        }
        | HtmlBlockType::Type7 => false,
    }
}

/// Parse an HTML block, allowing the caller to pick the wrapper SyntaxKind
/// (`HTML_BLOCK` for opaque preservation, `HTML_BLOCK_DIV` for the
/// Pandoc-dialect `<div>` lift). Children are emitted byte-for-byte
/// identical to the source either way; only the wrapper retag changes.
pub(crate) fn parse_html_block_with_wrapper(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    block_type: HtmlBlockType,
    bq_depth: usize,
    wrapper_kind: SyntaxKind,
) -> usize {
    // Start HTML block
    builder.start_node(wrapper_kind.into());

    let first_line = lines[start_pos];
    let blank_terminated = ends_at_blank_line(&block_type);

    // The block dispatcher has already emitted BLOCK_QUOTE_MARKER + WHITESPACE
    // tokens for the first line's blockquote prefix; emit only the inner
    // content as TEXT to keep the CST byte-equal to the source.
    let first_inner = if bq_depth > 0 {
        strip_n_blockquote_markers(first_line, bq_depth)
    } else {
        first_line
    };

    // Emit opening line
    builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());

    let (line_without_newline, newline_str) = strip_newline(first_inner);
    if !line_without_newline.is_empty() {
        // For HTML_BLOCK_DIV, expose the open tag's attributes
        // structurally so `AttributeNode::cast(HTML_ATTRS)` finds them
        // via the same descendants walk that handles fenced-div /
        // heading attrs. CST bytes stay byte-equal to source — we only
        // tokenize at finer granularity for matched div opens.
        if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV {
            emit_div_open_tag_tokens(builder, line_without_newline);
        } else {
            builder.token(SyntaxKind::TEXT.into(), line_without_newline);
        }
    }
    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

    builder.finish_node(); // HtmlBlockTag

    // Check if opening line also contains closing marker. Blank-line-terminated
    // blocks (CommonMark types 6 & 7) ignore inline close markers — they only
    // end at a blank line or end of input.
    if !blank_terminated && is_closing_marker(first_inner, &block_type) {
        log::trace!(
            "HTML block at line {} opens and closes on same line",
            start_pos + 1
        );
        builder.finish_node(); // HtmlBlock
        return start_pos + 1;
    }

    let mut current_pos = start_pos + 1;
    let mut content_lines: Vec<&str> = Vec::new();
    let mut found_closing = false;

    // Parse content until we find the closing marker
    while current_pos < lines.len() {
        let line = lines[current_pos];
        let (line_bq_depth, inner) = count_blockquote_markers(line);

        // Only process lines at the same or deeper blockquote depth
        if line_bq_depth < bq_depth {
            break;
        }

        // Blank-line-terminated blocks (types 6/7) end before the blank line.
        // The blank line itself is not part of the block.
        if blank_terminated && inner.trim().is_empty() {
            break;
        }

        // Check for closing marker. Match against the inner content so a `>`-
        // prefixed continuation line still recognises e.g. `</div>`.
        if is_closing_marker(inner, &block_type) {
            log::trace!("Found HTML block closing at line {}", current_pos + 1);
            found_closing = true;

            if !content_lines.is_empty() {
                builder.start_node(SyntaxKind::HTML_BLOCK_CONTENT.into());
                for content_line in &content_lines {
                    emit_html_block_line(builder, content_line, bq_depth);
                }
                builder.finish_node();
            }

            builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());
            emit_html_block_line(builder, line, bq_depth);
            builder.finish_node();

            current_pos += 1;
            break;
        }

        // Regular content line
        content_lines.push(line);
        current_pos += 1;
    }

    // If we didn't find a closing marker, emit what we collected
    if !found_closing {
        log::trace!("HTML block at line {} has no closing marker", start_pos + 1);
        if !content_lines.is_empty() {
            builder.start_node(SyntaxKind::HTML_BLOCK_CONTENT.into());
            for content_line in &content_lines {
                emit_html_block_line(builder, content_line, bq_depth);
            }
            builder.finish_node();
        }
    }

    builder.finish_node(); // HtmlBlock
    current_pos
}

/// Emit the open-tag line of an `HTML_BLOCK_DIV`, splitting the bytes
/// `[ws]<div[ ws ATTRS]>[trailing]` into
/// `WHITESPACE? + TEXT("<div") + (WHITESPACE + HTML_ATTRS{TEXT(attrs)})?
/// + TEXT(">") + TEXT(trailing)?`.
///
/// Bytes are byte-identical to the source — this only tokenizes at finer
/// granularity so `AttributeNode::cast(HTML_ATTRS)` can read the attribute
/// region structurally. Falls back to a single TEXT token if the line
/// doesn't fit the expected `<div ...>` shape (defensive — the parser
/// only retags as `HTML_BLOCK_DIV` when this shape was matched).
fn emit_div_open_tag_tokens(builder: &mut GreenNodeBuilder<'static>, line: &str) {
    let bytes = line.as_bytes();
    // Leading indent (CommonMark allows up to 3 spaces).
    let indent_end = bytes.iter().position(|&b| b != b' ').unwrap_or(bytes.len());
    if indent_end > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &line[..indent_end]);
    }
    let rest = &line[indent_end..];
    // Match the literal `<div` prefix (ASCII case-insensitive on `div`).
    if !rest.starts_with('<') || rest.len() < 4 || !rest[1..4].eq_ignore_ascii_case("div") {
        builder.token(SyntaxKind::TEXT.into(), rest);
        return;
    }
    let after_name = &rest[4..];
    let after_name_bytes = after_name.as_bytes();
    // Find the closing `>` of the open tag, respecting quoted attribute values.
    let mut i = 0usize;
    let mut quote: Option<u8> = None;
    let mut tag_close: Option<usize> = None;
    while i < after_name_bytes.len() {
        let b = after_name_bytes[i];
        match (quote, b) {
            (None, b'"') | (None, b'\'') => quote = Some(b),
            (Some(q), b2) if b2 == q => quote = None,
            (None, b'>') => {
                tag_close = Some(i);
                break;
            }
            _ => {}
        }
        i += 1;
    }
    let Some(tag_close) = tag_close else {
        // Open tag has no closing `>` on this line — defensive fallback.
        builder.token(SyntaxKind::TEXT.into(), rest);
        return;
    };
    // Whitespace between the tag name and the attribute region.
    let attrs_inner = &after_name[..tag_close];
    let ws_end = attrs_inner
        .as_bytes()
        .iter()
        .position(|&b| !matches!(b, b' ' | b'\t'))
        .unwrap_or(attrs_inner.len());
    let leading_ws = &attrs_inner[..ws_end];
    // Strip a trailing self-closing slash and the whitespace before it
    // from the attribute region; emit them as TEXT outside the
    // HTML_ATTRS node so the structural region only holds attribute
    // bytes (not formatting punctuation).
    let attrs_after_ws = &attrs_inner[ws_end..];
    let mut attr_end = attrs_after_ws.len();
    let attr_bytes = attrs_after_ws.as_bytes();
    let mut self_close_start = attr_end;
    if attr_end > 0 && attr_bytes[attr_end - 1] == b'/' {
        self_close_start = attr_end - 1;
        attr_end = self_close_start;
        while attr_end > 0 && matches!(attr_bytes[attr_end - 1], b' ' | b'\t') {
            attr_end -= 1;
        }
    }
    let attrs_text = &attrs_after_ws[..attr_end];
    let trailing_text = &attrs_after_ws[attr_end..self_close_start.max(attr_end)];
    let after_self_close = &attrs_after_ws[self_close_start..];

    builder.token(SyntaxKind::TEXT.into(), "<div");
    if !leading_ws.is_empty() {
        builder.token(SyntaxKind::WHITESPACE.into(), leading_ws);
    }
    if !attrs_text.is_empty() {
        builder.start_node(SyntaxKind::HTML_ATTRS.into());
        builder.token(SyntaxKind::TEXT.into(), attrs_text);
        builder.finish_node();
    }
    if !trailing_text.is_empty() {
        builder.token(SyntaxKind::WHITESPACE.into(), trailing_text);
    }
    if !after_self_close.is_empty() {
        builder.token(SyntaxKind::TEXT.into(), after_self_close);
    }
    builder.token(SyntaxKind::TEXT.into(), ">");
    let after_gt = &after_name[tag_close + 1..];
    if !after_gt.is_empty() {
        builder.token(SyntaxKind::TEXT.into(), after_gt);
    }
}

/// Emit one continuation line of an HTML block, preserving any blockquote
/// markers as structural tokens (so the CST stays byte-equal to the source
/// and downstream consumers can strip them per-context).
fn emit_html_block_line(builder: &mut GreenNodeBuilder<'static>, line: &str, bq_depth: usize) {
    let inner = if bq_depth > 0 {
        let stripped = strip_n_blockquote_markers(line, bq_depth);
        let prefix_len = line.len() - stripped.len();
        if prefix_len > 0 {
            for ch in line[..prefix_len].chars() {
                if ch == '>' {
                    builder.token(SyntaxKind::BLOCK_QUOTE_MARKER.into(), ">");
                } else {
                    let mut buf = [0u8; 4];
                    builder.token(SyntaxKind::WHITESPACE.into(), ch.encode_utf8(&mut buf));
                }
            }
        }
        stripped
    } else {
        line
    };

    let (line_without_newline, newline_str) = strip_newline(inner);
    if !line_without_newline.is_empty() {
        builder.token(SyntaxKind::TEXT.into(), line_without_newline);
    }
    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_parse_html_comment() {
        assert_eq!(
            try_parse_html_block_start("<!-- comment -->", false),
            Some(HtmlBlockType::Comment)
        );
        assert_eq!(
            try_parse_html_block_start("  <!-- comment -->", false),
            Some(HtmlBlockType::Comment)
        );
    }

    #[test]
    fn test_try_parse_div_tag() {
        assert_eq!(
            try_parse_html_block_start("<div>", false),
            Some(HtmlBlockType::BlockTag {
                tag_name: "div".to_string(),
                is_verbatim: false,
                closed_by_blank_line: false,
            })
        );
        assert_eq!(
            try_parse_html_block_start("<div class=\"test\">", false),
            Some(HtmlBlockType::BlockTag {
                tag_name: "div".to_string(),
                is_verbatim: false,
                closed_by_blank_line: false,
            })
        );
    }

    #[test]
    fn test_try_parse_script_tag() {
        assert_eq!(
            try_parse_html_block_start("<script>", false),
            Some(HtmlBlockType::BlockTag {
                tag_name: "script".to_string(),
                is_verbatim: true,
                closed_by_blank_line: false,
            })
        );
    }

    #[test]
    fn test_try_parse_processing_instruction() {
        assert_eq!(
            try_parse_html_block_start("<?xml version=\"1.0\"?>", false),
            Some(HtmlBlockType::ProcessingInstruction)
        );
    }

    #[test]
    fn test_try_parse_declaration() {
        // CommonMark dialect recognizes declarations as type-4 HTML blocks.
        assert_eq!(
            try_parse_html_block_start("<!DOCTYPE html>", true),
            Some(HtmlBlockType::Declaration)
        );
        // CommonMark §4.6 type 4 accepts any ASCII letter after `<!`, not
        // just uppercase. Lowercase doctype must match too.
        assert_eq!(
            try_parse_html_block_start("<!doctype html>", true),
            Some(HtmlBlockType::Declaration)
        );
        // Pandoc dialect does not — bare declarations fall through to
        // paragraph parsing.
        assert_eq!(try_parse_html_block_start("<!DOCTYPE html>", false), None);
        assert_eq!(try_parse_html_block_start("<!doctype html>", false), None);
    }

    #[test]
    fn test_try_parse_cdata() {
        // CommonMark dialect recognizes CDATA as type-5 HTML blocks.
        assert_eq!(
            try_parse_html_block_start("<![CDATA[content]]>", true),
            Some(HtmlBlockType::CData)
        );
        // Pandoc dialect does not.
        assert_eq!(
            try_parse_html_block_start("<![CDATA[content]]>", false),
            None
        );
    }

    #[test]
    fn test_extract_block_tag_name_open_only() {
        assert_eq!(
            extract_block_tag_name("<div>", false),
            Some("div".to_string())
        );
        assert_eq!(
            extract_block_tag_name("<div class=\"test\">", false),
            Some("div".to_string())
        );
        assert_eq!(
            extract_block_tag_name("<div/>", false),
            Some("div".to_string())
        );
        assert_eq!(extract_block_tag_name("</div>", false), None);
        assert_eq!(extract_block_tag_name("<>", false), None);
        assert_eq!(extract_block_tag_name("< div>", false), None);
    }

    #[test]
    fn test_extract_block_tag_name_with_closing() {
        // CommonMark §4.6 type-6 starts also accept closing tags.
        assert_eq!(
            extract_block_tag_name("</div>", true),
            Some("div".to_string())
        );
        assert_eq!(
            extract_block_tag_name("</div >", true),
            Some("div".to_string())
        );
    }

    #[test]
    fn test_commonmark_type6_closing_tag_start() {
        assert_eq!(
            try_parse_html_block_start("</div>", true),
            Some(HtmlBlockType::BlockTag {
                tag_name: "div".to_string(),
                is_verbatim: false,
                closed_by_blank_line: true,
            })
        );
    }

    #[test]
    fn test_commonmark_type7_open_tag() {
        // `<a>` (not a type-6 tag) on a line by itself is type 7 under
        // CommonMark; rejected under non-CommonMark.
        assert_eq!(
            try_parse_html_block_start("<a href=\"foo\">", true),
            Some(HtmlBlockType::Type7)
        );
        assert_eq!(try_parse_html_block_start("<a href=\"foo\">", false), None);
    }

    #[test]
    fn test_commonmark_type7_close_tag() {
        assert_eq!(
            try_parse_html_block_start("</ins>", true),
            Some(HtmlBlockType::Type7)
        );
    }

    #[test]
    fn test_commonmark_type7_rejects_with_trailing_text() {
        // A complete tag must be followed only by whitespace.
        assert_eq!(try_parse_html_block_start("<a> hi", true), None);
    }

    #[test]
    fn test_is_closing_marker_comment() {
        let block_type = HtmlBlockType::Comment;
        assert!(is_closing_marker("-->", &block_type));
        assert!(is_closing_marker("end -->", &block_type));
        assert!(!is_closing_marker("<!--", &block_type));
    }

    #[test]
    fn test_is_closing_marker_tag() {
        let block_type = HtmlBlockType::BlockTag {
            tag_name: "div".to_string(),
            is_verbatim: false,
            closed_by_blank_line: false,
        };
        assert!(is_closing_marker("</div>", &block_type));
        assert!(is_closing_marker("</DIV>", &block_type)); // Case insensitive
        assert!(is_closing_marker("content</div>", &block_type));
        assert!(!is_closing_marker("<div>", &block_type));
    }

    #[test]
    fn test_parse_html_comment_block() {
        let input = "<!-- comment -->\n";
        let lines: Vec<&str> = input.lines().collect();
        let mut builder = GreenNodeBuilder::new();

        let block_type = try_parse_html_block_start(lines[0], false).unwrap();
        let new_pos = parse_html_block_with_wrapper(
            &mut builder,
            &lines,
            0,
            block_type,
            0,
            SyntaxKind::HTML_BLOCK,
        );

        assert_eq!(new_pos, 1);
    }

    #[test]
    fn test_parse_div_block() {
        let input = "<div>\ncontent\n</div>\n";
        let lines: Vec<&str> = input.lines().collect();
        let mut builder = GreenNodeBuilder::new();

        let block_type = try_parse_html_block_start(lines[0], false).unwrap();
        let new_pos = parse_html_block_with_wrapper(
            &mut builder,
            &lines,
            0,
            block_type,
            0,
            SyntaxKind::HTML_BLOCK,
        );

        assert_eq!(new_pos, 3);
    }

    #[test]
    fn test_parse_html_block_no_closing() {
        let input = "<div>\ncontent\n";
        let lines: Vec<&str> = input.lines().collect();
        let mut builder = GreenNodeBuilder::new();

        let block_type = try_parse_html_block_start(lines[0], false).unwrap();
        let new_pos = parse_html_block_with_wrapper(
            &mut builder,
            &lines,
            0,
            block_type,
            0,
            SyntaxKind::HTML_BLOCK,
        );

        // Should consume all lines even without closing tag
        assert_eq!(new_pos, 2);
    }

    #[test]
    fn test_commonmark_type6_blank_line_terminates() {
        let input = "<div>\nfoo\n\nbar\n";
        let lines: Vec<&str> = input.lines().collect();
        let mut builder = GreenNodeBuilder::new();

        let block_type = try_parse_html_block_start(lines[0], true).unwrap();
        let new_pos = parse_html_block_with_wrapper(
            &mut builder,
            &lines,
            0,
            block_type,
            0,
            SyntaxKind::HTML_BLOCK,
        );

        // Block contains <div>\nfoo\n; stops at blank line (line 2).
        assert_eq!(new_pos, 2);
    }
}
