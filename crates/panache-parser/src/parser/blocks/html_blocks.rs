//! HTML block parsing utilities.

use crate::options::ParserOptions;
use crate::parser::inlines::inline_html::{parse_close_tag, parse_open_tag};
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

use super::blockquotes::{count_blockquote_markers, strip_n_blockquote_markers};
use super::container_prefix::{
    ContainerPrefix, ContainerPrefixLine, ContainerPrefixState, emit_grafted_token,
};
use crate::parser::utils::attributes::{emit_html_attrs_node, pandoc_html_attribute_names_valid};
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
///
/// A void tag no longer holds its *enclosing* element open: pandoc 3.10
/// closes `<source>` on its own line, so `<video>` around it still takes
/// the ordinary matched-pair lift and its `</video>` is a `RawBlock`
/// rather than a `RawInline` trailing the fallback text.
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
/// Inline-block tags (`<video>`, `<iframe>`, `<button>`, …) take the
/// lift even when the body opens with a void block tag: pandoc 3.10
/// closes the void element on its own line, so
/// `<video>\n<source ...>\nfallback\n</video>` is a matched pair whose
/// body happens to start with a `RawBlock`.
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

    if !trimmed.starts_with('<') {
        return None;
    }

    if trimmed.starts_with("<!--") {
        return Some(HtmlBlockType::Comment);
    }

    if trimmed.starts_with("<?") {
        return Some(HtmlBlockType::ProcessingInstruction);
    }

    if is_commonmark && trimmed.starts_with("<![CDATA[") {
        return Some(HtmlBlockType::CData);
    }

    if is_commonmark && trimmed.starts_with("<!") && trimmed.len() > 2 {
        let after_bang = &trimmed[2..];
        if after_bang.chars().next()?.is_ascii_alphabetic() {
            return Some(HtmlBlockType::Declaration);
        }
    }

    if let Some(tag_name) = extract_block_tag_name(trimmed, true) {
        let tag_lower = tag_name.to_lowercase();
        let is_closing = trimmed.starts_with("</");

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

        if !is_commonmark
            && is_closing
            && !PANDOC_INLINE_BLOCK_TAGS.contains(&tag_lower.as_str())
            && !PANDOC_VOID_BLOCK_TAGS.contains(&tag_lower.as_str())
        {
            return None;
        }

        let is_block_tag = if is_commonmark {
            BLOCK_TAGS.contains(&tag_lower.as_str())
        } else {
            PANDOC_BLOCK_TAGS.contains(&tag_lower.as_str())
        };
        if is_block_tag {
            let is_verbatim = VERBATIM_TAGS.contains(&tag_lower.as_str());
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

    if is_commonmark && let Some(end) = parse_open_tag(trimmed).or_else(|| parse_close_tag(trimmed))
    {
        let rest = &trimmed[end..];
        let only_ws = rest
            .bytes()
            .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'));
        if only_ws {
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

    let tag_end = after_slash
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .unwrap_or(after_slash.len());

    if tag_end == 0 {
        return None;
    }

    let tag_name = &after_slash[..tag_end];

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
/// `line`. Pandoc treats `<div/>` as an opening div, so it increases
/// nesting depth despite the slash. Other self-closing forms and tags
/// inside quoted attribute values are not counted.
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
            } else if !self_close || tag_lower == "div" {
                opens += 1;
            }
        }

        if found_gt {
            i = j + 1;
        } else {
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

/// How far the Pandoc comment/PI trailing-text split may fuse soft-break
/// continuation lines into the trailing paragraph.
///
/// Pandoc closes the comment/PI `RawBlock` at the close marker and then
/// parses the trailing bytes plus following lines as ordinary blocks, so a
/// bare continuation line joins the trailing text as a soft-break
/// (`<!-- --> t\nmore` -> `RawBlock, Para [t, SoftBreak, more]`). Fusing
/// requires reparsing past the close line, which is only safe up to the
/// point where an enclosing container's close marker begins — consuming it
/// would swallow the boundary (`:::`, `> `, list indent).
#[derive(Clone, Copy)]
pub(crate) enum SoftbreakFusion {
    /// Outermost level: fuse continuation lines to the end of the document.
    ToDocEnd,
    /// Inside a plain (no line-prefix) fenced div: fuse up to the div's
    /// closing `:::` fence, which the outer dispatcher still owns.
    ToFencedDivClose,
    /// Inside a pure blockquote (no list / content-indent / directive
    /// nesting): fuse up to the first line that no longer continues the
    /// blockquote at the current marker depth. The continuation lines'
    /// `> ` prefixes are stripped before reparse and re-injected during
    /// graft so the CST stays byte-equal to source.
    ToBlockquoteEnd,
    /// Inside a list / content-indent container, or fusion otherwise
    /// disabled: keep the close-line-only split.
    None,
}

/// Index of the line that closes the fenced div enclosing `start` (the
/// first bare `:::` close fence at the current nesting depth), or
/// `lines.len()` when the div is unclosed at end of input. `start` must be
/// past the comment/PI close line so `:::` lines inside the comment body
/// don't terminate the scan early.
fn fenced_div_body_end(lines: &[&str], start: usize) -> usize {
    let mut depth = 0usize;
    for (offset, line) in lines[start..].iter().enumerate() {
        if super::fenced_divs::try_parse_div_fence_open(line).is_some() {
            depth += 1;
        } else if super::fenced_divs::is_div_closing_fence(line) {
            if depth == 0 {
                return start + offset;
            }
            depth -= 1;
        }
    }
    lines.len()
}

fn blockquote_body_end(lines: &[&str], start: usize, bq_depth: usize) -> usize {
    for (offset, line) in lines[start..].iter().enumerate() {
        let (depth, _) = count_blockquote_markers(line);
        if depth < bq_depth {
            return start + offset;
        }
    }
    lines.len()
}

#[allow(clippy::too_many_arguments)]
fn try_parse_comment_pi_with_trailing_split(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    block_type: &HtmlBlockType,
    wrapper_kind: SyntaxKind,
    bq_depth: usize,
    fusion: SoftbreakFusion,
    config: &ParserOptions,
) -> Option<usize> {
    let marker: &str = match block_type {
        HtmlBlockType::Comment => "-->",
        HtmlBlockType::ProcessingInstruction => "?>",
        _ => return None,
    };

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
    let close_inner = if bq_depth > 0 {
        strip_n_blockquote_markers(lines[close_line_idx], bq_depth)
    } else {
        lines[close_line_idx]
    };
    if !close_inner[marker_end_in_inner..]
        .bytes()
        .any(|b| !b.is_ascii_whitespace())
    {
        return None;
    }

    Some(parse_raw_html_block_with_trailing(
        builder,
        lines,
        start_pos,
        if bq_depth > 0 {
            strip_n_blockquote_markers(lines[start_pos], bq_depth)
        } else {
            lines[start_pos]
        },
        (close_line_idx, marker_end_in_inner),
        html_block_node_kind(wrapper_kind, block_type, config.dialect),
        bq_depth,
        fusion,
        config,
    ))
}

#[allow(clippy::too_many_arguments)]
fn parse_raw_html_block_with_trailing(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    first_inner: &str,
    (close_line_idx, marker_end_in_inner): (usize, usize),
    wrapper_kind: SyntaxKind,
    bq_depth: usize,
    fusion: SoftbreakFusion,
    config: &ParserOptions,
) -> usize {
    let close_line = lines[close_line_idx];
    let close_inner = if close_line_idx == start_pos {
        first_inner
    } else if bq_depth > 0 {
        strip_n_blockquote_markers(close_line, bq_depth)
    } else {
        close_line
    };
    let close_prefix_len = close_line.len() - close_inner.len();
    let marker_end_in_inner = if close_inner[marker_end_in_inner..]
        .bytes()
        .all(|b| b.is_ascii_whitespace())
    {
        close_inner.len()
    } else {
        marker_end_in_inner
    };
    let trailing = &close_inner[marker_end_in_inner..];

    builder.start_node(wrapper_kind.into());

    if close_line_idx == start_pos {
        builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());
        let close_part = &close_inner[..marker_end_in_inner];
        emit_html_block_line(builder, close_part, 0);
        builder.finish_node();
    } else {
        builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());
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
        emit_html_block_line(builder, close_part, 0);
        builder.finish_node();
    }

    builder.finish_node(); // HTML_BLOCK

    let fusion_end: Option<usize> = match fusion {
        SoftbreakFusion::ToDocEnd => Some(lines.len()),
        SoftbreakFusion::ToFencedDivClose => Some(fenced_div_body_end(lines, close_line_idx + 1)),
        SoftbreakFusion::ToBlockquoteEnd => {
            Some(blockquote_body_end(lines, close_line_idx + 1, bq_depth))
        }
        SoftbreakFusion::None => None,
    };
    if !trailing.is_empty() {
        if let Some(end) = fusion_end
            && close_line_idx + 1 < end
        {
            let mut inner_options = config.clone();
            let refdefs = config.refdef_labels.clone().unwrap_or_default();
            inner_options.refdef_labels = Some(refdefs.clone());
            let mut fragment = String::from(trailing);
            let mut prefix_lines: Vec<ContainerPrefixLine> = vec![ContainerPrefixLine::default()];
            let mut stripped_lens: Vec<usize> = Vec::new();
            for line in &lines[close_line_idx + 1..end] {
                let inner = if bq_depth > 0 {
                    strip_n_blockquote_markers(line, bq_depth)
                } else {
                    line
                };
                let prefix = &line[..line.len() - inner.len()];
                fragment.push_str(inner);
                prefix_lines.push(ContainerPrefixLine::bq_only(prefix.to_string()));
                stripped_lens.push(inner.len());
            }
            let inner_root =
                crate::parser::parse_with_refdefs(&fragment, Some(inner_options), refdefs);
            if let Some(first) = inner_root.first_child() {
                let block_end: usize = first.text_range().end().into();
                let mut consumed_bytes = trailing.len();
                let mut extra_lines = 0usize;
                while extra_lines < stripped_lens.len() && consumed_bytes < block_end {
                    consumed_bytes += stripped_lens[extra_lines];
                    extra_lines += 1;
                }
                let mut bq = ContainerPrefixState::new(prefix_lines);
                graft_subtree(builder, &first, &mut bq);
                return close_line_idx + 1 + extra_lines;
            }
        }

        let mut inner_options = config.clone();
        let refdefs = config.refdef_labels.clone().unwrap_or_default();
        inner_options.refdef_labels = Some(refdefs.clone());
        let inner_root = crate::parser::parse_with_refdefs(trailing, Some(inner_options), refdefs);
        let mut bq = None;
        graft_document_children(builder, &inner_root, LastParaDemote::Never, &mut bq);
    }

    close_line_idx + 1
}

enum RejectedDivBoundary {
    Raw { line: usize, end: usize },
    Markdown,
}

/// Pandoc preserves rejected divs as raw HTML only when a balanced pair or
/// self-closing tag exists. An unclosed rejected div stays Markdown text.
fn rejected_div_boundary(
    lines: &[&str],
    start_pos: usize,
    prefix: &ContainerPrefix,
) -> Option<RejectedDivBoundary> {
    use std::borrow::Cow;

    let first_inner = prefix.strip_line_0_for_emission(lines[start_pos]);
    let open_end =
        find_multiline_open_end(lines, start_pos, first_inner, "div", prefix).unwrap_or(start_pos);
    let mut text = if open_end == start_pos {
        Cow::Borrowed(first_inner)
    } else {
        let mut text = first_inner.to_string();
        for line in &lines[start_pos + 1..=open_end] {
            text.push_str(prefix.strip(line));
        }
        Cow::Owned(text)
    };
    let open_gt = locate_open_tag_close_gt(&text, "div")?;
    let name_start = text.find('<')? + "<div".len();
    if pandoc_html_attribute_names_valid(&text[name_start..open_gt]) {
        return None;
    }

    let body_end = if prefix.bq_depth() > 0 {
        blockquote_body_end(lines, open_end + 1, prefix.bq_depth())
    } else {
        lines.len()
    };
    let mut remaining = if text[..open_gt].ends_with('/') {
        // Pandoc treats a rejected native div as raw HTML, whose slash closes
        // the tag instead of opening a container for following Markdown. Its
        // raw tag reader ends at the first `>`, even inside a quoted value.
        text.find('>')? + 1
    } else {
        let mut close_offset = matched_close_offset(&text[open_gt + 1..], "div", false);
        for line in &lines[open_end + 1..body_end] {
            if close_offset.is_some() {
                break;
            }
            let inner = prefix.strip(line);
            text.to_mut().push_str(inner);
            if inner.contains('>') {
                close_offset = matched_close_offset(&text[open_gt + 1..], "div", false);
            }
        }
        let Some((_, close_end)) = close_offset else {
            return Some(RejectedDivBoundary::Markdown);
        };
        open_gt + 1 + close_end
    };
    for (line_idx, line) in lines.iter().enumerate().take(body_end).skip(start_pos) {
        let inner = if line_idx == start_pos {
            first_inner
        } else {
            prefix.strip(line)
        };
        if remaining <= inner.len() {
            return Some(RejectedDivBoundary::Raw {
                line: line_idx,
                end: remaining,
            });
        }
        remaining -= inner.len();
    }
    None
}

enum StandaloneTagSegment<'a> {
    Whitespace(&'a str),
    Tag(&'a str),
}

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
/// `first_line_content` supplies post-marker content when the caller has
/// already emitted that line's prefixes. Continuation lines remain raw so
/// container boundaries still terminate the block.
#[allow(clippy::too_many_arguments)]
pub(crate) fn parse_html_block_with_wrapper(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    block_type: HtmlBlockType,
    prefix: &ContainerPrefix,
    wrapper_kind: SyntaxKind,
    fusion: SoftbreakFusion,
    config: &ParserOptions,
    first_line_content: Option<&str>,
    myst_directive_closer: Option<&super::myst_directives::DirectiveCloser>,
) -> usize {
    if config.dialect == crate::options::Dialect::CommonMark {
        return parse_commonmark_html_block(
            builder,
            lines,
            start_pos,
            &block_type,
            prefix,
            first_line_content,
            myst_directive_closer,
        );
    }
    let bq_depth = prefix.bq_depth();
    if config.dialect == crate::options::Dialect::Pandoc
        && matches!(&block_type, HtmlBlockType::BlockTag {
            tag_name, is_closing: false, ..
        } if tag_name == "div")
        && let Some(boundary) = rejected_div_boundary(lines, start_pos, prefix)
    {
        match boundary {
            RejectedDivBoundary::Markdown => return start_pos,
            RejectedDivBoundary::Raw { line, end }
                if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV =>
            {
                return parse_raw_html_block_with_trailing(
                    builder,
                    lines,
                    start_pos,
                    prefix.strip_line_0_for_emission(lines[start_pos]),
                    (line, end),
                    SyntaxKind::HTML_BLOCK_RAW,
                    bq_depth,
                    fusion,
                    config,
                );
            }
            _ => {}
        }
    }
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
            fusion,
            config,
        )
    {
        return consumed;
    }

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

    builder.start_node(html_block_node_kind(wrapper_kind, &block_type, config.dialect).into());

    let first_line = lines[start_pos];
    let blank_terminated = ends_at_blank_line(&block_type);

    let (first_inner, first_indent) = first_line_content.map_or_else(
        || prefix.strip_line_0_with_indent_emit(first_line),
        |content| (content, None),
    );

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
        let last_open_line = multiline_open_end.unwrap_or(start_pos);
        let mut opens = 0usize;
        let mut closes = 0usize;
        for (offset, line) in lines[start_pos..=last_open_line].iter().enumerate() {
            let inner = if offset == 0 {
                prefix.strip_dispatch_line(line)
            } else {
                prefix.strip(line)
            };
            let (o, c) = count_tag_balance(inner, tag_name);
            opens += o;
            closes += c;
        }
        depth = opens as i64 - closes as i64;
    }

    let is_same_line_div = wrapper_kind == SyntaxKind::HTML_BLOCK_DIV
        && multiline_open_end.is_none()
        && depth_aware_tag.is_some()
        && depth <= 0;
    let same_line_div_lift_safe = is_same_line_div && bq_depth == 0 && {
        let (line_without_newline, _) = strip_newline(first_inner);
        probe_same_line_lift(line_without_newline, "div")
    };

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
    let same_line_strict_lift_safe = strict_block_tag_name.is_some_and(|name| {
        multiline_open_end.is_none() && depth <= 0 && {
            let (line_no_nl, _) = strip_newline(first_inner);
            probe_same_line_lift(line_no_nl, name)
                && !same_line_trailing_forces_opaque(line_no_nl, name)
        }
    });
    let strict_block_lift = strict_block_tag_name.is_some_and(|name| {
        let (line_no_nl, _) = strip_newline(first_inner);
        if multiline_open_end.is_some() {
            true
        } else if depth > 0 {
            probe_open_tag_line_has_close_gt(line_no_nl, name)
        } else {
            same_line_strict_lift_safe
        }
    });

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
                } if is_pandoc_lift_eligible_block_tag(tag_name) => Some(tag_name.as_str()),
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
    };

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
                } if is_pandoc_lift_eligible_block_tag(tag_name) => Some(tag_name.as_str()),
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
    };

    let lift_mode = (wrapper_kind == SyntaxKind::HTML_BLOCK_DIV
        && bq_depth == 0
        && (!is_same_line_div || same_line_div_lift_safe))
        || strict_block_lift
        || same_line_bq_lift_tag.is_some()
        || bq_messy_lift_tag.is_some()
        || bq_multiline_close_lift_tag.is_some();

    let mut pre_content = String::new();

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
        // Content-container indentation belongs inside the raw block so that
        // both CST round-tripping and verbatim formatting preserve it.
        if let Some(indent) = first_indent {
            builder.token(SyntaxKind::WHITESPACE.into(), indent);
        }
        let (line_without_newline, newline_str) = strip_newline(first_inner);
        if !line_without_newline.is_empty() {
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
        if pre_content.is_empty() && !newline_str.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline_str);
        }
    }

    builder.finish_node(); // HtmlBlockTag

    let void_block = matches!(
        &block_type,
        HtmlBlockType::BlockTag {
            closes_at_open_tag: true,
            ..
        }
    );
    if void_block && let Some(end_line_idx) = multiline_open_end {
        log::trace!(
            "HTML void block at line {} closes after multi-line open ending at line {}",
            start_pos + 1,
            end_line_idx + 1
        );
        builder.finish_node(); // HtmlBlock
        return end_line_idx + 1;
    }
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
        let same_line_lift_tag: Option<&str> = if !lift_mode || pre_content.is_empty() {
            None
        } else if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV && same_line_div_lift_safe {
            Some("div")
        } else if same_line_strict_lift_safe {
            strict_block_tag_name
        } else if let Some(name) = same_line_bq_lift_tag {
            Some(name)
        } else {
            None
        };
        if let Some(tag_name) = same_line_lift_tag {
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
                    if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV {
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

    let fence_suspends_close = config.dialect == crate::options::Dialect::Pandoc
        && config.extensions.fenced_divs
        && depth_aware_tag.is_some();
    let mut body_fence_depth: usize = 0;

    while current_pos < lines.len() {
        let line = lines[current_pos];
        let (line_bq_depth, inner) = count_blockquote_markers(line);

        let gobbled_lazily = config.dialect == crate::options::Dialect::Pandoc
            && bq_depth > 0
            && !line.trim().is_empty();
        if line_bq_depth < bq_depth && !gobbled_lazily {
            break;
        }

        if blank_terminated && inner.trim().is_empty() {
            break;
        }

        if fence_suspends_close {
            if crate::parser::blocks::fenced_divs::try_parse_div_fence_open(inner).is_some() {
                body_fence_depth += 1;
            } else if body_fence_depth > 0
                && crate::parser::blocks::fenced_divs::is_div_closing_fence(inner)
            {
                body_fence_depth -= 1;
            }
        }

        let line_closes = match &depth_aware_tag {
            Some(tag_name) => {
                if fence_suspends_close && body_fence_depth > 0 {
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

            let bq_clean_lift = bq_lift_tag.is_some_and(|_tag_name| {
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
                close_no_nl
                    .trim_start_matches([' ', '\t'])
                    .starts_with("</")
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
                    if leading.is_empty() {
                        emit_bq_prefix_tokens(builder, close_prefix);
                    }
                    emit_html_block_line(builder, close_part, 0);
                    builder.finish_node();
                    current_pos += 1;
                    break;
                }
            }

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
                let close_tag_name = close_split_tag.expect("close_split_tag present");
                let close_marker_end =
                    split_close_marker_end(close_part, close_tag_name).unwrap_or(close_part.len());
                let close_marker = &close_part[..close_marker_end];
                let close_trailing = &close_part[close_marker_end..];

                if prefix.ops().is_empty() {
                    emit_html_block_body_lifted(
                        builder,
                        &pre_content,
                        &content_lines,
                        body_leading,
                        policy,
                        config,
                    );
                } else {
                    emit_html_block_body_lifted_in_containers(
                        builder,
                        &pre_content,
                        &content_lines,
                        body_leading,
                        prefix,
                        policy,
                        config,
                    );
                }
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
                    builder.token(SyntaxKind::TEXT.into(), close_marker);
                    builder.finish_node(); // HTML_BLOCK_TAG
                    builder.finish_node(); // HtmlBlock

                    let mut trailing_text =
                        String::with_capacity(close_trailing.len() + close_post_nl.len());
                    trailing_text.push_str(close_trailing);
                    trailing_text.push_str(close_post_nl);
                    if wrapper_kind == SyntaxKind::HTML_BLOCK_DIV && bq_depth == 0 {
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

        content_lines.push(line);
        current_pos += 1;
    }

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

fn parse_commonmark_html_block(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    block_type: &HtmlBlockType,
    prefix: &ContainerPrefix,
    first_line_content: Option<&str>,
    myst_directive_closer: Option<&super::myst_directives::DirectiveCloser>,
) -> usize {
    use crate::parser::inlines::sink::{InjectedMarker, InlineSink, MarkerInjectingSink};
    use crate::parser::utils::raw_html;

    let (first, indent) = first_line_content.map_or_else(
        || prefix.strip_line_0_with_indent_emit(lines[start_pos]),
        |content| (content, None),
    );
    let mut source = first.to_string();
    let mut markers = Vec::new();
    let mut end_pos = start_pos + 1;
    let mut close_start = None;
    if !is_closing_marker(first, block_type) {
        while let Some(&line) = lines.get(end_pos) {
            let (depth, inner) = count_blockquote_markers(line);
            // An outer directive fence also bounds opaque HTML in its body.
            if myst_directive_closer.is_some_and(|closer| closer.matches(line)) {
                break;
            }
            // Raw HTML accepts no lazy continuation outside its container.
            if depth < prefix.bq_depth()
                || (prefix.list_content_col() > 0
                    && !inner.trim().is_empty()
                    && !prefix.resolve(line).reaches_frame())
                || (ends_at_blank_line(block_type) && inner.trim().is_empty())
            {
                break;
            }
            // List indentation remains raw HTML content. The formatter needs
            // those bytes when it adjusts the enclosing list's indentation.
            let content = strip_n_blockquote_markers(line, prefix.bq_depth());
            let prefix_len = line.len() - content.len();
            for (offset, ch) in line[..prefix_len].char_indices() {
                markers.push((
                    source.len(),
                    InjectedMarker::Indent(&line[offset..offset + ch.len_utf8()]),
                ));
            }
            let closes = is_closing_marker(inner, block_type);
            if closes {
                close_start = Some(source.len());
            }
            source.push_str(content);
            end_pos += 1;
            if closes {
                break;
            }
        }
    }

    let attributes = raw_html::attribute_ranges(&source);
    // Neither line boundary may divide a multiline tag's attribute region.
    let extend_fragment = |mut end| {
        for attrs in &attributes {
            if attrs.start < end && end < attrs.end {
                end = source[attrs.end..]
                    .find('\n')
                    .map_or(source.len(), |newline| attrs.end + newline + 1);
            }
        }
        end
    };
    let first_end = extend_fragment(first.len());
    let close_start = extend_fragment(close_start.unwrap_or(source.len())).max(first_end);
    builder.start_node(SyntaxKind::HTML_BLOCK.into());
    builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());
    if let Some(indent) = indent {
        builder.token(SyntaxKind::WHITESPACE.into(), indent);
    }
    let mut sink = MarkerInjectingSink::new(builder, &markers);
    raw_html::emit_fragment(&mut sink, &source, &attributes, 0..first_end);
    sink.finish_node();
    for (kind, range) in [
        (SyntaxKind::HTML_BLOCK_CONTENT, first_end..close_start),
        (SyntaxKind::HTML_BLOCK_TAG, close_start..source.len()),
    ] {
        if !range.is_empty() {
            sink.start_node_with_leading_markers(kind.into());
            raw_html::emit_fragment(&mut sink, &source, &attributes, range);
            sink.finish_node();
        }
    }
    sink.finish();
    builder.finish_node();
    end_pos
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
    if lift_mode
        && (wrapper_kind == SyntaxKind::HTML_BLOCK_DIV
            || (wrapper_kind == SyntaxKind::HTML_BLOCK && bq_depth == 0))
    {
        if prefix.ops().is_empty() {
            emit_html_block_body_lifted(
                builder,
                pre_content,
                content_lines,
                "",
                LastParaDemote::Never,
                config,
            );
        } else {
            emit_html_block_body_lifted_in_containers(
                builder,
                pre_content,
                content_lines,
                "",
                prefix,
                LastParaDemote::Never,
                config,
            );
        }
        return;
    }
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
    builder.start_node(SyntaxKind::HTML_BLOCK_CONTENT.into());
    if !pre_content.is_empty() {
        builder.token(SyntaxKind::TEXT.into(), pre_content);
    }
    for content_line in content_lines {
        emit_html_block_line(builder, content_line, bq_depth);
    }
    builder.finish_node();
}

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
        let (line, inner) = prefix.split_pieces(cl);
        prefix_lines.push(line);
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
        let (line, inner) = prefix.split_pieces(cl);
        prefix_lines.push(line);
        stripped_lines.push(inner);
    }
    if !leading.is_empty() {
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

/// Body-lift variant for a block inside containers (list items,
/// blockquotes): strips each body line's container prefix, including
/// the close line's `leading` text before the closing tag, before the
/// recursive parse, and reinjects the prefixes when grafting. Without
/// this the inner parse sees a nested list item's indentation and reads
/// the body as indented code. `pre_content` (open-tag trailing bytes)
/// has no prefix of its own.
fn emit_html_block_body_lifted_in_containers(
    builder: &mut GreenNodeBuilder<'static>,
    pre_content: &str,
    content_lines: &[&str],
    leading: &str,
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
        let (line, inner) = prefix.split_pieces(cl);
        prefix_lines.push(line);
        stripped_lines.push(inner);
    }
    let (leading_prefix, leading_rest) = prefix.split_pieces(leading);
    if !leading_rest.is_empty() {
        prefix_lines.push(leading_prefix);
    }
    let mut state = ContainerPrefixState::new(prefix_lines);
    emit_html_block_body_lifted_inner(
        builder,
        pre_content,
        &stripped_lines,
        leading_rest,
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

fn graft_subtree(
    builder: &mut GreenNodeBuilder<'static>,
    node: &SyntaxNode,
    bq: &mut Option<ContainerPrefixState>,
) {
    graft_subtree_as(builder, node, node.kind(), bq);
}

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

fn emit_bq_prefix_tokens(builder: &mut GreenNodeBuilder<'static>, prefix: &str) {
    for ch in prefix.chars() {
        let mut buf = [0u8; 4];
        builder.token(SyntaxKind::LINE_PREFIX.into(), ch.encode_utf8(&mut buf));
    }
}

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
    let Some((_, close_end)) = matched_close_offset(trailing, tag_name, true) else {
        return false;
    };
    let after_close = &trailing[close_end..];
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
    matched_close_offset(trailing, tag_name, true).is_some()
}

fn matched_close_offset(
    trailing: &str,
    tag_name: &str,
    native_divs: bool,
) -> Option<(usize, usize)> {
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
            } else if !self_close || (native_divs && tag_lower == "div") {
                depth += 1;
            }
        }

        if found_gt {
            i = j + 1;
        } else {
            break;
        }
    }
    None
}

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
    let (close_start, _close_end) = matched_close_offset(line, tag_name, true)?;
    Some((&line[..close_start], &line[close_start..]))
}

fn find_next_matched_pair(s: &str, tag_name: &str) -> Option<(usize, usize)> {
    let bytes = s.as_bytes();
    let mut search = 0usize;
    while let Some(rel) = bytes[search..].iter().position(|&b| b == b'<') {
        let lt = search + rel;
        if let Some(gt_rel) = locate_open_tag_close_gt(&s[lt..], tag_name) {
            let after_open = lt + gt_rel + 1;
            if let Some((_close_start, close_end)) =
                matched_close_offset(&s[after_open..], tag_name, true)
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

fn emit_open_tag_tokens<'a>(
    builder: &mut GreenNodeBuilder<'static>,
    line: &'a str,
    tag_name: &str,
    lift_trailing: bool,
) -> &'a str {
    let bytes = line.as_bytes();
    let indent_end = bytes.iter().position(|&b| b != b' ').unwrap_or(bytes.len());
    if indent_end > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &line[..indent_end]);
    }
    let rest = &line[indent_end..];
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
        builder.token(SyntaxKind::TEXT.into(), rest);
        return "";
    };
    let attrs_inner = &after_name[..tag_close];
    let ws_end = attrs_inner
        .as_bytes()
        .iter()
        .position(|&b| !matches!(b, b' ' | b'\t'))
        .unwrap_or(attrs_inner.len());
    let leading_ws = &attrs_inner[..ws_end];
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
        return after_gt;
    }
    if !after_gt.is_empty() {
        builder.token(SyntaxKind::TEXT.into(), after_gt);
    }
    ""
}

