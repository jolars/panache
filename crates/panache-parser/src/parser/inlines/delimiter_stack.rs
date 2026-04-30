//! CommonMark §6.3 emphasis resolution via a delimiter stack.
//!
//! This module implements CMark's `process_emphasis` algorithm: pre-scan an
//! inline byte range to enumerate `*` / `_` delimiter runs (skipping bytes
//! inside opaque higher-precedence inline constructs — escapes, code spans,
//! autolinks, raw HTML, and link/image bracket spans), then walk those runs
//! using a delimiter stack to compute opener/closer pairings that obey rules
//! 1–17 of the CommonMark spec, including the multiple-of-3 rule (rules 9 &
//! 10) and lazy/late-opener preference (rule 7) that the previous greedy
//! recursive-descent emphasis parser couldn't express.
//!
//! The output is an [`EmphasisPlan`] mapping each delimiter byte position to
//! its disposition (open marker, close marker, or unmatched literal). The
//! existing inline emission walk in `core.rs` consults the plan when running
//! under [`Dialect::CommonMark`]; the Pandoc dialect is unchanged and stays
//! on its recursive-descent path until a follow-up session migrates it.
//!
//! Long-term direction: this module evolves into a full inline IR (with
//! pre-resolved opaque-construct subtrees and `[` / `]` bracket markers) so
//! the same algorithm also drives link-bracket resolution. Today it is
//! emphasis-only; brackets and Pandoc-dialect adoption are explicit
//! follow-ups.

use crate::options::{Dialect, ParserOptions};
use std::collections::BTreeMap;

use super::code_spans::try_parse_code_span;
use super::escapes::try_parse_escape;
use super::inline_html::try_parse_inline_html;
use super::links::{
    LinkScanContext, try_parse_autolink, try_parse_inline_image, try_parse_inline_link,
    try_parse_reference_image, try_parse_reference_link,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmphasisKind {
    Emph,
    Strong,
}

/// Disposition of a single delimiter byte after `process_emphasis` has run.
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
}

#[derive(Debug, Clone, Copy)]
struct DelimRun {
    ch: u8,
    /// Source-byte offset of the first surviving (unmatched) char in this run.
    /// As the closer side consumes inner-edge chars, this advances rightward.
    source_start: usize,
    /// Number of unmatched chars remaining in this run.
    count: u16,
    can_open: bool,
    can_close: bool,
    /// `true` once removed from the stack (either consumed to count 0, or
    /// rejected as unmatched closer).
    removed: bool,
}

/// Build the emphasis pair plan for `text[start..end]`.
///
/// CommonMark dialect only — callers should gate on
/// `config.dialect == Dialect::CommonMark`. The plan is consumed by the
/// emission walk in `core.rs` to drive opener/closer pairing without a
/// separate scanning pass during emission.
pub fn build_plan(text: &str, start: usize, end: usize, config: &ParserOptions) -> EmphasisPlan {
    let runs = scan_delim_runs(text, start, end, config);
    process_emphasis(text, runs, config.dialect)
}

