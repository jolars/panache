//! HTML block parsing utilities.

use crate::options::ParserOptions;
use crate::parser::inlines::inline_html::{parse_close_tag, parse_open_tag};
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

use super::blockquotes::{count_blockquote_markers, strip_n_blockquote_markers};
use super::container_prefix::{
    ContainerPrefix, ContainerPrefixLine, ContainerPrefixState, emit_container_prefix_tokens,
};
use crate::parser::utils::attributes::emit_html_attrs_node;
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

/// Pandoc's *strict-block* void elements: HTML void tags (`col`, `hr`,
/// `meta`) that live in [`PANDOC_BLOCK_TAGS`] (so they always split and
/// DO interrupt a running paragraph, unlike the `eitherBlockOrInline`
/// void set in [`PANDOC_VOID_BLOCK_TAGS`]) yet have no closing form.
///
/// Because they are void, the parser must close the block on the
/// open-tag line (`closes_at_open_tag: true`, `depth_aware: false`);
/// otherwise a bare `<hr>` would open a depth-aware block that swallows
/// the following lines as a matched-pair body lift (the pre-7c quirk
/// where `<hr>\n<hr>\n<hr>` nested the trailing tags as children rather
/// than emitting three sibling `RawBlock`s). They stay OUT of the
/// dispatcher's `cannot_interrupt` set, so `foo\n<hr>` still splits into
/// `Plain [foo]` + `RawBlock "<hr>"`, matching pandoc-native.
const PANDOC_VOID_STRICT_BLOCK_TAGS: &[&str] = &["col", "hr", "meta"];

/// Whether `name` (case-insensitive) is a Pandoc strict-block void tag
/// (`col`, `hr`, `meta`) — see [`PANDOC_VOID_STRICT_BLOCK_TAGS`].
pub(crate) fn is_pandoc_void_strict_block_tag_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    PANDOC_VOID_STRICT_BLOCK_TAGS.contains(&lower.as_str())
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
pub(crate) fn is_pandoc_lift_eligible_block_tag(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if VERBATIM_TAGS.contains(&lower.as_str()) {
        return false;
    }
    if PANDOC_VOID_BLOCK_TAGS.contains(&lower.as_str())
        || PANDOC_VOID_STRICT_BLOCK_TAGS.contains(&lower.as_str())
    {
        return false;
    }
    if lower == "div" {
        return false;
    }
    PANDOC_BLOCK_TAGS.contains(&lower.as_str())
        || PANDOC_INLINE_BLOCK_TAGS.contains(&lower.as_str())
}

/// Whether `name` (case-insensitive) is a Pandoc matched-pair block tag
/// — anything that has an opening and a matching closing form whose
/// `</tag>` would be recognized by the dispatcher as a separate block
/// start. Covers strict-block tags (incl. `<div>`), inline-block tags,
/// and verbatim tags (`<pre>`, `<style>`, `<script>`, `<textarea>`).
/// Void tags are excluded — they have no close form.
///
/// Used by `ListItemBuffer::unclosed_pandoc_matched_pair_tag` to detect
/// an open inside the buffer whose close would otherwise interrupt the
/// list item mid-construct.
pub(crate) fn is_pandoc_matched_pair_tag(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if PANDOC_VOID_BLOCK_TAGS.contains(&lower.as_str())
        || PANDOC_VOID_STRICT_BLOCK_TAGS.contains(&lower.as_str())
    {
        return false;
    }
    PANDOC_BLOCK_TAGS.contains(&lower.as_str())
        || PANDOC_INLINE_BLOCK_TAGS.contains(&lower.as_str())
        || VERBATIM_TAGS.contains(&lower.as_str())
}

/// Open-tag-attribute tokenization gate for non-div strict-block tags
/// inside a blockquote (`bq_depth > 0`). Returns the tag name when the
/// open tag is eligible for finer-grained tokenization
/// (`TEXT("<tag") + WS + HTML_ATTRS{TEXT(attrs)} + TEXT(">")`) without
/// driving the full body lift — that's the `bq_clean_lift` path. The
/// HTML_ATTRS region lets `AttributeNode::cast` register any `id` with
/// the salsa anchor index.
///
/// `<div>` is handled by its own structural path (`HTML_BLOCK_DIV`
/// wrapper) regardless of bq depth, so this gate skips it.
fn bq_strict_attr_emit_tag_name(
    wrapper_kind: SyntaxKind,
    block_type: &HtmlBlockType,
    bq_depth: usize,
) -> Option<&str> {
    if bq_depth == 0 || wrapper_kind != SyntaxKind::HTML_BLOCK {
        return None;
    }
    match block_type {
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
            // Void strict-block tags (`col`, `hr`, `meta`) have no
            // closing form, so under Pandoc the block must close on the
            // open-tag line — otherwise a bare `<hr>` opens a depth-aware
            // block and the matched-pair body lift swallows the following
            // lines (`<hr>\n<hr>` nesting the second as a child instead of
            // a sibling `RawBlock`). CommonMark keeps the type-6 shape
            // (block continues until a blank line).
            let is_void_strict = !is_commonmark && is_pandoc_void_strict_block_tag_name(&tag_lower);
            return Some(HtmlBlockType::BlockTag {
                tag_name: tag_lower,
                is_verbatim,
                closed_by_blank_line: is_commonmark && !is_verbatim,
                depth_aware: !is_commonmark && !is_void_strict,
                closes_at_open_tag: is_void_strict,
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
/// tags under Pandoc dialect (mirrors pandoc's `htmlInBalanced`), and by
/// `ListItemBuffer::unclosed_pandoc_matched_pair_tag` to suppress the
/// close-form dispatch that would otherwise break the list-item buffer
/// mid-`<div>...</div>`.
pub(crate) fn count_tag_balance(line: &str, tag_name: &str) -> (usize, usize) {
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

/// Pandoc-dialect lift for HTML comments / processing instructions
/// whose close marker is followed by additional bytes (same-line
/// trailing or following lines). Pandoc-native emits a `RawBlock` for
/// the marker bytes only, then parses the remainder as fresh blocks.
///
/// Returns `Some(consumed_lines)` when the split fires (caller must
/// NOT enter the legacy emission); `None` to fall back to the legacy
/// path (no close marker found, or no trailing content to split).
///
/// CST shape on success:
/// ```text
/// HTML_BLOCK
///   HTML_BLOCK_TAG (open)        // line[0] up to and incl close marker
///     TEXT  "<!-- hi -->"        // or with HTML_BLOCK_CONTENT in between
///     ...                        // for multi-line `<!--\n…\n-->` shape
/// <sibling blocks>               // recursive parse of trailing + lines[M+1..]
/// ```
/// The CST node kind to emit for an opaque single-construct HTML block.
/// Under `Dialect::Pandoc`, comments, processing instructions, and
/// verbatim raw-text elements (`<pre>`/`<script>`/`<style>`/`<textarea>`)
/// each project to exactly one `RawBlock "html"`; tagging the wrapper
/// `HTML_BLOCK_RAW` lets the pandoc-native projector route by kind instead
/// of re-sniffing the leading bytes. This changes only the wrapper `u16` —
/// the child tokens are emitted byte-for-byte identically, so the CST stays
/// lossless (the `HTML_BLOCK_DIV` precedent). The behavioral `wrapper_kind`
/// stays `HTML_BLOCK` everywhere else in `parse_html_block_with_wrapper`, so
/// no lift gate changes. CommonMark dialect keeps the opaque `HTML_BLOCK`
/// shape.
fn html_block_node_kind(
    wrapper_kind: SyntaxKind,
    block_type: &HtmlBlockType,
    dialect: crate::options::Dialect,
) -> SyntaxKind {
    if wrapper_kind == SyntaxKind::HTML_BLOCK
        && dialect == crate::options::Dialect::Pandoc
        && matches!(
            block_type,
            HtmlBlockType::Comment
                | HtmlBlockType::ProcessingInstruction
                | HtmlBlockType::BlockTag {
                    is_verbatim: true,
                    ..
                }
        )
    {
        SyntaxKind::HTML_BLOCK_RAW
    } else {
        wrapper_kind
    }
}

#[allow(clippy::too_many_arguments)]
fn try_parse_comment_pi_with_trailing_split(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    block_type: &HtmlBlockType,
    wrapper_kind: SyntaxKind,
    bq_depth: usize,
    at_outermost_level: bool,
    config: &ParserOptions,
) -> Option<usize> {
    let marker: &str = match block_type {
        HtmlBlockType::Comment => "-->",
        HtmlBlockType::ProcessingInstruction => "?>",
        _ => return None,
    };

    // Find the close marker in the bq-stripped line content. For
    // bq_depth == 0 the inner content equals the raw line; for
    // bq_depth > 0 we look past the `>` markers stripped by the
    // outer dispatcher (line 0) and emitted as bq prefix below
    // (lines > 0). `marker_end_in_inner` is the byte offset of the
    // first byte AFTER the close marker, measured from the start
    // of the inner (post-strip) content.
    let mut close_line_idx: Option<usize> = None;
    let mut marker_end_in_inner: usize = 0;
    for (offset, line) in lines[start_pos..].iter().enumerate() {
        let inner = if bq_depth > 0 {
            strip_n_blockquote_markers(line, bq_depth)
        } else {
            line
        };
        if let Some(pos) = inner.find(marker) {
            close_line_idx = Some(start_pos + offset);
            marker_end_in_inner = pos + marker.len();
            break;
        }
    }
    let close_line_idx = close_line_idx?;
    let close_line = lines[close_line_idx];
    let close_inner = if bq_depth > 0 {
        strip_n_blockquote_markers(close_line, bq_depth)
    } else {
        close_line
    };
    let close_prefix_len = close_line.len() - close_inner.len();
    let trailing = &close_inner[marker_end_in_inner..];

    // Only fire when there is non-whitespace content AFTER the close
    // marker on the close line. The legacy path correctly handles
    // the close-line-ends-at-close-marker shapes (`-->\n` followed
    // by separate blocks); only the same-line-trailing case needs
    // structural splitting. Trailing-whitespace-only handling
    // (`-->   \n`) is a projector-side trim — separate concern.
    let has_non_ws_trailing = trailing.bytes().any(|b| !b.is_ascii_whitespace());
    if !has_non_ws_trailing {
        return None;
    }

    builder.start_node(html_block_node_kind(wrapper_kind, block_type, config.dialect).into());

    // Emit open `HTML_BLOCK_TAG` (the opening marker line(s)) and any
    // middle `HTML_BLOCK_CONTENT` lines between open and close. The
    // close `HTML_BLOCK_TAG` carries only the bytes up to and
    // including the close marker — trailing bytes go to the sibling.
    if close_line_idx == start_pos {
        // Same-line shape: one HTML_BLOCK_TAG containing the close
        // marker's bytes. The newline lives on the trailing sibling.
        // Line 0's bq prefix (if any) was already emitted by the
        // outer dispatcher; emit only the inner marker bytes.
        builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());
        let close_part = &close_inner[..marker_end_in_inner];
        if !close_part.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), close_part);
        }
        builder.finish_node();
    } else {
        // Multi-line shape: open tag covers lines[start_pos..close],
        // middle lines go inside HTML_BLOCK_CONTENT, close tag holds
        // only the marker bytes. Line 0's bq prefix was emitted by
        // the outer dispatcher; subsequent lines (middle + close)
        // need bq prefix re-emission inside the wrapper.
        builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());
        let first_line = lines[start_pos];
        let first_inner = if bq_depth > 0 {
            strip_n_blockquote_markers(first_line, bq_depth)
        } else {
            first_line
        };
        let (line_no_nl, nl) = strip_newline(first_inner);
        if !line_no_nl.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), line_no_nl);
        }
        if !nl.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), nl);
        }
        builder.finish_node();

        if close_line_idx > start_pos + 1 {
            builder.start_node(SyntaxKind::HTML_BLOCK_CONTENT.into());
            for content_line in &lines[start_pos + 1..close_line_idx] {
                emit_html_block_line(builder, content_line, bq_depth);
            }
            builder.finish_node();
        }

        builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());
        if bq_depth > 0 && close_prefix_len > 0 {
            emit_bq_prefix_tokens(builder, &close_line[..close_prefix_len]);
        }
        let close_part = &close_inner[..marker_end_in_inner];
        if !close_part.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), close_part);
        }
        builder.finish_node();
    }

    builder.finish_node(); // HTML_BLOCK

    // Recursively parse the trailing bytes on the close line and graft
    // top-level children as siblings of the HTML_BLOCK we just closed.
    // Refdefs flow through from the outer config (same pattern as
    // `emit_html_block_body_lifted_inner`).
    //
    // Under Pandoc a paragraph greedily fuses following lines as
    // soft-break continuations (`<!-- --> trailing\nmore\n` ->
    // `RawBlock, Para [trailing, SoftBreak, more]`). When the comment/PI
    // sits at the outermost level (no enclosing fenced div, list,
    // blockquote, or content-indent container), reparse the trailing
    // bytes together with the remaining document so the parser's own
    // paragraph-continuation rules decide the extent, graft ONLY that
    // first block, and hand the rest back to the outer dispatcher. Inside
    // a container we cannot consume past the close line without risking
    // swallowing its close marker (`:::`, `> `, list indent), so keep the
    // close-line-only split there.
    if !trailing.is_empty() {
        if at_outermost_level && close_line_idx + 1 < lines.len() {
            let mut inner_options = config.clone();
            let refdefs = config.refdef_labels.clone().unwrap_or_default();
            inner_options.refdef_labels = Some(refdefs.clone());
            let mut fragment = String::from(trailing);
            for line in &lines[close_line_idx + 1..] {
                fragment.push_str(line);
            }
            let inner_root =
                crate::parser::parse_with_refdefs(&fragment, Some(inner_options), refdefs);
            if let Some(first) = inner_root.first_child() {
                // Map the first block's end offset in the reparsed fragment
                // back to a source-line count. Bytes up to `trailing.len()`
                // are the close line (already counted); each subsequent line
                // fully covered by the first block is an extra consumed line.
                let block_end: usize = first.text_range().end().into();
                let mut consumed_bytes = trailing.len();
                let mut extra_lines = 0usize;
                let mut idx = close_line_idx + 1;
                while idx < lines.len() && consumed_bytes < block_end {
                    consumed_bytes += lines[idx].len();
                    extra_lines += 1;
                    idx += 1;
                }
                let mut bq = None;
                graft_subtree(builder, &first, &mut bq);
                return Some(close_line_idx + 1 + extra_lines);
            }
        }

        let mut inner_options = config.clone();
        let refdefs = config.refdef_labels.clone().unwrap_or_default();
        inner_options.refdef_labels = Some(refdefs.clone());
        let inner_root = crate::parser::parse_with_refdefs(trailing, Some(inner_options), refdefs);
        let mut bq = None;
        graft_document_children(builder, &inner_root, LastParaDemote::Never, &mut bq);
    }

    Some(close_line_idx + 1)
}

