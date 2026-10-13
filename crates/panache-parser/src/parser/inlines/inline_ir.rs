//! Inline IR for both CommonMark and Pandoc dialects.
//!
//! The inline parsing pipeline runs in three passes over an intermediate
//! representation (IR):
//!
//! 1. **Scan** ([`build_ir`]): walk the source bytes once, producing a flat
//!    [`Vec<IrEvent>`]. Opaque higher-precedence constructs (escapes, code
//!    spans, autolinks, raw HTML, plus Pandoc math / native spans / inline
//!    footnotes / footnote references / citations / bracketed spans) are
//!    skipped past as a single [`IrEvent::Construct`] event whose source
//!    range is preserved for losslessness. Delimiter runs (`*`/`_`),
//!    bracket markers (`[`, `![`, `]`), soft line breaks, and plain text
//!    spans become distinct events.
//!
//! 2. **Process brackets** ([`process_brackets`]) — CommonMark §6.3: the
//!    bracket-stack algorithm walks `]` markers left-to-right. For each
//!    `]`, the algorithm finds the nearest active opener and tries to
//!    resolve the pair as a link or image: inline `[text](dest)`, full
//!    reference `[text][label]`, collapsed `[text][]`, or shortcut
//!    `[text]`. Under CommonMark, reference forms are validated against
//!    the document refdef map and a successful match deactivates all
//!    earlier active openers (§6.3 "links may not contain other links").
//!    Under Pandoc, reference forms resolve shape-only (any non-empty
//!    label) and the deactivation pass is skipped; outer-wins nested-link
//!    semantics are enforced by the emission walk's `suppress_inner_links`
//!    flag instead.
//!
//! 3. **Process emphasis** ([`process_emphasis_in_range`]): the classic
//!    delimiter-stack algorithm runs over the [`IrEvent::DelimRun`]
//!    events, pairing openers with closers and recording matches on the
//!    runs. Runs first scoped per resolved bracket pair (innermost
//!    first), then a top-level pass over the residual events. Each match
//!    consumes 1 or 2 inner-edge bytes from each side; leftover bytes
//!    fall through to literal text. Dialect gates (Pandoc flanking rules,
//!    mod-3 rejection, asymmetric (1,2)/(2,1) rejection, opener-count >= 4
//!    rejection, triple-emph nesting flip, cascade-then-rerun) branch on
//!    the `dialect` parameter.
//!
//! The emission walk in [`super::core::parse_inline_range_impl`] consumes
//! three byte-keyed plans built by [`build_full_plans`]: an
//! [`EmphasisPlan`] for delim-run dispositions, a [`BracketPlan`] for
//! resolved link/image bracket pairs, and a [`ConstructPlan`] for
//! standalone Pandoc constructs (inline footnotes, native spans, footnote
//! references, citations, bracketed spans). Matched delim runs become
//! `EMPHASIS` / `STRONG` nodes; matched bracket pairs become `LINK` /
//! `IMAGE` nodes via the dispatcher's `try_parse_*` recognizers (called
//! to *parse* a matched range, not to *resolve* it). Unmatched delims and
//! brackets fall through to plain text.

use crate::options::ParserOptions;
use crate::parser::inlines::hard_breaks;
use crate::parser::inlines::refdef_map::{RefdefMap, normalize_label};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmphasisKind {
    Emph,
    Strong,
}

/// Disposition of a single delimiter byte after emphasis resolution.
#[derive(Debug, Clone, Copy)]
pub enum DelimChar {
    /// Start of an opening marker. The marker spans `len` bytes from this
    /// position; the matching closer starts at `partner` and spans
    /// `partner_len` bytes.
    Open {
        len: u8,
        partner: usize,
        partner_len: u8,
        kind: EmphasisKind,
    },
    /// Start of a closing marker. The matching opener starts at `partner`.
    /// Emission jumps past close markers via the matching `Open` entry, so
    /// this variant is only consulted defensively.
    Close,
    /// Unmatched delimiter byte; emit as literal text.
    Literal,
}

/// Byte-keyed disposition map for `*` / `_` delimiter chars produced by
/// the IR's emphasis pass and consumed by the inline emission walk.
#[derive(Debug, Default, Clone)]
pub struct EmphasisPlan {
    by_pos: BTreeMap<usize, DelimChar>,
}

impl EmphasisPlan {
    pub fn lookup(&self, pos: usize) -> Option<DelimChar> {
        self.by_pos.get(&pos).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.by_pos.is_empty()
    }

    /// Construct an `EmphasisPlan` from a byte-keyed disposition map.
    pub fn from_dispositions(by_pos: BTreeMap<usize, DelimChar>) -> Self {
        Self { by_pos }
    }
}

use super::bracketed_spans::try_parse_bracketed_span;
use super::citations::{try_parse_bare_citation, try_parse_bracketed_citation};
use super::code_spans::try_parse_code_span;
use super::escapes::{EscapeType, try_parse_escape};
use super::inline_footnotes::{try_parse_footnote_reference, try_parse_inline_footnote};
use super::inline_html::try_parse_inline_html;
use super::latex::try_parse_raw_math_environment;
use super::links::{
    LinkScanContext, try_parse_autolink, try_parse_inline_image, try_parse_inline_link,
    try_parse_reference_image, try_parse_reference_link,
};
use super::math::{
    try_parse_display_math, try_parse_double_backslash_display_math,
    try_parse_double_backslash_inline_math, try_parse_gfm_inline_math, try_parse_inline_math,
    try_parse_single_backslash_display_math, try_parse_single_backslash_inline_math,
};
use super::native_spans::try_parse_native_span;

/// One event in the inline IR.
///
/// Events partition the source byte range covered by the IR exactly: their
/// `range()` values are contiguous and non-overlapping, so concatenating
/// them reproduces the original input. This is the losslessness invariant
/// the emission pass relies on.
#[derive(Debug, Clone)]
pub enum IrEvent {
    /// Plain text byte span. Emitted as a single `TEXT` token, possibly
    /// merged with adjacent literal-disposition delim/bracket bytes.
    Text { start: usize, end: usize },

    /// An opaque higher-precedence construct (escape, code span, autolink,
    /// raw HTML). The emission pass re-parses these from the source byte
    /// range using the existing per-construct emitters; we don't store a
    /// pre-built `GreenNode` because `rowan::GreenNodeBuilder` doesn't
    /// support inserting subtrees directly. The byte range is what makes
    /// emission well-defined — the construct kind is recovered by the
    /// emitter dispatching on the leading byte.
    Construct {
        start: usize,
        end: usize,
        kind: ConstructKind,
    },

    /// A `*` or `_` delimiter run. The `matches` vec is filled in by
    /// [`process_emphasis`]; before that pass it is empty.
    DelimRun {
        ch: u8,
        start: usize,
        end: usize,
        can_open: bool,
        can_close: bool,
        /// Matched fragments produced by `process_emphasis`. Each entry
        /// is one `(byte_offset_within_run, len, partner_event_idx,
        /// partner_byte_offset, kind, is_opener)` tuple. Empty until the
        /// pass runs; possibly multiple entries when a single run matches
        /// at multiple positions (e.g. a 4-run that closes 2+2 pairs).
        matches: Vec<DelimMatch>,
    },

    /// `[` or `![` bracket marker. Resolved by [`process_brackets`].
    OpenBracket {
        start: usize,
        /// `start + 1` for `[`, `start + 2` for `![`.
        end: usize,
        is_image: bool,
        /// True until a later resolution rule deactivates this opener.
        active: bool,
        /// Filled in when the matching `CloseBracket` resolves the pair
        /// to a link / image.
        resolution: Option<BracketResolution>,
        /// Pandoc-only: extents of an unresolved bracket-shape pattern
        /// (full reference / collapsed / shortcut whose label doesn't
        /// match a refdef). Mutually exclusive with `resolution:
        /// Some(...)`. When `Some`, emission wraps `[start, end)` in
        /// an `UNRESOLVED_REFERENCE` node so downstream tools can
        /// attach behavior to the bracket-shape pattern. Always
        /// `None` under `Dialect::CommonMark`.
        unresolved_ref: Option<UnresolvedRefShape>,
    },

    /// `]` bracket marker. Resolved by [`process_brackets`].
    CloseBracket {
        pos: usize,
        /// True if this `]` was paired with an opener and the pair was
        /// turned into a link / image.
        matched: bool,
    },

    /// A soft line break (a `\n` or `\r\n` ending a paragraph-internal
    /// line). Includes the line-ending bytes verbatim.
    SoftBreak { start: usize, end: usize },

    /// A hard line break (`  \n` / `\\\n` / `   \n` etc.). Includes any
    /// trailing-space bytes plus the line ending.
    HardBreak { start: usize, end: usize },
}

impl IrEvent {
    /// The source byte range this event covers.
    pub fn range(&self) -> (usize, usize) {
        match self {
            IrEvent::Text { start, end }
            | IrEvent::Construct { start, end, .. }
            | IrEvent::DelimRun { start, end, .. }
            | IrEvent::OpenBracket { start, end, .. }
            | IrEvent::SoftBreak { start, end }
            | IrEvent::HardBreak { start, end } => (*start, *end),
            IrEvent::CloseBracket { pos, .. } => (*pos, *pos + 1),
        }
    }
}

/// Categorical tag for a [`IrEvent::Construct`] event so emission knows
/// which parser to call to rebuild the CST subtree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstructKind {
    /// An opaque Hugo call or paired shortcode region.
    HugoShortcode,
    /// `\X` literal-character escape (CommonMark §2.4).
    Escape,
    /// `` `code` `` span (§6.1).
    CodeSpan,
    /// `<scheme://...>` or `<email@host>` (§6.5).
    Autolink,
    /// `<tag ...>` and friends (§6.6).
    InlineHtml,
    /// Pandoc opaque construct that doesn't have a dedicated kind yet
    /// (currently: math spans and raw TeX math environments). Pre-recognised in `build_ir` under
    /// `Dialect::Pandoc` solely so the emphasis pass treats the entire
    /// construct as opaque and delim runs inside don't cross its
    /// boundary. Emission re-parses the construct via the dispatcher's
    /// existing `try_parse_*` chain.
    PandocOpaque,
    /// Pandoc inline footnote `^[note text]`. Recognised in `build_ir`
    /// under `Dialect::Pandoc` and consumed by the emission walk via
    /// the IR's `ConstructPlan`. The dispatcher's legacy `^[` branch
    /// is gated to CommonMark dialect only.
    InlineFootnote,
    /// Pandoc native span `<span ...>...</span>`. Recognised in
    /// `build_ir` under `Dialect::Pandoc` and consumed by the emission
    /// walk via the IR's `ConstructPlan`. The dispatcher's legacy
    /// `<span>` branch is gated to CommonMark dialect only.
    NativeSpan,
    /// Pandoc footnote reference `[^id]`. Recognised in `build_ir`
    /// under `Dialect::Pandoc` and consumed by the emission walk via
    /// the IR's `ConstructPlan`. The dispatcher's legacy `[^id]`
    /// branch is gated to CommonMark dialect only.
    FootnoteReference,
    /// Pandoc bracketed citation `[@key]`, `[see @key, p. 1]`,
    /// `[@a; @b]`. Recognised in `build_ir` under `Dialect::Pandoc`
    /// and consumed by the emission walk via the IR's `ConstructPlan`.
    /// The dispatcher's legacy `[@cite]` branch is gated to CommonMark
    /// dialect only.
    BracketedCitation,
    /// Pandoc bare citation `@key` or `-@key` (author-in-text /
    /// suppress-author). Recognised in `build_ir` under
    /// `Dialect::Pandoc` and consumed by the emission walk via the
    /// IR's `ConstructPlan`. The dispatcher's legacy `@` and `-@`
    /// branches are gated to CommonMark dialect only.
    BareCitation,
    /// Pandoc bracketed span `[content]{attrs}`. Recognised in
    /// `build_ir` under `Dialect::Pandoc` and consumed by the emission
    /// walk via the IR's `ConstructPlan`. The dispatcher's legacy
    /// `[text]{attrs}` branch is gated to CommonMark dialect only.
    BracketedSpan,
    /// Pandoc wikilink `[[url]]` / `[[url|title]]` / `![[url]]` /
    /// `![[url|title]]`. Recognised in `build_ir` when either
    /// `wikilinks_title_after_pipe` or `wikilinks_title_before_pipe` is
    /// enabled. Dialect-agnostic (pandoc accepts the extension on both
    /// `markdown+` and `commonmark+`). The emission walk dispatches via
    /// the IR's `ConstructPlan`; the `is_image` variant is recovered by
    /// peeking the leading byte of the source range.
    WikiLink,
}

