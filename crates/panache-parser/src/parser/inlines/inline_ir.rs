//! Inline IR for the CommonMark dialect.
//!
//! The CommonMark inline parsing pipeline runs in three passes over an
//! intermediate representation (IR):
//!
//! 1. **Scan** ([`build_ir`]): walk the source bytes once, producing a flat
//!    [`Vec<IrEvent>`]. Opaque higher-precedence constructs (escapes, code
//!    spans, autolinks, raw HTML) are skipped past as a single
//!    [`IrEvent::Construct`] event whose source range is preserved for
//!    losslessness. Delimiter runs (`*`/`_`), bracket markers (`[`, `![`,
//!    `]`), soft line breaks, and plain text spans become distinct events.
//!
//! 2. **Process emphasis** ([`process_emphasis`]) — CommonMark §6.2: the
//!    classic delimiter-stack algorithm runs over the [`IrEvent::DelimRun`]
//!    events, pairing openers with closers and recording matches on the
//!    runs. Each match consumes 1 or 2 inner-edge bytes from each side;
//!    leftover bytes fall through to literal text.
//!
//! 3. **Process brackets** ([`process_brackets`]) — CommonMark §6.3: the
//!    bracket-stack algorithm walks `]` markers left-to-right. For each
//!    `]`, the algorithm finds the nearest active opener and tries to
//!    resolve the pair as a link or image: inline `[text](dest)`, full
//!    reference `[text][label]`, collapsed `[text][]`, or shortcut
//!    `[text]`. Reference forms are validated against the document refdef
//!    map. When a match resolves, all earlier active openers are marked
//!    inactive — this implements the "links may not contain other links"
//!    rule (§6.3) by suppressing outer bracket pairs once an inner link
//!    has been recognised.
//!
//! Emission ([`emit_from_ir`]) walks the resolved IR and writes CST nodes
//! via `GreenNodeBuilder`. Matched delim runs become `EMPHASIS` / `STRONG`
//! nodes wrapping a recursive emit of the inner range. Matched bracket
//! pairs become `LINK` / `IMAGE` nodes. Unmatched delims and brackets
//! fall through to plain text.
//!
//! The IR is `Dialect::CommonMark`-only. The Pandoc dialect retains its
//! existing recursive-descent inline parser; both paths coexist behind
//! the `dialect` switch in [`super::core::parse_inline_text_recursive`].

use crate::options::ParserOptions;
use crate::parser::inlines::refdef_map::{RefdefMap, normalize_label};
use std::collections::HashSet;

use super::code_spans::try_parse_code_span;
use super::delimiter_stack::EmphasisKind;
use super::escapes::{EscapeType, try_parse_escape};
use super::inline_html::try_parse_inline_html;
use super::links::try_parse_autolink;

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
    /// `\X` literal-character escape (CommonMark §2.4).
    Escape,
    /// `` `code` `` span (§6.1).
    CodeSpan,
    /// `<scheme://...>` or `<email@host>` (§6.5).
    Autolink,
    /// `<tag ...>` and friends (§6.6).
    InlineHtml,
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

// ============================================================================
// Pass 1: Scan
// ============================================================================