/// Walk `text[start..end]`, recording `*` and `_` delimiter runs while
/// skipping bytes inside opaque higher-precedence inline constructs.
fn scan_delim_runs(text: &str, start: usize, end: usize, config: &ParserOptions) -> Vec<DelimRun> {
    let mut runs = Vec::new();
    let bytes = text.as_bytes();
    let exts = &config.extensions;
    let link_ctx = LinkScanContext::from_options(config);
    let is_commonmark = config.dialect == Dialect::CommonMark;

    let mut pos = start;
    while pos < end {
        let b = bytes[pos];

        // Escapes — `\X` consumes 2 bytes if X is escapable. Under CommonMark,
        // any ASCII punctuation is escapable; gate the predicate the same way
        // the emission path does (`core.rs:1607-1617`).
        if b == b'\\'
            && let Some((len, _ch, escape_type)) = try_parse_escape(&text[pos..])
        {
            let enabled = match escape_type {
                super::escapes::EscapeType::Literal => is_commonmark || exts.all_symbols_escapable,
                super::escapes::EscapeType::HardLineBreak => exts.escaped_line_breaks,
                super::escapes::EscapeType::NonbreakingSpace => exts.all_symbols_escapable,
            };
            if enabled && pos + len <= end {
                pos += len;
                continue;
            }
        }

        // Code spans — opaque to emphasis (CommonMark §6 precedence).
        if b == b'`'
            && let Some((len, _, _, _)) = try_parse_code_span(&text[pos..])
            && pos + len <= end
        {
            pos += len;
            continue;
        }

        // Autolinks (`<scheme://…>`, `<email>`) and raw HTML — opaque inside
        // the angle-bracket span. Order matters: try autolink first since it's
        // a more specific shape than a raw HTML tag.
        if b == b'<' {
            if exts.autolinks
                && let Some((len, _)) = try_parse_autolink(&text[pos..], is_commonmark)
                && pos + len <= end
            {
                pos += len;
                continue;
            }
            if exts.raw_html
                && let Some(len) = try_parse_inline_html(&text[pos..])
                && pos + len <= end
            {
                pos += len;
                continue;
            }
        }

        // Inline image `![alt](dest)` — keep before inline link to consume
        // the leading `!`.
        if b == b'!'
            && pos + 1 < end
            && bytes[pos + 1] == b'['
            && exts.inline_images
            && let Some((len, _, _, _)) = try_parse_inline_image(&text[pos..], link_ctx)
            && pos + len <= end
        {
            pos += len;
            continue;
        }

        // Reference image `![alt][ref]` / `![alt]` shortcut variant.
        if b == b'!'
            && pos + 1 < end
            && bytes[pos + 1] == b'['
            && exts.reference_links
            && let Some((len, _, _, _)) =
                try_parse_reference_image(&text[pos..], exts.shortcut_reference_links)
            && pos + len <= end
        {
            pos += len;
            continue;
        }

        // Inline link `[text](dest)` — opaque to outer emphasis. Emphasis
        // inside the link text is resolved by the recursive emission building
        // a separate plan for that nested range.
        if b == b'['
            && exts.inline_links
            && let Some((len, _, _, _)) =
                try_parse_inline_link(&text[pos..], is_commonmark, link_ctx)
            && pos + len <= end
        {
            pos += len;
            continue;
        }

        // Reference link `[text][ref]`, `[text][]`, or `[text]` shortcut form.
        if b == b'['
            && exts.reference_links
            && let Some((len, _, _, _)) = try_parse_reference_link(
                &text[pos..],
                exts.shortcut_reference_links,
                false,
                link_ctx,
            )
            && pos + len <= end
        {
            pos += len;
            continue;
        }

        // Delimiter run.
        if b == b'*' || b == b'_' {
            let mut run_end = pos;
            while run_end < end && bytes[run_end] == b {
                run_end += 1;
            }
            let count = (run_end - pos) as u16;
            let (can_open, can_close) = compute_flanking(text, pos, count as usize, b, config);
            runs.push(DelimRun {
                ch: b,
                source_start: pos,
                count,
                can_open,
                can_close,
                removed: false,
            });
            pos = run_end;
            continue;
        }

        // Plain byte — advance one UTF-8 char (the run-detection path above
        // only fires on ASCII delimiters, so a multibyte char never starts a
        // run; advancing by `char` length keeps us aligned).
        let ch_len = text[pos..].chars().next().map_or(1, |c| c.len_utf8());
        pos += ch_len.max(1);
    }

    runs
}

/// CommonMark §6.2 left/right flanking, plus §6.2 underscore intra-word rules.
/// Parameterized by [`Dialect`] for future Pandoc reuse — only CommonMark is
/// exercised today.
fn compute_flanking(
    text: &str,
    pos: usize,
    count: usize,
    ch: u8,
    config: &ParserOptions,
) -> (bool, bool) {
    let lf = is_left_flanking_local(text, pos, count);
    let rf = is_right_flanking_local(text, pos, count);
    if ch == b'*' {
        (lf, rf)
    } else {
        // Underscore §6.2: a `_` run can open emphasis only if it is
        // left-flanking AND either (a) not right-flanking, or (b) right-flanking
        // and preceded by punctuation. Symmetric for closing.
        let prev_char = (pos > 0).then(|| text[..pos].chars().last()).flatten();
        let next_char = text.get(pos + count..).and_then(|s| s.chars().next());
        let preceded_by_punct = prev_char.is_some_and(is_unicode_punct_or_symbol_local);
        let followed_by_punct = next_char.is_some_and(is_unicode_punct_or_symbol_local);
        let _ = config;
        let can_open = lf && (!rf || preceded_by_punct);
        let can_close = rf && (!lf || followed_by_punct);
        (can_open, can_close)
    }
}

fn is_unicode_punct_or_symbol_local(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_punctuation()
    } else {
        !c.is_alphanumeric() && !c.is_whitespace()
    }
}