/// One matched fragment within a [`IrEvent::DelimRun`].
#[derive(Debug, Clone, Copy)]
pub struct DelimMatch {
    /// Byte offset of this fragment relative to the run's `start`.
    pub offset_in_run: u8,
    /// Number of bytes in this fragment (1 or 2).
    pub len: u8,
    /// Whether this fragment is the opener (`true`) or closer of the pair.
    pub is_opener: bool,
    /// IR event index of the partner run.
    pub partner_event: u32,
    /// Byte offset within the partner run of the partner fragment.
    pub partner_offset: u8,
    /// Emphasis kind (Emph for `len == 1`, Strong for `len == 2`).
    pub kind: EmphasisKind,
}

/// Pandoc-only: extents of an unresolved bracket-shape reference
/// pattern. Recorded on `IrEvent::OpenBracket.unresolved_ref` when the
/// no-resolution fall-through fires under `Dialect::Pandoc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnresolvedRefShape {
    /// IR event index of the matching `CloseBracket`. Used by the
    /// scoped-emphasis pass to treat the wrapper as a tree boundary.
    pub close_event: u32,
    /// One past the end of the inner text (the byte position of the
    /// outer `]`). Combined with the opener's `end` field, this is the
    /// inner text range that goes through normal inline parsing.
    pub text_end: usize,
    /// One past the end of the full bracket-shape pattern. For
    /// shortcut form `[text]`: `close_pos + 1`. For collapsed
    /// `[text][]`: `close_pos + 3`. For full `[text][label]`: the byte
    /// after the closing `]` of `[label]`.
    pub end: usize,
}

/// Successful bracket resolution: the `[`...`]` pair is a link or image.
#[derive(Debug, Clone)]
pub struct BracketResolution {
    /// IR event index of the matching `CloseBracket`.
    pub close_event: u32,
    /// Source range of the link text (between `[`/`![` and `]`).
    pub text_start: usize,
    pub text_end: usize,
    /// Source range of the link suffix (`(...)`, `[label]`, `[]`, or
    /// empty for shortcut). When `kind == ShortcutReference`,
    /// `suffix_start == suffix_end == close_pos + 1`.
    pub suffix_start: usize,
    pub suffix_end: usize,
    pub kind: LinkKind,
}

/// What kind of link/image we resolved a bracket pair to.
#[derive(Debug, Clone)]
pub enum LinkKind {
    /// `[text](dest)` or `[text](dest "title")`.
    Inline { dest: String, title: Option<String> },
    /// `[text][label]` — explicit reference.
    FullReference { label: String },
    /// `[text][]` — collapsed reference. Label is the link text.
    CollapsedReference,
    /// `[text]` — shortcut reference. Label is the link text.
    ShortcutReference,
}

/// Scan `text[start..end]` once, producing a flat IR of events.
///
/// The scan is forward-only and never backtracks: each iteration either
/// consumes a known construct (escape, code span, autolink, raw HTML),
/// records a delim run / bracket marker / line break, or steps past a
/// single UTF-8 boundary as plain text. Adjacent text bytes are coalesced
/// into a single [`IrEvent::Text`] event by the run-flush step.
pub fn build_ir(text: &str, start: usize, end: usize, config: &ParserOptions) -> Vec<IrEvent> {
    let mut events = Vec::new();
    let hugo = build_hugo_index(text, start, end, config);
    build_ir_into(text, start, end, config, &mut events, &hugo);
    events
}

fn build_hugo_index(
    text: &str,
    start: usize,
    end: usize,
    config: &ParserOptions,
) -> super::hugo::HugoIndex {
    if config.extensions.hugo_shortcodes {
        super::hugo::HugoIndex::for_range(text, start, end)
    } else {
        super::hugo::HugoIndex::default()
    }
}

/// Like [`build_ir`] but writes into a caller-provided `Vec<IrEvent>`,
/// clearing it first. Used by [`build_full_plans`] to amortise the
/// per-call allocation through a thread-local scratch pool.
pub(super) fn build_ir_into(
    text: &str,
    start: usize,
    end: usize,
    config: &ParserOptions,
    events: &mut Vec<IrEvent>,
    hugo: &super::hugo::HugoIndex,
) {
    events.clear();
    let bytes = text.as_bytes();
    let exts = &config.extensions;
    let is_commonmark = config.dialect == crate::options::Dialect::CommonMark;

    let mut pos = start;
    let mut text_run_start = start;
    let mut pandoc_bracket_extent: usize = 0;

    let mask = build_ir_byte_mask(config);

    macro_rules! flush_text {
        () => {
            if pos > text_run_start {
                events.push(IrEvent::Text {
                    start: text_run_start,
                    end: pos,
                });
            }
        };
    }

    while pos < end {
        while pos < end && !mask[bytes[pos] as usize] {
            pos += 1;
        }
        if pos >= end {
            break;
        }
        let b = bytes[pos];

        if !is_commonmark
            && pos >= pandoc_bracket_extent
            && (b == b'[' || (b == b'!' && pos + 1 < end && bytes[pos + 1] == b'['))
            && let Some(len) = try_pandoc_bracket_link_extent(text, pos, end, config)
        {
            pandoc_bracket_extent = pos + len;
        }
        let in_pandoc_bracket = !is_commonmark && pos < pandoc_bracket_extent;

        if b == b'\\'
            && let Some((len, _ch, escape_type)) = try_parse_escape(&text[pos..])
            && pos + len <= end
        {
            let enabled = match escape_type {
                EscapeType::Literal => is_commonmark || exts.all_symbols_escapable,
                EscapeType::HardLineBreak => {
                    exts.escaped_line_breaks
                        && !(is_commonmark && hard_breaks::ends_block(text, pos + len, end))
                }
                EscapeType::NonbreakingSpace => exts.all_symbols_escapable,
            };
            if enabled {
                if escape_type == EscapeType::HardLineBreak {
                    let ws_start = hard_breaks::ws_run_start(bytes, text_run_start, pos);
                    if ws_start > text_run_start {
                        events.push(IrEvent::Text {
                            start: text_run_start,
                            end: ws_start,
                        });
                    }
                    events.push(IrEvent::HardBreak {
                        start: ws_start,
                        end: pos + len,
                    });
                    pos += len;
                    text_run_start = pos;
                    continue;
                }
                flush_text!();
                events.push(IrEvent::Construct {
                    start: pos,
                    end: pos + len,
                    kind: ConstructKind::Escape,
                });
                pos += len;
                text_run_start = pos;
                continue;
            }
        }

        if b == b'{'
            && exts.hugo_shortcodes
            && let Some(span) = hugo.scan_at(&text[..end], pos)
        {
            flush_text!();
            events.push(IrEvent::Construct {
                start: pos,
                end: pos + span.end,
                kind: ConstructKind::HugoShortcode,
            });
            pos += span.end;
            text_run_start = pos;
            continue;
        }

        if b == b'`'
            && let Some((len, _, _, _)) = try_parse_code_span(&text[pos..])
            && pos + len <= end
        {
            flush_text!();
            events.push(IrEvent::Construct {
                start: pos,
                end: pos + len,
                kind: ConstructKind::CodeSpan,
            });
            pos += len;
            text_run_start = pos;
            continue;
        }

        if !is_commonmark && let Some(len) = try_pandoc_math_opaque(text, pos, end, config) {
            flush_text!();
            events.push(IrEvent::Construct {
                start: pos,
                end: pos + len,
                kind: ConstructKind::PandocOpaque,
            });
            pos += len;
            text_run_start = pos;
            continue;
        }

        if !is_commonmark
            && !in_pandoc_bracket
            && b == b'<'
            && exts.native_spans
            && let Some((len, _, _)) = try_parse_native_span(&text[pos..], config)
            && pos + len <= end
        {
            flush_text!();
            events.push(IrEvent::Construct {
                start: pos,
                end: pos + len,
                kind: ConstructKind::NativeSpan,
            });
            pos += len;
            text_run_start = pos;
            continue;
        }

        if b == b'<' && !in_pandoc_bracket {
            if exts.autolinks
                && let Some((len, _)) = try_parse_autolink(&text[pos..], is_commonmark)
                && pos + len <= end
            {
                flush_text!();
                events.push(IrEvent::Construct {
                    start: pos,
                    end: pos + len,
                    kind: ConstructKind::Autolink,
                });
                pos += len;
                text_run_start = pos;
                continue;
            }
            if exts.raw_html
                && let Some(len) = try_parse_inline_html(&text[pos..], config.dialect)
                && pos + len <= end
            {
                flush_text!();
                events.push(IrEvent::Construct {
                    start: pos,
                    end: pos + len,
                    kind: ConstructKind::InlineHtml,
                });
                pos += len;
                text_run_start = pos;
                continue;
            }
        }

        if !is_commonmark
            && b == b'^'
            && exts.inline_footnotes
            && let Some((len, _)) = try_parse_inline_footnote(&text[pos..])
            && pos + len <= end
        {
            flush_text!();
            events.push(IrEvent::Construct {
                start: pos,
                end: pos + len,
                kind: ConstructKind::InlineFootnote,
            });
            pos += len;
            text_run_start = pos;
            continue;
        }

        if !is_commonmark
            && b == b'['
            && pos + 1 < end
            && bytes[pos + 1] == b'^'
            && exts.footnotes
            && let Some((len, _)) = try_parse_footnote_reference(&text[pos..])
            && pos + len <= end
        {
            flush_text!();
            events.push(IrEvent::Construct {
                start: pos,
                end: pos + len,
                kind: ConstructKind::FootnoteReference,
            });
            pos += len;
            text_run_start = pos;
            continue;
        }

        if !is_commonmark
            && b == b'['
            && exts.citations
            && let Some((len, _)) = try_parse_bracketed_citation(&text[pos..])
            && pos + len <= end
        {
            flush_text!();
            events.push(IrEvent::Construct {
                start: pos,
                end: pos + len,
                kind: ConstructKind::BracketedCitation,
            });
            pos += len;
            text_run_start = pos;
            continue;
        }

        if !is_commonmark
            && (b == b'@' || (b == b'-' && pos + 1 < end && bytes[pos + 1] == b'@'))
            && (exts.citations || exts.quarto_crossrefs)
            && let Some((len, _, _)) = try_parse_bare_citation(text, pos)
            && pos + len <= end
        {
            flush_text!();
            events.push(IrEvent::Construct {
                start: pos,
                end: pos + len,
                kind: ConstructKind::BareCitation,
            });
            pos += len;
            text_run_start = pos;
            continue;
        }

        if !is_commonmark
            && b == b'['
            && exts.bracketed_spans
            && let Some((len, _, _)) = try_parse_bracketed_span(&text[pos..])
            && pos + len <= end
        {
            flush_text!();
            events.push(IrEvent::Construct {
                start: pos,
                end: pos + len,
                kind: ConstructKind::BracketedSpan,
            });
            pos += len;
            text_run_start = pos;
            continue;
        }

        if (b == b'[' || b == b'!')
            && super::wikilinks::any_enabled(config)
            && let Some(span) = super::wikilinks::try_parse_wikilink(text, pos, config)
            && span.end <= end
        {
            flush_text!();
            events.push(IrEvent::Construct {
                start: span.start,
                end: span.end,
                kind: ConstructKind::WikiLink,
            });
            pos = span.end;
            text_run_start = pos;
            continue;
        }

        if b == b'!'
            && pos + 1 < end
            && bytes[pos + 1] == b'['
            && (exts.inline_images || exts.reference_links)
        {
            flush_text!();
            events.push(IrEvent::OpenBracket {
                start: pos,
                end: pos + 2,
                is_image: true,
                active: true,
                resolution: None,
                unresolved_ref: None,
            });
            pos += 2;
            text_run_start = pos;
            continue;
        }

        if b == b'[' && (exts.inline_links || exts.reference_links) {
            flush_text!();
            events.push(IrEvent::OpenBracket {
                start: pos,
                end: pos + 1,
                is_image: false,
                active: true,
                resolution: None,
                unresolved_ref: None,
            });
            pos += 1;
            text_run_start = pos;
            continue;
        }

        if b == b']' {
            flush_text!();
            events.push(IrEvent::CloseBracket {
                pos,
                matched: false,
            });
            pos += 1;
            text_run_start = pos;
            continue;
        }

        if b == b'*' || b == b'_' {
            flush_text!();
            let mut run_end = pos;
            while run_end < end && bytes[run_end] == b {
                run_end += 1;
            }
            let count = run_end - pos;
            let (can_open, can_close) = compute_flanking(text, pos, count, b, config.dialect);
            events.push(IrEvent::DelimRun {
                ch: b,
                start: pos,
                end: run_end,
                can_open,
                can_close,
                matches: Vec::new(),
            });
            pos = run_end;
            text_run_start = pos;
            continue;
        }

        if b == b'\n' || (b == b'\r' && pos + 1 < end && bytes[pos + 1] == b'\n') {
            let nl_len = if b == b'\r' { 2 } else { 1 };
            let (s, wide_enough) = hard_breaks::trailing_ws_run(bytes, text_run_start, pos);
            if wide_enough && !hard_breaks::ends_block(text, pos + nl_len, end) {
                if s > text_run_start {
                    events.push(IrEvent::Text {
                        start: text_run_start,
                        end: s,
                    });
                }
                events.push(IrEvent::HardBreak {
                    start: s,
                    end: pos + nl_len,
                });
                pos += nl_len;
                text_run_start = pos;
                continue;
            }

            flush_text!();
            events.push(IrEvent::SoftBreak {
                start: pos,
                end: pos + nl_len,
            });
            pos += nl_len;
            text_run_start = pos;
            continue;
        }

        let ch_len = text[pos..]
            .chars()
            .next()
            .map_or(1, std::primitive::char::len_utf8);
        pos += ch_len.max(1);
    }

    flush_text!();
}

