//! HTML block parsing utilities.

use crate::options::ParserOptions;
use crate::parser::inlines::inline_html::{parse_close_tag, parse_open_tag};
use crate::syntax::{SyntaxKind, SyntaxNode};
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

/// Pandoc's `blockHtmlTags` (mirrors
/// `pandoc/src/Text/Pandoc/Readers/HTML/TagCategories.hs`). Pandoc-markdown
/// uses this narrower set rather than CommonMark §4.6 type-6: it omits a
/// number of CM type-6 tags (e.g. `dialog`, `legend`, `optgroup`, `option`,
/// `frame`, `link`, `param`, `base`, `basefont`, `menuitem`) that pandoc
/// treats as raw inline HTML, and adds a few pandoc keeps as block-level
/// (`canvas`, `hgroup`, `isindex`, `meta`, `output`).
///
/// Pandoc's `eitherBlockOrInline` set (`audio`, `button`, `iframe`,
/// `noscript`, `object`, `map`, `progress`, `video`, `del`, `ins`, `svg`,
/// `applet`, plus the void elements `embed`, `area`, `source`, `track`
/// and the verbatim `script`) is tracked separately as
/// [`PANDOC_INLINE_BLOCK_TAGS`]. Those tags act as block starters at
/// fresh-block positions but stay inline inside an existing HTML block
/// (e.g. `<form><input><button>X</button></form>`); the projector's
/// `split_html_block_by_tags` keys on `inline_pending` to keep them
/// inline once an inline-only tag or text byte has been seen since the
/// last splitter.
const PANDOC_BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "canvas",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "isindex",
    "li",
    "main",
    "menu",
    "meta",
    "nav",
    "noframes",
    "ol",
    "output",
    "p",
    "pre",
    "script",
    "section",
    "style",
    "summary",
    "table",
    "tbody",
    "td",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "tr",
    "ul",
];

/// Whether `name` (case-insensitive) is one of the HTML block-level tags
/// recognized by CommonMark §4.6 type-6.
pub fn is_html_block_tag_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    BLOCK_TAGS.contains(&lower.as_str())
}

/// Whether `name` (case-insensitive) is one of pandoc's `blockHtmlTags` —
/// the narrower set pandoc-markdown's `htmlBlock` reader recognizes.
/// Used by the pandoc-native projector's `split_html_block_by_tags` to
/// decide whether a complete HTML tag inside an `HTML_BLOCK` should split
/// the block — block-level tags emit as separate `RawBlock` entries;
/// inline tags stay inline in the surrounding `Plain` content.
pub fn is_pandoc_block_tag_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    PANDOC_BLOCK_TAGS.contains(&lower.as_str())
}

/// Pandoc's `eitherBlockOrInline` set (mirrors
/// `pandoc/src/Text/Pandoc/Readers/HTML/TagCategories.hs`): tags that
/// `isBlockTag` accepts as block starters but `isInlineTag` ALSO accepts
/// (because `name ∉ blockTags`). At top level (or after a blank line)
/// pandoc treats `<iframe>foo</iframe>` as RawBlock+Plain+RawBlock, but
/// inside an existing HTML block once a paragraph has started parsing,
/// the same tag stays inline as `RawInline`.
///
/// The projector's `split_html_block_by_tags` mirrors this with an
/// `inline_pending` flag — strict block tags ([`PANDOC_BLOCK_TAGS`])
/// always split; inline-block tags split only when no inline content
/// has been buffered since the last splitter.
///
/// Void elements (`area`, `embed`, `source`, `track`) live in
/// [`PANDOC_VOID_BLOCK_TAGS`]; they follow the same `inline_pending`
/// rule as non-void inline-block tags but emit a single RawBlock per
/// instance instead of a matched-pair lift.
/// `script` is omitted because it is already verbatim (handled by the
/// `<script>...</script>` raw-text path) and the strict-block check
/// fires first regardless.
const PANDOC_INLINE_BLOCK_TAGS: &[&str] = &[
    "applet", "audio", "button", "del", "iframe", "ins", "map", "noscript", "object", "progress",
    "svg", "video",
];

/// Whether `name` (case-insensitive) is one of pandoc's
/// `eitherBlockOrInline` tags (excluding void elements and `script`;
/// see [`PANDOC_INLINE_BLOCK_TAGS`]).
pub fn is_pandoc_inline_block_tag_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    PANDOC_INLINE_BLOCK_TAGS.contains(&lower.as_str())
}

/// Pandoc's void-element subset of `eitherBlockOrInline` (mirrors
/// `pandoc/src/Text/Pandoc/Readers/HTML/TagCategories.hs`'s void list
/// minus those handled elsewhere: `br` and `wbr` are inline-only;
/// `img` and `input` are inline-only; HTML void elements that pandoc
/// classifies as `eitherBlockOrInline` are `area`, `embed`, `source`,
/// `track`).
///
/// At fresh-block positions (or after a blank line) pandoc emits these
/// as a single `RawBlock`; inside a running paragraph they stay inline
/// as `RawInline`. The parser opens a depth-zero HTML block (closes
/// immediately on the open-tag line — there is no closing tag to
/// match) so subsequent lines start fresh blocks; the projector's
/// `split_html_block_by_tags` handles the same-line splitting via
/// `inline_pending`, emitting one `RawBlock` per void-tag instance.
const PANDOC_VOID_BLOCK_TAGS: &[&str] = &["area", "embed", "source", "track"];