fn is_left_flanking_local(text: &str, run_start: usize, run_len: usize) -> bool {
    let after = run_start + run_len;
    let next_char = text.get(after..).and_then(|s| s.chars().next());
    let prev_char = (run_start > 0)
        .then(|| text[..run_start].chars().last())
        .flatten();

    let followed_by_ws = next_char.is_none_or(|c| c.is_whitespace());
    if followed_by_ws {
        return false;
    }
    let followed_by_punct = next_char.is_some_and(is_unicode_punct_or_symbol_local);
    if !followed_by_punct {
        return true;
    }
    prev_char.is_none_or(|c| c.is_whitespace() || is_unicode_punct_or_symbol_local(c))
}

fn is_right_flanking_local(text: &str, run_start: usize, run_len: usize) -> bool {
    let after = run_start + run_len;
    let next_char = text.get(after..).and_then(|s| s.chars().next());
    let prev_char = (run_start > 0)
        .then(|| text[..run_start].chars().last())
        .flatten();

    let preceded_by_ws = prev_char.is_none_or(|c| c.is_whitespace());
    if preceded_by_ws {
        return false;
    }
    let preceded_by_punct = prev_char.is_some_and(is_unicode_punct_or_symbol_local);
    if !preceded_by_punct {
        return true;
    }
    next_char.is_none_or(|c| c.is_whitespace() || is_unicode_punct_or_symbol_local(c))
}

/// CommonMark §6.3 `process_emphasis`. Mutates `runs` in place to consume
/// matched chars and produces an [`EmphasisPlan`].
fn process_emphasis(_text: &str, mut runs: Vec<DelimRun>, _dialect: Dialect) -> EmphasisPlan {
    if runs.is_empty() {
        return EmphasisPlan::default();
    }

    // Each "char index" key corresponds to one of the four buckets CMark uses
    // for `openers_bottom`: `(ch_index, len%3, can_open as usize)` — eight
    // slots total ([2 chars][3 mod-classes][2 can_open]).
    let mut openers_bottom: [[[Option<usize>; 2]; 3]; 2] = [[[None; 2]; 3]; 2];

    let mut pairs: Vec<EmphasisPair> = Vec::new();

    let mut closer_idx = first_active(&runs, 0);
    while let Some(c) = closer_idx {
        if !runs[c].can_close || runs[c].removed {
            closer_idx = next_active(&runs, c);
            continue;
        }

        let ch_idx = if runs[c].ch == b'*' { 0 } else { 1 };
        let closer_mod = (runs[c].count as usize) % 3;
        let closer_open_bucket = runs[c].can_open as usize;
        let bottom = openers_bottom[ch_idx][closer_mod][closer_open_bucket];

        let mut found_opener: Option<usize> = None;
        let mut walk = prev_active(&runs, c);
        while let Some(o) = walk {
            // openers_bottom is the exclusive lower bound: stop when we
            // reach it without checking. CMark sets this to the closer's
            // previous run when no match is found, marking that the search
            // span up to (and not including) that point has already been
            // exhausted for this bucket.
            if Some(o) == bottom {
                break;
            }
            let r_o = &runs[o];
            let r_c = &runs[c];
            if !r_o.removed && r_o.ch == r_c.ch && r_o.can_open {
                let opener_count = r_o.count as usize;
                let closer_count = r_c.count as usize;
                let oc_sum = opener_count + closer_count;
                let opener_both = r_o.can_open && r_o.can_close;
                let closer_both = r_c.can_open && r_c.can_close;
                let mod3_reject = (opener_both || closer_both)
                    && oc_sum.is_multiple_of(3)
                    && !(opener_count.is_multiple_of(3) && closer_count.is_multiple_of(3));
                if !mod3_reject {
                    found_opener = Some(o);
                    break;
                }
            }
            if o == 0 {
                break;
            }
            walk = prev_active(&runs, o);
        }

        if let Some(o) = found_opener {
            let consume = if runs[o].count >= 2 && runs[c].count >= 2 {
                2
            } else {
                1
            };
            let opener_count = runs[o].count as usize;
            let closer_start = runs[c].source_start;

            // Opener consumes its inner-edge (rightmost) chars.
            let open_pos = runs[o].source_start + opener_count - consume;
            // Closer consumes its inner-edge (leftmost) chars.
            let close_pos = closer_start;

            pairs.push(EmphasisPair {
                open_pos,
                open_len: consume as u8,
                close_pos,
                close_len: consume as u8,
                kind: if consume == 2 {
                    EmphasisKind::Strong
                } else {
                    EmphasisKind::Emph
                },
            });

            // Update the runs.
            runs[o].count -= consume as u16;
            runs[c].source_start += consume;
            runs[c].count -= consume as u16;

            // Remove all openers strictly between o and c by walking the
            // active list and marking them removed (CMark's "remove all
            // delimiters between c and o" step).
            let mut between = next_active(&runs, o);
            while let Some(idx) = between {
                if idx == c {
                    break;
                }
                runs[idx].removed = true;
                between = next_active(&runs, idx);
            }

            if runs[o].count == 0 {
                runs[o].removed = true;
            }
            if runs[c].count == 0 {
                runs[c].removed = true;
                closer_idx = next_active(&runs, c);
            }
            // Else: re-process the same closer with reduced count.
        } else {
            // No opener found. Set openers_bottom to the run we walked from
            // (the closer's previous active run): future closers in the same
            // bucket can stop their walk-back at this point. Note this is
            // the *prev* of the current closer, not the closer itself —
            // openers_bottom is consulted as an exclusive lower bound, so
            // pointing at closer.prev means "we've already searched
            // everything below this," matching CMark's semantics.
            openers_bottom[ch_idx][closer_mod][closer_open_bucket] = prev_active(&runs, c);
            if !runs[c].can_open {
                runs[c].removed = true;
            }
            closer_idx = next_active(&runs, c);
        }
    }

    // Build the disposition map. Walk pairs and assign Open/Close per byte;
    // any remaining unconsumed delim chars are Literal.
    let mut by_pos: BTreeMap<usize, DelimChar> = BTreeMap::new();
    for p in &pairs {
        by_pos.insert(
            p.open_pos,
            DelimChar::Open {
                len: p.open_len,
                partner: p.close_pos,
                partner_len: p.close_len,
                kind: p.kind,
            },
        );
        by_pos.insert(p.close_pos, DelimChar::Close);
    }

    // Mark all delim-run byte positions that aren't already mapped as Literal,
    // so the emission walk doesn't have to re-detect them.
    for r in &runs {
        for i in 0..r.count as usize {
            let pos = r.source_start + i;
            by_pos.entry(pos).or_insert(DelimChar::Literal);
        }
    }

    EmphasisPlan { by_pos }
}