fn build_ir_byte_mask(config: &ParserOptions) -> [bool; 256] {
    let mut mask = [false; 256];
    let exts = &config.extensions;
    let is_commonmark = config.dialect == crate::options::Dialect::CommonMark;

    mask[b'\n' as usize] = true;
    mask[b'\r' as usize] = true;
    mask[b'\\' as usize] = true;
    mask[b'`' as usize] = true;
    mask[b'{' as usize] = exts.hugo_shortcodes;
    mask[b'*' as usize] = true;
    mask[b'_' as usize] = true;

    if exts.inline_links
        || exts.reference_links
        || exts.inline_images
        || exts.bracketed_spans
        || exts.footnotes
        || exts.citations
    {
        mask[b'[' as usize] = true;
        mask[b']' as usize] = true;
    }
    if exts.inline_images || exts.reference_links {
        mask[b'!' as usize] = true;
    }

    if exts.autolinks || exts.raw_html || (!is_commonmark && exts.native_spans) {
        mask[b'<' as usize] = true;
    }

    if !is_commonmark && exts.inline_footnotes {
        mask[b'^' as usize] = true;
    }

    if !is_commonmark && (exts.citations || exts.quarto_crossrefs) {
        mask[b'@' as usize] = true;
        mask[b'-' as usize] = true;
    }

    if !is_commonmark
        && (exts.tex_math_dollars
            || exts.tex_math_gfm
            || exts.tex_math_single_backslash
            || exts.tex_math_double_backslash)
    {
        mask[b'$' as usize] = true;
    }

    mask
}

fn compute_flanking(
    text: &str,
    pos: usize,
    count: usize,
    ch: u8,
    dialect: crate::options::Dialect,
) -> (bool, bool) {
    if dialect == crate::options::Dialect::Pandoc {
        let prev_char = (pos > 0).then(|| text[..pos].chars().last()).flatten();
        let next_char = text.get(pos + count..).and_then(|s| s.chars().next());
        let followed_by_ws = next_char.is_none_or(|c| c.is_whitespace());

        let mut can_open = !followed_by_ws;
        let mut can_close = true;

        if ch == b'_' {
            let prev_is_alnum = prev_char.is_some_and(|c| c.is_alphanumeric());
            let next_is_alnum = next_char.is_some_and(|c| c.is_alphanumeric());
            if prev_is_alnum {
                can_open = false;
            }
            if next_is_alnum {
                can_close = false;
            }
        }

        return (can_open, can_close);
    }

    let lf = is_left_flanking(text, pos, count);
    let rf = is_right_flanking(text, pos, count);
    if ch == b'*' {
        (lf, rf)
    } else {
        let prev_char = (pos > 0).then(|| text[..pos].chars().last()).flatten();
        let next_char = text.get(pos + count..).and_then(|s| s.chars().next());
        let preceded_by_punct = prev_char.is_some_and(is_unicode_punct_or_symbol);
        let followed_by_punct = next_char.is_some_and(is_unicode_punct_or_symbol);
        let can_open = lf && (!rf || preceded_by_punct);
        let can_close = rf && (!lf || followed_by_punct);
        (can_open, can_close)
    }
}

/// Pandoc-only: identify a math span starting at `pos` and return its
/// byte length. Tries raw TeX math environments (gated on `raw_tex`),
/// `$math$` and `$$display$$` (gated on
/// `tex_math_dollars`), GFM `$math$` (gated on `tex_math_gfm`), and the
/// `\(math\)` / `\[math\]` / `\\(math\\)` / `\\[math\\]` backslash
/// forms (gated on `tex_math_single_backslash` / `_double_backslash`).
/// Math content is opaque to emphasis: `$a * b$` must not produce an
/// emphasis closer at the inner `*`.
pub(crate) fn try_pandoc_math_opaque(
    text: &str,
    pos: usize,
    end: usize,
    config: &ParserOptions,
) -> Option<usize> {
    let bytes = text.as_bytes();
    let exts = &config.extensions;
    let b = bytes[pos];
    let multiline = config.dialect == crate::options::Dialect::Pandoc;

    if exts.raw_tex
        && b == b'\\'
        && let Some(len) = try_parse_raw_math_environment(&text[pos..])
        && pos + len <= end
    {
        return Some(len);
    }
    if exts.tex_math_dollars && b == b'$' {
        if let Some((len, _)) = try_parse_display_math(&text[pos..])
            && pos + len <= end
        {
            return Some(len);
        }
        if let Some((len, _)) = try_parse_inline_math(&text[pos..], multiline)
            && pos + len <= end
        {
            return Some(len);
        }
    }
    if exts.tex_math_gfm
        && b == b'$'
        && let Some((len, _)) = try_parse_gfm_inline_math(&text[pos..], multiline)
        && pos + len <= end
    {
        return Some(len);
    }
    if exts.tex_math_double_backslash && b == b'\\' {
        if let Some((len, _)) = try_parse_double_backslash_display_math(&text[pos..])
            && pos + len <= end
        {
            return Some(len);
        }
        if let Some((len, _)) = try_parse_double_backslash_inline_math(&text[pos..], multiline)
            && pos + len <= end
        {
            return Some(len);
        }
    }
    if exts.tex_math_single_backslash && b == b'\\' {
        if let Some((len, _)) = try_parse_single_backslash_display_math(&text[pos..])
            && pos + len <= end
        {
            return Some(len);
        }
        if let Some((len, _)) = try_parse_single_backslash_inline_math(&text[pos..], multiline)
            && pos + len <= end
        {
            return Some(len);
        }
    }
    None
}

/// Pandoc-only: identify a bracket-shaped opaque construct starting at
/// `pos` and return its byte length. Tries the dispatcher's precedence
/// order:
///   1. `![alt](dest)` inline image
///   2. `![alt][ref]` / `![alt]` reference image (shape-only opacity)
///   3. `[^id]` footnote reference
///   4. `[text](dest)` inline link
///   5. `[text][ref]` / `[text]` reference link (shape-only opacity)
///   6. `[@cite]` bracketed citation
///   7. `[text]{attrs}` bracketed span
///
/// Returns `None` if the bytes at `pos` don't open any recognised Pandoc
/// bracket-shaped construct. In that case the scanner falls through to
/// the generic `OpenBracket`/`CloseBracket` emission and the dispatcher
/// emits the bracket bytes as literal text (or as plain emphasis if the
/// pattern matches an opener).
/// Lookahead helper: at a `[` or `![` byte under Pandoc dialect, return
/// the total byte length of the bracket-shape link/image if it forms a
/// valid one, else `None`. Used by `build_ir` to suppress autolink /
/// raw HTML / native span recognition inside Pandoc link text —
/// pandoc-native treats link text as opaque to those constructs
/// (CommonMark spec example #526 / #538 differs). Mirrors the
/// dispatcher's `try_parse_*` precedence so the lookahead, the IR's
/// `process_brackets` resolution, and the dispatcher's emission agree
/// on the bracket-shape's byte boundaries.
fn try_pandoc_bracket_link_extent(
    text: &str,
    pos: usize,
    end: usize,
    config: &ParserOptions,
) -> Option<usize> {
    let bytes = text.as_bytes();
    let exts = &config.extensions;
    let ctx = LinkScanContext::from_options(config);
    let allow_shortcut = exts.shortcut_reference_links;

    if bytes[pos] == b'!' {
        if pos + 1 >= end || bytes[pos + 1] != b'[' {
            return None;
        }
        if exts.inline_images
            && let Some((len, _, _, _)) = try_parse_inline_image(&text[pos..], ctx)
            && pos + len <= end
        {
            return Some(len);
        }
        if exts.reference_links
            && let Some((len, _, _, _, _)) =
                try_parse_reference_image(&text[pos..], allow_shortcut, exts.spaced_reference_links)
            && pos + len <= end
        {
            return Some(len);
        }
        return None;
    }

    if exts.inline_links
        && let Some((len, _, _, _)) = try_parse_inline_link(&text[pos..], false, ctx)
        && pos + len <= end
    {
        return Some(len);
    }
    if exts.reference_links
        && let Some((len, _, _, _, _)) = try_parse_reference_link(
            &text[pos..],
            allow_shortcut,
            exts.inline_links,
            exts.spaced_reference_links,
            ctx,
        )
        && pos + len <= end
    {
        return Some(len);
    }

    None
}

fn is_unicode_punct_or_symbol(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_punctuation()
    } else {
        !c.is_alphanumeric() && !c.is_whitespace()
    }
}

fn is_left_flanking(text: &str, run_start: usize, run_len: usize) -> bool {
    let after = run_start + run_len;
    let next_char = text.get(after..).and_then(|s| s.chars().next());
    let prev_char = (run_start > 0)
        .then(|| text[..run_start].chars().last())
        .flatten();

    let followed_by_ws = next_char.is_none_or(|c| c.is_whitespace());
    if followed_by_ws {
        return false;
    }
    let followed_by_punct = next_char.is_some_and(is_unicode_punct_or_symbol);
    if !followed_by_punct {
        return true;
    }
    prev_char.is_none_or(|c| c.is_whitespace() || is_unicode_punct_or_symbol(c))
}

fn is_right_flanking(text: &str, run_start: usize, run_len: usize) -> bool {
    let after = run_start + run_len;
    let next_char = text.get(after..).and_then(|s| s.chars().next());
    let prev_char = (run_start > 0)
        .then(|| text[..run_start].chars().last())
        .flatten();

    let preceded_by_ws = prev_char.is_none_or(|c| c.is_whitespace());
    if preceded_by_ws {
        return false;
    }
    let preceded_by_punct = prev_char.is_some_and(is_unicode_punct_or_symbol);
    if !preceded_by_punct {
        return true;
    }
    next_char.is_none_or(|c| c.is_whitespace() || is_unicode_punct_or_symbol(c))
}

/// Run the CommonMark §6.3 `process_emphasis` algorithm over the IR's
/// delim runs. Mutates the IR in place: matched runs gain entries in their
/// `matches` vec, unmatched bytes stay implicit (the emission pass treats
/// any byte not covered by a match as literal text).
///
/// The algorithm tracks a per-bucket `openers_bottom` exclusive lower
/// bound to keep walk-back bounded; consume rules and the §6.2 mod-3
/// rejection match the reference implementation.
pub fn process_emphasis(events: &mut [IrEvent], dialect: crate::options::Dialect) {
    process_emphasis_in_range(events, 0, events.len(), dialect);
}

/// Range-scoped variant of [`process_emphasis`].
///
/// Only delim runs whose IR event index lies in `[lo, hi)` are considered.
/// Used by [`build_full_plans`] to run emphasis pairing inside each
/// resolved bracket pair *before* the global top-level pass, so emphasis
/// can never form across a link's bracket boundary (CommonMark §6.3
/// requires bracket resolution to happen first when at a `]`, with
/// emphasis processed on the link's inner range).
///
/// The function additionally skips delim runs that already carry a
/// recorded match in their `matches` vec — this lets the second
/// (top-level) pass reuse the same algorithm without re-pairing bytes
/// already consumed by inner-range passes.
pub fn process_emphasis_in_range(
    events: &mut [IrEvent],
    lo: usize,
    hi: usize,
    dialect: crate::options::Dialect,
) {
    process_emphasis_in_range_filtered(events, lo, hi, None, dialect);
}