/// Whether `name` (case-insensitive) is one of pandoc's void
/// `eitherBlockOrInline` tags (`area`, `embed`, `source`, `track`).
pub fn is_pandoc_void_block_tag_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    PANDOC_VOID_BLOCK_TAGS.contains(&lower.as_str())
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
    /// `depth_aware` extends the matching-tag close path with balanced
    /// open/close tracking of the same tag name (mirrors pandoc's
    /// `htmlInBalanced`); used under Pandoc dialect to handle nested
    /// `<div>...<div>...</div>...</div>` shapes correctly. Ignored when
    /// `closed_by_blank_line` is true.
    /// `closes_at_open_tag` short-circuits the close search: the block
    /// always ends after the open-tag line. Used for void
    /// `eitherBlockOrInline` tags (`<embed>`, `<area>`, `<source>`,
    /// `<track>`) which have no closing tag — depth-aware matching
    /// would walk to end-of-input.
    /// `is_closing` records whether the tag at the start position is a
    /// closing form (`</tag>`) rather than an opening form (`<tag>`).
    /// The dispatcher's `cannot_interrupt` consults this to mirror
    /// pandoc's `isInlineTag` special cases (e.g. `</script>` is inline
    /// even when `<script>` is not — pandoc treats the close-form as
    /// always-inline regardless of attributes).
    BlockTag {
        tag_name: String,
        is_verbatim: bool,
        closed_by_blank_line: bool,
        depth_aware: bool,
        closes_at_open_tag: bool,
        is_closing: bool,
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

    // Try to parse as opening tag (or closing tag, under CommonMark and Pandoc).
    // Pandoc-native recognizes standalone closing forms of strict-block tags
    // (`</p>`, `</nav>`, `</section>`), verbatim tags (`</pre>`, `</style>`,
    // `</script>`, `</textarea>`), and inline-block / void tags (`</video>`,
    // `</button>`, `</embed>`) as single-line `RawBlock`s — they always end on
    // the open-tag line via `closes_at_open_tag: true`.
    if let Some(tag_name) = extract_block_tag_name(trimmed, true) {
        let tag_lower = tag_name.to_lowercase();
        let is_closing = trimmed.starts_with("</");

        // Pandoc dialect: strict-block (`PANDOC_BLOCK_TAGS`) and verbatim
        // (`VERBATIM_TAGS`) closing forms emit as single-line `RawBlock`.
        // Unlike inline-block / void closes, these CAN interrupt a running
        // paragraph (the dispatcher's `cannot_interrupt` only covers the
        // inline-block / void categories). Inline-block / void closes are
        // handled by their own branches further below.
        if !is_commonmark
            && is_closing
            && (PANDOC_BLOCK_TAGS.contains(&tag_lower.as_str())
                || VERBATIM_TAGS.contains(&tag_lower.as_str()))
            && !PANDOC_INLINE_BLOCK_TAGS.contains(&tag_lower.as_str())
            && !PANDOC_VOID_BLOCK_TAGS.contains(&tag_lower.as_str())
        {
            return Some(HtmlBlockType::BlockTag {
                tag_name: tag_lower,
                is_verbatim: false,
                closed_by_blank_line: false,
                depth_aware: false,
                closes_at_open_tag: true,
                is_closing: true,
            });
        }

        // Under Pandoc, remaining closing forms (truly inline-only tags like
        // `</em>`, `</span>`) are not block starts — fall through to the
        // existing inline-html path. Inline-block + void closes are caught
        // by the dedicated branches further below.
        if !is_commonmark
            && is_closing
            && !PANDOC_INLINE_BLOCK_TAGS.contains(&tag_lower.as_str())
            && !PANDOC_VOID_BLOCK_TAGS.contains(&tag_lower.as_str())
        {
            return None;
        }

        // Check if it's a block-level tag. Pandoc and CommonMark disagree on
        // membership: pandoc's `blockHtmlTags` (see
        // `pandoc/src/Text/Pandoc/Readers/HTML/TagCategories.hs`) treats some
        // CM type-6 tags as inline (e.g. `dialog`, `legend`, `option`) and
        // some non-CM tags as block (e.g. `canvas`, `hgroup`, `meta`).
        let is_block_tag = if is_commonmark {
            BLOCK_TAGS.contains(&tag_lower.as_str())
        } else {
            PANDOC_BLOCK_TAGS.contains(&tag_lower.as_str())
        };
        if is_block_tag {
            let is_verbatim = VERBATIM_TAGS.contains(&tag_lower.as_str());
            return Some(HtmlBlockType::BlockTag {
                tag_name: tag_lower,
                is_verbatim,
                closed_by_blank_line: is_commonmark && !is_verbatim,
                depth_aware: !is_commonmark,
                closes_at_open_tag: false,
                is_closing,
            });
        }

        // Pandoc dialect also treats `eitherBlockOrInline` tags as block
        // starters at fresh-block positions. The block dispatcher caller
        // gates these as `cannot_interrupt` (mirrors pandoc — they never
        // interrupt a running paragraph; only start a fresh block when
        // following a blank line or at document start). Closing forms
        // (`</video>`) emit as a single-line `RawBlock` with no balanced
        // match — pandoc-native pins this for standalone closes.
        if !is_commonmark && PANDOC_INLINE_BLOCK_TAGS.contains(&tag_lower.as_str()) {
            return Some(HtmlBlockType::BlockTag {
                tag_name: tag_lower,
                is_verbatim: false,
                closed_by_blank_line: false,
                depth_aware: !is_closing,
                closes_at_open_tag: is_closing,
                is_closing,
            });
        }

        // Pandoc dialect also recognizes the void subset of
        // `eitherBlockOrInline` (`area`, `embed`, `source`, `track`).
        // These have no closing tag, so the parser closes the block
        // immediately on the open-tag line; the projector's
        // `split_html_block_by_tags` handles the same-line splitting
        // (e.g. `<embed src="a"> trailing` → RawBlock + Para). Like
        // non-void inline-block tags, void tags never interrupt a
        // running paragraph (gated as `cannot_interrupt` in the
        // dispatcher). Closing forms (`</embed>`) — semantically
        // nonsensical for void elements — pandoc still emits as a
        // single-line `RawBlock`; mirror that.
        if !is_commonmark && PANDOC_VOID_BLOCK_TAGS.contains(&tag_lower.as_str()) {
            return Some(HtmlBlockType::BlockTag {
                tag_name: tag_lower,
                is_verbatim: false,
                closed_by_blank_line: false,
                depth_aware: false,
                closes_at_open_tag: true,
                is_closing,
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
                depth_aware: !is_commonmark,
                closes_at_open_tag: false,
                is_closing: false,
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

/// Count occurrences of `<tag_name ...>` (open) and `</tag_name>` (close) in
/// `line`. Self-closing forms (`<tag .../>`) and tags whose name appears
/// inside a quoted attribute value are NOT counted — the scanner walks
/// `<...>` brackets and respects `"`/`'` quoting.
///
/// Used by [`parse_html_block_with_wrapper`] to balance nested same-name
/// tags under Pandoc dialect (mirrors pandoc's `htmlInBalanced`).
fn count_tag_balance(line: &str, tag_name: &str) -> (usize, usize) {
    let bytes = line.as_bytes();
    let lower_line = line.to_ascii_lowercase();
    let lower_bytes = lower_line.as_bytes();
    let tag_lower = tag_name.to_ascii_lowercase();
    let tag_bytes = tag_lower.as_bytes();

    let mut opens = 0usize;
    let mut closes = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let after = i + 1;
        let is_close = after < bytes.len() && bytes[after] == b'/';
        let name_start = if is_close { after + 1 } else { after };
        let matched = name_start + tag_bytes.len() <= bytes.len()
            && &lower_bytes[name_start..name_start + tag_bytes.len()] == tag_bytes;
        let after_name = name_start + tag_bytes.len();
        let is_boundary = matched
            && matches!(
                bytes.get(after_name).copied(),
                Some(b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/') | None
            );

        // Walk forward to the closing `>` of this tag bracket, skipping
        // inside quoted attribute values. Self-closing form ends with `/>`.
        let mut j = if matched { after_name } else { after };
        let mut quote: Option<u8> = None;
        let mut self_close = false;
        let mut found_gt = false;
        while j < bytes.len() {
            let b = bytes[j];
            match (quote, b) {
                (Some(q), x) if x == q => quote = None,
                (None, b'"') | (None, b'\'') => quote = Some(b),
                (None, b'>') => {
                    found_gt = true;
                    if j > i + 1 && bytes[j - 1] == b'/' {
                        self_close = true;
                    }
                    break;
                }
                _ => {}
            }
            j += 1;
        }

        if matched && is_boundary {
            if is_close {
                closes += 1;
            } else if !self_close {
                opens += 1;
            }
        }

        if found_gt {
            i = j + 1;
        } else {
            // Unterminated `<...` — bail out to avoid an infinite loop.
            // The remaining bytes don't form a complete tag.
            break;
        }
    }

    (opens, closes)
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
    config: &ParserOptions,
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

    // Detect a multi-line open tag.
    // - `<div>` (Pandoc lift): we tokenize each line structurally so the
    //   salsa anchor walk picks up `id` from the HTML_ATTRS region.
    // - Void block tags (`<embed>`, `<area>`, `<source>`, `<track>`):
    //   without this, the parser closes the block after line 0 and the
    //   remainder of the open tag falls into following paragraphs;
    //   pandoc-native treats the whole multi-line open tag as a single
    //   `RawBlock`. Emission for void/non-div tags uses simple per-line
    //   TEXT + NEWLINE (no HTML_ATTRS — the projector doesn't read attrs
    //   from these tags).
    let multiline_open_end = if bq_depth == 0 {
        match (wrapper_kind, &block_type) {
            (SyntaxKind::HTML_BLOCK_DIV, _) => {
                find_multiline_open_end(lines, start_pos, first_inner, "div")
            }
            (
                _,
                HtmlBlockType::BlockTag {
                    tag_name,
                    closes_at_open_tag: true,
                    ..
                },
            ) => find_multiline_open_end(lines, start_pos, first_inner, tag_name),
            _ => None,
        }
    } else {
        None
    };

    // Set up depth-aware close tracking when the block type asks for it
    // (Pandoc dialect, balanced same-name tag matching). A `None` means
    // we fall back to the legacy "first matching close" path via
    // `is_closing_marker`. Computed up front so the lift-mode gate
    // below can decide whether the open line already balances the
    // block (same-line `<div>...</div>`).
    let depth_aware_tag: Option<String> = match &block_type {
        HtmlBlockType::BlockTag {
            tag_name,
            closed_by_blank_line: false,
            depth_aware: true,
            ..
        } => Some(tag_name.clone()),
        _ => None,
    };
    let mut depth: i64 = 1;
    if let Some(tag_name) = &depth_aware_tag {
        // Sum opens/closes across all open-tag lines (single-line: just
        // line 0; multi-line: lines 0..=end_line_idx).
        let last_open_line = multiline_open_end.unwrap_or(start_pos);
        let mut opens = 0usize;
        let mut closes = 0usize;
        for line in &lines[start_pos..=last_open_line] {
            let inner = if bq_depth > 0 {
                strip_n_blockquote_markers(line, bq_depth)
            } else {
                line
            };
            let (o, c) = count_tag_balance(inner, tag_name);
            opens += o;
            closes += c;
        }
        depth = opens as i64 - closes as i64;
    }

    // Whether this block participates in the Phase 6 structural lift
    // (recursively parse body as Pandoc markdown and graft children).
    // Covers `<div>` outside blockquote context. Disabled when the
    // open line already balances the block (same-line `<div>foo</div>`):
    // for those shapes the parser produces a single `HTML_BLOCK_TAG`
    // with no separate close — the byte-reparse path in the projector
    // still handles them.
    let lift_mode = wrapper_kind == SyntaxKind::HTML_BLOCK_DIV
        && bq_depth == 0
        && !(multiline_open_end.is_none() && depth_aware_tag.is_some() && depth <= 0);

    // Trailing content from the open tag (after `>`). When the lift is
    // active and the open line is `<div ATTRS>foo\n`, this captures
    // `"foo\n"` so it becomes the leading bytes of the recursive-parse
    // input. Stays empty for clean opens (`<div>\n`) and for non-lift
    // shapes (same-line / blockquote-wrapped).
    let mut pre_content = String::new();

    // Emit opening line(s)
    builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());

    if let Some(end_line_idx) = multiline_open_end {
        if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV {
            emit_multiline_div_open_tag(builder, lines, start_pos, end_line_idx);
        } else {
            emit_multiline_open_tag_simple(builder, lines, start_pos, end_line_idx);
        }
    } else {
        let (line_without_newline, newline_str) = strip_newline(first_inner);
        if !line_without_newline.is_empty() {
            // For HTML_BLOCK_DIV, expose the open tag's attributes
            // structurally so `AttributeNode::cast(HTML_ATTRS)` finds them
            // via the same descendants walk that handles fenced-div /
            // heading attrs. CST bytes stay byte-equal to source — we only
            // tokenize at finer granularity for matched div opens.
            if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV {
                let trailing = emit_div_open_tag_tokens(builder, line_without_newline, lift_mode);
                if !trailing.is_empty() {
                    pre_content.push_str(trailing);
                    pre_content.push_str(newline_str);
                }
            } else {
                builder.token(SyntaxKind::TEXT.into(), line_without_newline);
            }
        }
        // When the open tag has trailing content under lift mode, the
        // newline belongs to that trailing line (it terminates the
        // synthetic body line, not the open tag). Don't double-emit.
        if pre_content.is_empty() && !newline_str.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline_str);
        }
    }

    builder.finish_node(); // HtmlBlockTag

    // Check if opening line also contains closing marker. Blank-line-terminated
    // blocks (CommonMark types 6 & 7) ignore inline close markers — they only
    // end at a blank line or end of input. Void `eitherBlockOrInline` tags
    // (`closes_at_open_tag: true`) close immediately — the block always
    // ends on the open-tag line since there is no closing tag to find.
    let void_block = matches!(
        &block_type,
        HtmlBlockType::BlockTag {
            closes_at_open_tag: true,
            ..
        }
    );
    // Void tags with a multi-line open close immediately after the open
    // tag's last line. The HTML_BLOCK_TAG already covers all open-tag
    // lines (`emit_multiline_open_tag_simple` above); pandoc-native emits
    // a single RawBlock for the whole multi-line tag, with no following
    // content.
    if void_block && let Some(end_line_idx) = multiline_open_end {
        log::trace!(
            "HTML void block at line {} closes after multi-line open ending at line {}",
            start_pos + 1,
            end_line_idx + 1
        );
        builder.finish_node(); // HtmlBlock
        return end_line_idx + 1;
    }
    let same_line_closed = !blank_terminated
        && multiline_open_end.is_none()
        && (void_block
            || match &depth_aware_tag {
                Some(_) => depth <= 0,
                None => is_closing_marker(first_inner, &block_type),
            });
    if same_line_closed {
        log::trace!(
            "HTML block at line {} opens and closes on same line",
            start_pos + 1
        );
        builder.finish_node(); // HtmlBlock
        return start_pos + 1;
    }

    let mut current_pos = multiline_open_end
        .map(|end| end + 1)
        .unwrap_or(start_pos + 1);
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

        // Check for closing marker. Under depth-aware mode (Pandoc dialect)
        // count opens/closes of the same tag name and only close when depth
        // returns to 0; otherwise fall back to substring-match on the line.
        let line_closes = match &depth_aware_tag {
            Some(tag_name) => {
                let (opens, closes) = count_tag_balance(inner, tag_name);
                depth += opens as i64;
                depth -= closes as i64;
                depth <= 0
            }
            None => is_closing_marker(inner, &block_type),
        };

        if line_closes {
            log::trace!("Found HTML block closing at line {}", current_pos + 1);
            found_closing = true;

            // Under lift mode, try to split the close line into a
            // leading "body content" prefix and a clean `</div>...`
            // remainder. Lift only when the close line has exactly one
            // `</div>` and no nested `<div>` opens — depth-aware corner
            // cases (e.g. `<inner></inner></div>` on the close line)
            // fall back to the non-lift path. `close_butted` propagates
            // pandoc's `markdown_in_html_blocks` Plain demotion rule:
            // any non-newline byte immediately before `</div>` means
            // the LAST inner block demotes Para→Plain.
            let close_split = if lift_mode {
                try_split_close_line(line, "div")
            } else {
                None
            };

            if let Some((leading, close_part)) = close_split {
                emit_html_block_body_lifted(builder, &pre_content, &content_lines, leading, config);
                builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());
                emit_html_block_line(builder, close_part, 0);
                builder.finish_node();
            } else {
                emit_html_block_body(
                    builder,
                    &pre_content,
                    &content_lines,
                    bq_depth,
                    wrapper_kind,
                    lift_mode,
                    config,
                );
                builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());
                emit_html_block_line(builder, line, bq_depth);
                builder.finish_node();
            }

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
        emit_html_block_body(
            builder,
            &pre_content,
            &content_lines,
            bq_depth,
            wrapper_kind,
            lift_mode,
            config,
        );
    }

    builder.finish_node(); // HtmlBlock
    current_pos
}