/// One source-order piece of a standalone-tag line: either a complete
/// HTML tag or a run of inter-tag/leading/trailing whitespace.
enum StandaloneTagSegment<'a> {
    Whitespace(&'a str),
    Tag(&'a str),
}

/// Extract the tag name from an open-tag slice (`<name ...>`), or `None`
/// if `tag` is not a well-formed open tag start.
fn open_tag_name(tag: &str) -> Option<&str> {
    let bytes = tag.as_bytes();
    if bytes.first() != Some(&b'<') {
        return None;
    }
    let start = 1;
    let mut i = start;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
        i += 1;
    }
    if i == start {
        return None;
    }
    Some(&tag[start..i])
}

/// Recognize a line consisting entirely of two or more complete
/// standalone block-level HTML tags — closing tags (`</p>`, `</div>`)
/// and void block tags (`<embed>`, `<source>`, …) — separated by
/// optional inter-tag whitespace, with optional leading indent and
/// trailing whitespace. Returns the source-order segments (tags +
/// whitespace) when the whole line is consumed by ≥ 2 such tags;
/// `None` otherwise (markdown text, strict/inline-block opens, or a
/// single tag — those stay on the legacy emission path).
fn split_line_into_standalone_tags(line: &str) -> Option<Vec<StandaloneTagSegment<'_>>> {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut segments = Vec::new();
    let mut tag_count = 0usize;
    let take_ws = |line: &str, from: usize| -> usize {
        let bytes = line.as_bytes();
        let mut j = from;
        while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
            j += 1;
        }
        j
    };
    let ws_end = take_ws(line, i);
    if ws_end > i {
        segments.push(StandaloneTagSegment::Whitespace(&line[i..ws_end]));
        i = ws_end;
    }
    while i < bytes.len() {
        let rest = &line[i..];
        // Any closing tag is an unconditional block splitter. For open
        // tags, only void block tags (`<embed>`, `<source>`, …) split
        // unconditionally at a fresh-block position — strict-block and
        // inline-block opens start a region (matched-pair lift /
        // `inline_pending` context), so leave those to the legacy byte
        // walker.
        let len = parse_close_tag(rest).or_else(|| {
            parse_open_tag(rest).filter(|&len| {
                open_tag_name(&rest[..len]).is_some_and(is_pandoc_void_block_tag_name)
            })
        })?;
        segments.push(StandaloneTagSegment::Tag(&line[i..i + len]));
        tag_count += 1;
        i += len;
        let ws_end = take_ws(line, i);
        if ws_end > i {
            segments.push(StandaloneTagSegment::Whitespace(&line[i..ws_end]));
            i = ws_end;
        }
    }
    (tag_count >= 2).then_some(segments)
}