fn process_emphasis_in_range_filtered(
    events: &mut [IrEvent],
    lo: usize,
    hi: usize,
    excluded: Option<&[bool]>,
    dialect: crate::options::Dialect,
) {
    let is_commonmark = dialect == crate::options::Dialect::CommonMark;
    if is_commonmark {
        run_emphasis_pass(events, lo, hi, excluded, dialect, &[], false);
        return;
    }
    let mut rejected: Vec<(usize, usize)> = Vec::new();
    let max_iters = events.len().saturating_add(2);
    let mut iter = 0;
    loop {
        let strict = iter > 0;
        run_emphasis_pass(events, lo, hi, excluded, dialect, &rejected, strict);
        let invalidations = pandoc_cascade_invalidate(events, excluded);
        if invalidations.is_empty() {
            break;
        }
        rejected.extend(invalidations);
        iter += 1;
        if iter >= max_iters {
            break;
        }
    }
    pandoc_inner_strong_recovery(events);
}

/// One pass of the CommonMark §6.2 emphasis pairing algorithm over the
/// IR's [`DelimRun`](IrEvent::DelimRun) events in `[lo, hi)`. Pandoc
/// dialect gates apply when `dialect == Dialect::Pandoc`. The
/// `rejected_pairs` list (Pandoc only) excludes specific
/// (opener_event_idx, closer_event_idx) pairs from matching — used by
/// the cascade-then-rerun loop to prevent invalidated pairs from
/// re-forming on the next iteration.
fn run_emphasis_pass(
    events: &mut [IrEvent],
    lo: usize,
    hi: usize,
    excluded: Option<&[bool]>,
    dialect: crate::options::Dialect,
    rejected_pairs: &[(usize, usize)],
    strict_pandoc: bool,
) {
    let is_commonmark = dialect == crate::options::Dialect::CommonMark;
    let hi = hi.min(events.len());
    if lo >= hi {
        return;
    }
    let mut delim_idxs: Vec<usize> = events[lo..hi]
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let abs = lo + i;
            match e {
                IrEvent::DelimRun { matches, .. }
                    if matches.is_empty()
                        && excluded.is_none_or(|ex| ex.get(abs).copied() != Some(true)) =>
                {
                    Some(abs)
                }
                _ => None,
            }
        })
        .collect();
    if delim_idxs.is_empty() {
        return;
    }

    let mut count: Vec<usize> = Vec::with_capacity(delim_idxs.len());
    let mut source_start: Vec<usize> = Vec::with_capacity(delim_idxs.len());
    let mut removed: Vec<bool> = vec![false; delim_idxs.len()];

    for &ev_idx in &delim_idxs {
        if let IrEvent::DelimRun { start, end, .. } = &events[ev_idx] {
            count.push(end - start);
            source_start.push(*start);
        }
    }

    let mut openers_bottom: [[[Option<usize>; 2]; 3]; 2] = [[[None; 2]; 3]; 2];

    let first_active =
        |removed: &[bool]| -> Option<usize> { (0..removed.len()).find(|&i| !removed[i]) };
    let next_active = |removed: &[bool], from: usize| -> Option<usize> {
        (from + 1..removed.len()).find(|&i| !removed[i])
    };
    let prev_active =
        |removed: &[bool], from: usize| -> Option<usize> { (0..from).rev().find(|&i| !removed[i]) };

    let min_closer_count = 1usize;
    let mut closer_local = first_active(&removed);
    while let Some(c) = closer_local {
        let ev_c_idx = delim_idxs[c];
        let (ch_c, can_open_c, can_close_c) = match &events[ev_c_idx] {
            IrEvent::DelimRun {
                ch,
                can_open,
                can_close,
                ..
            } => (*ch, *can_open, *can_close),
            _ => unreachable!(),
        };
        if !can_close_c || removed[c] || count[c] < min_closer_count {
            closer_local = next_active(&removed, c);
            continue;
        }

        let ch_idx = if ch_c == b'*' { 0 } else { 1 };
        let closer_mod = count[c] % 3;
        let closer_open_bucket = can_open_c as usize;
        let bottom = openers_bottom[ch_idx][closer_mod][closer_open_bucket];

        let mut found_opener: Option<usize> = None;
        let mut walk = prev_active(&removed, c);
        while let Some(o) = walk {
            if Some(o) == bottom {
                break;
            }
            let ev_o_idx = delim_idxs[o];
            let (ch_o, can_open_o, can_close_o) = match &events[ev_o_idx] {
                IrEvent::DelimRun {
                    ch,
                    can_open,
                    can_close,
                    ..
                } => (*ch, *can_open, *can_close),
                _ => unreachable!(),
            };
            if !removed[o] && ch_o == ch_c && can_open_o {
                let oc_sum = count[o] + count[c];
                let opener_both = can_open_o && can_close_o;
                let closer_both = can_open_c && can_close_c;
                let mod3_reject = is_commonmark
                    && (opener_both || closer_both)
                    && oc_sum.is_multiple_of(3)
                    && !(count[o].is_multiple_of(3) && count[c].is_multiple_of(3));
                let pandoc_reject = !is_commonmark
                    && ((count[o] == 1 && count[c] == 2)
                        || (count[o] == 2 && count[c] == 1)
                        || count[o] >= 4);
                let pair_rejected = !is_commonmark && {
                    let oe = delim_idxs[o];
                    let ce = delim_idxs[c];
                    rejected_pairs.iter().any(|&(ro, rc)| ro == oe && rc == ce)
                };
                let strict_block = strict_pandoc && {
                    let tentative_consume = if !is_commonmark && count[o] >= 3 && count[c] >= 3 {
                        1
                    } else if count[o] >= 2 && count[c] >= 2 {
                        2
                    } else {
                        1
                    };
                    let lo_evt = delim_idxs[o] + 1;
                    let hi_evt = delim_idxs[c];
                    (lo_evt..hi_evt).any(|k| match &events[k] {
                        IrEvent::DelimRun {
                            ch: ch_k,
                            start,
                            end,
                            matches,
                            ..
                        } => {
                            *ch_k == ch_c && {
                                let total = end - start;
                                let consumed: usize = matches.iter().map(|m| m.len as usize).sum();
                                total.saturating_sub(consumed) > tentative_consume
                            }
                        }
                        _ => false,
                    })
                };
                if !mod3_reject && !pandoc_reject && !pair_rejected && !strict_block {
                    found_opener = Some(o);
                    break;
                }
            }
            if o == 0 {
                break;
            }
            walk = prev_active(&removed, o);
        }

        if let Some(o) = found_opener {
            let consume = if !is_commonmark && count[o] >= 3 && count[c] >= 3 {
                1
            } else if count[o] >= 2 && count[c] >= 2 {
                2
            } else {
                1
            };
            let kind = if consume == 2 {
                EmphasisKind::Strong
            } else {
                EmphasisKind::Emph
            };

            let opener_match_offset =
                source_start[o] + count[o] - consume - source_start_event(&events[delim_idxs[o]]);
            let closer_match_offset = source_start[c] - source_start_event(&events[delim_idxs[c]]);

            if let IrEvent::DelimRun { matches, .. } = &mut events[delim_idxs[o]] {
                matches.push(DelimMatch {
                    offset_in_run: opener_match_offset as u8,
                    len: consume as u8,
                    is_opener: true,
                    partner_event: delim_idxs[c] as u32,
                    partner_offset: closer_match_offset as u8,
                    kind,
                });
            }
            if let IrEvent::DelimRun { matches, .. } = &mut events[delim_idxs[c]] {
                matches.push(DelimMatch {
                    offset_in_run: closer_match_offset as u8,
                    len: consume as u8,
                    is_opener: false,
                    partner_event: delim_idxs[o] as u32,
                    partner_offset: opener_match_offset as u8,
                    kind,
                });
            }

            count[o] -= consume;
            source_start[c] += consume;
            count[c] -= consume;

            let mut between = next_active(&removed, o);
            while let Some(idx) = between {
                if idx == c {
                    break;
                }
                removed[idx] = true;
                between = next_active(&removed, idx);
            }

            if count[o] == 0 {
                removed[o] = true;
            }
            if count[c] == 0 {
                removed[c] = true;
                closer_local = next_active(&removed, c);
            }
        } else {
            openers_bottom[ch_idx][closer_mod][closer_open_bucket] = prev_active(&removed, c);
            if !can_open_c {
                removed[c] = true;
            }
            closer_local = next_active(&removed, c);
        }
    }

    let _ = (&mut delim_idxs, &mut openers_bottom, min_closer_count);
}

/// Pandoc-only post-processing pass over [`process_emphasis_in_range_filtered`]
/// matches: invalidate any matched delim pair that contains an unmatched
/// same-character run between its opener and closer. Returns the list
/// of (opener_event_idx, closer_event_idx) pairs that were invalidated
/// in this call, so the caller can seed a rejected-pairs list and
/// re-run the standard pass — this lets Pandoc re-pair the inner runs
/// that the invalidated outer match would have stolen via
/// between-removal (e.g. `*foo **bar* baz**` → after the outer
/// `ev0..ev2` Emph is invalidated, `ev1..ev3` matches as Strong on the
/// next iteration).
fn pandoc_cascade_invalidate(
    events: &mut [IrEvent],
    excluded: Option<&[bool]>,
) -> Vec<(usize, usize)> {
    let mut invalidated_pairs: Vec<(usize, usize)> = Vec::new();
    if !events.iter().any(|e| matches!(e, IrEvent::DelimRun { .. })) {
        return invalidated_pairs;
    }
    let is_excluded = |k: usize| excluded.is_some_and(|ex| ex.get(k).copied() == Some(true));
    let mut total: Vec<usize> = Vec::with_capacity(events.len());
    let mut consumed: Vec<usize> = Vec::with_capacity(events.len());
    loop {
        total.clear();
        consumed.clear();
        total.extend(events.iter().map(|e| match e {
            IrEvent::DelimRun { start, end, .. } => end - start,
            _ => 0,
        }));
        consumed.extend(events.iter().map(|e| match e {
            IrEvent::DelimRun { matches, .. } => matches.iter().map(|m| m.len as usize).sum(),
            _ => 0,
        }));

        let mut to_invalidate: Option<(usize, u8)> = None;
        'outer: for opener_idx in 0..events.len() {
            let IrEvent::DelimRun {
                ch: ch_o, matches, ..
            } = &events[opener_idx]
            else {
                continue;
            };
            for (mi, m) in matches.iter().enumerate() {
                if !m.is_opener {
                    continue;
                }
                let closer_idx = m.partner_event as usize;
                if closer_idx <= opener_idx || closer_idx >= events.len() {
                    continue;
                }
                for k in (opener_idx + 1)..closer_idx {
                    if is_excluded(k) {
                        continue;
                    }
                    if let IrEvent::DelimRun {
                        ch: ch_k,
                        can_open: co_k,
                        can_close: cc_k,
                        ..
                    } = &events[k]
                        && *ch_k == *ch_o
                        && consumed[k] < total[k]
                        && *co_k
                        && *cc_k
                    {
                        to_invalidate = Some((opener_idx, mi as u8));
                        break 'outer;
                    }
                }
            }
        }

        let Some((opener_idx, mi)) = to_invalidate else {
            break;
        };

        let (closer_idx, opener_offset) = match &events[opener_idx] {
            IrEvent::DelimRun { matches, .. } => {
                let m = matches[mi as usize];
                (m.partner_event as usize, m.offset_in_run)
            }
            _ => break,
        };

        if let IrEvent::DelimRun { matches, .. } = &mut events[opener_idx] {
            matches.remove(mi as usize);
        }
        if let IrEvent::DelimRun { matches, .. } = &mut events[closer_idx] {
            matches.retain(|m| m.is_opener || m.partner_offset != opener_offset);
        }
        invalidated_pairs.push((opener_idx, closer_idx));
    }
    invalidated_pairs
}