/// Emit the collected inner content lines for an HTML block.
///
/// For `HTML_BLOCK_DIV` under Pandoc with `lift_mode == true` (single-
/// line `<div>` open outside blockquote), recursively parse the inner
/// content (including any open-tag trailing) as Pandoc-flavored
/// markdown and graft the resulting top-level blocks as direct children
/// of the wrapper. This is the Phase 6 structural lift — the projector
/// and downstream consumers (linter, salsa, LSP) can walk the
/// structural children instead of re-tokenizing the body bytes.
///
/// All other shapes — opaque `HTML_BLOCK`, `HTML_BLOCK_DIV` inside a
/// blockquote, multi-line open, or no content at all — fall through to
/// the legacy `HTML_BLOCK_CONTENT`-with-TEXT capture.
///
/// CST bytes remain byte-identical to source: the recursive parser is
/// lossless on the same byte slice the legacy path would have captured
/// as TEXT.
fn emit_html_block_body(
    builder: &mut GreenNodeBuilder<'static>,
    pre_content: &str,
    content_lines: &[&str],
    bq_depth: usize,
    wrapper_kind: SyntaxKind,
    lift_mode: bool,
    config: &ParserOptions,
) {
    if pre_content.is_empty() && content_lines.is_empty() {
        return;
    }
    if lift_mode && wrapper_kind == SyntaxKind::HTML_BLOCK_DIV {
        emit_html_block_body_lifted(builder, pre_content, content_lines, "", config);
        return;
    }
    // Legacy path: opaque TEXT capture. `pre_content` is always empty
    // here (lift_mode is the only path that populates it), but be
    // defensive — if a trailing prefix snuck in, emit it as TEXT so
    // bytes are preserved.
    builder.start_node(SyntaxKind::HTML_BLOCK_CONTENT.into());
    if !pre_content.is_empty() {
        builder.token(SyntaxKind::TEXT.into(), pre_content);
    }
    for content_line in content_lines {
        emit_html_block_line(builder, content_line, bq_depth);
    }
    builder.finish_node();
}