#[derive(Debug, Clone, Copy)]
struct EmphasisPair {
    open_pos: usize,
    open_len: u8,
    close_pos: usize,
    close_len: u8,
    kind: EmphasisKind,
}

fn first_active(runs: &[DelimRun], from: usize) -> Option<usize> {
    (from..runs.len()).find(|&i| !runs[i].removed)
}

fn next_active(runs: &[DelimRun], from: usize) -> Option<usize> {
    (from + 1..runs.len()).find(|&i| !runs[i].removed)
}

fn prev_active(runs: &[DelimRun], from: usize) -> Option<usize> {
    (0..from).rev().find(|&i| !runs[i].removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::{Flavor, ParserOptions};

    fn cm_opts() -> ParserOptions {
        let flavor = Flavor::CommonMark;
        ParserOptions {
            flavor,
            dialect: Dialect::for_flavor(flavor),
            extensions: crate::options::Extensions::for_flavor(flavor),
            pandoc_compat: crate::options::PandocCompat::default(),
        }
    }

    fn plan(text: &str) -> EmphasisPlan {
        let opts = cm_opts();
        build_plan(text, 0, text.len(), &opts)
    }

    #[test]
    fn simple_emph_pair() {
        // *foo*  → Open at 0 partner 4, Close at 4
        let p = plan("*foo*");
        match p.lookup(0) {
            Some(DelimChar::Open {
                len,
                partner,
                partner_len,
                kind,
            }) => {
                assert_eq!(len, 1);
                assert_eq!(partner, 4);
                assert_eq!(partner_len, 1);
                assert_eq!(kind, EmphasisKind::Emph);
            }
            other => panic!("expected Open at 0, got {:?}", other),
        }
        assert!(matches!(p.lookup(4), Some(DelimChar::Close)));
    }

    #[test]
    fn simple_strong_pair() {
        let p = plan("**foo**");
        // Strong opener at pos 0 (len 2), closer at pos 5 (len 2)
        match p.lookup(0) {
            Some(DelimChar::Open {
                len,
                partner,
                partner_len,
                kind,
            }) => {
                assert_eq!(len, 2);
                assert_eq!(partner, 5);
                assert_eq!(partner_len, 2);
                assert_eq!(kind, EmphasisKind::Strong);
            }
            other => panic!("expected Strong Open at 0, got {:?}", other),
        }
    }

    #[test]
    fn rule_9_multiple_of_3_rejects() {
        // *foo**bar* → outer pair is single emph, ** in middle stays literal.
        let p = plan("*foo**bar*");
        match p.lookup(0) {
            Some(DelimChar::Open { len, partner, .. }) => {
                assert_eq!(len, 1);
                assert_eq!(partner, 9);
            }
            other => panic!("expected Open at 0, got {:?}", other),
        }
        // Middle ** chars must be literal
        assert!(matches!(p.lookup(4), Some(DelimChar::Literal)));
        assert!(matches!(p.lookup(5), Some(DelimChar::Literal)));
    }

    #[test]
    fn run_split_4_open_1_close() {
        // ****foo* → ***  + <em>foo</em>
        // Opener run of 4, closer run of 1. consume = min(4, 1) = 1.
        // Opener inner end is pos 3 (run starts at 0, inner-rightmost char).
        // Closer at pos 7 consumes 1.
        let p = plan("****foo*");
        // Pos 3 should be the Open
        match p.lookup(3) {
            Some(DelimChar::Open { len, partner, .. }) => {
                assert_eq!(len, 1);
                assert_eq!(partner, 7);
            }
            other => panic!("expected Open at 3, got {:?}", other),
        }
        // Pos 0, 1, 2 are unmatched literals
        assert!(matches!(p.lookup(0), Some(DelimChar::Literal)));
        assert!(matches!(p.lookup(1), Some(DelimChar::Literal)));
        assert!(matches!(p.lookup(2), Some(DelimChar::Literal)));
    }

    #[test]
    fn lazy_opener_preference() {
        // *foo *bar baz* → first * unmatched, second * pairs with last *
        let p = plan("*foo *bar baz*");
        // First * (pos 0) should be Literal
        assert!(matches!(p.lookup(0), Some(DelimChar::Literal)));
        // Second * (pos 5) should be Open
        match p.lookup(5) {
            Some(DelimChar::Open { partner, .. }) => {
                assert_eq!(partner, 13);
            }
            other => panic!("expected Open at 5, got {:?}", other),
        }
    }

    #[test]
    fn nested_strong_inside_emph_triple_run() {
        // ***foo*** → CommonMark prefers <em><strong>foo</strong></em>
        // pos 0: Open Emph (len 1, partner 8)
        // pos 1: Open Strong (len 2, partner 6)
        let p = plan("***foo***");
        match p.lookup(0) {
            Some(DelimChar::Open {
                len, partner, kind, ..
            }) => {
                assert_eq!(len, 1);
                assert_eq!(partner, 8);
                assert_eq!(kind, EmphasisKind::Emph);
            }
            other => panic!("expected Emph Open at 0, got {:?}", other),
        }
        match p.lookup(1) {
            Some(DelimChar::Open {
                len, partner, kind, ..
            }) => {
                assert_eq!(len, 2);
                assert_eq!(partner, 6);
                assert_eq!(kind, EmphasisKind::Strong);
            }
            other => panic!("expected Strong Open at 1, got {:?}", other),
        }
    }

    #[test]
    fn intraword_underscore_rejected() {
        // foo_bar_baz → underscores not at word boundary; no emphasis
        let p = plan("foo_bar_baz");
        assert!(matches!(p.lookup(3), Some(DelimChar::Literal)));
        assert!(matches!(p.lookup(7), Some(DelimChar::Literal)));
    }

    #[test]
    fn empty_input() {
        let p = plan("");
        assert!(p.is_empty());
    }

    #[test]
    fn no_delimiters() {
        let p = plan("just plain text");
        assert!(p.is_empty());
    }

    #[test]
    fn escape_blocks_delim() {
        // \*foo* → backslash-escape on first * means only second * is a delim
        // But there's no closer for the second *, so it's literal.
        let p = plan(r"\*foo*");
        // Pos 5 should be Literal (escaped pos 1 isn't recorded; pos 5 is unmatched)
        assert!(matches!(p.lookup(5), Some(DelimChar::Literal)));
        // Pos 1 should not appear (skipped as escape)
        assert!(p.lookup(1).is_none());
    }
}