/// Pandoc-only post-pass: recover the inner Strong match in
/// `***A **B** C***` patterns where the IR's standard pass produced
/// `Emph[Strong[A], "B**...** C"]` (matching the outer triple as
/// Strong+Emph but losing the inner `**...**`-as-Strong-of-`C` pair).
///
/// Pandoc's recursive descent here goes
/// `three c → ender c 2 → one c → option2 → two c`, producing
/// `Emph[Strong[A], "B", Strong[C]]` — two Strong nodes inside an outer
/// Emph. The standard delim-stack algorithm can't reach this pairing
/// because between-removal during the outer Emph match removes the
/// inner closer-side `**` (e.g. `bar**`) from the candidate pool.
///
/// This recovery scans Emph matches whose opener and closer originally
/// had count >= 3, and whose closer has unmatched bytes >= 2 after the
/// standard pass; for each, we look for an unmatched same-char
/// between-run with count >= 2 and `can_close = true` (the would-be
/// inner-Strong opener) and synthesise a Strong match that consumes
/// the leftmost 2 bytes of the closer (where the existing Emph match
/// shifts to the rightmost 1 byte). The byte-position rewrite lets
/// the CST emission produce well-nested `Emph[..., Strong[...]]` —
/// outer Emph close at the rightmost outer-triple byte, inner Strong
/// close at the leftmost two.
fn pandoc_inner_strong_recovery(events: &mut [IrEvent]) {
    let n = events.len();
    let mut to_apply: Vec<(usize, usize, usize, u8)> = Vec::new();

    for opener_idx in 0..n {
        let (open_total, open_matches_clone, ch_o) = match &events[opener_idx] {
            IrEvent::DelimRun {
                start,
                end,
                matches,
                ch,
                ..
            } => (*end - *start, matches.clone(), *ch),
            _ => continue,
        };
        if open_total < 3 {
            continue;
        }

        for m in open_matches_clone.iter() {
            if !m.is_opener || m.kind != EmphasisKind::Emph {
                continue;
            }
            let closer_idx = m.partner_event as usize;
            if closer_idx <= opener_idx || closer_idx >= n {
                continue;
            }

            let (close_total, close_consumed) = match &events[closer_idx] {
                IrEvent::DelimRun {
                    start,
                    end,
                    matches,
                    ..
                } => {
                    let total = end - start;
                    let consumed: usize = matches.iter().map(|m| m.len as usize).sum();
                    (total, consumed)
                }
                _ => continue,
            };
            if close_total < 3 {
                continue;
            }
            let leftover = close_total.saturating_sub(close_consumed);
            if leftover < 2 {
                continue;
            }

            for k in ((opener_idx + 1)..closer_idx).rev() {
                if let IrEvent::DelimRun {
                    ch,
                    start,
                    end,
                    matches,
                    can_close,
                    ..
                } = &events[k]
                {
                    if *ch != ch_o || !*can_close {
                        continue;
                    }
                    let total = end - start;
                    let consumed: usize = matches.iter().map(|m| m.len as usize).sum();
                    let remaining = total.saturating_sub(consumed);
                    if remaining < 2 {
                        continue;
                    }
                    to_apply.push((k, opener_idx, closer_idx, 2));
                    break;
                }
            }
        }
    }

    for (between_idx, opener_idx, closer_idx, len) in to_apply {
        let (closer_emph_match_idx, closer_emph_offset) = {
            let mut found: Option<(usize, u8)> = None;
            if let IrEvent::DelimRun { matches, .. } = &events[closer_idx] {
                for (mi, m) in matches.iter().enumerate() {
                    if !m.is_opener
                        && m.partner_event as usize == opener_idx
                        && m.kind == EmphasisKind::Emph
                    {
                        found = Some((mi, m.offset_in_run));
                        break;
                    }
                }
            }
            match found {
                Some(x) => x,
                None => continue,
            }
        };

        let opener_emph_match_idx = {
            let mut found: Option<usize> = None;
            if let IrEvent::DelimRun { matches, .. } = &events[opener_idx] {
                for (mi, m) in matches.iter().enumerate() {
                    if m.is_opener
                        && m.partner_event as usize == closer_idx
                        && m.kind == EmphasisKind::Emph
                    {
                        found = Some(mi);
                        break;
                    }
                }
            }
            match found {
                Some(x) => x,
                None => continue,
            }
        };

        let new_closer_emph_offset = closer_emph_offset + len;

        if let IrEvent::DelimRun { matches, .. } = &mut events[closer_idx] {
            matches[closer_emph_match_idx].offset_in_run = new_closer_emph_offset;
        }
        if let IrEvent::DelimRun { matches, .. } = &mut events[opener_idx] {
            matches[opener_emph_match_idx].partner_offset = new_closer_emph_offset;
        }

        if let IrEvent::DelimRun { matches, .. } = &mut events[between_idx] {
            matches.push(DelimMatch {
                offset_in_run: 0,
                len,
                is_opener: true,
                partner_event: closer_idx as u32,
                partner_offset: closer_emph_offset,
                kind: EmphasisKind::Strong,
            });
        }
        if let IrEvent::DelimRun { matches, .. } = &mut events[closer_idx] {
            matches.push(DelimMatch {
                offset_in_run: closer_emph_offset,
                len,
                is_opener: false,
                partner_event: between_idx as u32,
                partner_offset: 0,
                kind: EmphasisKind::Strong,
            });
        }
    }
}

fn source_start_event(event: &IrEvent) -> usize {
    match event {
        IrEvent::DelimRun { start, .. } => *start,
        _ => unreachable!("source_start_event called on non-DelimRun"),
    }
}

/// Resolve `[`/`![`/`]` markers into link/image nodes per CommonMark §6.3
/// (with Pandoc-aware variations under `Dialect::Pandoc`).
///
/// Walks the IR forward looking for `]` markers. For each one, finds the
/// nearest active matching `[`/`![` and tries to resolve the bracket pair
/// as a link or image. Resolution is tried in spec order:
///
/// 1. Inline link / image: `[text](dest)` or `[text](dest "title")`.
/// 2. Full reference: `[text][label]`, where `label` is in `refdefs`.
/// 3. Collapsed reference: `[text][]`, where `text` (normalised) is in
///    `refdefs`.
/// 4. Shortcut reference: `[text]` not followed by `(` or `[`, where
///    `text` (normalised) is in `refdefs`.
///
/// On a match, the opener gets a `BracketResolution` and the closer is
/// flagged `matched`. Under `Dialect::CommonMark`, all earlier active link
/// openers are deactivated to implement the §6.3 "links may not contain
/// other links" rule (image brackets do not deactivate earlier link
/// openers — only links do). Under `Dialect::Pandoc`, the deactivate-pass
/// is skipped: pandoc-native is outer-wins for nested links (the inner
/// `[inner](u2)` of `[link [inner](u2)](u1)` is literal text inside the
/// outer link), and the dispatcher enforces this via a `suppress_inner_links`
/// flag during LINK-text recursion. So under Pandoc the IR can leave both
/// outer and inner resolved and trust the dispatcher to suppress inner
/// LINK emission.
///
/// On a miss the bracket pair stays opaque-as-literal and the closer is
/// dropped from the bracket stack so the next `]` can re-pair.
///
/// Reference-form resolution consults the refdef map under both
/// dialects (CommonMark §6.3 and Pandoc-markdown agree on the
/// document-scoped lookup rule). Under Pandoc, when a bracket-shape
/// pattern (`[text][label]`, `[text][]`, `[text]`) doesn't resolve to
/// a refdef, the opener is tagged with `unresolved_ref = Some(...)`
/// and the closer's `matched` is set to `true` so that
/// [`build_bracket_plan`] emits a [`BracketDispo::UnresolvedReference`]
/// keyed at the opener. Emission then wraps `[start, end)` in an
/// `UNRESOLVED_REFERENCE` node — distinct from `LINK` — so downstream
/// tools (linter, LSP) can attach behavior to the bracket-shape
/// pattern without the parser having to lie about resolution.
///
/// Under CommonMark, no `unresolved_ref` is recorded; the
/// no-resolution fall-through behaves as today (opener deactivated,
/// brackets emit as literal text).
pub fn process_brackets(
    events: &mut [IrEvent],
    text: &str,
    refdefs: Option<&RefdefMap>,
    dialect: crate::options::Dialect,
    preserve_unresolved_references: bool,
    allow_spaced: bool,
    hugo_shortcodes: bool,
) {
    let empty: HashSet<String> = HashSet::new();
    let labels: &HashSet<String> = match refdefs {
        Some(map) => map.as_ref(),
        None => &empty,
    };
    let is_commonmark = dialect == crate::options::Dialect::CommonMark;
    let label_resolves =
        |key_norm: &str| -> bool { !key_norm.is_empty() && labels.contains(key_norm) };

    let mut i = 0;
    while i < events.len() {
        let close_pos = match &events[i] {
            IrEvent::CloseBracket { pos, .. } => *pos,
            _ => {
                i += 1;
                continue;
            }
        };

        let mut o = match find_active_opener(events, i) {
            Some(o) => o,
            None => {
                i += 1;
                continue;
            }
        };

        let (open_end, is_image) = match &events[o] {
            IrEvent::OpenBracket { end, is_image, .. } => (*end, *is_image),
            _ => unreachable!(),
        };
        let text_start = open_end;
        let text_end = close_pos;
        let after_close = close_pos + 1;

        if let Some((suffix_end, dest, title)) =
            try_inline_suffix(text, after_close, hugo_shortcodes)
        {
            if !is_image && is_commonmark {
                deactivate_earlier_link_openers(events, o);
            }
            commit_resolution(
                events,
                o,
                i,
                text_start,
                text_end,
                after_close,
                suffix_end,
                LinkKind::Inline { dest, title },
            );
            mark_opener_resolved(events, o);
            i += 1;
            continue;
        }

        let full_ref_suffix = try_full_reference_suffix(text, after_close, allow_spaced);
        if let Some((suffix_end, label_raw)) = &full_ref_suffix {
            let label_norm = normalize_label(label_raw);
            if label_resolves(&label_norm) {
                if !is_image && is_commonmark {
                    deactivate_earlier_link_openers(events, o);
                }
                commit_resolution(
                    events,
                    o,
                    i,
                    text_start,
                    text_end,
                    after_close,
                    *suffix_end,
                    LinkKind::FullReference {
                        label: label_raw.clone(),
                    },
                );
                mark_opener_resolved(events, o);
                i += 1;
                continue;
            }
        }

        let link_text = &text[text_start..text_end];
        let link_text_norm = normalize_label(link_text);
        let (is_collapsed, collapsed_suffix_end) =
            collapsed_marker_span(text, after_close, allow_spaced)
                .map_or((false, after_close + 2), |end| (true, end));

        if is_collapsed && label_resolves(&link_text_norm) {
            if !is_image && is_commonmark {
                deactivate_earlier_link_openers(events, o);
            }
            commit_resolution(
                events,
                o,
                i,
                text_start,
                text_end,
                after_close,
                collapsed_suffix_end,
                LinkKind::CollapsedReference,
            );
            mark_opener_resolved(events, o);
            i += 1;
            continue;
        }

        let shortcut_suppressed = full_ref_suffix.is_some() || is_collapsed;
        if !shortcut_suppressed && label_resolves(&link_text_norm) {
            if !is_image && is_commonmark {
                deactivate_earlier_link_openers(events, o);
            }
            commit_resolution(
                events,
                o,
                i,
                text_start,
                text_end,
                after_close,
                after_close,
                LinkKind::ShortcutReference,
            );
            mark_opener_resolved(events, o);
            i += 1;
            continue;
        }

        let unresolved_shape = if !is_commonmark || preserve_unresolved_references {
            let (end, has_substantive_label) =
                if let Some((suffix_end, label_raw)) = &full_ref_suffix {
                    (*suffix_end, !normalize_label(label_raw).is_empty())
                } else if is_collapsed {
                    (collapsed_suffix_end, !link_text_norm.is_empty())
                } else {
                    (after_close, !link_text_norm.is_empty())
                };
            if has_substantive_label {
                Some(UnresolvedRefShape {
                    close_event: i as u32,
                    text_end,
                    end,
                })
            } else {
                None
            }
        } else {
            None
        };
        if let IrEvent::OpenBracket {
            active,
            unresolved_ref,
            ..
        } = &mut events[o]
        {
            *active = false;
            *unresolved_ref = unresolved_shape;
        }
        if unresolved_shape.is_some()
            && let IrEvent::CloseBracket { matched, .. } = &mut events[i]
        {
            *matched = true;
        }
        let _ = &mut o;
        i += 1;
    }
}

fn find_active_opener(events: &[IrEvent], close_idx: usize) -> Option<usize> {
    (0..close_idx).rev().find(|&i| {
        matches!(
            &events[i],
            IrEvent::OpenBracket {
                active: true,
                resolution: None,
                ..
            }
        )
    })
}

fn deactivate_earlier_link_openers(events: &mut [IrEvent], open_idx: usize) {
    for ev in &mut events[..open_idx] {
        if let IrEvent::OpenBracket {
            is_image: false,
            active,
            resolution: None,
            ..
        } = ev
        {
            *active = false;
        }
    }
}

fn mark_opener_resolved(events: &mut [IrEvent], open_idx: usize) {
    if let IrEvent::OpenBracket { active, .. } = &mut events[open_idx] {
        *active = false;
    }
}