/// Lift the `<div>` body into structural CST children: build the inner
/// text from `pre_content` + `content_lines` + `post_content` (in
/// order), recursively parse it as Pandoc-flavored markdown, and graft
/// the resulting top-level blocks into `builder`. When `post_content`
/// is non-empty the close tag is "butted" against text, which under
/// pandoc's `markdown_in_html_blocks` rule demotes the last block in
/// the body Para→Plain; the graft retags the matching `PARAGRAPH` node
/// as `PLAIN` to encode that decision structurally.
fn emit_html_block_body_lifted(
    builder: &mut GreenNodeBuilder<'static>,
    pre_content: &str,
    content_lines: &[&str],
    post_content: &str,
    config: &ParserOptions,
) {
    if pre_content.is_empty() && content_lines.is_empty() && post_content.is_empty() {
        return;
    }
    let mut inner_text = String::with_capacity(
        pre_content.len()
            + content_lines.iter().map(|s| s.len()).sum::<usize>()
            + post_content.len(),
    );
    inner_text.push_str(pre_content);
    for line in content_lines {
        inner_text.push_str(line);
    }
    inner_text.push_str(post_content);

    let close_butted = !post_content.is_empty();

    let mut inner_options = config.clone();
    let refdefs = config.refdef_labels.clone().unwrap_or_default();
    inner_options.refdef_labels = Some(refdefs.clone());
    let inner_root = crate::parser::parse_with_refdefs(&inner_text, Some(inner_options), refdefs);
    graft_document_children(builder, &inner_root, close_butted);
}

/// Walk a parsed inner document's top-level children and re-emit them
/// into `builder`. The document's wrapper node is skipped — only its
/// children are grafted.
///
/// When `demote_last_para` is true, the LAST top-level PARAGRAPH
/// (skipping trailing BLANK_LINEs) is grafted as a PLAIN node instead.
/// This encodes pandoc's `markdown_in_html_blocks` rule that the
/// trailing paragraph inside a `<div>foo</div>`-style butted close
/// demotes from Para to Plain.
fn graft_document_children(
    builder: &mut GreenNodeBuilder<'static>,
    doc: &SyntaxNode,
    demote_last_para: bool,
) {
    let children: Vec<rowan::NodeOrToken<SyntaxNode, _>> = doc.children_with_tokens().collect();

    // Find the last "real block" — the last child node that isn't a
    // BLANK_LINE. Demote it only if it's a PARAGRAPH.
    let mut demote_idx: Option<usize> = None;
    if demote_last_para {
        for (i, c) in children.iter().enumerate().rev() {
            if let rowan::NodeOrToken::Node(n) = c {
                if n.kind() == SyntaxKind::BLANK_LINE {
                    continue;
                }
                if n.kind() == SyntaxKind::PARAGRAPH {
                    demote_idx = Some(i);
                }
                break;
            }
        }
    }

    for (i, child) in children.into_iter().enumerate() {
        match child {
            rowan::NodeOrToken::Node(n) => {
                if Some(i) == demote_idx {
                    graft_subtree_as(builder, &n, SyntaxKind::PLAIN);
                } else {
                    graft_subtree(builder, &n);
                }
            }
            rowan::NodeOrToken::Token(t) => {
                builder.token(t.kind().into(), t.text());
            }
        }
    }
}

/// Recursively re-emit `node` and its descendants into `builder`.
/// Token text is copied verbatim so the result is byte-identical to
/// the input span.
fn graft_subtree(builder: &mut GreenNodeBuilder<'static>, node: &SyntaxNode) {
    graft_subtree_as(builder, node, node.kind());
}

/// Like `graft_subtree` but the outer wrapper's `SyntaxKind` is
/// overridden. Used to retag a top-level `PARAGRAPH` as `PLAIN` for
/// the close-butted demotion rule.
fn graft_subtree_as(builder: &mut GreenNodeBuilder<'static>, node: &SyntaxNode, kind: SyntaxKind) {
    builder.start_node(kind.into());
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n) => graft_subtree(builder, &n),
            rowan::NodeOrToken::Token(t) => {
                builder.token(t.kind().into(), t.text());
            }
        }
    }
    builder.finish_node();
}

/// Try to split the close line of an HTML_BLOCK_DIV body into a
/// leading content prefix and a clean `</tag>...` remainder. Returns
/// `Some((leading, close_part))` only when the line contains exactly
/// one `</tag>` and no `<tag>` opens — the safe shape for the lift.
/// Returns `None` for nested closes (e.g. `<inner></inner></div>`),
/// for missing close tags, or for compound shapes the parser
/// shouldn't attempt to lift in this pass.
///
/// `leading` may be empty (close starts at column 0) or pure
/// whitespace (close on an indented line). Both count as "butted" per
/// pandoc's `markdown_in_html_blocks` rule — if leading is non-empty
/// the trailing paragraph inside the div demotes Para→Plain.
fn try_split_close_line<'a>(line: &'a str, tag_name: &str) -> Option<(&'a str, &'a str)> {
    let (opens, closes) = count_tag_balance(line, tag_name);
    if opens != 0 || closes != 1 {
        return None;
    }
    // Locate the close tag's opening `<` by lowercased substring search.
    // Safe because we've already established (above) that the line has
    // exactly one `</tag>` and no `<tag>` opens, so the first match is
    // THE close.
    let needle = format!("</{}", tag_name);
    let lower = line.to_ascii_lowercase();
    let close_lt = lower.find(&needle)?;
    Some((&line[..close_lt], &line[close_lt..]))
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
///
/// `lift_trailing`: when true, bytes after `>` are NOT emitted as TEXT —
/// returned as `&str` instead so the caller can splice them into the
/// recursive-parse input for the structural body lift. When false
/// (legacy / non-lift path), trailing bytes are emitted as TEXT and an
/// empty slice is returned.
fn emit_div_open_tag_tokens<'a>(
    builder: &mut GreenNodeBuilder<'static>,
    line: &'a str,
    lift_trailing: bool,
) -> &'a str {
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
        return "";
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
        return "";
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

    // Use the original 4 source bytes (preserves source casing — losslessness).
    builder.token(SyntaxKind::TEXT.into(), &rest[..4]);
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
    if lift_trailing {
        // Return trailing bytes to the caller (will be spliced into the
        // recursive-parse input for the body lift).
        return after_gt;
    }
    if !after_gt.is_empty() {
        builder.token(SyntaxKind::TEXT.into(), after_gt);
    }
    ""
}