/// Pandoc-dialect Phase 7b lift: a single-line opaque HTML block whose
/// content is two or more complete standalone block-level tags (void
/// tags and/or closing tags) — e.g. `</p></div>`, `<embed><embed>`.
/// Pandoc's `markdown_in_html_blocks` splits these into one `RawBlock`
/// per tag. The legacy emission bakes them into a single
/// `HTML_BLOCK_TAG` TEXT token, forcing the projector to re-tokenize
/// the bytes; this lift emits one `HTML_BLOCK_TAG` per tag so the CST
/// structurally encodes the split and the projector can route by kind.
///
/// Single-tag blocks (`</p>`, `<embed>`) stay on the legacy path —
/// their CST is already faithful (one tag, one `HTML_BLOCK_TAG`) and
/// changing it would churn snapshots with no fidelity gain.
///
/// Blockquote context (`bq_depth > 0`) is handled too: the dispatcher
/// emits the `> ` prefix tokens as siblings, so the tags inside the
/// `HTML_BLOCK` carry no bq markers and split cleanly. The prefix is
/// stripped via `strip_line_0_for_emission`; if that strip leaves any
/// non-tag bytes (e.g. an un-stripped list marker), the segment scan
/// bails and the block falls through to the legacy byte walker. Returns
/// the number of lines consumed (always 1) on success.
fn try_parse_standalone_block_tags_split(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    block_type: &HtmlBlockType,
    wrapper_kind: SyntaxKind,
    prefix: &ContainerPrefix,
    config: &ParserOptions,
) -> Option<usize> {
    if config.dialect != crate::options::Dialect::Pandoc {
        return None;
    }
    // Void/close forms keep the opaque `HTML_BLOCK` wrapper; `<div>`
    // and lifted strict/inline-block tags carry their own wrappers.
    if wrapper_kind != SyntaxKind::HTML_BLOCK {
        return None;
    }
    if !matches!(
        block_type,
        HtmlBlockType::BlockTag {
            closes_at_open_tag: true,
            ..
        }
    ) {
        return None;
    }
    let first_inner = prefix.strip_line_0_for_emission(lines[start_pos]);
    let (line, nl) = strip_newline(first_inner);
    let segments = split_line_into_standalone_tags(line)?;

    builder.start_node(SyntaxKind::HTML_BLOCK.into());
    for segment in segments {
        match segment {
            StandaloneTagSegment::Whitespace(ws) => {
                builder.token(SyntaxKind::WHITESPACE.into(), ws);
            }
            StandaloneTagSegment::Tag(tag) => {
                builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());
                builder.token(SyntaxKind::TEXT.into(), tag);
                builder.finish_node();
            }
        }
    }
    if !nl.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), nl);
    }
    builder.finish_node(); // HTML_BLOCK

    Some(start_pos + 1)
}