#[allow(clippy::too_many_arguments)]
fn commit_resolution(
    events: &mut [IrEvent],
    open_idx: usize,
    close_idx: usize,
    text_start: usize,
    text_end: usize,
    suffix_start: usize,
    suffix_end: usize,
    kind: LinkKind,
) {
    if let IrEvent::OpenBracket { resolution, .. } = &mut events[open_idx] {
        *resolution = Some(BracketResolution {
            close_event: close_idx as u32,
            text_start,
            text_end,
            suffix_start,
            suffix_end,
            kind,
        });
    }
    if let IrEvent::CloseBracket { matched, .. } = &mut events[close_idx] {
        *matched = true;
    }
}

fn try_inline_suffix(
    text: &str,
    pos: usize,
    hugo_shortcodes: bool,
) -> Option<(usize, String, Option<String>)> {
    let bytes = text.as_bytes();
    if pos >= bytes.len() || bytes[pos] != b'(' {
        return None;
    }
    let mut p = pos + 1;
    while p < bytes.len() && matches!(bytes[p], b' ' | b'\t' | b'\n') {
        p += 1;
    }
    if p < bytes.len() && bytes[p] == b')' {
        return Some((p + 1, String::new(), None));
    }

    let (dest, dest_end) = parse_link_destination(text, p, hugo_shortcodes)?;
    p = dest_end;

    while p < bytes.len() && matches!(bytes[p], b' ' | b'\t' | b'\n') {
        p += 1;
    }

    let mut title = None;
    if p < bytes.len() && matches!(bytes[p], b'"' | b'\'' | b'(') {
        let (t, t_end) = parse_link_title(text, p)?;
        title = Some(t);
        p = t_end;
        while p < bytes.len() && matches!(bytes[p], b' ' | b'\t' | b'\n') {
            p += 1;
        }
    }

    if p >= bytes.len() || bytes[p] != b')' {
        return None;
    }
    Some((p + 1, dest, title))
}

fn parse_link_destination(
    text: &str,
    start: usize,
    hugo_shortcodes: bool,
) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    if start >= bytes.len() {
        return None;
    }
    if bytes[start] == b'<' {
        let mut p = start + 1;
        let begin = p;
        while p < bytes.len() && bytes[p] != b'>' && bytes[p] != b'\n' && bytes[p] != b'<' {
            if hugo_shortcodes
                && bytes[p] == b'{'
                && let Some(tag) = super::hugo::scan_tag(&text[p..]).filter(|tag| tag.complete)
            {
                p += tag.end;
                continue;
            }
            if bytes[p] == b'\\' && p + 1 < bytes.len() {
                p += 2;
            } else {
                p += 1;
            }
        }
        if p >= bytes.len() || bytes[p] != b'>' {
            return None;
        }
        let dest = text[begin..p].to_string();
        Some((dest, p + 1))
    } else {
        let mut p = start;
        let mut paren_depth: i32 = 0;
        while p < bytes.len() {
            let b = bytes[p];
            if hugo_shortcodes
                && b == b'{'
                && let Some(tag) = super::hugo::scan_tag(&text[p..]).filter(|tag| tag.complete)
            {
                p += tag.end;
                continue;
            }
            if b == b'\\' && p + 1 < bytes.len() {
                p += 2;
                continue;
            }
            if b == b'(' {
                paren_depth += 1;
                p += 1;
                continue;
            }
            if b == b')' {
                if paren_depth == 0 {
                    break;
                }
                paren_depth -= 1;
                p += 1;
                continue;
            }
            if b == b' ' || b == b'\t' || b == b'\n' || b < 0x20 || b == 0x7f {
                break;
            }
            p += 1;
        }
        if p == start || paren_depth != 0 {
            return None;
        }
        Some((text[start..p].to_string(), p))
    }
}

fn parse_link_title(text: &str, start: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    let q = bytes[start];
    let close = match q {
        b'"' => b'"',
        b'\'' => b'\'',
        b'(' => b')',
        _ => return None,
    };
    let mut p = start + 1;
    let begin = p;
    while p < bytes.len() {
        let b = bytes[p];
        if b == b'\\' && p + 1 < bytes.len() {
            p += 2;
            continue;
        }
        if b == close {
            let title = text[begin..p].to_string();
            return Some((title, p + 1));
        }
        p += 1;
    }
    None
}

fn try_full_reference_suffix(
    text: &str,
    pos: usize,
    allow_spaced: bool,
) -> Option<(usize, String)> {
    let bytes = text.as_bytes();
    let bracket_pos = if allow_spaced {
        skip_spaced_ref_gap(bytes, pos)
    } else {
        pos
    };
    if bracket_pos >= bytes.len() || bytes[bracket_pos] != b'[' {
        return None;
    }
    let label_start = bracket_pos + 1;
    let mut p = label_start;
    let mut escape_next = false;
    while p < bytes.len() {
        if escape_next {
            escape_next = false;
            p += 1;
            continue;
        }
        match bytes[p] {
            b'\\' => {
                escape_next = true;
                p += 1;
            }
            b']' => break,
            b'[' => return None,
            b'\n' => {
                p += 1;
            }
            _ => p += 1,
        }
    }
    if p >= bytes.len() || bytes[p] != b']' {
        return None;
    }
    let label = text[label_start..p].to_string();
    if label.is_empty() {
        return None;
    }
    Some((p + 1, label))
}

fn collapsed_marker_span(text: &str, pos: usize, allow_spaced: bool) -> Option<usize> {
    let bytes = text.as_bytes();
    let bracket_pos = if allow_spaced {
        skip_spaced_ref_gap(bytes, pos)
    } else {
        pos
    };
    if bytes.get(bracket_pos) == Some(&b'[') && bytes.get(bracket_pos + 1) == Some(&b']') {
        Some(bracket_pos + 2)
    } else {
        None
    }
}

/// Skip the whitespace gap permitted by `spaced_reference_links` between a
/// closing `]` and the next opening `[`/`[]`: spaces, tabs, and at most one LF.
/// Block parsing already guarantees a blank line cannot appear inside a single
/// inline-parse range, so a single newline is the upper bound.
fn skip_spaced_ref_gap(bytes: &[u8], pos: usize) -> usize {
    let mut p = pos;
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
}

/// Disposition of a single bracket byte after [`process_brackets`].
#[derive(Debug, Clone)]
pub enum BracketDispo {
    /// `[` or `![` of a resolved link/image. Emission emits the LINK/IMAGE
    /// node and skips past `suffix_end`.
    Open {
        is_image: bool,
        text_start: usize,
        text_end: usize,
        suffix_start: usize,
        suffix_end: usize,
        kind: LinkKind,
    },
    /// Pandoc-only: `[` or `![` of a bracket-shape reference pattern
    /// whose label didn't resolve. Emission wraps `[start, end)` in an
    /// `UNRESOLVED_REFERENCE` node so downstream tools can attach
    /// behavior to the bracket-shape pattern. `text_start..text_end` is
    /// the inner text range (between the outer `[`/`![` and `]`).
    UnresolvedReference {
        is_image: bool,
        text_start: usize,
        text_end: usize,
        end: usize,
    },
    /// Bracket byte (one of `[`, `]`, or `!`) that fell through to literal
    /// text. Emission accumulates into the surrounding text run.
    Literal,
}

/// A byte-keyed view of the IR's bracket resolutions.
#[derive(Debug, Default, Clone)]
pub struct BracketPlan {
    by_pos: BTreeMap<usize, BracketDispo>,
}

impl BracketPlan {
    pub fn lookup(&self, pos: usize) -> Option<&BracketDispo> {
        self.by_pos.get(&pos)
    }

    pub fn is_empty(&self) -> bool {
        self.by_pos.is_empty()
    }
}

/// A standalone Pandoc inline construct recognised by `build_ir` and
/// dispatched directly from the emission walk. Carries the construct's
/// full source range so the emission walk can slice the content for the
/// existing `emit_*` helpers without re-running the recognition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstructDispo {
    /// `^[note text]` — emit via `emit_inline_footnote` after slicing
    /// the inner content.
    InlineFootnote { end: usize },
    /// `<span ...>...</span>` — emit via `emit_native_span` after
    /// re-parsing the open-tag attributes from the source range.
    NativeSpan { end: usize },
    /// `[^id]` — emit via `emit_footnote_reference` after extracting
    /// the label id from the source range.
    FootnoteReference { end: usize },
    /// `[@cite]` — emit via `emit_bracketed_citation` after slicing
    /// the inner content.
    BracketedCitation { end: usize },
    /// `@key` or `-@key` — emit via `emit_bare_citation` (or
    /// `emit_crossref` when `is_quarto_crossref_key` matches and
    /// `extensions.quarto_crossrefs` is enabled).
    BareCitation { end: usize },
    /// `[content]{attrs}` — emit via `emit_bracketed_span` after
    /// slicing the inner content and attribute string.
    BracketedSpan { end: usize },
    /// `[[url]]` / `[[url|title]]` (or image variant `![[...]]`) —
    /// emit via `emit_wikilink` after re-locating the pipe within the
    /// source range.
    WikiLink { end: usize },
}

/// A byte-keyed view of the IR's standalone Pandoc constructs that the
/// emission walk consumes directly: inline footnotes, native spans,
/// footnote references, bracketed citations, bare citations, and
/// bracketed spans. Recognition is authoritative in `build_ir` under
/// `Dialect::Pandoc`; the dispatcher's legacy branches for these
/// constructs (`^[`, `<span>`, `[^id]`, `[@cite]`, `@cite` / `-@cite`,
/// `[text]{attrs}`) are gated to `Dialect::CommonMark` only and only
/// fire when the relevant extension is explicitly enabled.
#[derive(Debug, Default, Clone)]
pub struct ConstructPlan {
    by_pos: BTreeMap<usize, ConstructDispo>,
}

impl ConstructPlan {
    pub fn lookup(&self, pos: usize) -> Option<&ConstructDispo> {
        self.by_pos.get(&pos)
    }

    pub fn is_empty(&self) -> bool {
        self.by_pos.is_empty()
    }
}

/// Build a [`ConstructPlan`] from the resolved IR. Each
/// `Construct { kind: InlineFootnote | NativeSpan, .. }` becomes one
/// entry keyed at its start byte.
pub fn build_construct_plan(events: &[IrEvent]) -> ConstructPlan {
    let mut by_pos: BTreeMap<usize, ConstructDispo> = BTreeMap::new();
    for ev in events {
        if let IrEvent::Construct { start, end, kind } = ev {
            match kind {
                ConstructKind::InlineFootnote => {
                    by_pos.insert(*start, ConstructDispo::InlineFootnote { end: *end });
                }
                ConstructKind::NativeSpan => {
                    by_pos.insert(*start, ConstructDispo::NativeSpan { end: *end });
                }
                ConstructKind::FootnoteReference => {
                    by_pos.insert(*start, ConstructDispo::FootnoteReference { end: *end });
                }
                ConstructKind::BracketedCitation => {
                    by_pos.insert(*start, ConstructDispo::BracketedCitation { end: *end });
                }
                ConstructKind::BareCitation => {
                    by_pos.insert(*start, ConstructDispo::BareCitation { end: *end });
                }
                ConstructKind::BracketedSpan => {
                    by_pos.insert(*start, ConstructDispo::BracketedSpan { end: *end });
                }
                ConstructKind::WikiLink => {
                    by_pos.insert(*start, ConstructDispo::WikiLink { end: *end });
                }
                _ => {}
            }
        }
    }
    ConstructPlan { by_pos }
}

/// Pandoc `notAfterString`, delimiter-adjacent case: demote a bare
/// author-in-text `@key` whose `@` sits immediately after a *resolved*
/// emphasis/strong closing marker to literal text. Pandoc parses
/// `*em*@key` as `Emph [Str "em"]` followed by `Str "@key"` (the
/// citation is suppressed), because its `notAfterString` guard keys off
/// the parser's last-string position and the emphasis closer leaves that
/// position glued to the `@`.
///
/// The scan-time guard [`prev_char_suppresses_bare_citation`] already
/// handles the word / CJK / `.`-glued cases, but it can only inspect the
/// raw preceding byte — at scan time a `*`/`_` before the `@` is just an
/// unresolved delimiter. Whether that delimiter *resolves* into an
/// emphasis/strong closer is only known after [`process_emphasis`], so
/// this correction runs here, consuming the emphasis pass's result
/// rather than re-classifying it (single scan preserved).
///
/// Only the plain `@key` form is affected. The suppress-author `-@key`
/// form keys off the `-` (its `@` never abuts the delimiter), so its
/// construct — whose first byte is `-`, not `@` — is left untouched,
/// matching `*em*-@key` → `Emph` + `Str "-"` + `Cite`.
fn demote_bare_citation_after_emphasis_closer(events: &mut [IrEvent], text: &str) {
    let bytes = text.as_bytes();
    let has_bare_at = events.iter().any(|e| {
        matches!(
            e,
            IrEvent::Construct {
                kind: ConstructKind::BareCitation,
                start,
                ..
            } if bytes.get(*start) == Some(&b'@')
        )
    });
    if !has_bare_at {
        return;
    }

    let mut closer_ends: Vec<usize> = Vec::new();
    for ev in events.iter() {
        if let IrEvent::DelimRun { start, matches, .. } = ev {
            for m in matches {
                if !m.is_opener {
                    closer_ends.push(*start + m.offset_in_run as usize + m.len as usize);
                }
            }
        }
    }
    if closer_ends.is_empty() {
        return;
    }

    for ev in events.iter_mut() {
        if let IrEvent::Construct {
            start,
            end,
            kind: ConstructKind::BareCitation,
        } = *ev
            && bytes.get(start) == Some(&b'@')
            && closer_ends.contains(&start)
        {
            *ev = IrEvent::Text { start, end };
        }
    }
}

