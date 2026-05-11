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

/// Whether the given tag name is eligible for the Phase 6 / Fix #4
/// structural body lift inside an `HTML_BLOCK` wrapper: it's a Pandoc
/// block-level tag (strict-block from `PANDOC_BLOCK_TAGS` OR non-void
/// inline-block from `PANDOC_INLINE_BLOCK_TAGS`) that is NOT verbatim
/// and NOT void. These are the tags where pandoc parses the body as
/// fresh markdown between RawBlock emissions of the open/close tags —
/// exactly the shape we can lift into structural CST children.
///
/// Inline-block tags (`<video>`, `<iframe>`, `<button>`, …) have an
/// additional gate at the lift-gate site: the lift is abandoned when
/// the body's first non-blank content is a void block tag at a
/// fresh-block position (`<video>\n<source ...>\n</video>` projects
/// per-tag rather than matched-pair, mirroring pandoc).
///
/// `<div>` is intentionally excluded — it has its own lift path
/// (`HTML_BLOCK_DIV` wrapper retag) with different demotion rules
/// (Plain/Para keyed on `close_butted`, not on trailing blank line).
fn is_pandoc_lift_eligible_block_tag(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if VERBATIM_TAGS.contains(&lower.as_str()) {
        return false;
    }
    if PANDOC_VOID_BLOCK_TAGS.contains(&lower.as_str()) {
        return false;
    }
    if lower == "div" {
        return false;
    }
    PANDOC_BLOCK_TAGS.contains(&lower.as_str())
        || PANDOC_INLINE_BLOCK_TAGS.contains(&lower.as_str())
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
    // - Pandoc strict-block tags eligible for the Fix #4 lift (`<form>`,
    //   `<section>`, `<header>`, …): same structural emission, exposing
    //   `id` to the salsa anchor walk and enabling the body lift below.
    // - Void block tags (`<embed>`, `<area>`, `<source>`, `<track>`):
    //   without this, the parser closes the block after line 0 and the
    //   remainder of the open tag falls into following paragraphs;
    //   pandoc-native treats the whole multi-line open tag as a single
    //   `RawBlock`. Emission for void tags uses simple per-line
    //   TEXT + NEWLINE (no HTML_ATTRS — the projector doesn't read attrs
    //   from void tags).
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
            (
                _,
                HtmlBlockType::BlockTag {
                    tag_name,
                    is_verbatim: false,
                    closed_by_blank_line: false,
                    depth_aware: true,
                    closes_at_open_tag: false,
                    is_closing: false,
                },
            ) if is_pandoc_lift_eligible_block_tag(tag_name) => {
                find_multiline_open_end(lines, start_pos, first_inner, tag_name)
            }
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

    // Same-line `<div>foo</div>` shape: the open line balances the
    // block under depth-aware tracking. We can lift this structurally
    // only when the open-tag trailing has exactly one `</div>` close,
    // zero `<div>` opens, and no non-whitespace content after the
    // close. Other same-line shapes (nested, trailing text, malformed)
    // fall through to the byte-reparse path.
    let is_same_line_div = wrapper_kind == SyntaxKind::HTML_BLOCK_DIV
        && multiline_open_end.is_none()
        && depth_aware_tag.is_some()
        && depth <= 0;
    let same_line_div_lift_safe = is_same_line_div && bq_depth == 0 && {
        let (line_without_newline, _) = strip_newline(first_inner);
        probe_same_line_lift(line_without_newline, "div")
    };

    // Strict-block-tag Fix #4 lift (`<form>`, `<section>`, `<header>`,
    // `<nav>`, …): the body parses as fresh markdown between RawBlock
    // emissions of the open/close tags. Covers the clean multi-line
    // shape (open tag stands alone on its line), open-trailing
    // (`<form>foo\n…\n</form>`), butted-close (`<form>\n…\nfoo</form>`),
    // and same-line (`<form>foo</form>`). Multi-line open and
    // blockquote-wrapped non-div shapes still fall through to the
    // byte-walker path.
    let strict_block_tag_name: Option<&str> =
        if wrapper_kind == SyntaxKind::HTML_BLOCK && bq_depth == 0 {
            match &block_type {
                HtmlBlockType::BlockTag {
                    tag_name,
                    is_verbatim: false,
                    closed_by_blank_line: false,
                    depth_aware: true,
                    closes_at_open_tag: false,
                    is_closing: false,
                } if is_pandoc_lift_eligible_block_tag(tag_name) => Some(tag_name.as_str()),
                _ => None,
            }
        } else {
            None
        };
    // Same-line `<form>foo</form>` shape: the open line already
    // balances the block (`depth <= 0`). Lift only when the trailing
    // bytes after the open `>` end with `</tag>` and contain exactly
    // one close + zero nested opens.
    let same_line_strict_lift_safe = strict_block_tag_name.is_some_and(|name| {
        multiline_open_end.is_none() && depth <= 0 && {
            let (line_no_nl, _) = strip_newline(first_inner);
            probe_same_line_lift(line_no_nl, name)
        }
    });
    // Strict-block lift gate: accept (a) a multi-line open tag spanning
    // `lines[start_pos..=multiline_open_end]`, or (b) a clean / open-
    // trailing single-line open (depth > 0, open `>` is present with
    // quote-aware matching), or (c) a safe same-line shape. For
    // inline-block matched-pair tags (`<video>`, `<iframe>`, `<button>`,
    // …) the lift additionally abandons when the body starts at a
    // fresh-block position with a void block tag — pandoc-native pins
    // per-tag emission rather than a matched-pair lift in that case.
    let strict_block_lift = strict_block_tag_name.is_some_and(|name| {
        let (line_no_nl, _) = strip_newline(first_inner);
        let shape_ok = if multiline_open_end.is_some() {
            // `find_multiline_open_end` already verified the open tag
            // closes with a quote-aware `>` somewhere in lines
            // `start_pos+1..=end`. No same-line trailing content to
            // probe; defer trailing-on-close-`>`-line handling to a
            // future session (rare in practice).
            true
        } else if depth > 0 {
            probe_open_tag_line_has_close_gt(line_no_nl, name)
        } else {
            same_line_strict_lift_safe
        };
        if !shape_ok {
            return false;
        }
        if !is_pandoc_inline_block_tag_name(name) {
            return true;
        }
        !inline_block_void_interior_abandons(
            first_inner,
            lines,
            start_pos,
            multiline_open_end,
            bq_depth,
            name,
        )
    });

    // Whether this block participates in the Phase 6 structural lift
    // (recursively parse body as Pandoc markdown and graft children).
    // Covers `<div>` outside blockquote context. For same-line shapes
    // the lift is gated on `same_line_*_lift_safe` — when unsafe we
    // keep the legacy single-HTML_BLOCK_TAG shape and let the
    // byte-reparse path handle projection.
    let lift_mode = (wrapper_kind == SyntaxKind::HTML_BLOCK_DIV
        && bq_depth == 0
        && (!is_same_line_div || same_line_div_lift_safe))
        || strict_block_lift;

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
            emit_multiline_open_tag_with_attrs(builder, lines, start_pos, end_line_idx, "div");
        } else if let Some(name) = strict_block_tag_name
            && strict_block_lift
        {
            emit_multiline_open_tag_with_attrs(builder, lines, start_pos, end_line_idx, name);
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
                let trailing =
                    emit_open_tag_tokens(builder, line_without_newline, "div", lift_mode);
                if !trailing.is_empty() {
                    pre_content.push_str(trailing);
                    pre_content.push_str(newline_str);
                }
            } else if let Some(name) = strict_block_tag_name
                && strict_block_lift
            {
                let trailing = emit_open_tag_tokens(builder, line_without_newline, name, lift_mode);
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
        // Same-line structural lift (div or non-div strict-block):
        // pre_content holds the bytes after the open `>` (including
        // the close `</tag>` and the trailing newline). Split into
        // body + close tag, emit body via recursive parse, emit close
        // tag as a sibling `HTML_BLOCK_TAG`.
        let same_line_lift_tag: Option<&str> = if !lift_mode || pre_content.is_empty() {
            None
        } else if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV && same_line_div_lift_safe {
            Some("div")
        } else if same_line_strict_lift_safe {
            strict_block_tag_name
        } else {
            None
        };
        if let Some(tag_name) = same_line_lift_tag {
            let (pre_no_nl, post_nl) = strip_newline(&pre_content);
            if let Some((leading, close_part)) = try_split_close_line(pre_no_nl, tag_name) {
                // Same-line is always close-butted; div demotes the
                // trailing Para→Plain via `SkipTrailingBlanks`.
                // Non-div strict-block uses `OnlyIfLast` (consistent
                // with butted-close — no trailing BLANK_LINE before
                // the close means the trailing Para demotes).
                let policy = if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV {
                    LastParaDemote::SkipTrailingBlanks
                } else {
                    LastParaDemote::OnlyIfLast
                };
                emit_html_block_body_lifted(builder, "", &[], leading, policy, config);
                builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());
                let mut close_line = String::with_capacity(close_part.len() + post_nl.len());
                close_line.push_str(close_part);
                close_line.push_str(post_nl);
                emit_html_block_line(builder, &close_line, 0);
                builder.finish_node();
                builder.finish_node(); // HtmlBlock
                return start_pos + 1;
            }
        }
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

            // Pandoc-dialect blockquote-wrapped clean-shape lift: when
            // the open and close tags stand alone on their source lines
            // (no trailing on open, no body content on close after
            // stripping bq markers), lift the body lines structurally
            // so the projector walks CST children instead of
            // byte-reparsing via `collect_html_block_text_skip_bq_markers`.
            //
            // Covers `<div>` (HTML_BLOCK_DIV → Block::Div with body
            // grafted, Para preserved), non-div strict-block tags
            // (`<form>`, `<section>`, …) and inline-block matched-pair
            // tags (`<video>`, `<iframe>`, …) — the latter two under
            // HTML_BLOCK with the structural lift hitting pandoc's
            // RawBlock + Plain + RawBlock shape via `OnlyIfLast`
            // demotion. Inline-block additionally bails if the body
            // starts at a fresh-block position with a void block tag
            // (mirrors the non-bq matched-pair gate).
            //
            // Other bq-wrapped shapes (butted-close / open-trailing /
            // same-line) still fall through to the opaque path.
            let bq_lift_tag: Option<&str> = if bq_depth > 0
                && multiline_open_end.is_none()
                && pre_content.is_empty()
            {
                if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV {
                    Some("div")
                } else if wrapper_kind == SyntaxKind::HTML_BLOCK {
                    match &block_type {
                        HtmlBlockType::BlockTag {
                            tag_name,
                            is_verbatim: false,
                            closed_by_blank_line: false,
                            depth_aware: true,
                            closes_at_open_tag: false,
                            is_closing: false,
                        } if is_pandoc_lift_eligible_block_tag(tag_name) => Some(tag_name.as_str()),
                        _ => None,
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let bq_clean_lift = bq_lift_tag.is_some_and(|tag_name| {
                let (open_no_nl, _) = strip_newline(first_inner);
                if !open_no_nl.trim_end_matches([' ', '\t']).ends_with('>') {
                    return false;
                }
                let close_stripped = strip_n_blockquote_markers(line, bq_depth);
                let (close_no_nl, _) = strip_newline(close_stripped);
                if !close_no_nl
                    .trim_start_matches([' ', '\t'])
                    .starts_with("</")
                {
                    return false;
                }
                if is_pandoc_inline_block_tag_name(tag_name)
                    && inline_block_void_interior_abandons(
                        first_inner,
                        lines,
                        start_pos,
                        multiline_open_end,
                        bq_depth,
                        tag_name,
                    )
                {
                    return false;
                }
                true
            });

            if bq_clean_lift {
                let demote_policy = if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV {
                    LastParaDemote::Never
                } else {
                    LastParaDemote::OnlyIfLast
                };
                emit_html_block_body_lifted_bq(
                    builder,
                    &content_lines,
                    bq_depth,
                    demote_policy,
                    config,
                );
                builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());
                emit_html_block_line(builder, line, bq_depth);
                builder.finish_node();
                current_pos += 1;
                break;
            }

            // Under lift mode, try to split the close line into a
            // leading "body content" prefix and a clean `</tag>...`
            // remainder. Lift only when the close line has exactly one
            // `</tag>` and no nested `<tag>` opens — depth-aware corner
            // cases (e.g. `<inner></inner></tag>` on the close line)
            // fall back to the non-lift path. For `<div>`, non-empty
            // `leading` propagates pandoc's `markdown_in_html_blocks`
            // Plain demotion rule. For non-div strict-block tags,
            // demotion follows pandoc's `OnlyIfLast` rule (demote the
            // trailing Para only when no blank line precedes the close).
            let close_split_tag = if lift_mode {
                if strict_block_lift {
                    strict_block_tag_name
                } else if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV {
                    Some("div")
                } else {
                    None
                }
            } else {
                None
            };
            let close_split = close_split_tag.and_then(|name| try_split_close_line(line, name));

            if let Some((leading, close_part)) = close_split {
                let policy = if strict_block_lift {
                    LastParaDemote::OnlyIfLast
                } else if !leading.is_empty() {
                    LastParaDemote::SkipTrailingBlanks
                } else {
                    LastParaDemote::Never
                };
                emit_html_block_body_lifted(
                    builder,
                    &pre_content,
                    &content_lines,
                    leading,
                    policy,
                    config,
                );
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
        // Reached when the parser walked to end-of-input without finding
        // `</div>` (unbalanced div) — no close tag, no Plain demotion.
        emit_html_block_body_lifted(
            builder,
            pre_content,
            content_lines,
            "",
            LastParaDemote::Never,
            config,
        );
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

/// Rule for promoting the trailing `PARAGRAPH` of an HTML-block body
/// to `PLAIN` when grafting children into the structural CST.
#[derive(Copy, Clone, Debug)]
enum LastParaDemote {
    /// Never demote — pandoc preserves the trailing `Para`.
    Never,
    /// Demote the LAST `PARAGRAPH` child, skipping any trailing
    /// `BLANK_LINE` children. Used for `<div>` shapes where the close
    /// tag is butted against the paragraph text on its source line —
    /// pandoc's `markdown_in_html_blocks` Plain demotion.
    SkipTrailingBlanks,
    /// Demote the LAST top-level child only when it is a `PARAGRAPH`
    /// (i.e. no trailing `BLANK_LINE` precedes the close tag). Used
    /// for non-div strict-block tags whose body emits at top-level
    /// adjacent to the close-tag `RawBlock`; pandoc's rule there
    /// demotes the trailing `Para` to `Plain` unless a blank line
    /// separates them.
    OnlyIfLast,
}

/// Lift the HTML-block body into structural CST children: build the
/// inner text from `pre_content` + `content_lines` + `post_content`
/// (in order), recursively parse it as Pandoc-flavored markdown, and
/// graft the resulting top-level blocks into `builder`. `demote_policy`
/// controls whether the trailing paragraph is retagged as `PLAIN` to
/// encode pandoc's Plain/Para adjacency rules structurally.
fn emit_html_block_body_lifted(
    builder: &mut GreenNodeBuilder<'static>,
    pre_content: &str,
    content_lines: &[&str],
    post_content: &str,
    demote_policy: LastParaDemote,
    config: &ParserOptions,
) {
    emit_html_block_body_lifted_inner(
        builder,
        pre_content,
        content_lines,
        post_content,
        demote_policy,
        config,
        &mut None,
    )
}

/// Body-lift variant for `<div>` inside a blockquote. Strips
/// `bq_depth` levels of blockquote markers from each `content_line`,
/// captures the per-line prefix bytes, and grafts the recursive parse
/// with prefix injection so the output CST stays byte-equal to the
/// source. `pre_content` and `post_content` must be empty (the bq
/// clean lift only handles the shape where the open and close tags
/// stand alone on their source lines).
fn emit_html_block_body_lifted_bq(
    builder: &mut GreenNodeBuilder<'static>,
    content_lines: &[&str],
    bq_depth: usize,
    demote_policy: LastParaDemote,
    config: &ParserOptions,
) {
    let mut prefixes: Vec<String> = Vec::with_capacity(content_lines.len());
    let mut stripped_lines: Vec<&str> = Vec::with_capacity(content_lines.len());
    for cl in content_lines {
        let stripped = strip_n_blockquote_markers(cl, bq_depth);
        let prefix_len = cl.len() - stripped.len();
        prefixes.push(cl[..prefix_len].to_string());
        stripped_lines.push(stripped);
    }
    let mut bq = Some(BqPrefixState {
        prefixes,
        line_idx: 0,
        at_line_start: true,
    });
    emit_html_block_body_lifted_inner(
        builder,
        "",
        &stripped_lines,
        "",
        demote_policy,
        config,
        &mut bq,
    )
}

fn emit_html_block_body_lifted_inner(
    builder: &mut GreenNodeBuilder<'static>,
    pre_content: &str,
    content_lines: &[&str],
    post_content: &str,
    demote_policy: LastParaDemote,
    config: &ParserOptions,
    bq: &mut Option<BqPrefixState>,
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

    let mut inner_options = config.clone();
    let refdefs = config.refdef_labels.clone().unwrap_or_default();
    inner_options.refdef_labels = Some(refdefs.clone());
    let inner_root = crate::parser::parse_with_refdefs(&inner_text, Some(inner_options), refdefs);
    graft_document_children(builder, &inner_root, demote_policy, bq);
}

/// Per-line blockquote-prefix injection state used by the graft helpers
/// when the lifted body originated inside a `> …` blockquote: the
/// recursive parse was fed the bq-stripped text, so the prefix bytes
/// (`BLOCK_QUOTE_MARKER` + `WHITESPACE`) must be re-emitted at the
/// start of each source line to keep the CST byte-equal to the source.
///
/// `prefixes[i]` is the literal prefix bytes for source line `i` of the
/// body (e.g. `"> "`, `">  "`, or `">"`). `line_idx` is the index of
/// the next prefix to emit; `at_line_start` flips to `true` after every
/// `NEWLINE` so the next token triggers prefix emission.
struct BqPrefixState {
    prefixes: Vec<String>,
    line_idx: usize,
    at_line_start: bool,
}

/// Walk a parsed inner document's top-level children and re-emit them
/// into `builder`. The document's wrapper node is skipped — only its
/// children are grafted.
///
/// `demote_policy` controls whether a trailing `PARAGRAPH` is retagged
/// as `PLAIN` — see [`LastParaDemote`].
///
/// `bq` is `Some` when grafting a body that lived inside a blockquote
/// — token emission then injects `BLOCK_QUOTE_MARKER + WHITESPACE`
/// prefix tokens at line starts. See [`BqPrefixState`].
fn graft_document_children(
    builder: &mut GreenNodeBuilder<'static>,
    doc: &SyntaxNode,
    demote_policy: LastParaDemote,
    bq: &mut Option<BqPrefixState>,
) {
    let children: Vec<rowan::NodeOrToken<SyntaxNode, _>> = doc.children_with_tokens().collect();

    let mut demote_idx: Option<usize> = None;
    match demote_policy {
        LastParaDemote::Never => {}
        LastParaDemote::SkipTrailingBlanks => {
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
        LastParaDemote::OnlyIfLast => {
            for (i, c) in children.iter().enumerate().rev() {
                if let rowan::NodeOrToken::Node(n) = c {
                    if n.kind() == SyntaxKind::PARAGRAPH {
                        demote_idx = Some(i);
                    }
                    break;
                }
            }
        }
    }

    for (i, child) in children.into_iter().enumerate() {
        match child {
            rowan::NodeOrToken::Node(n) => {
                if Some(i) == demote_idx {
                    graft_subtree_as(builder, &n, SyntaxKind::PLAIN, bq);
                } else {
                    graft_subtree(builder, &n, bq);
                }
            }
            rowan::NodeOrToken::Token(t) => {
                emit_grafted_token(builder, t.kind(), t.text(), bq);
            }
        }
    }
}

/// Recursively re-emit `node` and its descendants into `builder`.
/// Token text is copied verbatim so the result is byte-identical to
/// the input span (modulo bq prefix tokens injected at line starts
/// when `bq` is `Some`).
fn graft_subtree(
    builder: &mut GreenNodeBuilder<'static>,
    node: &SyntaxNode,
    bq: &mut Option<BqPrefixState>,
) {
    graft_subtree_as(builder, node, node.kind(), bq);
}

/// Like `graft_subtree` but the outer wrapper's `SyntaxKind` is
/// overridden. Used to retag a top-level `PARAGRAPH` as `PLAIN` for
/// the close-butted demotion rule.
fn graft_subtree_as(
    builder: &mut GreenNodeBuilder<'static>,
    node: &SyntaxNode,
    kind: SyntaxKind,
    bq: &mut Option<BqPrefixState>,
) {
    builder.start_node(kind.into());
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n) => graft_subtree(builder, &n, bq),
            rowan::NodeOrToken::Token(t) => {
                emit_grafted_token(builder, t.kind(), t.text(), bq);
            }
        }
    }
    builder.finish_node();
}

/// Emit a single token while optionally injecting blockquote prefix
/// tokens at line starts. When `bq` is `None`, this is a plain
/// `builder.token()` passthrough.
fn emit_grafted_token(
    builder: &mut GreenNodeBuilder<'static>,
    kind: SyntaxKind,
    text: &str,
    bq: &mut Option<BqPrefixState>,
) {
    if let Some(state) = bq.as_mut() {
        if state.at_line_start {
            if let Some(prefix) = state.prefixes.get(state.line_idx) {
                emit_bq_prefix_tokens(builder, prefix);
            }
            state.at_line_start = false;
        }
        builder.token(kind.into(), text);
        // `BLANK_LINE` token represents an entirely blank source line —
        // its text is `\n`. Treat both `NEWLINE` and the `BLANK_LINE`
        // token as line-ending so the per-line prefix index advances
        // correctly.
        if kind == SyntaxKind::NEWLINE || kind == SyntaxKind::BLANK_LINE {
            state.line_idx += 1;
            state.at_line_start = true;
        }
    } else {
        builder.token(kind.into(), text);
    }
}

/// Emit a captured per-line bq prefix as a stream of `BLOCK_QUOTE_MARKER`
/// (`>`) and `WHITESPACE` (everything else, byte-by-byte) tokens.
fn emit_bq_prefix_tokens(builder: &mut GreenNodeBuilder<'static>, prefix: &str) {
    for ch in prefix.chars() {
        if ch == '>' {
            builder.token(SyntaxKind::BLOCK_QUOTE_MARKER.into(), ">");
        } else {
            let mut buf = [0u8; 4];
            builder.token(SyntaxKind::WHITESPACE.into(), ch.encode_utf8(&mut buf));
        }
    }
}

/// Locate the byte index (within `line`) of the open-tag's closing `>`
/// after a quote-aware scan of `<tag_name ATTRS>`. Returns `None` when
/// the line doesn't fit the expected shape. Mirrors the inner scan of
/// `probe_open_tag_line_has_close_gt` but exposes the position so the
/// caller can slice off the trailing bytes.
fn locate_open_tag_close_gt(line: &str, tag_name: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let indent_end = bytes
        .iter()
        .position(|&b| b != b' ' && b != b'\t')
        .unwrap_or(bytes.len());
    let rest = &line[indent_end..];
    let rest_bytes = rest.as_bytes();
    let prefix_len = 1 + tag_name.len();
    if rest_bytes.len() < prefix_len + 1
        || rest_bytes[0] != b'<'
        || !rest_bytes[1..prefix_len].eq_ignore_ascii_case(tag_name.as_bytes())
    {
        return None;
    }
    let after_name = &rest[prefix_len..];
    let after_name_bytes = after_name.as_bytes();
    let mut i = 0usize;
    let mut quote: Option<u8> = None;
    while i < after_name_bytes.len() {
        match (quote, after_name_bytes[i]) {
            (None, b'"') | (None, b'\'') => quote = Some(after_name_bytes[i]),
            (Some(q), b2) if b2 == q => quote = None,
            (None, b'>') => return Some(indent_end + prefix_len + i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// Whether `slice` begins (after leading ASCII whitespace) with an
/// open tag whose name is a Pandoc void block tag (`<source>`,
/// `<embed>`, `<area>`, `<track>`). Close tags (`</...>`) and non-void
/// open tags return false.
///
/// Used by the inline-block matched-pair lift gate: pandoc-native
/// abandons the lift when the body's first non-blank content is a
/// fresh-block void tag (e.g. `<video>\n<source ...>\n</video>`
/// projects as RawBlock+RawBlock+Plain[..,RawInline</video>], not a
/// matched-pair lift).
fn slice_starts_with_void_block_tag(slice: &str) -> bool {
    let trimmed = slice.trim_start_matches([' ', '\t', '\n', '\r']);
    if !trimmed.starts_with('<') || trimmed.starts_with("</") {
        return false;
    }
    let Some(tag_end) = parse_open_tag(trimmed) else {
        return false;
    };
    let bytes = trimmed.as_bytes();
    let mut name_end = 1usize;
    while name_end < tag_end && (bytes[name_end].is_ascii_alphanumeric() || bytes[name_end] == b'-')
    {
        name_end += 1;
    }
    if name_end == 1 {
        return false;
    }
    is_pandoc_void_block_tag_name(&trimmed[1..name_end])
}

/// Whether the body of an inline-block matched-pair (`<video>...`,
/// `<iframe>...`, `<button>...`) begins at a fresh-block position with
/// a void block tag — the condition under which pandoc-native abandons
/// the matched-pair lift. Probes three shapes:
///
/// - **Same-line** (`<video><source ...></video>`): trailing bytes
///   after the open `>` on `first_inner` start with `<source`.
/// - **Single-line open + multi-line body**: open-trailing on the open
///   line is empty/whitespace AND the first non-blank body line
///   (`lines[start_pos+1..]`) starts with a void tag.
/// - **Multi-line open**: same body-line scan starting at
///   `lines[multiline_open_end+1..]`.
///
/// Returns `false` when the body begins with text, with a close tag,
/// or with a non-void block tag — those cases all proceed with the
/// matched-pair lift.
fn inline_block_void_interior_abandons(
    first_inner: &str,
    lines: &[&str],
    start_pos: usize,
    multiline_open_end: Option<usize>,
    bq_depth: usize,
    tag_name: &str,
) -> bool {
    let (line_no_nl, _) = strip_newline(first_inner);
    let (body_start_line_idx, open_trailing) = match multiline_open_end {
        Some(end) => (end + 1, ""),
        None => {
            let gt = locate_open_tag_close_gt(line_no_nl, tag_name);
            let trailing = gt.map(|i| &line_no_nl[i + 1..]).unwrap_or("");
            (start_pos + 1, trailing)
        }
    };
    let trimmed = open_trailing.trim_start_matches([' ', '\t']);
    if !trimmed.is_empty() {
        return slice_starts_with_void_block_tag(trimmed);
    }
    for line in &lines[body_start_line_idx..] {
        let inner = if bq_depth > 0 {
            strip_n_blockquote_markers(line, bq_depth)
        } else {
            line
        };
        let trimmed = inner.trim_start_matches([' ', '\t', '\n', '\r']);
        if trimmed.is_empty() {
            continue;
        }
        return slice_starts_with_void_block_tag(trimmed);
    }
    false
}

/// Probe whether the open-tag line has a valid (quote-aware) closing
/// `>` after the tag name. Admits trailing content after `>` (the
/// open-trailing shape `<form>foo`) — the caller is expected to capture
/// that trailing into the structural lift's `pre_content`.
fn probe_open_tag_line_has_close_gt(line: &str, tag_name: &str) -> bool {
    let bytes = line.as_bytes();
    let indent_end = bytes
        .iter()
        .position(|&b| b != b' ' && b != b'\t')
        .unwrap_or(bytes.len());
    let rest = &line[indent_end..];
    let rest_bytes = rest.as_bytes();
    let prefix_len = 1 + tag_name.len();
    if rest_bytes.len() < prefix_len + 1
        || rest_bytes[0] != b'<'
        || !rest_bytes[1..prefix_len].eq_ignore_ascii_case(tag_name.as_bytes())
    {
        return false;
    }
    let after_name = &rest[prefix_len..];
    let after_name_bytes = after_name.as_bytes();
    let mut i = 0usize;
    let mut quote: Option<u8> = None;
    while i < after_name_bytes.len() {
        match (quote, after_name_bytes[i]) {
            (None, b'"') | (None, b'\'') => quote = Some(after_name_bytes[i]),
            (Some(q), b2) if b2 == q => quote = None,
            (None, b'>') => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

/// Probe whether the same-line `<tag>BODY</tag>` shape on `line` can
/// be lifted structurally. Returns `true` only when:
/// - The line starts with `<tag_name` (modulo leading whitespace).
/// - The open tag's `>` exists with proper quote handling.
/// - The bytes after the open `>` end with `</tag_name>` (case-
///   insensitive, allowing trailing whitespace).
/// - The trailing has exactly one `</tag_name>` close and zero
///   `<tag_name>` opens (rejects nested same-line shapes).
///
/// Trailing non-whitespace content after `</tag_name>` (e.g.
/// `<form>foo</form>extra`) rejects the lift — pandoc projects that
/// shape as RawBlock + content + RawBlock + trailing-Para, which the
/// byte walker handles via `split_html_block_by_tags`.
fn probe_same_line_lift(line: &str, tag_name: &str) -> bool {
    let bytes = line.as_bytes();
    let indent_end = bytes
        .iter()
        .position(|&b| b != b' ' && b != b'\t')
        .unwrap_or(bytes.len());
    let rest = &line[indent_end..];
    let rest_bytes = rest.as_bytes();
    let prefix_len = 1 + tag_name.len();
    if rest_bytes.len() < prefix_len
        || rest_bytes[0] != b'<'
        || !rest_bytes[1..prefix_len].eq_ignore_ascii_case(tag_name.as_bytes())
    {
        return false;
    }
    let after_name = &rest[prefix_len..];
    let after_name_bytes = after_name.as_bytes();
    let mut i = 0usize;
    let mut quote: Option<u8> = None;
    let mut gt_idx: Option<usize> = None;
    while i < after_name_bytes.len() {
        match (quote, after_name_bytes[i]) {
            (None, b'"') | (None, b'\'') => quote = Some(after_name_bytes[i]),
            (Some(q), b2) if b2 == q => quote = None,
            (None, b'>') => {
                gt_idx = Some(i);
                break;
            }
            _ => {}
        }
        i += 1;
    }
    let Some(gt_idx) = gt_idx else {
        return false;
    };
    let trailing = &after_name[gt_idx + 1..];
    let trimmed = trailing.trim_end_matches([' ', '\t']);
    let close_marker = format!("</{}>", tag_name);
    if !trimmed
        .to_ascii_lowercase()
        .ends_with(&close_marker.to_ascii_lowercase())
    {
        return false;
    }
    let (opens, closes) = count_tag_balance(trailing, tag_name);
    opens == 0 && closes == 1
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

/// Emit the open-tag line of a lift-eligible HTML block (div or non-div
/// strict-block tag), splitting the bytes `[ws]<tag[ ws ATTRS]>[trailing]`
/// into `WHITESPACE? + TEXT("<tag") + (WHITESPACE + HTML_ATTRS{TEXT(attrs)})?
/// + TEXT(">") + TEXT(trailing)?`.
///
/// Bytes are byte-identical to the source — this only tokenizes at finer
/// granularity so `AttributeNode::cast(HTML_ATTRS)` can read the attribute
/// region structurally. Falls back to a single TEXT token if the line
/// doesn't fit the expected `<tag ...>` shape (defensive — the parser
/// only retags as the lift kind when this shape was matched).
///
/// `lift_trailing`: when true, bytes after `>` are NOT emitted as TEXT —
/// returned as `&str` instead so the caller can splice them into the
/// recursive-parse input for the structural body lift. When false
/// (legacy / non-lift path), trailing bytes are emitted as TEXT and an
/// empty slice is returned.
fn emit_open_tag_tokens<'a>(
    builder: &mut GreenNodeBuilder<'static>,
    line: &'a str,
    tag_name: &str,
    lift_trailing: bool,
) -> &'a str {
    let bytes = line.as_bytes();
    // Leading indent (CommonMark allows up to 3 spaces).
    let indent_end = bytes.iter().position(|&b| b != b' ').unwrap_or(bytes.len());
    if indent_end > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &line[..indent_end]);
    }
    let rest = &line[indent_end..];
    // Match the literal `<tag_name` prefix (ASCII case-insensitive on the tag name).
    let prefix_len = 1 + tag_name.len();
    if !rest.starts_with('<')
        || rest.len() < prefix_len
        || !rest.as_bytes()[1..prefix_len].eq_ignore_ascii_case(tag_name.as_bytes())
    {
        builder.token(SyntaxKind::TEXT.into(), rest);
        return "";
    }
    let after_name = &rest[prefix_len..];
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

    // Use the original source bytes for the `<tag` prefix (preserves
    // source casing — losslessness).
    builder.token(SyntaxKind::TEXT.into(), &rest[..prefix_len]);
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

/// Emit a multi-line open tag spanning `lines[start_pos..=end_line_idx]` as
/// structural CST tokens, exposing the attribute region as `HTML_ATTRS` for
/// `AttributeNode::cast` to find. Bytes are byte-identical to the source —
/// only tokenization granularity changes. Used for `<div>` (Pandoc dialect)
/// and non-div strict-block tags (`<form>`, `<section>`, …) under the
/// Phase 6 structural lift.
///
/// Per-line layout (with `prefix_len = 1 + tag_name.len()`):
/// - Line 0: TEXT("<{tag_name}") + (optional WHITESPACE + HTML_ATTRS) + NEWLINE
/// - Lines 1..N-1: (optional WHITESPACE indent) + HTML_ATTRS + NEWLINE
/// - Line N (last): (optional WHITESPACE indent) + (HTML_ATTRS + WHITESPACE)?
///   + TEXT(">") + (TEXT(trailing))? + NEWLINE
///
/// Bytes inside HTML_ATTRS may include trailing whitespace before the next
/// newline; `parse_html_attribute_list` tolerates whitespace.
fn emit_multiline_open_tag_with_attrs(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    end_line_idx: usize,
    tag_name: &str,
) {
    let prefix_len = 1 + tag_name.len();
    for (line_idx, line) in lines
        .iter()
        .enumerate()
        .take(end_line_idx + 1)
        .skip(start_pos)
    {
        let (line_no_nl, newline_str) = strip_newline(line);

        if line_idx == start_pos {
            // Line 0: leading indent (if any) + "<{tag_name}" + (whitespace
            // + attrs)?. The closing `>` is on a later line, so any
            // remaining bytes after "<{tag_name}" on this line are the
            // start of the attribute region.
            let bytes = line_no_nl.as_bytes();
            let indent_end = bytes.iter().position(|&b| b != b' ').unwrap_or(bytes.len());
            if indent_end > 0 {
                builder.token(SyntaxKind::WHITESPACE.into(), &line_no_nl[..indent_end]);
            }
            // Defensive: caller verified the line starts with `<{tag_name}`.
            let after_indent = &line_no_nl[indent_end..];
            if after_indent.len() >= prefix_len {
                builder.token(SyntaxKind::TEXT.into(), &after_indent[..prefix_len]);
                let rest = &after_indent[prefix_len..];
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
/// `emit_multiline_open_tag_with_attrs`. The `>` is on a later line, so this is
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