/// Parse an HTML block, allowing the caller to pick the wrapper SyntaxKind
/// (`HTML_BLOCK` for opaque preservation, `HTML_BLOCK_DIV` for the
/// Pandoc-dialect `<div>` lift). Children are emitted byte-for-byte
/// identical to the source either way; only the wrapper retag changes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn parse_html_block_with_wrapper(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    block_type: HtmlBlockType,
    prefix: &ContainerPrefix,
    wrapper_kind: SyntaxKind,
    at_outermost_level: bool,
    config: &ParserOptions,
) -> usize {
    let bq_depth = prefix.bq_depth();
    // Pandoc-dialect Comment / PI trailing-text split. Pandoc-native
    // closes the RawBlock at the close marker (`-->` / `?>`) and parses
    // any subsequent bytes (same-line trailing or following lines) as
    // fresh blocks. The legacy path absorbs them into the HTML block
    // wrapper, producing one oversized RawBlock. Handle the split here
    // before entering the legacy emission so the CST encodes the
    // sibling structure.
    if config.dialect == crate::options::Dialect::Pandoc
        && matches!(
            block_type,
            HtmlBlockType::Comment | HtmlBlockType::ProcessingInstruction
        )
        && let Some(consumed) = try_parse_comment_pi_with_trailing_split(
            builder,
            lines,
            start_pos,
            &block_type,
            wrapper_kind,
            bq_depth,
            at_outermost_level,
            config,
        )
    {
        return consumed;
    }

    // Pandoc-dialect Phase 7b standalone-tag split. A single line of two
    // or more complete standalone block-level tags (`</p></div>`,
    // `<embed><embed>`) projects to one `RawBlock` per tag; emit one
    // `HTML_BLOCK_TAG` per tag so the CST encodes the split structurally
    // instead of leaving the projector to re-tokenize a baked TEXT token.
    if let Some(consumed) = try_parse_standalone_block_tags_split(
        builder,
        lines,
        start_pos,
        &block_type,
        wrapper_kind,
        prefix,
        config,
    ) {
        return consumed;
    }

    // Start HTML block. The node kind may retag to `HTML_BLOCK_RAW` for
    // single-construct opaque shapes (comment / PI / verbatim) under
    // Pandoc; `wrapper_kind` itself stays the behavioral gate below so no
    // lift logic changes and the child tokens stay byte-identical.
    builder.start_node(html_block_node_kind(wrapper_kind, &block_type, config.dialect).into());

    let first_line = lines[start_pos];
    let blank_terminated = ends_at_blank_line(&block_type);

    // The block dispatcher has already emitted the bq prefix tokens for
    // the first line; emit only the inner content as TEXT to keep the
    // CST byte-equal to the source. List-marker bytes are stripped only
    // when this dispatch fires on a list-marker line — for
    // continuation-line dispatches (the much more common case) the
    // leading indent is inner content, not upstream-emitted prefix.
    let first_inner = prefix.strip_line_0_for_emission(first_line);

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
    let multiline_open_end = match (wrapper_kind, &block_type) {
        (SyntaxKind::HTML_BLOCK_DIV, _) => {
            find_multiline_open_end(lines, start_pos, first_inner, "div", prefix)
        }
        (
            _,
            HtmlBlockType::BlockTag {
                tag_name,
                closes_at_open_tag: true,
                ..
            },
        ) => find_multiline_open_end(lines, start_pos, first_inner, tag_name, prefix),
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
            find_multiline_open_end(lines, start_pos, first_inner, tag_name, prefix)
        }
        _ => None,
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
            let inner = prefix.strip(line);
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
                // Suppress the lift when the same-line trailing after the
                // first matched close holds inter-tag TEXT before another
                // matched-pair block tag. Pandoc treats the whole line as
                // one opaque type-6 block (`<p>foo</p> bar <p>baz</p>`,
                // corpus 0472); keep it opaque for the projector splitter.
                && !same_line_trailing_forces_opaque(line_no_nl, name)
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

    // Same-line lift inside a blockquote (`> <tag>body</tag>`). Bytes
    // are byte-equal to the non-bq same-line shape minus the leading
    // `> ` (which sits on the outer BLOCK_QUOTE, not inside HTML_BLOCK).
    // The body has no inner newlines, so no bq prefix re-injection is
    // needed when grafting — `emit_html_block_body_lifted` (passing
    // `bq: &mut None`) is enough. Other bq shapes (butted-close,
    // open-trailing) still fall through to the projector's byte
    // walker — they need per-line prefix injection.
    let same_line_bq_lift_tag: Option<&str> = if bq_depth > 0
        && multiline_open_end.is_none()
        && depth_aware_tag.is_some()
        && depth <= 0
    {
        let (line_no_nl, _) = strip_newline(first_inner);
        if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV {
            if probe_same_line_lift(line_no_nl, "div") {
                Some("div")
            } else {
                None
            }
        } else if wrapper_kind == SyntaxKind::HTML_BLOCK {
            match &block_type {
                HtmlBlockType::BlockTag {
                    tag_name,
                    is_verbatim: false,
                    closed_by_blank_line: false,
                    depth_aware: true,
                    closes_at_open_tag: false,
                    is_closing: false,
                } if is_pandoc_lift_eligible_block_tag(tag_name)
                    && probe_same_line_lift(line_no_nl, tag_name.as_str()) =>
                {
                    // Inline-block tags (`<video>`, `<iframe>`, …) skip
                    // the void-interior check at same-line — the shape
                    // has no inner block content to interfere with.
                    Some(tag_name.as_str())
                }
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
    };

    // Messy-shape lift inside a blockquote — covers open-trailing
    // (`> <div>foo\n> </div>`), butted-close (`> <div>\n> foo</div>`),
    // and open-trailing + butted-close (`> <div>foo\n> bar</div>`),
    // including the multi-line-open variants (`> <div\n>   id="x">foo\n>
    // body\n> </div>`) where the trailing is captured into `pre_content`
    // by `emit_multiline_open_tag_with_attrs` with `lift_trailing=true`.
    // The open line does NOT balance the block (depth > 0 after the
    // open line, distinguishing this from `same_line_bq_lift_tag` which
    // requires depth <= 0). The close line — possibly with leading body
    // text — closes the block when depth returns to 0. Body lines (incl.
    // open trailing and close leading) graft via prefix re-injection.
    let bq_messy_lift_tag: Option<&str> = if bq_depth > 0 && depth_aware_tag.is_some() && depth > 0
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
                } if is_pandoc_lift_eligible_block_tag(tag_name) => {
                    // Inline-block matched-pair tags (`<video>`, `<iframe>`,
                    // …) abandon the lift when the body starts at a
                    // fresh-block position with a void block tag. Same gate
                    // as the non-bq matched-pair lift (`strict_block_lift`).
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
                        None
                    } else {
                        Some(tag_name.as_str())
                    }
                }
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
    };

    // Multi-line open + matched close-on-the-open's-last-line shape inside
    // a blockquote (`> <div\n>   id="x">foo</div>` and depth-aware variants:
    // nested same-tag, trailing close, trailing text, strict-block `<form>`).
    // Mirrors the non-bq `pre_content`-close branch (line ~1363) but inside
    // a blockquote. Distinguishing features from `bq_messy_lift_tag`: the
    // close is on the open's last line (`depth <= 0` after the open lines)
    // AND `multiline_open_end.is_some()`. The trailing bytes after the
    // last `>` get lifted into `pre_content` via
    // `emit_multiline_open_tag_with_attrs(... lift_trailing=true)`, then the
    // new branch below splits `pre_content` at the matched close marker
    // and grafts body + close + any trailing siblings.
    let bq_multiline_close_lift_tag: Option<&str> = if bq_depth > 0
        && multiline_open_end.is_some()
        && depth_aware_tag.is_some()
        && depth <= 0
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
                } if is_pandoc_lift_eligible_block_tag(tag_name) => {
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
                        None
                    } else {
                        Some(tag_name.as_str())
                    }
                }
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
    };

    // Whether this block participates in the Phase 6 structural lift
    // (recursively parse body as Pandoc markdown and graft children).
    // Covers `<div>` outside blockquote context. For same-line shapes
    // the lift is gated on `same_line_*_lift_safe` — when unsafe we
    // keep the legacy single-HTML_BLOCK_TAG shape and let the
    // byte-reparse path handle projection.
    let lift_mode = (wrapper_kind == SyntaxKind::HTML_BLOCK_DIV
        && bq_depth == 0
        && (!is_same_line_div || same_line_div_lift_safe))
        || strict_block_lift
        || same_line_bq_lift_tag.is_some()
        || bq_messy_lift_tag.is_some()
        || bq_multiline_close_lift_tag.is_some();

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
            emit_multiline_open_tag_with_attrs(
                builder,
                lines,
                start_pos,
                end_line_idx,
                "div",
                bq_depth,
                lift_mode,
                &mut pre_content,
            );
        } else if let Some(name) = strict_block_tag_name
            && strict_block_lift
        {
            emit_multiline_open_tag_with_attrs(
                builder,
                lines,
                start_pos,
                end_line_idx,
                name,
                bq_depth,
                lift_mode,
                &mut pre_content,
            );
        } else if let Some(name) = bq_strict_attr_emit_tag_name(wrapper_kind, &block_type, bq_depth)
        {
            // Multi-line open of a lift-eligible strict-block tag inside a
            // blockquote (`> <section\n>   id=...>`). The non-bq
            // `strict_block_tag_name` gate is `bq_depth == 0`; this branch
            // covers the bq side so the open tag emits HTML_ATTRS regions
            // for `AttributeNode::cast` and the projector's canonicalizer.
            //
            // `lift_trailing` mirrors the single-line `emit_open_tag_tokens`
            // call below: only push trailing bytes into `pre_content` when
            // the structural lift will consume them (bq messy lift). The
            // bq clean-lift requires `pre_content.is_empty()`, so for clean
            // multi-line opens the trailing is empty anyway and this is
            // a no-op.
            let lift_trailing =
                bq_messy_lift_tag == Some(name) || bq_multiline_close_lift_tag == Some(name);
            emit_multiline_open_tag_with_attrs(
                builder,
                lines,
                start_pos,
                end_line_idx,
                name,
                bq_depth,
                lift_trailing,
                &mut pre_content,
            );
        } else {
            emit_multiline_open_tag_simple(builder, lines, start_pos, end_line_idx, bq_depth);
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
            } else if let Some(name) =
                bq_strict_attr_emit_tag_name(wrapper_kind, &block_type, bq_depth)
            {
                // Inside a blockquote, lift trailing bytes into
                // `pre_content` when either the same-line bq gate fires
                // (`> <tag>body</tag>` — handled by `same_line_closed`)
                // or the messy-shape bq gate fires (`> <tag>foo\n…\n>
                // </tag>` and butted-close — handled at the close-marker
                // site below). For the clean-shape bq lift the open has
                // no trailing bytes regardless, so `lift_trailing=true`
                // is a no-op there.
                let lift_trailing =
                    same_line_bq_lift_tag == Some(name) || bq_messy_lift_tag == Some(name);
                let trailing =
                    emit_open_tag_tokens(builder, line_without_newline, name, lift_trailing);
                if lift_trailing && !trailing.is_empty() {
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
    // Multi-line open with all matched closes on the open's last line:
    // `pre_content` holds the bytes after the last open `>` (lifted there
    // by `emit_multiline_open_tag_with_attrs` when `lift_trailing=true`).
    // When `depth <= 0` after the multi-line open and the trailing bytes
    // contain the depth-zero matched close, do the same-line lift on
    // `pre_content` directly. Mirrors the single-line `same_line_closed`
    // lift below — same body / close-marker / trailing-graft shape, just
    // consuming `end_line_idx + 1` lines instead of `start_pos + 1`.
    //
    // The body bytes of `pre_content` come from the open's last line,
    // which `emit_multiline_open_tag_with_attrs` already prefixed with the
    // re-emitted bq prefix tokens (for `bq_depth > 0`). The body and close
    // tag thus inherit the bq context without per-line prefix injection,
    // so `emit_html_block_body_lifted` (with `bq: &mut None`) suffices for
    // both the non-bq and bq variants of this shape.
    if let Some(end_line_idx) = multiline_open_end
        && !blank_terminated
        && depth_aware_tag.is_some()
        && depth <= 0
        && lift_mode
        && (bq_depth == 0 || bq_multiline_close_lift_tag.is_some())
        && !pre_content.is_empty()
    {
        let tag_name_opt: Option<&str> = if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV {
            Some("div")
        } else if strict_block_lift {
            strict_block_tag_name
        } else if let Some(name) = bq_multiline_close_lift_tag {
            Some(name)
        } else {
            None
        };
        if let Some(tag_name) = tag_name_opt {
            let (pre_no_nl, post_nl) = strip_newline(&pre_content);
            if let Some((leading, close_part)) =
                try_split_close_line_depth_aware(pre_no_nl, tag_name)
            {
                let close_marker_end =
                    split_close_marker_end(close_part, tag_name).unwrap_or(close_part.len());
                let close_marker = &close_part[..close_marker_end];
                let same_line_trailing = &close_part[close_marker_end..];
                let policy = if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV {
                    LastParaDemote::SkipTrailingBlanks
                } else {
                    LastParaDemote::OnlyIfLast
                };
                emit_html_block_body_lifted(builder, "", &[], leading, policy, config);
                builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());
                if same_line_trailing.is_empty() {
                    let mut close_line = String::with_capacity(close_marker.len() + post_nl.len());
                    close_line.push_str(close_marker);
                    close_line.push_str(post_nl);
                    emit_html_block_line(builder, &close_line, 0);
                    builder.finish_node();
                    builder.finish_node(); // HtmlBlock
                } else {
                    builder.token(SyntaxKind::TEXT.into(), close_marker);
                    builder.finish_node(); // HTML_BLOCK_TAG
                    builder.finish_node(); // HtmlBlock

                    let mut trailing_text =
                        String::with_capacity(same_line_trailing.len() + post_nl.len());
                    trailing_text.push_str(same_line_trailing);
                    trailing_text.push_str(post_nl);
                    let mut inner_options = config.clone();
                    let refdefs = config.refdef_labels.clone().unwrap_or_default();
                    inner_options.refdef_labels = Some(refdefs.clone());
                    let inner_root = crate::parser::parse_with_refdefs(
                        &trailing_text,
                        Some(inner_options),
                        refdefs,
                    );
                    let mut bq = None;
                    graft_document_children(builder, &inner_root, LastParaDemote::Never, &mut bq);
                }
                return end_line_idx + 1;
            }
        }
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
        } else if let Some(name) = same_line_bq_lift_tag {
            // Bq same-line: body has no inner newlines so the standard
            // `emit_html_block_body_lifted` (with `bq: &mut None`) is
            // sufficient. The bq prefix `> ` lives on the outer
            // BLOCK_QUOTE, outside the HTML_BLOCK[_DIV] span.
            Some(name)
        } else {
            None
        };
        if let Some(tag_name) = same_line_lift_tag {
            let (pre_no_nl, post_nl) = strip_newline(&pre_content);
            // Depth-aware split: handles `<tag>foo</tag>bar` (single
            // close, trailing text), `<tag>foo</tag></tag>` (matched
            // close + unmatched trailing close → sibling RawBlock),
            // and `<tag><tag>x</tag></tag>bar` (nested same-tag,
            // recursive body parse).
            if let Some((leading, close_part)) =
                try_split_close_line_depth_aware(pre_no_nl, tag_name)
            {
                // `close_part` starts with `</tag` and contains the close
                // marker followed by any same-line trailing text. Split
                // off the close marker bytes (`</tag>`) so the close
                // `HTML_BLOCK_TAG` carries only those bytes; trailing
                // text is parsed and grafted as a sibling block at the
                // parent level (matches pandoc-native shape:
                // `<div>foo</div>bar` → `Div [Plain[foo]] + Para [bar]`).
                let close_marker_end =
                    split_close_marker_end(close_part, tag_name).unwrap_or(close_part.len());
                let close_marker = &close_part[..close_marker_end];
                let same_line_trailing = &close_part[close_marker_end..];

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
                if same_line_trailing.is_empty() {
                    let mut close_line = String::with_capacity(close_marker.len() + post_nl.len());
                    close_line.push_str(close_marker);
                    close_line.push_str(post_nl);
                    emit_html_block_line(builder, &close_line, 0);
                    builder.finish_node();
                    builder.finish_node(); // HtmlBlock
                } else {
                    // Close tag holds only the close-marker bytes;
                    // trailing + newline graft as siblings of the
                    // wrapper (matches pandoc's per-tag block split).
                    builder.token(SyntaxKind::TEXT.into(), close_marker);
                    builder.finish_node(); // HTML_BLOCK_TAG
                    builder.finish_node(); // HtmlBlock

                    let mut trailing_text =
                        String::with_capacity(same_line_trailing.len() + post_nl.len());
                    trailing_text.push_str(same_line_trailing);
                    trailing_text.push_str(post_nl);
                    if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV {
                        // `<div>x</div> y <div>z</div>`: peel each further
                        // matched `<div>...</div>` pair in the trailing into
                        // its own sibling `HTML_BLOCK_DIV`, with inter-tag
                        // text as demoted `Plain` blocks. A plain reparse of
                        // the trailing keeps mid-line `<div>`s as inline raw
                        // HTML (`Para [.., RawInline "<div>", ..]`); pandoc-
                        // native lifts each to a block-level `Div`.
                        graft_same_line_div_peel(builder, &trailing_text, config);
                    } else {
                        let mut inner_options = config.clone();
                        let refdefs = config.refdef_labels.clone().unwrap_or_default();
                        inner_options.refdef_labels = Some(refdefs.clone());
                        let inner_root = crate::parser::parse_with_refdefs(
                            &trailing_text,
                            Some(inner_options),
                            refdefs,
                        );
                        let mut bq = None;
                        graft_document_children(
                            builder,
                            &inner_root,
                            LastParaDemote::Never,
                            &mut bq,
                        );
                    }
                }
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

    // Pandoc: an unclosed fenced div (`::: x`) opened inside the body
    // greedily consumes the following `</div>` line as a `RawBlock`, so it
    // does NOT close the outer HTML `<div>`. Track body-level fence nesting
    // and suspend the depth-aware close match while a fence is open. Gated
    // to Pandoc + the `fenced_divs` extension so CommonMark (where `:::` is
    // ordinary text and `<div>` stays opaque) is unaffected.
    let fence_suspends_close = config.dialect == crate::options::Dialect::Pandoc
        && config.extensions.fenced_divs
        && depth_aware_tag.is_some();
    let mut body_fence_depth: usize = 0;

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

        // Track body-level fenced-div nesting before the close check so an
        // open fence suspends the outer `</div>` match (see the note at the
        // fence tracker above).
        if fence_suspends_close {
            if crate::parser::blocks::fenced_divs::try_parse_div_fence_open(inner).is_some() {
                body_fence_depth += 1;
            } else if body_fence_depth > 0
                && crate::parser::blocks::fenced_divs::is_div_closing_fence(inner)
            {
                body_fence_depth -= 1;
            }
        }

        // Check for closing marker. Under depth-aware mode (Pandoc dialect)
        // count opens/closes of the same tag name and only close when depth
        // returns to 0; otherwise fall back to substring-match on the line.
        let line_closes = match &depth_aware_tag {
            Some(tag_name) => {
                if fence_suspends_close && body_fence_depth > 0 {
                    // Inside an open fenced div: the `</div>` line is body
                    // content the fence swallows, not the outer close. Do
                    // not advance `depth` or close here — the whole body
                    // (including `</div>`) lifts on the no-close path with
                    // an implicit EOF close, matching pandoc-native.
                    false
                } else {
                    let (opens, closes) = count_tag_balance(inner, tag_name);
                    depth += opens as i64;
                    depth -= closes as i64;
                    depth <= 0
                }
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
            // Multi-line opens are allowed here as of 2026-05-12: the
            // open `HTML_BLOCK_TAG` was emitted (potentially with HTML_ATTRS
            // per attr line and per-line bq prefix tokens) by the bq-aware
            // `emit_multiline_open_tag_with_attrs`. `pre_content` stays
            // empty for multi-line opens (the emitter writes any trailing
            // bytes on the last open line directly as TEXT inside
            // HTML_BLOCK_TAG, not into `pre_content`) — so multi-line +
            // trailing falls through to the opaque path, matching the non-
            // bq deferral.
            let bq_lift_tag: Option<&str> = if bq_depth > 0 && pre_content.is_empty() {
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
                // Open-shape: last open line must end with `>` (clean
                // close-of-open). For single-line, that's `first_inner`
                // (already bq-stripped); for multi-line, strip bq markers
                // from `lines[end_line_idx]` and check the same.
                let last_open_line: &str = match multiline_open_end {
                    None => first_inner,
                    Some(end) if prefix.bq_depth() > 0 || prefix.list_content_col() > 0 => {
                        prefix.strip(lines[end])
                    }
                    Some(end) => lines[end],
                };
                let (open_no_nl, _) = strip_newline(last_open_line);
                if !open_no_nl.trim_end_matches([' ', '\t']).ends_with('>') {
                    return false;
                }
                let close_stripped = prefix.strip(line);
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
                    prefix,
                    demote_policy,
                    config,
                );
                builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());
                emit_html_block_line(builder, line, bq_depth);
                builder.finish_node();
                current_pos += 1;
                break;
            }

            // Bq messy-shape lift — single-line open with trailing or
            // butted-close (or both). `pre_content` already captures any
            // open-trailing bytes (open `HTML_BLOCK_TAG` ends at `>`);
            // strip the close line's bq markers before splitting so
            // `leading` and `close_part` are bq-prefix-free. Body parses
            // recursively from `pre_content + stripped(content_lines) +
            // leading`, with per-line bq prefixes re-injected so the CST
            // stays byte-equal to the source. Demote: div is keyed on
            // close-butted-ness (Plain when leading non-empty, Para
            // otherwise); non-div uses OnlyIfLast either way.
            if let Some(tag_name) = bq_messy_lift_tag {
                let close_stripped = prefix.strip(line);
                let close_prefix_len = line.len() - close_stripped.len();
                let close_prefix = &line[..close_prefix_len];
                if let Some((leading, close_part)) = try_split_close_line(close_stripped, tag_name)
                {
                    let policy = if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV {
                        if leading.is_empty() {
                            LastParaDemote::Never
                        } else {
                            LastParaDemote::SkipTrailingBlanks
                        }
                    } else {
                        LastParaDemote::OnlyIfLast
                    };
                    emit_html_block_body_lifted_bq_messy(
                        builder,
                        &pre_content,
                        &content_lines,
                        leading,
                        close_prefix,
                        prefix,
                        policy,
                        config,
                    );
                    builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());
                    // When `leading` is empty, no recursive-parse output carries
                    // the close line's bq prefix, so emit it here before the
                    // close tag. When `leading` is non-empty,
                    // `emit_html_block_body_lifted_bq_messy` already injected
                    // the prefix at the start of the leading bytes (via the
                    // BqPrefixState entry); emitting again would double the
                    // prefix bytes and break losslessness.
                    if leading.is_empty() {
                        emit_bq_prefix_tokens(builder, close_prefix);
                    }
                    emit_html_block_line(builder, close_part, 0);
                    builder.finish_node();
                    current_pos += 1;
                    break;
                }
            }

            // Under lift mode, try to split the close line into a
            // leading "body content" prefix and the close-marker
            // remainder using depth-aware matching. Walks at depth 1
            // (we're inside the open tag) so nested same-tag opens
            // (e.g. `<inner></inner></tag>` style with a nested div)
            // are absorbed into the body and parsed recursively, and
            // multi-close shapes (`foo</div></div>` on the close line)
            // peel off the matched-pair close — the unmatched
            // trailing close projects as a sibling `RawBlock` per
            // pandoc-native. For `<div>`, non-empty `leading`
            // propagates pandoc's `markdown_in_html_blocks` Plain
            // demotion rule. For non-div strict-block tags, demotion
            // follows pandoc's `OnlyIfLast` rule (demote the trailing
            // Para only when no blank line precedes the close).
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
            let (close_no_nl, close_post_nl) = strip_newline(line);
            let close_split = close_split_tag
                .and_then(|name| try_split_close_line_depth_aware(close_no_nl, name));

            if let Some((leading, close_part)) = close_split {
                // Close-line leading that is whitespace-only is close-tag
                // indentation, not body content (pandoc-native strips it
                // from the close RawBlock and treats the close as butted —
                // see `   </tag>` shapes). Route those bytes into the
                // close `HTML_BLOCK_TAG` as a WHITESPACE token so the
                // projector strips them; keep the demote policy keyed on
                // the original leading so butted-close detection (Plain
                // demotion for div, OnlyIfLast for non-div) still fires.
                let leading_is_ws_only =
                    !leading.is_empty() && leading.bytes().all(|b| b == b' ' || b == b'\t');
                let body_leading = if leading_is_ws_only { "" } else { leading };
                let policy = if strict_block_lift {
                    LastParaDemote::OnlyIfLast
                } else if !leading.is_empty() {
                    LastParaDemote::SkipTrailingBlanks
                } else {
                    LastParaDemote::Never
                };
                // Split close_part into close-marker bytes (`</tag>`)
                // and trailing bytes (e.g. an extra `</div>` for the
                // double-close case, or `bar` for trailing text after
                // a normal close). Trailing bytes are recursively
                // parsed and grafted as siblings of the HTML_BLOCK_DIV
                // wrapper.
                let close_tag_name = close_split_tag.expect("close_split_tag present");
                let close_marker_end =
                    split_close_marker_end(close_part, close_tag_name).unwrap_or(close_part.len());
                let close_marker = &close_part[..close_marker_end];
                let close_trailing = &close_part[close_marker_end..];

                emit_html_block_body_lifted(
                    builder,
                    &pre_content,
                    &content_lines,
                    body_leading,
                    policy,
                    config,
                );
                builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());
                if leading_is_ws_only {
                    builder.token(SyntaxKind::WHITESPACE.into(), leading);
                }
                if close_trailing.is_empty() {
                    let mut close_line =
                        String::with_capacity(close_marker.len() + close_post_nl.len());
                    close_line.push_str(close_marker);
                    close_line.push_str(close_post_nl);
                    emit_html_block_line(builder, &close_line, 0);
                    builder.finish_node();
                } else {
                    // Close tag holds only the close-marker bytes;
                    // trailing + newline graft as siblings.
                    builder.token(SyntaxKind::TEXT.into(), close_marker);
                    builder.finish_node(); // HTML_BLOCK_TAG
                    builder.finish_node(); // HtmlBlock

                    let mut trailing_text =
                        String::with_capacity(close_trailing.len() + close_post_nl.len());
                    trailing_text.push_str(close_trailing);
                    trailing_text.push_str(close_post_nl);
                    if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV && bq_depth == 0 {
                        // Multi-line div variant of `<div>x</div> y
                        // <div>z</div>` — the close line carries the
                        // inter-tag text and a further `<div>` pair. Peel
                        // each into a sibling `HTML_BLOCK_DIV` (see
                        // `graft_same_line_div_peel`).
                        graft_same_line_div_peel(builder, &trailing_text, config);
                    } else {
                        let mut inner_options = config.clone();
                        let refdefs = config.refdef_labels.clone().unwrap_or_default();
                        inner_options.refdef_labels = Some(refdefs.clone());
                        let inner_root = crate::parser::parse_with_refdefs(
                            &trailing_text,
                            Some(inner_options),
                            refdefs,
                        );
                        let mut bq = None;
                        graft_document_children(
                            builder,
                            &inner_root,
                            LastParaDemote::Never,
                            &mut bq,
                        );
                    }
                    current_pos += 1;
                    return current_pos;
                }
            } else {
                emit_html_block_body(
                    builder,
                    &pre_content,
                    &content_lines,
                    prefix,
                    wrapper_kind,
                    lift_mode,
                    false,
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
            prefix,
            wrapper_kind,
            lift_mode,
            true,
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
#[allow(clippy::too_many_arguments)]
fn emit_html_block_body(
    builder: &mut GreenNodeBuilder<'static>,
    pre_content: &str,
    content_lines: &[&str],
    prefix: &ContainerPrefix,
    wrapper_kind: SyntaxKind,
    lift_mode: bool,
    open_only: bool,
    config: &ParserOptions,
) {
    let bq_depth = prefix.bq_depth();
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
    // Phase 7c: non-div strict-block / inline-block open tag with trailing
    // body and no matching close (`<section>foo`, `<video>foo\nbar`).
    // Pandoc-native emits the open tag as a standalone `RawBlock` followed
    // by the body parsed as fresh markdown siblings (`RawBlock "<section>"`
    // + `Para [foo]`) — NOT a wrapping like `<div>`. Lift the body into
    // structural CST children so the open `HTML_BLOCK_TAG` stays a lone
    // RawBlock and the body (`Para`, `Header`, `BulletList`, …) becomes
    // visible to consumers instead of opaque `HTML_BLOCK_CONTENT` TEXT.
    // No demotion (pandoc keeps the trailing `Para`). Gated to `bq_depth ==
    // 0`: inside a blockquote the body lines still carry `> ` markers that
    // this path would not re-inject, so those stay on the opaque byte
    // walker.
    if lift_mode && wrapper_kind == SyntaxKind::HTML_BLOCK && bq_depth == 0 {
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
    // Phase 7c (blockquote): same open-only strict-block / inline-block
    // shape as above, but inside a blockquote (`> <section>foo\n> bar`).
    // Line 0's `> ` is consumed by the outer BLOCK_QUOTE, so the
    // open-tag trailing sits in `pre_content` with no bq prefix; the
    // continuation `content_lines` still carry their `> ` markers.
    // `emit_html_block_body_lifted_bq_messy` re-injects each line's
    // captured bq prefix at line start so the lifted `PARAGRAPH` stays
    // byte-equal to source. No close line (open-only), so `leading` and
    // `close_line_prefix` are empty. No demotion — pandoc keeps the
    // trailing `Para` (`RawBlock "<section>"` + `Para [foo, SoftBreak,
    // bar]`).
    if open_only && lift_mode && wrapper_kind == SyntaxKind::HTML_BLOCK && bq_depth > 0 {
        emit_html_block_body_lifted_bq_messy(
            builder,
            pre_content,
            content_lines,
            "",
            "",
            prefix,
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
    prefix: &ContainerPrefix,
    demote_policy: LastParaDemote,
    config: &ParserOptions,
) {
    let mut prefix_lines: Vec<ContainerPrefixLine> = Vec::with_capacity(content_lines.len());
    let mut stripped_lines: Vec<&str> = Vec::with_capacity(content_lines.len());
    for cl in content_lines {
        let (li, bq, inner) = prefix.split(cl);
        prefix_lines.push(ContainerPrefixLine {
            list_indent: li.to_string(),
            bq_prefix: bq.to_string(),
        });
        stripped_lines.push(inner);
    }
    let mut state = ContainerPrefixState::new(prefix_lines);
    emit_html_block_body_lifted_inner(
        builder,
        "",
        &stripped_lines,
        "",
        demote_policy,
        config,
        &mut state,
    )
}

/// Body-lift variant for the bq messy-shape lift — open-trailing,
/// butted-close, or both. The open-trailing bytes (if any) sit in
/// `pre_content` (line 0 of the body — no bq prefix in source because
/// line 0's `> ` is consumed by the outer BLOCK_QUOTE). Content lines
/// each carry their own bq prefix. The close line's `leading` (body
/// bytes before `</tag>`) sits on the close line, prefixed in source
/// by `close_line_prefix` (the bq prefix captured from `line`).
///
/// Builds `prefixes` so each emitted line in the recursive parse
/// output gets the right per-line bq prefix re-injected at line start:
/// `pre_content` → empty prefix (no source `> ` precedes it); each
/// content line → its stripped prefix; `leading` → `close_line_prefix`.
/// Result CST stays byte-equal to source.
#[allow(clippy::too_many_arguments)]
fn emit_html_block_body_lifted_bq_messy(
    builder: &mut GreenNodeBuilder<'static>,
    pre_content: &str,
    content_lines: &[&str],
    leading: &str,
    close_line_prefix: &str,
    prefix: &ContainerPrefix,
    demote_policy: LastParaDemote,
    config: &ParserOptions,
) {
    let mut prefix_lines: Vec<ContainerPrefixLine> = Vec::new();
    if !pre_content.is_empty() {
        prefix_lines.push(ContainerPrefixLine::default());
    }
    let mut stripped_lines: Vec<&str> = Vec::with_capacity(content_lines.len());
    for cl in content_lines {
        let (li, bq, inner) = prefix.split(cl);
        prefix_lines.push(ContainerPrefixLine {
            list_indent: li.to_string(),
            bq_prefix: bq.to_string(),
        });
        stripped_lines.push(inner);
    }
    if !leading.is_empty() {
        // The close line carries its own captured prefix bytes; treat
        // them as bq-prefix only (no list-indent split applied) to keep
        // the legacy bq-only re-injection behavior for messy-shape
        // close-line lifts.
        prefix_lines.push(ContainerPrefixLine::bq_only(close_line_prefix.to_string()));
    }
    let mut state = ContainerPrefixState::new(prefix_lines);
    emit_html_block_body_lifted_inner(
        builder,
        pre_content,
        &stripped_lines,
        leading,
        demote_policy,
        config,
        &mut state,
    )
}

fn emit_html_block_body_lifted_inner(
    builder: &mut GreenNodeBuilder<'static>,
    pre_content: &str,
    content_lines: &[&str],
    post_content: &str,
    demote_policy: LastParaDemote,
    config: &ParserOptions,
    bq: &mut Option<ContainerPrefixState>,
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

/// Walk a parsed inner document's top-level children and re-emit them
/// into `builder`. The document's wrapper node is skipped — only its
/// children are grafted.
///
/// `demote_policy` controls whether a trailing `PARAGRAPH` is retagged
/// as `PLAIN` — see [`LastParaDemote`].
///
/// `bq` is `Some` when grafting a body that lived inside an outer
/// container (blockquote, list-item, or both) — token emission then
/// injects the captured per-line prefix tokens at line starts so the
/// CST stays byte-equal to source. See
/// [`super::container_prefix::ContainerPrefixState`].
fn graft_document_children(
    builder: &mut GreenNodeBuilder<'static>,
    doc: &SyntaxNode,
    demote_policy: LastParaDemote,
    bq: &mut Option<ContainerPrefixState>,
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
    bq: &mut Option<ContainerPrefixState>,
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
    bq: &mut Option<ContainerPrefixState>,
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
    bq: &mut Option<ContainerPrefixState>,
) {
    if let Some(state) = bq.as_mut() {
        if state.at_line_start {
            if let Some(line_prefix) = state.prefixes.get(state.line_idx) {
                emit_container_prefix_tokens(builder, line_prefix);
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
pub(crate) fn probe_open_tag_line_has_close_gt(line: &str, tag_name: &str) -> bool {
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

/// When a non-div strict-block same-line matched pair (`<p>foo</p>...`)
/// has trailing content after the first matched close, decide whether
/// pandoc would keep the WHOLE line as one opaque type-6 HTML block
/// (flat `RawBlock`/`Plain` alternation, resolved later by the
/// projector's byte splitter) rather than lifting the first pair and
/// reparsing the tail.
///
/// Pandoc keeps the line opaque when the after-close trailing contains
/// genuine inter-tag TEXT followed by another matched-pair block tag:
/// `<p>foo</p> bar <p>baz</p>` -> 7 flat blocks with `bar` demoted to
/// `Plain` (corpus 0472). It does NOT keep it opaque when:
/// - the trailing is empty or begins (after whitespace) with a tag —
///   consecutive / whitespace-only pairs (`<p>a</p><p>b</p>`,
///   `<p>a</p> <p>b</p>`) reparse into clean sibling `HTML_BLOCK`s; or
/// - the trailing is plain text with no later matched-pair tag
///   (`<p>a</p> bar` -> `Para [bar]`); or
/// - the only later tag is a void `eitherBlockOrInline` tag
///   (`<p>a</p> mid <embed> end` -> inline `<embed>` in a `Para`,
///   corpus 0474).
///
/// Only relevant for non-div strict-block tags: `<div>` projects each
/// tag as a structural `Div` and keeps its own lift path.
fn same_line_trailing_forces_opaque(line: &str, tag_name: &str) -> bool {
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
    // Locate the open tag's closing `>` (quote-aware).
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
    let Some((_, close_end)) = matched_close_offset(trailing, tag_name) else {
        return false;
    };
    let after_close = &trailing[close_end..];
    // Trailing that is empty or begins (after whitespace) with a tag is
    // handled correctly by the tail reparse; only inter-tag TEXT forces
    // the opaque whole-line treatment.
    let trimmed = after_close.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('<') {
        return false;
    }
    trailing_contains_matched_pair_tag(after_close)
}

/// Scan `s` for any pandoc matched-pair block tag (`<name ...>` or
/// `</name>`). Byte walk over tag-name starts; good enough for the
/// same-line-opaque heuristic (attribute values rarely embed `<tag`).
fn trailing_contains_matched_pair_tag(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if j < bytes.len() && bytes[j] == b'/' {
            j += 1;
        }
        let name_start = j;
        while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'-') {
            j += 1;
        }
        if j > name_start && is_pandoc_matched_pair_tag(&s[name_start..j]) {
            return true;
        }
        i = j.max(i + 1);
    }
    false
}

/// Probe whether the same-line `<tag>BODY</tag>` shape on `line` can
/// be lifted structurally. Returns `true` only when:
/// - The line starts with `<tag_name` (modulo leading whitespace).
/// - The open tag's `>` exists with proper quote handling.
/// - The bytes after the open `>` contain a depth-zero matched
///   `</tag_name>` close (depth-aware: nested `<tag>` opens
///   increment depth; matching is case-insensitive, quote-aware).
///
/// Trailing bytes after the matched close are accepted and grafted
/// as a sibling block by the caller. Examples:
/// - `<div>foo</div>bar` → body=`foo`, trailing=`bar`.
/// - `<div>foo</div></div>` → body=`foo`, trailing=`</div>` (which
///   recursively parses to a `RawBlock`).
/// - `<div><div>x</div></div>bar` → body=`<div>x</div>` (nested div
///   parsed recursively), trailing=`bar`.
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
    // Depth-aware: walk `trailing` (we begin inside the open tag at
    // depth 1). Return true iff a matched `</tag>` exists where depth
    // returns to 0. Self-closing `<tag/>` opens don't bump depth.
    matched_close_offset(trailing, tag_name).is_some()
}

/// Walk `trailing` (the bytes after an open `<tag ...>`'s closing `>`)
/// looking for the depth-zero matched `</tag>` close. Counts `<tag>`
/// opens and `</tag>` closes case-insensitively, quote-aware. Depth
/// starts at 1 (we begin inside the open tag). Self-closing opens
/// (`<tag/>`) do not increment depth.
///
/// Returns `Some((close_start, close_end))` where:
/// - `close_start` is the byte offset of `<` in the matched `</tag>`.
/// - `close_end` is one past the matched `>`.
///
/// Returns `None` when no matched close is present (unclosed tag,
/// depth never returns to 0).
fn matched_close_offset(trailing: &str, tag_name: &str) -> Option<(usize, usize)> {
    let bytes = trailing.as_bytes();
    let lower_line = trailing.to_ascii_lowercase();
    let lower_bytes = lower_line.as_bytes();
    let tag_lower = tag_name.to_ascii_lowercase();
    let tag_bytes = tag_lower.as_bytes();

    let mut depth: i32 = 1;
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

        // Scan forward to this tag bracket's `>`, respecting quoted
        // attribute values; track self-closing form (`/>`).
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
                depth -= 1;
                if depth == 0 && found_gt {
                    return Some((i, j + 1));
                }
            } else if !self_close {
                depth += 1;
            }
        }

        if found_gt {
            i = j + 1;
        } else {
            // Unterminated `<...` — give up.
            break;
        }
    }
    None
}

/// Locate the byte offset of the first `>` after a `</tag` prefix at
/// the start of `close_part`. Returns `Some(end_of_close_marker)` so
/// the caller can split `close_part` into the close-marker bytes
/// (`</tag>`) and any same-line trailing text. Returns `None` if the
/// expected prefix shape is missing — caller treats the whole slice
/// as the close marker (no trailing).
fn split_close_marker_end(close_part: &str, tag_name: &str) -> Option<usize> {
    let prefix_len = 2 + tag_name.len();
    let bytes = close_part.as_bytes();
    if bytes.len() < prefix_len
        || bytes[0] != b'<'
        || bytes[1] != b'/'
        || !bytes[2..prefix_len].eq_ignore_ascii_case(tag_name.as_bytes())
    {
        return None;
    }
    // Scan from after `</tag` to the first unquoted `>`.
    let mut i = prefix_len;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        match (quote, bytes[i]) {
            (None, b'"') | (None, b'\'') => quote = Some(bytes[i]),
            (Some(q), b2) if b2 == q => quote = None,
            (None, b'>') => return Some(i + 1),
            _ => {}
        }
        i += 1;
    }
    None
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

/// Depth-aware variant of `try_split_close_line` used by the same-line
/// lift path. Walks `line` starting at depth 1 (we begin inside the
/// open `<tag>`) and splits at the byte position where the matched
/// `</tag>` close brings depth to 0. Returns `Some((body,
/// close_part))` where `body` is the bytes before the matched-close
/// start and `close_part` is the bytes from the matched close onward.
///
/// Unlike `try_split_close_line` this accepts nested same-tag opens
/// and multiple closes: for `<div><div>x</div></div>bar` it returns
/// body=`<div>x</div>` (a nested div the body lift parses
/// recursively) and close_part=`</div>bar`. For `<div>foo</div></div>`
/// it returns body=`foo`, close_part=`</div></div>` — the unmatched
/// trailing close projects as a sibling `RawBlock` per pandoc-native.
fn try_split_close_line_depth_aware<'a>(
    line: &'a str,
    tag_name: &str,
) -> Option<(&'a str, &'a str)> {
    let (close_start, _close_end) = matched_close_offset(line, tag_name)?;
    Some((&line[..close_start], &line[close_start..]))
}

/// Locate the next `<tag ...>...</tag>` matched pair in `s`. Returns
/// `(open_start, pair_end)` where `open_start` is the byte offset of the
/// open `<` and `pair_end` is one past the matched close's `>`. Opens
/// without a depth-zero matched close (unclosed) are skipped. Close-only
/// forms (`</tag>`) never match an open. Quote-aware throughout.
fn find_next_matched_pair(s: &str, tag_name: &str) -> Option<(usize, usize)> {
    let bytes = s.as_bytes();
    let mut search = 0usize;
    while let Some(rel) = bytes[search..].iter().position(|&b| b == b'<') {
        let lt = search + rel;
        // `locate_open_tag_close_gt` returns `None` for close tags and
        // non-`<tag` shapes; when it matches, the returned index is the
        // open tag's `>` relative to `s[lt..]`.
        if let Some(gt_rel) = locate_open_tag_close_gt(&s[lt..], tag_name) {
            let after_open = lt + gt_rel + 1;
            if let Some((_close_start, close_end)) =
                matched_close_offset(&s[after_open..], tag_name)
            {
                return Some((lt, after_open + close_end));
            }
        }
        search = lt + 1;
    }
    None
}

/// Peel a same-line `<div>` trailing (the bytes after the first matched
/// `</div>`) into alternating interstitial-text and `<div>...</div>`
/// sibling blocks, matching pandoc-native's per-tag block split:
/// `<div>x</div> y <div>z</div>` → `Div[Plain x], Plain[y], Div[Plain z]`.
///
/// Each segment is reparsed as a fresh document and grafted, so the CST
/// stays byte-equal to source: only the final segment carries the source
/// line's terminating newline, and no synthetic bytes are introduced.
/// Inter-tag text between two divs demotes `Para`→`Plain` (butted
/// between blocks); trailing text after the last div stays `Para`.
/// Whitespace-only gaps parse to `BLANK_LINE` (no block emitted, bytes
/// preserved) and are skipped by the projector.
fn graft_same_line_div_peel(
    builder: &mut GreenNodeBuilder<'static>,
    trailing: &str,
    config: &ParserOptions,
) {
    let mut rest = trailing;
    loop {
        match find_next_matched_pair(rest, "div") {
            Some((open_start, pair_end)) => {
                let interstitial = &rest[..open_start];
                let div_segment = &rest[open_start..pair_end];
                if !interstitial.is_empty() {
                    emit_html_block_body_lifted(
                        builder,
                        interstitial,
                        &[],
                        "",
                        LastParaDemote::SkipTrailingBlanks,
                        config,
                    );
                }
                emit_html_block_body_lifted(
                    builder,
                    div_segment,
                    &[],
                    "",
                    LastParaDemote::Never,
                    config,
                );
                rest = &rest[pair_end..];
                if rest.is_empty() {
                    break;
                }
            }
            None => {
                if !rest.is_empty() {
                    emit_html_block_body_lifted(
                        builder,
                        rest,
                        &[],
                        "",
                        LastParaDemote::Never,
                        config,
                    );
                }
                break;
            }
        }
    }
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
        emit_html_attrs_node(builder, attrs_text);
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
    prefix: &ContainerPrefix,
) -> Option<usize> {
    // Locate the `<tag_name` literal in `first_inner` to start scanning past
    // it. Match is ASCII case-insensitive; the parser preserves source casing.
    // `first_inner` is already bq-stripped by the caller; subsequent lines are
    // stripped inline below via `strip_n_blockquote_markers`.
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

    // No `>` on first line. Scan subsequent lines, stripping `bq_depth`
    // blockquote markers per line so `> ` prefixes don't count toward the
    // quote-aware scan. Mirrors `pandoc_html_open_tag_closes`.
    let mut line_idx = start_pos + 1;
    while line_idx < lines.len() {
        let raw = lines[line_idx];
        let inner = prefix.strip(raw);
        for &b in inner.as_bytes() {
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
    prefix: &ContainerPrefix,
) -> bool {
    if start_pos >= lines.len() {
        return false;
    }
    let mut quote: Option<u8> = None;
    for (offset, line) in lines.iter().enumerate().skip(start_pos) {
        let inner = prefix.strip(line);
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
#[allow(clippy::too_many_arguments)]
fn emit_multiline_open_tag_with_attrs(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    end_line_idx: usize,
    tag_name: &str,
    bq_depth: usize,
    lift_trailing: bool,
    pre_content: &mut String,
) {
    let prefix_len = 1 + tag_name.len();
    for (line_idx, raw) in lines
        .iter()
        .enumerate()
        .take(end_line_idx + 1)
        .skip(start_pos)
    {
        // Strip `bq_depth` blockquote markers from the source line so
        // indent/HTML_ATTRS/TEXT splitting ignores the bq prefix bytes.
        // Re-emit the stripped prefix as `BLOCK_QUOTE_MARKER` /
        // `WHITESPACE` tokens — but ONLY for lines past `start_pos`.
        // Line 0's bq prefix is consumed by the outer BLOCK_QUOTE node
        // before this parser runs; re-emitting it here would double
        // the bytes and break losslessness.
        let stripped = if bq_depth > 0 {
            strip_n_blockquote_markers(raw, bq_depth)
        } else {
            raw
        };
        let bq_prefix_len = raw.len() - stripped.len();
        if bq_prefix_len > 0 && line_idx != start_pos {
            emit_bq_prefix_tokens(builder, &raw[..bq_prefix_len]);
        }
        let line = stripped;
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
                emit_html_attrs_node(builder, attrs_text);
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
                emit_html_attrs_node(builder, attrs_text);
            }
            if !trailing_ws.is_empty() {
                builder.token(SyntaxKind::WHITESPACE.into(), trailing_ws);
            }
            builder.token(SyntaxKind::TEXT.into(), ">");
            let after_gt = &line_no_nl[gt + 1..];
            if lift_trailing && !after_gt.is_empty() {
                // Lift trailing bytes (and the trailing newline) into
                // `pre_content` so the open `HTML_BLOCK_TAG` ends cleanly
                // with `TEXT(">")`. The recursive parse at the close-marker
                // site treats `pre_content` as the leading bytes of the
                // structural body — same shape produced by `emit_open_tag_tokens`
                // for single-line opens.
                pre_content.push_str(after_gt);
                pre_content.push_str(newline_str);
                continue;
            }
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
    bq_depth: usize,
) {
    for (line_idx, raw) in lines
        .iter()
        .enumerate()
        .take(end_line_idx + 1)
        .skip(start_pos)
    {
        let stripped = if bq_depth > 0 {
            strip_n_blockquote_markers(raw, bq_depth)
        } else {
            raw
        };
        let bq_prefix_len = raw.len() - stripped.len();
        // Line 0's bq prefix is owned by the outer BLOCK_QUOTE node;
        // re-emit prefixes only for subsequent lines.
        if bq_prefix_len > 0 && line_idx != start_pos {
            emit_bq_prefix_tokens(builder, &raw[..bq_prefix_len]);
        }
        let (line_no_nl, newline_str) = strip_newline(stripped);
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
        emit_html_attrs_node(builder, attrs_text);
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
            find_multiline_open_end(
                &["<div id=\"x\">"],
                0,
                "<div id=\"x\">",
                "div",
                &ContainerPrefix::default()
            ),
            None
        );
        assert_eq!(
            find_multiline_open_end(
                &["<embed src=\"x\">"],
                0,
                "<embed src=\"x\">",
                "embed",
                &ContainerPrefix::default()
            ),
            None
        );
        // Multi-line opens return the line index of the closing `>`.
        assert_eq!(
            find_multiline_open_end(
                &["<embed", "  src=\"x\">"],
                0,
                "<embed",
                "embed",
                &ContainerPrefix::default()
            ),
            Some(1)
        );
        assert_eq!(
            find_multiline_open_end(
                &["<embed", "  src=\"x\"", "  type=\"video\">"],
                0,
                "<embed",
                "embed",
                &ContainerPrefix::default()
            ),
            Some(2)
        );
        // Tag-name mismatch returns None (case-insensitive on the tag name).
        assert_eq!(
            find_multiline_open_end(
                &["<embed", "  src=\"x\">"],
                0,
                "<embed",
                "div",
                &ContainerPrefix::default()
            ),
            None
        );
        assert_eq!(
            find_multiline_open_end(
                &["<EMBED", "  src=\"x\">"],
                0,
                "<EMBED",
                "embed",
                &ContainerPrefix::default()
            ),
            Some(1)
        );
        // Quoted `>` does not terminate the open tag; quote state threads
        // across line boundaries.
        assert_eq!(
            find_multiline_open_end(
                &["<embed title=\"a>b", "  c\">"],
                0,
                "<embed title=\"a>b",
                "embed",
                &ContainerPrefix::default()
            ),
            Some(1)
        );
        // No `>` anywhere returns None.
        assert_eq!(
            find_multiline_open_end(
                &["<embed", "  src=\"x\""],
                0,
                "<embed",
                "embed",
                &ContainerPrefix::default()
            ),
            None
        );
        // Subsequent lines inside a blockquote: bq markers stripped before
        // scanning so `> ` prefixes don't count.
        assert_eq!(
            find_multiline_open_end(
                &["<div", ">   id=\"x\">"],
                0,
                "<div",
                "div",
                &ContainerPrefix::bq_only(1)
            ),
            Some(1)
        );
        // Nested bq: strips two `> ` per line.
        assert_eq!(
            find_multiline_open_end(
                &["<section", "> >   id=\"x\">"],
                0,
                "<section",
                "section",
                &ContainerPrefix::bq_only(2)
            ),
            Some(1)
        );
    }

    #[test]
    fn test_pandoc_html_open_tag_closes() {
        // Single-line complete: scanner finds `>` on the first line.
        assert!(pandoc_html_open_tag_closes(
            &["<div>"],
            0,
            &ContainerPrefix::default()
        ));
        assert!(pandoc_html_open_tag_closes(
            &["<embed src=\"x\">"],
            0,
            &ContainerPrefix::default()
        ));
        // Multi-line complete: scanner finds `>` on a later line.
        assert!(pandoc_html_open_tag_closes(
            &["<div", "  id=\"x\">", "body", "</div>"],
            0,
            &ContainerPrefix::default()
        ));
        assert!(pandoc_html_open_tag_closes(
            &["<embed", "  src=\"x.png\" alt=\"y\">"],
            0,
            &ContainerPrefix::default()
        ));
        // Quoted `>` does not close: scanner threads quote state.
        assert!(!pandoc_html_open_tag_closes(
            &["<div title=\"a>b", "  c\""],
            0,
            &ContainerPrefix::default()
        ));
        assert!(pandoc_html_open_tag_closes(
            &["<div title=\"a>b", "  c\">"],
            0,
            &ContainerPrefix::default()
        ));
        // Incomplete: no `>` anywhere — pandoc treats as paragraph text.
        assert!(!pandoc_html_open_tag_closes(
            &["<embed"],
            0,
            &ContainerPrefix::default()
        ));
        assert!(!pandoc_html_open_tag_closes(
            &["<div", "foo", "bar"],
            0,
            &ContainerPrefix::default()
        ));
        // Pandoc tolerates blank lines mid-open-tag (its `htmlTag` reads
        // across them); the scan continues until EOF or `>`.
        assert!(pandoc_html_open_tag_closes(
            &["<div", "", "id=\"x\">"],
            0,
            &ContainerPrefix::default()
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
            &ContainerPrefix::default(),
            SyntaxKind::HTML_BLOCK,
            true,
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
            &ContainerPrefix::default(),
            SyntaxKind::HTML_BLOCK,
            true,
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
            &ContainerPrefix::default(),
            SyntaxKind::HTML_BLOCK,
            true,
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
            &ContainerPrefix::default(),
            SyntaxKind::HTML_BLOCK_DIV,
            true,
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
            &ContainerPrefix::default(),
            SyntaxKind::HTML_BLOCK_DIV,
            true,
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
            &ContainerPrefix::default(),
            SyntaxKind::HTML_BLOCK,
            true,
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
            &ContainerPrefix::default(),
            SyntaxKind::HTML_BLOCK_DIV,
            true,
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
            &ContainerPrefix::default(),
            SyntaxKind::HTML_BLOCK_DIV,
            true,
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
            &ContainerPrefix::default(),
            SyntaxKind::HTML_BLOCK,
            true,
            &opts,
        );

        // Block contains <div>\nfoo\n; stops at blank line (line 2).
        assert_eq!(new_pos, 2);
    }
}
