//! Pandoc wikilink extensions: `wikilinks_title_after_pipe` and
//! `wikilinks_title_before_pipe`.
//!
//! Shape: `[[url]]` (or `[[url|title]]` with after-pipe semantics, or
//! `[[title|url]]` with before-pipe). Image variant: `![[url]]` /
//! `![[url|title]]`. Single line, non-greedy on the first `]]`, rejects
//! empty body. When both extensions are enabled, after-pipe wins
//! (matches pandoc behavior).
//!
//! Title content is NOT recursively parsed for inlines — `[[url|**bold**]]`
//! emits the title as a flat TEXT span containing the literal bytes
//! `**bold**`. Verified against `pandoc 3.9.0.2 -f
//! markdown+wikilinks_title_after_pipe -t native`.
//!
//! Lives in the inline IR's `ConstructKind` dispatch path so that
//! everything inside `[[...]]` is opaque to emphasis / bracket / autolink
//! resolution. The emitter walks the byte range and re-locates the pipe.

use super::sink::InlineSink;
use crate::ParserOptions;
use crate::syntax::SyntaxKind;

/// A successfully matched wikilink span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WikiLinkSpan {
    /// Byte index of the leading `[` (or `!` for image variant).
    pub start: usize,
    /// One-past-end byte index of the closing `]]`.
    pub end: usize,
    /// Byte index of the separating `|`, if present. Absolute (not
    /// relative to `start`).
    pub pipe: Option<usize>,
    /// True if the variant is `![[...]]` (image wikilink).
    pub is_image: bool,
}

impl WikiLinkSpan {
    /// Byte index of the first byte after the opening `[[` (or `![[`).
    pub(crate) fn body_start(&self) -> usize {
        if self.is_image {
            self.start + 3 // ![[
        } else {
            self.start + 2 // [[
        }
    }

    /// Byte index of the closing `]]` (start of the two `]` bytes).
    pub(crate) fn body_end(&self) -> usize {
        self.end - 2
    }
}

/// True if either wikilink extension is enabled in `opts`.
pub(crate) fn any_enabled(opts: &ParserOptions) -> bool {
    opts.extensions.wikilinks_title_after_pipe || opts.extensions.wikilinks_title_before_pipe
}

/// Try to recognise a wikilink starting at byte index `pos` in `text`.
///
/// Returns `None` unless `text[pos..]` begins with `[[` (or `![[` for the
/// image variant) and the matching `]]` occurs before the next newline.
/// Empty body (`[[]]`) is rejected per pandoc behavior. Matching is
/// non-greedy: the first `]]` after the opener closes the wikilink.
pub(crate) fn try_parse_wikilink(
    text: &str,
    pos: usize,
    opts: &ParserOptions,
) -> Option<WikiLinkSpan> {
    if !any_enabled(opts) {
        return None;
    }

    let bytes = text.as_bytes();
    let n = bytes.len();
    if pos >= n {
        return None;
    }

    let (is_image, open_end) = if bytes[pos] == b'!' {
        if pos + 2 >= n || bytes[pos + 1] != b'[' || bytes[pos + 2] != b'[' {
            return None;
        }
        (true, pos + 3)
    } else if bytes[pos] == b'[' {
        if pos + 1 >= n || bytes[pos + 1] != b'[' {
            return None;
        }
        (false, pos + 2)
    } else {
        return None;
    };

    let body_start = open_end;
    let mut i = body_start;
    let mut pipe: Option<usize> = None;
    while i + 1 < n {
        let b = bytes[i];
        if b == b'\n' || b == b'\r' {
            return None;
        }
        if b == b']' && bytes[i + 1] == b']' {
            if i == body_start {
                // Empty body — `[[]]` is literal text per pandoc.
                return None;
            }
            return Some(WikiLinkSpan {
                start: pos,
                end: i + 2,
                pipe,
                is_image,
            });
        }
        if b == b'|' && pipe.is_none() {
            pipe = Some(i);
        }
        i += 1;
    }
    None
}