/// Build a [`BracketPlan`] from the resolved IR. Each `OpenBracket`
/// resolution becomes an [`BracketDispo::Open`] keyed at the opener's
/// start byte. Unresolved openers and unmatched closers become
/// `BracketDispo::Literal` so the emission path can recognise them
/// without re-parsing.
pub fn build_bracket_plan(events: &[IrEvent]) -> BracketPlan {
    let mut by_pos: BTreeMap<usize, BracketDispo> = BTreeMap::new();
    for ev in events {
        match ev {
            IrEvent::OpenBracket {
                start,
                is_image,
                resolution: Some(res),
                ..
            } => {
                by_pos.insert(
                    *start,
                    BracketDispo::Open {
                        is_image: *is_image,
                        text_start: res.text_start,
                        text_end: res.text_end,
                        suffix_start: res.suffix_start,
                        suffix_end: res.suffix_end,
                        kind: res.kind.clone(),
                    },
                );
            }
            IrEvent::OpenBracket {
                start,
                end,
                is_image,
                resolution: None,
                unresolved_ref: Some(shape),
                ..
            } => {
                by_pos.insert(
                    *start,
                    BracketDispo::UnresolvedReference {
                        is_image: *is_image,
                        text_start: *end,
                        text_end: shape.text_end,
                        end: shape.end,
                    },
                );
            }
            IrEvent::OpenBracket {
                start,
                is_image,
                resolution: None,
                unresolved_ref: None,
                ..
            } => {
                let len = if *is_image { 2 } else { 1 };
                for off in 0..len {
                    by_pos.insert(*start + off, BracketDispo::Literal);
                }
            }
            IrEvent::CloseBracket {
                pos,
                matched: false,
            } => {
                by_pos.insert(*pos, BracketDispo::Literal);
            }
            _ => {}
        }
    }
    BracketPlan { by_pos }
}

/// One-shot helper: build the IR, run all passes, and return the
/// bundled [`InlinePlans`] (emphasis dispositions, bracket resolutions,
/// and standalone Pandoc constructs) — packaged together so the inline
/// emission path can consume them in one go for either dialect.
///
/// Pass ordering follows the CommonMark §6.3 reference impl: bracket
/// resolution runs first, then emphasis is processed *scoped per resolved
/// bracket pair's inner event range*, then once more on the residual
/// top-level events. This prevents emphasis pairs from forming across a
/// link's bracket boundary, which the previous "all-emphasis-then-all-
/// brackets" order got wrong (e.g. spec example #473).
pub fn build_full_plans(
    text: &str,
    start: usize,
    end: usize,
    config: &ParserOptions,
) -> InlinePlans {
    let mut scratch = ScratchEvents::checkout();
    let bundle = scratch.inner.as_mut().unwrap();
    bundle.events.clear();
    bundle.bracket_pairs.clear();
    bundle.excluded.clear();

    let hugo = build_hugo_index(text, start, end, config);
    build_ir_into(text, start, end, config, &mut bundle.events, &hugo);
    process_brackets(
        &mut bundle.events,
        text,
        config.refdef_labels.as_ref(),
        config.dialect,
        config.preserve_unresolved_references,
        config.extensions.spaced_reference_links,
        config.extensions.hugo_shortcodes,
    );

    bundle.bracket_pairs.extend(
        bundle
            .events
            .iter()
            .enumerate()
            .filter_map(|(i, ev)| match ev {
                IrEvent::OpenBracket {
                    resolution: Some(res),
                    ..
                } => Some((i, res.close_event as usize)),
                IrEvent::OpenBracket {
                    resolution: None,
                    unresolved_ref: Some(shape),
                    ..
                } => Some((i, shape.close_event as usize)),
                _ => None,
            }),
    );
    bundle
        .bracket_pairs
        .sort_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)));
    for i in 0..bundle.bracket_pairs.len() {
        let (open_idx, close_idx) = bundle.bracket_pairs[i];
        process_emphasis_in_range(&mut bundle.events, open_idx + 1, close_idx, config.dialect);
    }

    for i in 0..bundle.bracket_pairs.len() {
        let (open_idx, close_idx) = bundle.bracket_pairs[i];
        let is_unresolved = matches!(
            &bundle.events[open_idx],
            IrEvent::OpenBracket {
                resolution: None,
                unresolved_ref: Some(_),
                ..
            }
        );
        if !is_unresolved {
            continue;
        }
        if !range_has_unmatched_delim_bytes(&bundle.events, open_idx + 1, close_idx) {
            continue;
        }
        if let IrEvent::OpenBracket { unresolved_ref, .. } = &mut bundle.events[open_idx] {
            *unresolved_ref = None;
        }
        if let IrEvent::CloseBracket { matched, .. } = &mut bundle.events[close_idx] {
            *matched = false;
        }
    }

    let len = bundle.events.len();
    if bundle.bracket_pairs.is_empty() {
        process_emphasis_in_range_filtered(&mut bundle.events, 0, len, None, config.dialect);
    } else {
        bundle.excluded.resize(len, false);
        for &(open_idx, close_idx) in &bundle.bracket_pairs {
            for slot in bundle
                .excluded
                .iter_mut()
                .take(close_idx)
                .skip(open_idx + 1)
            {
                *slot = true;
            }
        }
        process_emphasis_in_range_filtered(
            &mut bundle.events,
            0,
            len,
            Some(&bundle.excluded),
            config.dialect,
        );
    }

    demote_bare_citation_after_emphasis_closer(&mut bundle.events, text);

    InlinePlans {
        emphasis: build_emphasis_plan(&bundle.events),
        brackets: build_bracket_plan(&bundle.events),
        constructs: build_construct_plan(&bundle.events),
        hugo,
    }
}

/// Returns true if any [`IrEvent::DelimRun`] in the event range
/// `[lo, hi)` has byte coverage from its `matches` vec that is less
/// than the run length — i.e. at least one byte of the run failed to
/// pair as emphasis. Used by the Pandoc unresolved-reference degrade
/// pass in [`build_full_plans`].
///
/// Delim runs whose flanking rules forbid both opening *and* closing
/// (e.g. intraword `_` inside `foo_bar`) are skipped: those bytes were
/// never a pairing candidate, so an "unmatched" count for them isn't
/// evidence of a failed emphasis attempt. Without this exclusion every
/// URL or identifier with an underscore inside an unresolved bracket
/// pair would spuriously degrade the bracket-shape to literal text.
fn range_has_unmatched_delim_bytes(events: &[IrEvent], lo: usize, hi: usize) -> bool {
    let hi = hi.min(events.len());
    for ev in &events[lo..hi] {
        if let IrEvent::DelimRun {
            start,
            end,
            matches,
            can_open,
            can_close,
            ..
        } = ev
        {
            if !can_open && !can_close {
                continue;
            }
            let total = end - start;
            let matched: usize = matches.iter().map(|m| m.len as usize).sum();
            if matched < total {
                return true;
            }
        }
    }
    false
}

struct ScratchEvents {
    inner: Option<ScratchBundle>,
}

#[derive(Default)]
struct ScratchBundle {
    events: Vec<IrEvent>,
    bracket_pairs: Vec<(usize, usize)>,
    excluded: Vec<bool>,
}