/// Detect a multi-line HTML open tag for `tag_name`. Returns
/// `Some(end_line_idx)` when the open tag's closing `>` is on a line *after*
/// `start_pos` and within `lines`; `None` for single-line opens (handled by
/// the existing path) or when the `>` is missing entirely.
///
/// Quoted attribute values (`"..."`, `'...'`) are honored so a `>` inside an
/// attribute value doesn't terminate the open tag. Quote state carries
/// across line boundaries.
fn find_multiline_open_end(
    lines: &[&str],
    start_pos: usize,
    first_inner: &str,
    tag_name: &str,
) -> Option<usize> {
    // Locate the `<tag_name` literal in `first_inner` to start scanning past
    // it. Match is ASCII case-insensitive; the parser preserves source casing.
    let trimmed = strip_leading_spaces(first_inner);
    let prefix_len = 1 + tag_name.len();
    if !trimmed.starts_with('<')
        || trimmed.len() < prefix_len
        || !trimmed[1..prefix_len].eq_ignore_ascii_case(tag_name)
    {
        return None;
    }
    let leading_indent = first_inner.len() - trimmed.len();
    let mut i = leading_indent + prefix_len; // past `<tag_name`
    let mut quote: Option<u8> = None;

    // Scan first line for an unquoted `>`.
    let line0_bytes = first_inner.as_bytes();
    while i < line0_bytes.len() {
        match (quote, line0_bytes[i]) {
            (None, b'"') | (None, b'\'') => quote = Some(line0_bytes[i]),
            (Some(q), x) if x == q => quote = None,
            (None, b'>') => return None, // single-line case
            _ => {}
        }
        i += 1;
    }

    // No `>` on first line. Scan subsequent lines.
    let mut line_idx = start_pos + 1;
    while line_idx < lines.len() {
        let bytes = lines[line_idx].as_bytes();
        for &b in bytes {
            match (quote, b) {
                (None, b'"') | (None, b'\'') => quote = Some(b),
                (Some(q), x) if x == q => quote = None,
                (None, b'>') => return Some(line_idx),
                _ => {}
            }
        }
        line_idx += 1;
    }

    None
}

/// Pandoc-only: validate that the HTML open tag starting at `lines[start_pos]`
/// is syntactically complete — i.e. an unquoted `>` exists somewhere from the
/// `<` onward, possibly spanning subsequent lines. Pandoc treats an unclosed
/// open tag (no `>` in the remaining input) as paragraph text rather than
/// starting a `RawBlock`; recognizing it as an HTML block makes the projector
/// reparse the same content recursively, causing a stack overflow.
///
/// Quote state (`"..."` / `'...'`) is threaded across line boundaries so a
/// `>` inside an attribute value doesn't count. Blank lines do not stop the
/// scan — pandoc's `htmlTag` reads across them, just emitting a warning when
/// the tag eventually closes far away.
pub(crate) fn pandoc_html_open_tag_closes(
    lines: &[&str],
    start_pos: usize,
    bq_depth: usize,
) -> bool {
    if start_pos >= lines.len() {
        return false;
    }
    let mut quote: Option<u8> = None;
    for (offset, line) in lines.iter().enumerate().skip(start_pos) {
        let inner = if bq_depth > 0 {
            strip_n_blockquote_markers(line, bq_depth)
        } else {
            line
        };
        let bytes = inner.as_bytes();
        let mut i = 0usize;
        if offset == start_pos {
            while i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
            if bytes.get(i) != Some(&b'<') {
                return false;
            }
            i += 1;
        }
        while i < bytes.len() {
            match (quote, bytes[i]) {
                (None, b'"') | (None, b'\'') => quote = Some(bytes[i]),
                (Some(q), x) if x == q => quote = None,
                (None, b'>') => return true,
                _ => {}
            }
            i += 1;
        }
    }
    false
}

/// Emit a multi-line `<div>` open tag spanning `lines[start_pos..=end_line_idx]`
/// as structural CST tokens. Bytes are byte-identical to the source — only
/// tokenization granularity changes so `AttributeNode::cast(HTML_ATTRS)` finds
/// the attribute region.
///
/// Per-line layout:
/// - Line 0: TEXT("<div") + (optional WHITESPACE + HTML_ATTRS) + NEWLINE
/// - Lines 1..N-1: (optional WHITESPACE indent) + HTML_ATTRS + NEWLINE
/// - Line N (last): (optional WHITESPACE indent) + (HTML_ATTRS + WHITESPACE)?
///   + TEXT(">") + (TEXT(trailing))? + NEWLINE
///
/// Bytes inside HTML_ATTRS may include trailing whitespace before the next
/// newline; `parse_html_attribute_list` tolerates whitespace.
fn emit_multiline_div_open_tag(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    end_line_idx: usize,
) {
    for (line_idx, line) in lines
        .iter()
        .enumerate()
        .take(end_line_idx + 1)
        .skip(start_pos)
    {
        let (line_no_nl, newline_str) = strip_newline(line);

        if line_idx == start_pos {
            // Line 0: leading indent (if any) + "<div" + (whitespace +
            // attrs)?. The closing `>` is on a later line, so any
            // remaining bytes after "<div" on this line are the start of
            // the attribute region.
            let bytes = line_no_nl.as_bytes();
            let indent_end = bytes.iter().position(|&b| b != b' ').unwrap_or(bytes.len());
            if indent_end > 0 {
                builder.token(SyntaxKind::WHITESPACE.into(), &line_no_nl[..indent_end]);
            }
            // Defensive: caller verified the line starts with `<div`.
            let after_indent = &line_no_nl[indent_end..];
            if after_indent.len() >= 4 {
                builder.token(SyntaxKind::TEXT.into(), &after_indent[..4]); // "<div"
                let rest = &after_indent[4..];
                emit_attr_region(builder, rest);
            } else {
                builder.token(SyntaxKind::TEXT.into(), after_indent);
            }
        } else if line_idx < end_line_idx {
            // Pure attribute line.
            let bytes = line_no_nl.as_bytes();
            let indent_end = bytes
                .iter()
                .position(|&b| !matches!(b, b' ' | b'\t'))
                .unwrap_or(bytes.len());
            if indent_end > 0 {
                builder.token(SyntaxKind::WHITESPACE.into(), &line_no_nl[..indent_end]);
            }
            let attrs_text = &line_no_nl[indent_end..];
            if !attrs_text.is_empty() {
                builder.start_node(SyntaxKind::HTML_ATTRS.into());
                builder.token(SyntaxKind::TEXT.into(), attrs_text);
                builder.finish_node();
            }
        } else {
            // Last line: indent + attrs + ">" + trailing.
            let bytes = line_no_nl.as_bytes();
            let indent_end = bytes
                .iter()
                .position(|&b| !matches!(b, b' ' | b'\t'))
                .unwrap_or(bytes.len());
            if indent_end > 0 {
                builder.token(SyntaxKind::WHITESPACE.into(), &line_no_nl[..indent_end]);
            }
            // Find the unquoted `>` byte position in this line.
            let mut quote: Option<u8> = None;
            let mut gt_pos: Option<usize> = None;
            for (j, &b) in line_no_nl.as_bytes()[indent_end..].iter().enumerate() {
                let actual_j = indent_end + j;
                match (quote, b) {
                    (None, b'"') | (None, b'\'') => quote = Some(b),
                    (Some(q), x) if x == q => quote = None,
                    (None, b'>') => {
                        gt_pos = Some(actual_j);
                        break;
                    }
                    _ => {}
                }
            }
            let Some(gt) = gt_pos else {
                // Defensive — caller said `>` is on this line.
                builder.token(SyntaxKind::TEXT.into(), &line_no_nl[indent_end..]);
                if !newline_str.is_empty() {
                    builder.token(SyntaxKind::NEWLINE.into(), newline_str);
                }
                continue;
            };
            // Attribute region: between indent_end and gt, with possibly
            // trailing whitespace before `>`.
            let attrs_region = &line_no_nl[indent_end..gt];
            let region_bytes = attrs_region.as_bytes();
            // Strip trailing whitespace from attrs region; emit as
            // separate WHITESPACE so HTML_ATTRS only contains attribute
            // bytes.
            let mut attr_end = region_bytes.len();
            while attr_end > 0 && matches!(region_bytes[attr_end - 1], b' ' | b'\t') {
                attr_end -= 1;
            }
            let attrs_text = &attrs_region[..attr_end];
            let trailing_ws = &attrs_region[attr_end..];
            if !attrs_text.is_empty() {
                builder.start_node(SyntaxKind::HTML_ATTRS.into());
                builder.token(SyntaxKind::TEXT.into(), attrs_text);
                builder.finish_node();
            }
            if !trailing_ws.is_empty() {
                builder.token(SyntaxKind::WHITESPACE.into(), trailing_ws);
            }
            builder.token(SyntaxKind::TEXT.into(), ">");
            let after_gt = &line_no_nl[gt + 1..];
            if !after_gt.is_empty() {
                builder.token(SyntaxKind::TEXT.into(), after_gt);
            }
        }

        if !newline_str.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline_str);
        }
    }
}