/// Scan `text[start..end]` once, producing a flat IR of events.
///
/// The scan is forward-only and never backtracks: each iteration either
/// consumes a known construct (escape, code span, autolink, raw HTML),
/// records a delim run / bracket marker / line break, or steps past a
/// single UTF-8 boundary as plain text. Adjacent text bytes are coalesced
/// into a single [`IrEvent::Text`] event by the run-flush step.
pub fn build_ir(text: &str, start: usize, end: usize, config: &ParserOptions) -> Vec<IrEvent> {
    let mut events = Vec::new();
    let bytes = text.as_bytes();
    let exts = &config.extensions;
    let is_commonmark = config.dialect == crate::options::Dialect::CommonMark;

    let mut pos = start;
    let mut text_run_start = start;

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
        let b = bytes[pos];

        // Backslash escape (§2.4) — including `\\\n` hard line break.
        if b == b'\\'
            && let Some((len, _ch, escape_type)) = try_parse_escape(&text[pos..])
            && pos + len <= end
        {
            let enabled = match escape_type {
                EscapeType::Literal => is_commonmark || exts.all_symbols_escapable,
                EscapeType::HardLineBreak => exts.escaped_line_breaks,
                EscapeType::NonbreakingSpace => exts.all_symbols_escapable,
            };
            if enabled {
                flush_text!();
                let kind = match escape_type {
                    EscapeType::HardLineBreak => {
                        events.push(IrEvent::HardBreak {
                            start: pos,
                            end: pos + len,
                        });
                        pos += len;
                        text_run_start = pos;
                        continue;
                    }
                    EscapeType::Literal | EscapeType::NonbreakingSpace => ConstructKind::Escape,
                };
                events.push(IrEvent::Construct {
                    start: pos,
                    end: pos + len,
                    kind,
                });
                pos += len;
                text_run_start = pos;
                continue;
            }
        }

        // Code span (§6.1) — opaque to emphasis and brackets.
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

        // Autolink (§6.5) before raw HTML — autolinks are the more specific
        // shape inside `<...>`.
        if b == b'<' {
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
                && let Some(len) = try_parse_inline_html(&text[pos..])
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

        // `![` opens an image bracket.
        if b == b'!' && pos + 1 < end && bytes[pos + 1] == b'[' && exts.inline_images {
            flush_text!();
            events.push(IrEvent::OpenBracket {
                start: pos,
                end: pos + 2,
                is_image: true,
                active: true,
                resolution: None,
            });
            pos += 2;
            text_run_start = pos;
            continue;
        }

        // `[` opens a link bracket.
        if b == b'[' && exts.inline_links {
            flush_text!();
            // Citations / footnote references / bracketed spans are Pandoc
            // constructs, not enabled in CommonMark dialect by default.
            // The IR is CommonMark-only; if any of those are enabled by
            // user override under a CommonMark flavor, we leave them to
            // the existing emission path by skipping IR usage at the
            // dispatcher level (handled in `core::parse_inline_text_recursive`).
            events.push(IrEvent::OpenBracket {
                start: pos,
                end: pos + 1,
                is_image: false,
                active: true,
                resolution: None,
            });
            pos += 1;
            text_run_start = pos;
            continue;
        }

        // `]` closes a link/image bracket.
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

        // `*` or `_` delimiter run.
        if b == b'*' || b == b'_' {
            flush_text!();
            let mut run_end = pos;
            while run_end < end && bytes[run_end] == b {
                run_end += 1;
            }
            let count = run_end - pos;
            let (can_open, can_close) = compute_flanking(text, pos, count, b);
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

        // Hard line break: 2+ trailing spaces before newline. We detect
        // this when we're sitting on a `\n` (or `\r\n`) and the preceding
        // bytes within the current text run are spaces.
        if b == b'\n' || (b == b'\r' && pos + 1 < end && bytes[pos + 1] == b'\n') {
            // Count trailing spaces in the text accumulated so far.
            let nl_len = if b == b'\r' { 2 } else { 1 };
            let mut trailing_spaces = 0;
            let mut s = pos;
            while s > text_run_start && bytes[s - 1] == b' ' {
                trailing_spaces += 1;
                s -= 1;
            }
            if trailing_spaces >= 2 {
                // Flush text *before* the trailing spaces.
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

            // Soft line break: flush preceding text, emit the line ending
            // as its own event so the emitter can render `NEWLINE` tokens
            // verbatim.
            flush_text!();
            events.push(IrEvent::SoftBreak {
                start: pos,
                end: pos + nl_len,
            });
            pos += nl_len;
            text_run_start = pos;
            continue;
        }

        // Plain byte — advance one UTF-8 char.
        let ch_len = text[pos..]
            .chars()
            .next()
            .map_or(1, std::primitive::char::len_utf8);
        pos += ch_len.max(1);
    }

    flush_text!();
    events
}

// ============================================================================
// Flanking (CommonMark §6.2)
// ============================================================================

fn compute_flanking(text: &str, pos: usize, count: usize, ch: u8) -> (bool, bool) {
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

// ============================================================================
// Pass 2: Process emphasis (CommonMark §6.2)
// ============================================================================

/// Run the CommonMark §6.3 `process_emphasis` algorithm over the IR's
/// delim runs. Mutates the IR in place: matched runs gain entries in their
/// `matches` vec, unmatched bytes stay implicit (the emission pass treats
/// any byte not covered by a match as literal text).
///
/// The algorithm tracks a per-bucket `openers_bottom` exclusive lower
/// bound to keep walk-back bounded; consume rules and the §6.2 mod-3
/// rejection match the reference implementation.
pub fn process_emphasis(events: &mut [IrEvent]) {
    // Indices of DelimRun events, in order.
    let mut delim_idxs: Vec<usize> = events
        .iter()
        .enumerate()
        .filter_map(|(i, e)| matches!(e, IrEvent::DelimRun { .. }).then_some(i))
        .collect();
    if delim_idxs.is_empty() {
        return;
    }

    // Working state: count (remaining unmatched chars) and source_start
    // (first remaining char) per delim run. Indexed by position in
    // `delim_idxs`.
    let mut count: Vec<usize> = Vec::with_capacity(delim_idxs.len());
    let mut source_start: Vec<usize> = Vec::with_capacity(delim_idxs.len());
    let mut removed: Vec<bool> = vec![false; delim_idxs.len()];

    for &ev_idx in &delim_idxs {
        if let IrEvent::DelimRun { start, end, .. } = &events[ev_idx] {
            count.push(end - start);
            source_start.push(*start);
        }
    }

    // openers_bottom[ch_idx][len%3][can_open] → exclusive lower bound
    // (an index into `delim_idxs`, or None meaning "no bottom yet").
    let mut openers_bottom: [[[Option<usize>; 2]; 3]; 2] = [[[None; 2]; 3]; 2];

    // First active index, scanning forward.
    let first_active =
        |removed: &[bool]| -> Option<usize> { (0..removed.len()).find(|&i| !removed[i]) };
    let next_active = |removed: &[bool], from: usize| -> Option<usize> {
        (from + 1..removed.len()).find(|&i| !removed[i])
    };
    let prev_active =
        |removed: &[bool], from: usize| -> Option<usize> { (0..from).rev().find(|&i| !removed[i]) };

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
        if !can_close_c || removed[c] {
            closer_local = next_active(&removed, c);
            continue;
        }

        let ch_idx = if ch_c == b'*' { 0 } else { 1 };
        let closer_mod = count[c] % 3;
        let closer_open_bucket = can_open_c as usize;
        let bottom = openers_bottom[ch_idx][closer_mod][closer_open_bucket];

        // Walk back to find a compatible opener.
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
                let mod3_reject = (opener_both || closer_both)
                    && oc_sum.is_multiple_of(3)
                    && !(count[o].is_multiple_of(3) && count[c].is_multiple_of(3));
                if !mod3_reject {
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
            let consume = if count[o] >= 2 && count[c] >= 2 { 2 } else { 1 };
            let kind = if consume == 2 {
                EmphasisKind::Strong
            } else {
                EmphasisKind::Emph
            };

            // Opener consumes inner-edge (rightmost) chars.
            let opener_match_offset =
                source_start[o] + count[o] - consume - source_start_event(&events[delim_idxs[o]]);
            // Closer consumes inner-edge (leftmost) chars.
            let closer_match_offset = source_start[c] - source_start_event(&events[delim_idxs[c]]);

            // Record match on opener.
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
            // Record match on closer.
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

            // Remove all openers strictly between o and c.
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
            // Else re-process the same closer with reduced count.
        } else {
            openers_bottom[ch_idx][closer_mod][closer_open_bucket] = prev_active(&removed, c);
            if !can_open_c {
                removed[c] = true;
            }
            closer_local = next_active(&removed, c);
        }
    }

    // No further mutation needed: matches are recorded; remaining bytes
    // stay implicit literal.
    let _ = &mut delim_idxs;
}

fn source_start_event(event: &IrEvent) -> usize {
    match event {
        IrEvent::DelimRun { start, .. } => *start,
        _ => unreachable!("source_start_event called on non-DelimRun"),
    }
}

// ============================================================================
// Pass 3: Process brackets (CommonMark §6.3)
// ============================================================================

/// Resolve `[`/`![`/`]` markers into link/image nodes per CommonMark §6.3.
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
/// flagged `matched`. All earlier active openers are deactivated to
/// implement the §6.3 "links may not contain other links" rule. (Image
/// brackets do not deactivate earlier link openers — only links do.)
///
/// On a miss the bracket pair stays opaque-as-literal and the closer is
/// dropped from the bracket stack so the next `]` can re-pair.
pub fn process_brackets(events: &mut [IrEvent], text: &str, refdefs: Option<&RefdefMap>) {
    let empty: HashSet<String> = HashSet::new();
    let labels: &HashSet<String> = match refdefs {
        Some(map) => map.as_ref(),
        None => &empty,
    };

    // Walk forward through events, treating it as a linear scan for `]`.
    let mut i = 0;
    while i < events.len() {
        let close_pos = match &events[i] {
            IrEvent::CloseBracket { pos, .. } => *pos,
            _ => {
                i += 1;
                continue;
            }
        };

        // Find the nearest active OpenBracket before `i`.
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

        // 1. Inline link / image.
        if let Some((suffix_end, dest, title)) = try_inline_suffix(text, after_close) {
            // §6.3 link-in-link rule: if this is a *link* (not an image),
            // and any earlier active link opener exists, deactivate them.
            // We also deactivate openers strictly before `o` here because
            // matching means the inner link wins; the spec applies this
            // *after* matching.
            if !is_image {
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
            // Remove the opener from the bracket stack: it has been
            // matched (active=false will fall out automatically since
            // resolution is Some).
            mark_opener_resolved(events, o);
            i += 1;
            continue;
        }

        // 2. Full reference link: `[text][label]`.
        if let Some((suffix_end, label_raw)) = try_full_reference_suffix(text, after_close) {
            let label_norm = normalize_label(&label_raw);
            if !label_norm.is_empty() && labels.contains(&label_norm) {
                if !is_image {
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
                    LinkKind::FullReference { label: label_raw },
                );
                mark_opener_resolved(events, o);
                i += 1;
                continue;
            }
            // Bracketed but unresolved label: §6.3 says we still treat
            // `[text][label]` as not-a-link, but the brackets get consumed
            // as literal text. Continue to next `]`.
        }

        // 3 & 4. Collapsed `[]` or shortcut.
        let link_text = &text[text_start..text_end];
        let link_text_norm = normalize_label(link_text);
        let is_collapsed = is_collapsed_marker(text, after_close);
        let collapsed_suffix_end = after_close + 2;

        if !link_text_norm.is_empty() && labels.contains(&link_text_norm) {
            if is_collapsed {
                if !is_image {
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
            // Shortcut: bracket must NOT be followed by `(` or `[`.
            // Also, the link text must not itself contain a bracket pair
            // that resolved to a link (handled by deactivation above —
            // when we resolved an inner link, this opener was earlier
            // and got deactivated, so we won't reach here for that case).
            // CommonMark also rules out shortcut when followed by `{...}`
            // attribute, but that's a Pandoc construct; CommonMark treats
            // `{...}` as literal text after the bracket.
            if !is_followed_by_inline_or_full_ref_or_collapsed(text, after_close) {
                if !is_image {
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
        }

        // No resolution. Drop the opener — its `]` partner is this one,
        // but since neither matched, the opener falls through to literal
        // text. We do this by deactivating the opener (so it won't be
        // considered for later `]` markers either).
        if let IrEvent::OpenBracket { active, .. } = &mut events[o] {
            *active = false;
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

/// Try to parse `(dest)` or `(dest "title")` inline link suffix starting
/// at `text[pos]`. Returns `(end_pos_exclusive, dest, title)`.
fn try_inline_suffix(text: &str, pos: usize) -> Option<(usize, String, Option<String>)> {
    let bytes = text.as_bytes();
    if pos >= bytes.len() || bytes[pos] != b'(' {
        return None;
    }
    let mut p = pos + 1;
    // Skip leading whitespace.
    while p < bytes.len() && matches!(bytes[p], b' ' | b'\t' | b'\n') {
        p += 1;
    }
    // Empty `()` — link with empty destination.
    if p < bytes.len() && bytes[p] == b')' {
        return Some((p + 1, String::new(), None));
    }

    // Parse destination.
    let (dest, dest_end) = parse_link_destination(text, p)?;
    p = dest_end;

    // Skip whitespace.
    while p < bytes.len() && matches!(bytes[p], b' ' | b'\t' | b'\n') {
        p += 1;
    }

    // Optional title.
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

fn parse_link_destination(text: &str, start: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    if start >= bytes.len() {
        return None;
    }
    if bytes[start] == b'<' {
        // <bracketed>
        let mut p = start + 1;
        let begin = p;
        while p < bytes.len() && bytes[p] != b'>' && bytes[p] != b'\n' && bytes[p] != b'<' {
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
        // unbracketed: balanced parens, no spaces, no controls
        let mut p = start;
        let mut paren_depth: i32 = 0;
        while p < bytes.len() {
            let b = bytes[p];
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

/// Try to parse `[label]` after a `]`. Returns `(suffix_end, label_raw)`.
/// For the collapsed form `[]`, returns `None` here (handled separately
/// by `is_collapsed_marker`).
fn try_full_reference_suffix(text: &str, pos: usize) -> Option<(usize, String)> {
    let bytes = text.as_bytes();
    if pos >= bytes.len() || bytes[pos] != b'[' {
        return None;
    }
    let label_start = pos + 1;
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

fn is_collapsed_marker(text: &str, pos: usize) -> bool {
    text.as_bytes().get(pos) == Some(&b'[') && text.as_bytes().get(pos + 1) == Some(&b']')
}

fn is_followed_by_inline_or_full_ref_or_collapsed(text: &str, pos: usize) -> bool {
    let bytes = text.as_bytes();
    bytes.get(pos) == Some(&b'(') || bytes.get(pos) == Some(&b'[')
}

// ============================================================================
// Bracket plan — byte-position-keyed view of resolved brackets, consumed by
// the existing emission walk in `core::parse_inline_range_impl`.
// ============================================================================

use std::collections::BTreeMap;

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
                is_image,
                resolution: None,
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

/// One-shot helper: build the IR, run all passes, and return both the
/// [`BracketPlan`] and the byte-keyed emphasis plan
/// ([`EmphasisPlan`](super::delimiter_stack::EmphasisPlan)) — packaged
/// together so the CommonMark inline emission path can consume them in
/// one go.
pub fn build_full_plans(
    text: &str,
    start: usize,
    end: usize,
    config: &ParserOptions,
) -> InlinePlans {
    let mut events = build_ir(text, start, end, config);
    process_emphasis(&mut events);
    process_brackets(&mut events, text, config.refdef_labels.as_ref());
    InlinePlans {
        emphasis: build_emphasis_plan(&events),
        brackets: build_bracket_plan(&events),
    }
}

/// Bundle of plans produced by [`build_full_plans`] and consumed by the
/// CommonMark dialect's emission walk.
#[derive(Debug, Default, Clone)]
pub struct InlinePlans {
    pub emphasis: super::delimiter_stack::EmphasisPlan,
    pub brackets: BracketPlan,
}

/// Convert the IR's delim-run match decisions into an
/// [`EmphasisPlan`](super::delimiter_stack::EmphasisPlan), preserving the
/// byte-keyed disposition shape the existing emission walk consumes.
///
/// Each match on a [`DelimRun`](IrEvent::DelimRun) produces one entry in
/// the plan: the opener side records `Open` with the partner's source
/// byte and length; the closer side records `Close`. Bytes within a run
/// that are *not* covered by any match get a `Literal` entry, which the
/// emission walk uses to coalesce unmatched delimiter bytes with
/// surrounding plain text.
pub fn build_emphasis_plan(events: &[IrEvent]) -> super::delimiter_stack::EmphasisPlan {
    use super::delimiter_stack::DelimChar;
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
            // Any remaining bytes (not covered by a match) are literal.
            for pos in *start..*end {
                by_pos.entry(pos).or_insert(DelimChar::Literal);
            }
        }
    }
    super::delimiter_stack::EmphasisPlan::from_dispositions(by_pos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::Flavor;
    use std::sync::Arc;

    fn cm_opts() -> ParserOptions {
        let flavor = Flavor::CommonMark;
        ParserOptions {
            flavor,
            dialect: crate::options::Dialect::for_flavor(flavor),
            extensions: crate::options::Extensions::for_flavor(flavor),
            pandoc_compat: crate::options::PandocCompat::default(),
            refdef_labels: None,
        }
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
        };
        assert_eq!(open.range(), (1, 3));
    }

    #[test]
    fn scan_records_text_and_delim_run() {
        let opts = cm_opts();
        let ir = build_ir("foo *bar*", 0, 9, &opts);
        // Expect: Text "foo ", DelimRun "*", Text "bar", DelimRun "*"
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
        // Code span `*x*` should be a Construct, NOT delim runs.
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
        process_emphasis(&mut ir);
        // First DelimRun (open) gets a match.
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
        process_brackets(&mut ir, "[foo](/url)", None);
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
        process_brackets(&mut ir, text, Some(&map));
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
        // CMark example #523 mechanic: `[bar* baz]` is not a refdef, so
        // it must NOT resolve as a link — the brackets stay literal so
        // the inner `*` becomes available to the outer emphasis scanner.
        let opts = cm_opts();
        let text = "[bar* baz]";
        let mut ir = build_ir(text, 0, text.len(), &opts);
        process_brackets(&mut ir, text, None);
        let open = ir
            .iter()
            .find(|e| matches!(e, IrEvent::OpenBracket { start: 0, .. }))
            .unwrap();
        if let IrEvent::OpenBracket { resolution, .. } = open {
            assert!(resolution.is_none(), "no refdef → bracket stays literal");
        }
    }
}