thread_local! {
    static IR_EVENT_POOL: std::cell::RefCell<Vec<ScratchBundle>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

impl ScratchEvents {
    fn checkout() -> Self {
        let bundle = IR_EVENT_POOL
            .with(|p| p.borrow_mut().pop())
            .unwrap_or_default();
        Self {
            inner: Some(bundle),
        }
    }
}

impl Drop for ScratchEvents {
    fn drop(&mut self) {
        if let Some(mut bundle) = self.inner.take() {
            bundle.events.clear();
            bundle.bracket_pairs.clear();
            bundle.excluded.clear();
            if bundle.events.capacity() <= 8192 {
                IR_EVENT_POOL.with(|p| {
                    let mut pool = p.borrow_mut();
                    if pool.len() < 8 {
                        pool.push(bundle);
                    }
                });
            }
        }
    }
}

/// Bundle of plans produced by [`build_full_plans`] and consumed by the
/// inline emission walk.
#[derive(Debug, Default, Clone)]
pub struct InlinePlans {
    pub emphasis: EmphasisPlan,
    pub brackets: BracketPlan,
    pub constructs: ConstructPlan,
    pub(super) hugo: super::hugo::HugoIndex,
}

/// Convert the IR's delim-run match decisions into an [`EmphasisPlan`],
/// preserving the byte-keyed disposition shape the existing emission walk
/// consumes.
///
/// Each match on a [`DelimRun`](IrEvent::DelimRun) produces one entry in
/// the plan: the opener side records `Open` with the partner's source
/// byte and length; the closer side records `Close`. Bytes within a run
/// that are *not* covered by any match get a `Literal` entry, which the
/// emission walk uses to coalesce unmatched delimiter bytes with
/// surrounding plain text.
pub fn build_emphasis_plan(events: &[IrEvent]) -> EmphasisPlan {
    let mut by_pos: BTreeMap<usize, DelimChar> = BTreeMap::new();
    for ev in events {
        if let IrEvent::DelimRun {
            start,
            end,
            matches,
            ..
        } = ev
        {
            for m in matches {
                let pos = *start + m.offset_in_run as usize;
                let partner_run_start = match &events[m.partner_event as usize] {
                    IrEvent::DelimRun { start: ps, .. } => *ps,
                    _ => continue,
                };
                let partner_pos = partner_run_start + m.partner_offset as usize;
                if m.is_opener {
                    by_pos.insert(
                        pos,
                        DelimChar::Open {
                            len: m.len,
                            partner: partner_pos,
                            partner_len: m.len,
                            kind: m.kind,
                        },
                    );
                } else {
                    by_pos.insert(pos, DelimChar::Close);
                }
            }
            for pos in *start..*end {
                by_pos.entry(pos).or_insert(DelimChar::Literal);
            }
        }
    }
    EmphasisPlan::from_dispositions(by_pos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::Flavor;
    use crate::parser::inlines::inline_ir::DelimChar;
    use std::sync::Arc;

    fn cm_opts() -> ParserOptions {
        let flavor = Flavor::CommonMark;
        ParserOptions::for_flavor(flavor)
    }

    fn refdefs<I: IntoIterator<Item = &'static str>>(labels: I) -> RefdefMap {
        Arc::new(labels.into_iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn ir_event_range_covers_all_variants() {
        let txt = IrEvent::Text { start: 0, end: 5 };
        assert_eq!(txt.range(), (0, 5));

        let close = IrEvent::CloseBracket {
            pos: 7,
            matched: false,
        };
        assert_eq!(close.range(), (7, 8));

        let open = IrEvent::OpenBracket {
            start: 1,
            end: 3,
            is_image: true,
            active: true,
            resolution: None,
            unresolved_ref: None,
        };
        assert_eq!(open.range(), (1, 3));
    }

    #[test]
    fn scan_records_text_and_delim_run() {
        let opts = cm_opts();
        let ir = build_ir("foo *bar*", 0, 9, &opts);
        assert!(matches!(ir[0], IrEvent::Text { start: 0, end: 4 }));
        assert!(matches!(
            ir[1],
            IrEvent::DelimRun {
                ch: b'*',
                start: 4,
                end: 5,
                ..
            }
        ));
        assert!(matches!(ir[2], IrEvent::Text { start: 5, end: 8 }));
        assert!(matches!(
            ir[3],
            IrEvent::DelimRun {
                ch: b'*',
                start: 8,
                end: 9,
                ..
            }
        ));
    }

    #[test]
    fn scan_records_brackets() {
        let opts = cm_opts();
        let ir = build_ir("[foo]", 0, 5, &opts);
        assert!(matches!(
            ir[0],
            IrEvent::OpenBracket {
                start: 0,
                end: 1,
                is_image: false,
                ..
            }
        ));
        assert!(matches!(ir[1], IrEvent::Text { start: 1, end: 4 }));
        assert!(matches!(
            ir[2],
            IrEvent::CloseBracket {
                pos: 4,
                matched: false
            }
        ));
    }

    #[test]
    fn scan_records_image_bracket() {
        let opts = cm_opts();
        let ir = build_ir("![alt]", 0, 6, &opts);
        assert!(matches!(
            ir[0],
            IrEvent::OpenBracket {
                start: 0,
                end: 2,
                is_image: true,
                ..
            }
        ));
    }

    #[test]
    fn scan_handles_code_span_opacity() {
        let opts = cm_opts();
        let ir = build_ir("a `*x*` b", 0, 9, &opts);
        let has_delim_run = ir.iter().any(|e| matches!(e, IrEvent::DelimRun { .. }));
        assert!(
            !has_delim_run,
            "code span content should not produce delim runs"
        );
        assert!(ir.iter().any(|e| matches!(
            e,
            IrEvent::Construct {
                kind: ConstructKind::CodeSpan,
                ..
            }
        )));
    }

    #[test]
    fn process_emphasis_simple_pair() {
        let opts = cm_opts();
        let mut ir = build_ir("*foo*", 0, 5, &opts);
        process_emphasis(&mut ir, opts.dialect);
        let opener = ir
            .iter()
            .find(|e| matches!(e, IrEvent::DelimRun { start: 0, .. }))
            .unwrap();
        if let IrEvent::DelimRun { matches, .. } = opener {
            assert_eq!(matches.len(), 1);
            assert!(matches[0].is_opener);
            assert_eq!(matches[0].kind, EmphasisKind::Emph);
        }
    }

    #[test]
    fn brackets_resolve_inline_link() {
        let opts = cm_opts();
        let mut ir = build_ir("[foo](/url)", 0, 11, &opts);
        process_brackets(
            &mut ir,
            "[foo](/url)",
            None,
            opts.dialect,
            false,
            false,
            false,
        );
        let open = ir
            .iter()
            .find(|e| matches!(e, IrEvent::OpenBracket { start: 0, .. }))
            .unwrap();
        if let IrEvent::OpenBracket { resolution, .. } = open {
            let r = resolution.as_ref().expect("inline link resolved");
            assert!(matches!(r.kind, LinkKind::Inline { .. }));
            if let LinkKind::Inline { dest, .. } = &r.kind {
                assert_eq!(dest, "/url");
            }
        }
    }

    #[test]
    fn brackets_shortcut_resolves_only_with_refdef() {
        let opts = cm_opts();
        let text = "[foo]";
        let map = refdefs(["foo"]);
        let mut ir = build_ir(text, 0, text.len(), &opts);
        process_brackets(&mut ir, text, Some(&map), opts.dialect, false, false, false);
        let open = ir
            .iter()
            .find(|e| matches!(e, IrEvent::OpenBracket { start: 0, .. }))
            .unwrap();
        if let IrEvent::OpenBracket { resolution, .. } = open {
            assert!(matches!(
                resolution.as_ref().unwrap().kind,
                LinkKind::ShortcutReference
            ));
        }
    }

    #[test]
    fn brackets_shortcut_falls_through_without_refdef() {
        let opts = cm_opts();
        let text = "[bar* baz]";
        let mut ir = build_ir(text, 0, text.len(), &opts);
        process_brackets(&mut ir, text, None, opts.dialect, false, false, false);
        let open = ir
            .iter()
            .find(|e| matches!(e, IrEvent::OpenBracket { start: 0, .. }))
            .unwrap();
        if let IrEvent::OpenBracket { resolution, .. } = open {
            assert!(resolution.is_none(), "no refdef → bracket stays literal");
        }
    }

    /// Spec #473: `*[bar*](/url)`. The link `[bar*](/url)` resolves; the
    /// outer `*...*` MUST NOT pair across the link's bracket boundary,
    /// because the inner `*` belongs to the link text.
    #[test]
    fn full_plans_emphasis_does_not_cross_resolved_link_boundary() {
        let opts = cm_opts();
        let text = "*[bar*](/url)";
        let plans = build_full_plans(text, 0, text.len(), &opts);
        assert!(
            matches!(plans.emphasis.lookup(0), Some(DelimChar::Literal) | None),
            "outer `*` at byte 0 must not pair across link boundary, got {:?}",
            plans.emphasis.lookup(0)
        );
        assert!(
            matches!(plans.brackets.lookup(1), Some(BracketDispo::Open { .. })),
            "link [bar*](/url) must resolve at byte 1"
        );
    }

    fn pandoc_opts() -> ParserOptions {
        let flavor = Flavor::Pandoc;
        ParserOptions::for_flavor(flavor)
    }

    #[test]
    fn scan_treats_raw_tex_math_environment_as_opaque() {
        let opts = pandoc_opts();
        let text = "\\begin{equation}\n*x*\n\\end{equation}";
        let ir = build_ir(text, 0, text.len(), &opts);

        assert!(matches!(
            ir.as_slice(),
            [IrEvent::Construct {
                start: 0,
                end,
                kind: ConstructKind::PandocOpaque,
            }] if *end == text.len()
        ));
    }

    /// Bug #2 (a): unresolved Pandoc bracket-shape with unmatched delim
    /// inside its text degrades to literal `[`/`]`. Outer emphasis pair
    /// across the (now-literal) brackets must form.
    #[test]
    fn full_plans_unresolved_bracket_degrades_when_inner_delim_unmatched() {
        let opts = pandoc_opts();
        let text = "*foo [bar*] baz*";
        let plans = build_full_plans(text, 0, text.len(), &opts);
        assert!(
            matches!(plans.brackets.lookup(5), Some(BracketDispo::Literal) | None),
            "degraded `[` at byte 5 must be Literal/None, got {:?}",
            plans.brackets.lookup(5)
        );
        assert!(
            matches!(plans.emphasis.lookup(0), Some(DelimChar::Open { .. })),
            "outer `*` at byte 0 must open Emph after degrade, got {:?}",
            plans.emphasis.lookup(0)
        );
    }

    /// Intraword `_` (e.g. inside a URL like
    /// `hyperparameter_optimization`) is not flanking — `can_open` and
    /// `can_close` are both false — so it can never pair as emphasis.
    /// The degrade pass must not treat such delim runs as "failed
    /// emphasis attempts" and demote the surrounding bracket-shape to
    /// literal text, otherwise every URL/identifier inside an
    /// unresolved reference round-trips through `\[` / `\]` escapes
    /// under `tex_math_single_backslash` and reparses as display math.
    #[test]
    fn full_plans_unresolved_bracket_keeps_wrapper_with_intraword_underscore() {
        let opts = pandoc_opts();
        let text = "[foo_bar more]";
        let plans = build_full_plans(text, 0, text.len(), &opts);
        assert!(
            matches!(
                plans.brackets.lookup(0),
                Some(BracketDispo::UnresolvedReference { .. })
            ),
            "wrapper must be preserved across intraword `_`, got {:?}",
            plans.brackets.lookup(0)
        );
    }

    /// Bug #2 (b): unresolved Pandoc bracket whose interior emphasis
    /// pairs cleanly keeps the wrapper (linter/LSP hook).
    #[test]
    fn full_plans_unresolved_bracket_keeps_wrapper_when_inner_paired() {
        let opts = pandoc_opts();
        let text = "[foo *bar*]";
        let plans = build_full_plans(text, 0, text.len(), &opts);
        assert!(
            matches!(
                plans.brackets.lookup(0),
                Some(BracketDispo::UnresolvedReference { .. })
            ),
            "wrapper must be preserved when inner emph pairs, got {:?}",
            plans.brackets.lookup(0)
        );
    }

    /// Spec #533: `[foo *bar [baz][ref]*][ref]` with `[ref]: /uri`.
    /// Inner `[baz][ref]` resolves as a link; §6.3 link-in-link rule
    /// deactivates the outer `[foo ...][ref]` so it falls through to
    /// literal brackets. Emphasis `*bar [baz][ref]*` wraps the inner link.
    #[test]
    fn full_plans_link_in_link_suppression_for_reference_links() {
        let opts = cm_opts();
        let text = "[foo *bar [baz][ref]*][ref]";
        let mut opts_with_refs = opts.clone();
        let labels: HashSet<String> = ["ref".to_string()].into_iter().collect();
        opts_with_refs.refdef_labels = Some(std::sync::Arc::new(labels));
        let plans = build_full_plans(text, 0, text.len(), &opts_with_refs);

        assert!(
            matches!(plans.brackets.lookup(10), Some(BracketDispo::Open { .. })),
            "inner [baz][ref] must resolve at byte 10, got {:?}",
            plans.brackets.lookup(10)
        );
        assert!(
            matches!(plans.brackets.lookup(0), Some(BracketDispo::Literal) | None),
            "outer [foo ...][ref] must fall through to literal at byte 0, got {:?}",
            plans.brackets.lookup(0)
        );
        assert!(
            matches!(plans.brackets.lookup(22), Some(BracketDispo::Open { .. })),
            "trailing [ref] must resolve at byte 22, got {:?}",
            plans.brackets.lookup(22)
        );
        assert!(
            matches!(plans.emphasis.lookup(5), Some(DelimChar::Open { .. })),
            "emphasis opener at byte 5 must pair, got {:?}",
            plans.emphasis.lookup(5)
        );
    }

    /// Pandoc `notAfterString`, delimiter-adjacent case: a bare `@key`
    /// glued to a resolved emphasis closer (`*em*@key`) is demoted to
    /// literal text — pandoc parses it as `Emph [Str "em"]` + `Str
    /// "@key"`, not a citation.
    #[test]
    fn full_plans_bare_citation_after_emphasis_closer_demoted() {
        let opts = pandoc_opts();
        let plans = build_full_plans("*em*@key", 0, 8, &opts);
        assert!(
            plans.constructs.lookup(4).is_none(),
            "bare `@key` after emphasis closer must be demoted, got {:?}",
            plans.constructs.lookup(4)
        );
        assert!(
            matches!(plans.emphasis.lookup(0), Some(DelimChar::Open { .. })),
            "emphasis `*em*` must still pair, got {:?}",
            plans.emphasis.lookup(0)
        );
    }

    #[test]
    fn full_plans_bare_citation_after_strong_closer_demoted() {
        let opts = pandoc_opts();
        let plans = build_full_plans("**s**@key", 0, 9, &opts);
        assert!(
            plans.constructs.lookup(5).is_none(),
            "bare `@key` after strong closer must be demoted, got {:?}",
            plans.constructs.lookup(5)
        );
    }

    /// A bare `@key` after an emphasis *opener* (`*@key*`) is NOT
    /// suppressed — pandoc parses it as `Emph [Cite ...]`. The demotion
    /// must key off closers only.
    #[test]
    fn full_plans_bare_citation_after_emphasis_opener_kept() {
        let opts = pandoc_opts();
        let plans = build_full_plans("*@key*", 0, 6, &opts);
        assert!(
            matches!(
                plans.constructs.lookup(1),
                Some(ConstructDispo::BareCitation { .. })
            ),
            "bare `@key` after emphasis opener must stay a citation, got {:?}",
            plans.constructs.lookup(1)
        );
    }

    /// The suppress-author `-@key` form is unaffected: `*em*-@key` keys
    /// off the `-`, so pandoc keeps the citation (`Emph` + `Str "-"` +
    /// `Cite`). The construct starts at the `-` (byte 4), not the `@`.
    #[test]
    fn full_plans_suppress_author_citation_after_emphasis_kept() {
        let opts = pandoc_opts();
        let plans = build_full_plans("*em*-@key", 0, 9, &opts);
        assert!(
            matches!(
                plans.constructs.lookup(4),
                Some(ConstructDispo::BareCitation { .. })
            ),
            "suppress-author `-@key` after emphasis must stay a citation, got {:?}",
            plans.constructs.lookup(4)
        );
    }

    /// A literal `_` between the emphasis closer and the `@` breaks the
    /// gluing: `*em*_@key` keeps the citation because the `@`'s
    /// predecessor is the unmatched `_`, not the resolved closer.
    #[test]
    fn full_plans_bare_citation_after_unmatched_delim_kept() {
        let opts = pandoc_opts();
        let plans = build_full_plans("*em*_@key", 0, 9, &opts);
        assert!(
            matches!(
                plans.constructs.lookup(5),
                Some(ConstructDispo::BareCitation { .. })
            ),
            "bare `@key` after unmatched `_` must stay a citation, got {:?}",
            plans.constructs.lookup(5)
        );
    }
}