/// Emit the CST nodes for a previously matched wikilink span.
///
/// `text` is the full document buffer; `span.start..span.end` is the
/// wikilink range. Pipe direction (URL/title order) is decided by
/// `opts.extensions.wikilinks_title_after_pipe` vs
/// `wikilinks_title_before_pipe`. After-pipe wins when both are on.
pub(crate) fn emit_wikilink<S: InlineSink>(
    builder: &mut S,
    text: &str,
    span: WikiLinkSpan,
    opts: &ParserOptions,
) {
    let outer_kind = if span.is_image {
        SyntaxKind::IMAGE_WIKI_LINK
    } else {
        SyntaxKind::WIKI_LINK
    };
    let open_str = if span.is_image { "![[" } else { "[[" };

    builder.start_node(outer_kind.into());
    builder.token(SyntaxKind::WIKI_LINK_OPEN.into(), open_str);

    let body_start = span.body_start();
    let body_end = span.body_end();

    let (url_range, title_range) = match span.pipe {
        Some(p) => {
            let url;
            let title;
            if opts.extensions.wikilinks_title_after_pipe {
                // [[url|title]]
                url = body_start..p;
                title = (p + 1)..body_end;
            } else {
                // [[title|url]] (only before-pipe is on)
                title = body_start..p;
                url = (p + 1)..body_end;
            }
            (url, Some((p, title)))
        }
        None => (body_start..body_end, None),
    };

    // URL slot first by CST order when there is no pipe, or in the
    // source order when there is one. We preserve the source byte
    // sequence so the formatter can round-trip verbatim.
    let url_first = match span.pipe {
        Some(_) => opts.extensions.wikilinks_title_after_pipe,
        None => true,
    };

    let emit_url = |b: &mut S| {
        b.start_node(SyntaxKind::WIKI_LINK_URL.into());
        b.token(SyntaxKind::TEXT.into(), &text[url_range.clone()]);
        b.finish_node();
    };
    let emit_pipe_and_title = |b: &mut S| {
        if let Some((_p, ref tr)) = title_range {
            b.token(SyntaxKind::WIKI_LINK_PIPE.into(), "|");
            b.start_node(SyntaxKind::WIKI_LINK_TITLE.into());
            b.token(SyntaxKind::TEXT.into(), &text[tr.clone()]);
            b.finish_node();
        }
    };

    if url_first {
        emit_url(builder);
        emit_pipe_and_title(builder);
    } else {
        // Before-pipe: source order is title, |, url.
        if let Some((_p, ref tr)) = title_range {
            builder.start_node(SyntaxKind::WIKI_LINK_TITLE.into());
            builder.token(SyntaxKind::TEXT.into(), &text[tr.clone()]);
            builder.finish_node();
            builder.token(SyntaxKind::WIKI_LINK_PIPE.into(), "|");
        }
        emit_url(builder);
    }

    builder.token(SyntaxKind::WIKI_LINK_CLOSE.into(), "]]");
    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::{Extensions, ParserOptions};

    fn opts_with(after: bool, before: bool) -> ParserOptions {
        let extensions = Extensions {
            wikilinks_title_after_pipe: after,
            wikilinks_title_before_pipe: before,
            ..Extensions::default()
        };
        ParserOptions {
            extensions,
            ..ParserOptions::default()
        }
    }

    fn opts_after() -> ParserOptions {
        opts_with(true, false)
    }

    fn opts_before() -> ParserOptions {
        opts_with(false, true)
    }

    fn opts_both() -> ParserOptions {
        opts_with(true, true)
    }

    fn opts_off() -> ParserOptions {
        opts_with(false, false)
    }

    #[test]
    fn parses_simple_wikilink() {
        let text = "[[https://example.org]]";
        let span = try_parse_wikilink(text, 0, &opts_after()).unwrap();
        assert_eq!(span.start, 0);
        assert_eq!(span.end, text.len());
        assert_eq!(span.pipe, None);
        assert!(!span.is_image);
    }

    #[test]
    fn parses_with_title() {
        let text = "[[url|hello]]";
        let span = try_parse_wikilink(text, 0, &opts_after()).unwrap();
        assert_eq!(span.pipe, Some(5));
        assert_eq!(span.end, text.len());
    }

    #[test]
    fn parses_image_wikilink() {
        let text = "![[url]]";
        let span = try_parse_wikilink(text, 0, &opts_after()).unwrap();
        assert!(span.is_image);
        assert_eq!(span.end, text.len());
    }

    #[test]
    fn rejects_empty_body() {
        // Pandoc: `[[]]` renders as literal text.
        assert!(try_parse_wikilink("[[]]", 0, &opts_after()).is_none());
        assert!(try_parse_wikilink("![[]]", 0, &opts_after()).is_none());
    }

    #[test]
    fn rejects_unclosed() {
        assert!(try_parse_wikilink("[[unclosed", 0, &opts_after()).is_none());
        assert!(try_parse_wikilink("[[no closing here", 0, &opts_after()).is_none());
    }

    #[test]
    fn rejects_newline_inside() {
        // Pandoc: `[[a\nb]]` is literal text (single-line shape).
        assert!(try_parse_wikilink("[[a\nb]]", 0, &opts_after()).is_none());
        assert!(try_parse_wikilink("[[a\r\nb]]", 0, &opts_after()).is_none());
    }

    #[test]
    fn rejects_when_disabled() {
        // No wikilink when neither extension is enabled.
        assert!(try_parse_wikilink("[[a|b]]", 0, &opts_off()).is_none());
        assert!(try_parse_wikilink("[[just url]]", 0, &opts_off()).is_none());
    }

    #[test]
    fn non_greedy_close() {
        // `[[a]]b]]` → wikilink is `[[a]]`, the rest is literal.
        let text = "[[a]]b]]";
        let span = try_parse_wikilink(text, 0, &opts_after()).unwrap();
        assert_eq!(span.end, 5); // just `[[a]]`
    }

    #[test]
    fn first_pipe_is_separator() {
        // `[[a|b|c]]` → pipe is at position of the first `|`.
        let text = "[[a|b|c]]";
        let span = try_parse_wikilink(text, 0, &opts_after()).unwrap();
        assert_eq!(span.pipe, Some(3));
    }

    #[test]
    fn parses_with_before_pipe_extension() {
        let text = "[[title|url]]";
        let span = try_parse_wikilink(text, 0, &opts_before()).unwrap();
        assert_eq!(span.pipe, Some(7));
    }

    #[test]
    fn both_extensions_enabled_still_matches() {
        // Detection is identical; only emission differs.
        let text = "[[a|b]]";
        let span = try_parse_wikilink(text, 0, &opts_both()).unwrap();
        assert_eq!(span.pipe, Some(3));
    }

    #[test]
    fn parse_at_offset() {
        // Wikilink not at position 0.
        let text = "prefix [[url|title]] suffix";
        let span = try_parse_wikilink(text, 7, &opts_after()).unwrap();
        assert_eq!(span.start, 7);
        assert_eq!(span.end, 20);
    }

    #[test]
    fn body_indexing_is_correct() {
        let text = "[[abc|def]]";
        let span = try_parse_wikilink(text, 0, &opts_after()).unwrap();
        assert_eq!(span.body_start(), 2);
        assert_eq!(span.body_end(), 9);
        assert_eq!(&text[span.body_start()..span.body_end()], "abc|def");
    }

    #[test]
    fn image_body_indexing_is_correct() {
        let text = "![[abc]]";
        let span = try_parse_wikilink(text, 0, &opts_after()).unwrap();
        assert_eq!(span.body_start(), 3);
        assert_eq!(span.body_end(), 6);
        assert_eq!(&text[span.body_start()..span.body_end()], "abc");
    }
}