/// Emit a multi-line HTML open tag spanning `lines[start_pos..=end_line_idx]`
/// for non-`<div>` tags (void tags `<embed>`/`<area>`/`<source>`/`<track>`).
/// Each line is emitted as plain TEXT + NEWLINE; no `HTML_ATTRS` structural
/// node is added. Pandoc's projector reads attributes only for `<div>` /
/// `<span>` lifts, so non-div multi-line opens just need byte preservation.
fn emit_multiline_open_tag_simple(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    end_line_idx: usize,
) {
    for line in lines.iter().take(end_line_idx + 1).skip(start_pos) {
        let (line_no_nl, newline_str) = strip_newline(line);
        if !line_no_nl.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), line_no_nl);
        }
        if !newline_str.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline_str);
        }
    }
}

/// Emit the trailing portion of `<div`'s line 0 — i.e. anything after the
/// `<div` literal up to end-of-line. Called only from
/// `emit_multiline_div_open_tag`. The `>` is on a later line, so this is
/// pure attribute (and possibly inter-attribute whitespace).
fn emit_attr_region(builder: &mut GreenNodeBuilder<'static>, region: &str) {
    if region.is_empty() {
        return;
    }
    let bytes = region.as_bytes();
    // Split a leading run of whitespace into a WHITESPACE token so the
    // HTML_ATTRS node holds only attribute bytes.
    let ws_end = bytes
        .iter()
        .position(|&b| !matches!(b, b' ' | b'\t'))
        .unwrap_or(bytes.len());
    if ws_end > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &region[..ws_end]);
    }
    let attrs_text = &region[ws_end..];
    if !attrs_text.is_empty() {
        builder.start_node(SyntaxKind::HTML_ATTRS.into());
        builder.token(SyntaxKind::TEXT.into(), attrs_text);
        builder.finish_node();
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
                depth_aware: true,
                closes_at_open_tag: false,
                is_closing: false,
            })
        );
        assert_eq!(
            try_parse_html_block_start("<div class=\"test\">", false),
            Some(HtmlBlockType::BlockTag {
                tag_name: "div".to_string(),
                is_verbatim: false,
                closed_by_blank_line: false,
                depth_aware: true,
                closes_at_open_tag: false,
                is_closing: false,
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
                depth_aware: true,
                closes_at_open_tag: false,
                is_closing: false,
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
    fn test_dialect_specific_block_tag_membership() {
        // Pandoc-markdown's `blockHtmlTags` is a strict subset of
        // CommonMark §4.6 type-6 plus a few additions. These tags
        // diverge between dialects:
        //   CM-only block tags (Pandoc treats as inline raw HTML):
        //     dialog, legend, menuitem, optgroup, option, frame,
        //     base, basefont, link, param
        //   Pandoc-only block tags (CM doesn't recognize):
        //     canvas, hgroup, isindex, meta, output
        for cm_only in [
            "<dialog>",
            "<legend>",
            "<menuitem>",
            "<optgroup>",
            "<option>",
            "<frame>",
            "<base>",
            "<basefont>",
            "<link>",
            "<param>",
        ] {
            assert!(
                matches!(
                    try_parse_html_block_start(cm_only, true),
                    Some(HtmlBlockType::BlockTag { .. })
                ),
                "{cm_only} should be a block-tag start under CommonMark",
            );
            assert_eq!(
                try_parse_html_block_start(cm_only, false),
                None,
                "{cm_only} should NOT be a block-tag start under Pandoc",
            );
        }
        for pandoc_only in ["<canvas>", "<hgroup>", "<isindex>", "<meta>", "<output>"] {
            // Under CM these are not type-6 BlockTags; they may still match
            // type-7 (complete tag on a line) which has different semantics.
            assert!(
                !matches!(
                    try_parse_html_block_start(pandoc_only, true),
                    Some(HtmlBlockType::BlockTag { .. })
                ),
                "{pandoc_only} should NOT be a type-6 block-tag start under CommonMark",
            );
            assert!(
                matches!(
                    try_parse_html_block_start(pandoc_only, false),
                    Some(HtmlBlockType::BlockTag { .. })
                ),
                "{pandoc_only} should be a block-tag start under Pandoc",
            );
        }
    }

    #[test]
    fn test_pandoc_inline_block_tag_membership() {
        // Pandoc's `eitherBlockOrInline` tags start an HTML block at
        // fresh-block positions under Pandoc dialect. We list the
        // non-void, non-script subset (verbatim `script` is handled
        // via the verbatim path; void elements are deferred — see
        // PANDOC_INLINE_BLOCK_TAGS docs).
        for tag in [
            "<button>",
            "<iframe>",
            "<video>",
            "<audio>",
            "<noscript>",
            "<object>",
            "<map>",
            "<progress>",
            "<del>",
            "<ins>",
            "<svg>",
            "<applet>",
        ] {
            assert!(
                matches!(
                    try_parse_html_block_start(tag, false),
                    Some(HtmlBlockType::BlockTag {
                        depth_aware: true,
                        ..
                    })
                ),
                "{tag} should be a depth-aware block-tag start under Pandoc",
            );
        }
        // Closing forms of inline-block tags also start a block under
        // Pandoc — pandoc-native pins `</button>` standalone as a
        // single-line `RawBlock`. These use `closes_at_open_tag: true`
        // (no balanced match — the close emits as a one-line block on
        // its own).
        for closing in ["</button>", "</iframe>", "</video>", "</audio>"] {
            assert!(
                matches!(
                    try_parse_html_block_start(closing, false),
                    Some(HtmlBlockType::BlockTag {
                        depth_aware: false,
                        closes_at_open_tag: true,
                        ..
                    })
                ),
                "{closing} (closing form) should be a single-line block-tag start under Pandoc",
            );
        }
    }

    #[test]
    fn test_pandoc_void_block_tag_membership() {
        // Pandoc's void `eitherBlockOrInline` tags start an HTML block
        // at fresh-block positions under Pandoc dialect, with
        // `closes_at_open_tag: true` — the block always ends on the
        // open-tag line (no closing tag to match).
        for tag in [
            "<area>",
            "<embed>",
            "<source>",
            "<track>",
            "<embed src=\"foo.swf\">",
            "<source src=\"foo.mp4\" type=\"video/mp4\">",
        ] {
            assert!(
                matches!(
                    try_parse_html_block_start(tag, false),
                    Some(HtmlBlockType::BlockTag {
                        depth_aware: false,
                        closes_at_open_tag: true,
                        ..
                    })
                ),
                "{tag} should be a void block-tag start under Pandoc",
            );
        }
        // Closing forms of void tags also start a single-line block
        // under Pandoc. Void elements have no closing tag in HTML, but
        // `</embed>` etc. can appear in the wild — pandoc-native still
        // emits them as `RawBlock`s at fresh-block positions; mirror
        // that with the same `closes_at_open_tag: true` shape.
        for closing in ["</area>", "</embed>", "</source>", "</track>"] {
            assert!(
                matches!(
                    try_parse_html_block_start(closing, false),
                    Some(HtmlBlockType::BlockTag {
                        depth_aware: false,
                        closes_at_open_tag: true,
                        ..
                    })
                ),
                "{closing} (closing form) should be a single-line void block-tag start under Pandoc",
            );
        }
        // Under CommonMark dialect, the void-tag block-start path is
        // skipped. `<source>` and `<track>` are in the CM type-6
        // BLOCK_TAGS set so they DO start a block, but with CM type-6
        // semantics (`closed_by_blank_line: true`,
        // `closes_at_open_tag: false`), not the Pandoc void-tag path.
        // `<embed>` and `<area>` aren't in the CM type-6 list — they
        // fall through to type 7 (complete tag on a line by itself).
        assert_eq!(
            try_parse_html_block_start("<embed>", true),
            Some(HtmlBlockType::Type7)
        );
        assert_eq!(
            try_parse_html_block_start("<area>", true),
            Some(HtmlBlockType::Type7)
        );
        assert!(matches!(
            try_parse_html_block_start("<source src=\"x\">", true),
            Some(HtmlBlockType::BlockTag {
                closed_by_blank_line: true,
                closes_at_open_tag: false,
                ..
            })
        ));
        assert!(matches!(
            try_parse_html_block_start("<track src=\"x\">", true),
            Some(HtmlBlockType::BlockTag {
                closed_by_blank_line: true,
                closes_at_open_tag: false,
                ..
            })
        ));
    }

    #[test]
    fn test_find_multiline_open_end() {
        // Single-line opens return None (caller takes the regular path).
        assert_eq!(
            find_multiline_open_end(&["<div id=\"x\">"], 0, "<div id=\"x\">", "div"),
            None
        );
        assert_eq!(
            find_multiline_open_end(&["<embed src=\"x\">"], 0, "<embed src=\"x\">", "embed"),
            None
        );
        // Multi-line opens return the line index of the closing `>`.
        assert_eq!(
            find_multiline_open_end(&["<embed", "  src=\"x\">"], 0, "<embed", "embed"),
            Some(1)
        );
        assert_eq!(
            find_multiline_open_end(
                &["<embed", "  src=\"x\"", "  type=\"video\">"],
                0,
                "<embed",
                "embed"
            ),
            Some(2)
        );
        // Tag-name mismatch returns None (case-insensitive on the tag name).
        assert_eq!(
            find_multiline_open_end(&["<embed", "  src=\"x\">"], 0, "<embed", "div"),
            None
        );
        assert_eq!(
            find_multiline_open_end(&["<EMBED", "  src=\"x\">"], 0, "<EMBED", "embed"),
            Some(1)
        );
        // Quoted `>` does not terminate the open tag; quote state threads
        // across line boundaries.
        assert_eq!(
            find_multiline_open_end(
                &["<embed title=\"a>b", "  c\">"],
                0,
                "<embed title=\"a>b",
                "embed"
            ),
            Some(1)
        );
        // No `>` anywhere returns None.
        assert_eq!(
            find_multiline_open_end(&["<embed", "  src=\"x\""], 0, "<embed", "embed"),
            None
        );
    }

    #[test]
    fn test_pandoc_html_open_tag_closes() {
        // Single-line complete: scanner finds `>` on the first line.
        assert!(pandoc_html_open_tag_closes(&["<div>"], 0, 0));
        assert!(pandoc_html_open_tag_closes(&["<embed src=\"x\">"], 0, 0));
        // Multi-line complete: scanner finds `>` on a later line.
        assert!(pandoc_html_open_tag_closes(
            &["<div", "  id=\"x\">", "body", "</div>"],
            0,
            0
        ));
        assert!(pandoc_html_open_tag_closes(
            &["<embed", "  src=\"x.png\" alt=\"y\">"],
            0,
            0
        ));
        // Quoted `>` does not close: scanner threads quote state.
        assert!(!pandoc_html_open_tag_closes(
            &["<div title=\"a>b", "  c\""],
            0,
            0
        ));
        assert!(pandoc_html_open_tag_closes(
            &["<div title=\"a>b", "  c\">"],
            0,
            0
        ));
        // Incomplete: no `>` anywhere — pandoc treats as paragraph text.
        assert!(!pandoc_html_open_tag_closes(&["<embed"], 0, 0));
        assert!(!pandoc_html_open_tag_closes(&["<div", "foo", "bar"], 0, 0));
        // Pandoc tolerates blank lines mid-open-tag (its `htmlTag` reads
        // across them); the scan continues until EOF or `>`.
        assert!(pandoc_html_open_tag_closes(
            &["<div", "", "id=\"x\">"],
            0,
            0
        ));
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
                depth_aware: false,
                closes_at_open_tag: false,
                is_closing: true,
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
            depth_aware: false,
            closes_at_open_tag: false,
            is_closing: false,
        };
        assert!(is_closing_marker("</div>", &block_type));
        assert!(is_closing_marker("</DIV>", &block_type)); // Case insensitive
        assert!(is_closing_marker("content</div>", &block_type));
        assert!(!is_closing_marker("<div>", &block_type));
    }

    #[test]
    fn test_parse_html_comment_block() {
        let input = "<!-- comment -->\n";
        let lines: Vec<&str> = crate::parser::utils::helpers::split_lines_inclusive(input);
        let mut builder = GreenNodeBuilder::new();

        let block_type = try_parse_html_block_start(lines[0], false).unwrap();
        let opts = ParserOptions::default();
        let new_pos = parse_html_block_with_wrapper(
            &mut builder,
            &lines,
            0,
            block_type,
            0,
            SyntaxKind::HTML_BLOCK,
            &opts,
        );

        assert_eq!(new_pos, 1);
    }

    #[test]
    fn test_parse_div_block() {
        let input = "<div>\ncontent\n</div>\n";
        let lines: Vec<&str> = crate::parser::utils::helpers::split_lines_inclusive(input);
        let mut builder = GreenNodeBuilder::new();

        let block_type = try_parse_html_block_start(lines[0], false).unwrap();
        let opts = ParserOptions::default();
        let new_pos = parse_html_block_with_wrapper(
            &mut builder,
            &lines,
            0,
            block_type,
            0,
            SyntaxKind::HTML_BLOCK,
            &opts,
        );

        assert_eq!(new_pos, 3);
    }

    #[test]
    fn test_parse_html_block_no_closing() {
        let input = "<div>\ncontent\n";
        let lines: Vec<&str> = crate::parser::utils::helpers::split_lines_inclusive(input);
        let mut builder = GreenNodeBuilder::new();

        let block_type = try_parse_html_block_start(lines[0], false).unwrap();
        let opts = ParserOptions::default();
        let new_pos = parse_html_block_with_wrapper(
            &mut builder,
            &lines,
            0,
            block_type,
            0,
            SyntaxKind::HTML_BLOCK,
            &opts,
        );

        // Should consume all lines even without closing tag
        assert_eq!(new_pos, 2);
    }

    #[test]
    fn test_parse_div_block_nested_pandoc() {
        // Pandoc dialect: a nested `<div>...<div>...</div>...</div>` must
        // close on the OUTER `</div>`, not the first `</div>` seen. The
        // CommonMark-style "first close" scanner is wrong here; Pandoc's
        // div parser is depth-aware (mirrors `htmlInBalanced`).
        let input =
            "<div id=\"outer\">\n\n<div id=\"inner\">\n\ndeep content\n\n</div>\n\n</div>\n";
        let lines: Vec<&str> = crate::parser::utils::helpers::split_lines_inclusive(input);
        let mut builder = GreenNodeBuilder::new();

        // is_commonmark = false → Pandoc dialect.
        let block_type = try_parse_html_block_start(lines[0], false).unwrap();
        let opts = ParserOptions::default();
        let new_pos = parse_html_block_with_wrapper(
            &mut builder,
            &lines,
            0,
            block_type,
            0,
            SyntaxKind::HTML_BLOCK_DIV,
            &opts,
        );

        // 9 lines: outer-open, blank, inner-open, blank, content, blank,
        // inner-close, blank, outer-close. All consumed.
        assert_eq!(new_pos, 9);
    }

    #[test]
    fn test_parse_div_block_same_line_pandoc() {
        // <div>foo</div> on a single line: opens=1, closes=1, depth=0 →
        // close on first line. Depth-aware tracking must not regress this.
        let input = "<div>foo</div>\n";
        let lines: Vec<&str> = crate::parser::utils::helpers::split_lines_inclusive(input);
        let mut builder = GreenNodeBuilder::new();

        let block_type = try_parse_html_block_start(lines[0], false).unwrap();
        let opts = ParserOptions::default();
        let new_pos = parse_html_block_with_wrapper(
            &mut builder,
            &lines,
            0,
            block_type,
            0,
            SyntaxKind::HTML_BLOCK_DIV,
            &opts,
        );
        assert_eq!(new_pos, 1);
    }

    #[test]
    fn test_commonmark_verbatim_first_close() {
        // CommonMark verbatim tag (`<script>`): per CommonMark §4.6 type-1,
        // ends at the first matching close — not depth-aware. Stash a
        // bogus inner `<script>` inside a JS string; the outer block
        // still closes at the first `</script>`.
        let input = "<script>\nlet x = '<script>';\n</script>\n";
        let lines: Vec<&str> = crate::parser::utils::helpers::split_lines_inclusive(input);
        let mut builder = GreenNodeBuilder::new();

        // is_commonmark = true.
        let block_type = try_parse_html_block_start(lines[0], true).unwrap();
        let opts = ParserOptions::default();
        let new_pos = parse_html_block_with_wrapper(
            &mut builder,
            &lines,
            0,
            block_type,
            0,
            SyntaxKind::HTML_BLOCK,
            &opts,
        );
        // Three lines, closed at first `</script>` (line 2). new_pos = 3.
        assert_eq!(new_pos, 3);
    }

    #[test]
    fn test_parse_div_block_multiline_open_close_separate_line_pandoc() {
        // Multi-line open tag with the closing `>` on its own line:
        //
        //   <div
        //     id="x"
        //     class="y"
        //   >
        //
        //   foo
        //
        //   </div>
        //
        // Open tag spans lines 0..=3. Content starts at line 4.
        let input = "<div\n  id=\"x\"\n  class=\"y\"\n>\n\nfoo\n\n</div>\n";
        let lines: Vec<&str> = crate::parser::utils::helpers::split_lines_inclusive(input);
        let mut builder = GreenNodeBuilder::new();

        let block_type = try_parse_html_block_start(lines[0], false).unwrap();
        let opts = ParserOptions::default();
        let new_pos = parse_html_block_with_wrapper(
            &mut builder,
            &lines,
            0,
            block_type,
            0,
            SyntaxKind::HTML_BLOCK_DIV,
            &opts,
        );

        // 8 lines: open-line 0, open-line 1 (`  id="x"`), open-line 2
        // (`  class="y"`), open-line 3 (`>`), blank, foo, blank, </div>.
        assert_eq!(new_pos, 8);

        // CST must contain a structural HTML_ATTRS region holding the
        // attribute bytes (so the salsa anchor walk picks up `id="x"`).
        let green = builder.finish();
        let root = crate::syntax::SyntaxNode::new_root(green);
        let attrs_count = root
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::HTML_ATTRS)
            .count();
        assert!(attrs_count >= 1, "expected at least one HTML_ATTRS node");

        // Byte-identical losslessness check.
        let collected: String = root
            .descendants_with_tokens()
            .filter_map(|n| n.into_token())
            .map(|t| t.text().to_string())
            .collect();
        assert_eq!(collected, input);
    }

    #[test]
    fn test_parse_div_block_multiline_open_close_inline_pandoc() {
        // Multi-line open tag with the closing `>` on the last attribute
        // line (case 0262 already covers this pattern; pin behavior to
        // also ensure HTML_ATTRS structural exposure).
        let input = "<div\n  id=\"x\"\n  class=\"y\">\nfoo\n</div>\n";
        let lines: Vec<&str> = crate::parser::utils::helpers::split_lines_inclusive(input);
        let mut builder = GreenNodeBuilder::new();

        let block_type = try_parse_html_block_start(lines[0], false).unwrap();
        let opts = ParserOptions::default();
        let new_pos = parse_html_block_with_wrapper(
            &mut builder,
            &lines,
            0,
            block_type,
            0,
            SyntaxKind::HTML_BLOCK_DIV,
            &opts,
        );

        // 5 lines: open-line 0, open-line 1, open-line 2 (with `>`), foo,
        // </div>.
        assert_eq!(new_pos, 5);

        let green = builder.finish();
        let root = crate::syntax::SyntaxNode::new_root(green);
        let attrs_count = root
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::HTML_ATTRS)
            .count();
        assert!(attrs_count >= 1, "expected at least one HTML_ATTRS node");

        let collected: String = root
            .descendants_with_tokens()
            .filter_map(|n| n.into_token())
            .map(|t| t.text().to_string())
            .collect();
        assert_eq!(collected, input);
    }

    #[test]
    fn test_commonmark_type6_blank_line_terminates() {
        let input = "<div>\nfoo\n\nbar\n";
        let lines: Vec<&str> = crate::parser::utils::helpers::split_lines_inclusive(input);
        let mut builder = GreenNodeBuilder::new();

        let block_type = try_parse_html_block_start(lines[0], true).unwrap();
        let opts = ParserOptions::default();
        let new_pos = parse_html_block_with_wrapper(
            &mut builder,
            &lines,
            0,
            block_type,
            0,
            SyntaxKind::HTML_BLOCK,
            &opts,
        );

        // Block contains <div>\nfoo\n; stops at blank line (line 2).
        assert_eq!(new_pos, 2);
    }
}