fn find_multiline_open_end(
    lines: &[&str],
    start_pos: usize,
    first_inner: &str,
    tag_name: &str,
    prefix: &ContainerPrefix,
) -> Option<usize> {
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
        let inner = if offset == start_pos {
            prefix.strip_line_0_for_emission(line)
        } else {
            prefix.strip(line)
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

/// Apply the same Pandoc HTML start gate to dispatch and lazy interruption.
/// Rejecting an unclosed div before dispatch keeps paragraph and container
/// boundaries intact, rather than repairing a lifted div after emission.
pub(crate) fn pandoc_html_block_start_is_valid(
    lines: &[&str],
    start_pos: usize,
    prefix: &ContainerPrefix,
    block_type: &HtmlBlockType,
) -> bool {
    let HtmlBlockType::BlockTag {
        tag_name,
        is_closing,
        ..
    } = block_type
    else {
        return true;
    };
    pandoc_html_open_tag_closes(lines, start_pos, prefix)
        && (*is_closing
            || tag_name != "div"
            || !matches!(
                rejected_div_boundary(lines, start_pos, prefix),
                Some(RejectedDivBoundary::Markdown)
            ))
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
            let bytes = line_no_nl.as_bytes();
            let indent_end = bytes.iter().position(|&b| b != b' ').unwrap_or(bytes.len());
            if indent_end > 0 {
                builder.token(SyntaxKind::WHITESPACE.into(), &line_no_nl[..indent_end]);
            }
            let after_indent = &line_no_nl[indent_end..];
            if after_indent.len() >= prefix_len {
                builder.token(SyntaxKind::TEXT.into(), &after_indent[..prefix_len]);
                let rest = &after_indent[prefix_len..];
                emit_attr_region(builder, rest);
            } else {
                builder.token(SyntaxKind::TEXT.into(), after_indent);
            }
        } else if line_idx < end_line_idx {
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
            let bytes = line_no_nl.as_bytes();
            let indent_end = bytes
                .iter()
                .position(|&b| !matches!(b, b' ' | b'\t'))
                .unwrap_or(bytes.len());
            if indent_end > 0 {
                builder.token(SyntaxKind::WHITESPACE.into(), &line_no_nl[..indent_end]);
            }
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
                builder.token(SyntaxKind::TEXT.into(), &line_no_nl[indent_end..]);
                if !newline_str.is_empty() {
                    builder.token(SyntaxKind::NEWLINE.into(), newline_str);
                }
                continue;
            };
            let attrs_region = &line_no_nl[indent_end..gt];
            let region_bytes = attrs_region.as_bytes();
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

fn emit_attr_region(builder: &mut GreenNodeBuilder<'static>, region: &str) {
    if region.is_empty() {
        return;
    }
    let bytes = region.as_bytes();
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

fn emit_html_block_line(builder: &mut GreenNodeBuilder<'static>, line: &str, bq_depth: usize) {
    let inner = if bq_depth > 0 {
        let stripped = strip_n_blockquote_markers(line, bq_depth);
        let prefix_len = line.len() - stripped.len();
        if prefix_len > 0 {
            for ch in line[..prefix_len].chars() {
                let mut buf = [0u8; 4];
                builder.token(SyntaxKind::LINE_PREFIX.into(), ch.encode_utf8(&mut buf));
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
        assert_eq!(
            try_parse_html_block_start("<!DOCTYPE html>", true),
            Some(HtmlBlockType::Declaration)
        );
        assert_eq!(
            try_parse_html_block_start("<!doctype html>", true),
            Some(HtmlBlockType::Declaration)
        );
        assert_eq!(try_parse_html_block_start("<!DOCTYPE html>", false), None);
        assert_eq!(try_parse_html_block_start("<!doctype html>", false), None);
    }

    #[test]
    fn test_dialect_specific_block_tag_membership() {
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
            SoftbreakFusion::ToDocEnd,
            &opts,
            None,
            None,
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
            SoftbreakFusion::ToDocEnd,
            &opts,
            None,
            None,
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
            SoftbreakFusion::ToDocEnd,
            &opts,
            None,
            None,
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
            SoftbreakFusion::ToDocEnd,
            &opts,
            None,
            None,
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
            SoftbreakFusion::ToDocEnd,
            &opts,
            None,
            None,
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
            SoftbreakFusion::ToDocEnd,
            &opts,
            None,
            None,
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
            SoftbreakFusion::ToDocEnd,
            &opts,
            None,
            None,
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
            SoftbreakFusion::ToDocEnd,
            &opts,
            None,
            None,
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
            SoftbreakFusion::ToDocEnd,
            &opts,
            None,
            None,
        );

        // Block contains <div>\nfoo\n; stops at blank line (line 2).
        assert_eq!(new_pos, 2);
    }
}
