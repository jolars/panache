//! The incremental-reparse entry point and its edit currency.
//!
//! [`reparse`] is a *hint*, not a parser: it either produces a spliced tree
//! that the governing invariant guarantees is byte-identical to a full parse of
//! the edited text, or it declines with [`None`] and leaves the full parse to
//! the caller. Every guard in the cascade is a decline, never an error and
//! never a best effort -- "when in doubt, return `None`" is the whole
//! correctness argument, and it is what lets the caller (salsa's
//! `parsed_document` on the host side) treat the reparse as a pure
//! optimization.
//!
//! # The tier ladder
//!
//! Four strategies, cheapest first, first success wins ([`ReparseStrategy`]):
//!
//! | tier | re-parses | scales with |
//! | --- | --- | --- |
//! | `reparse_token` | nothing --- one green `TEXT` token is replaced | the token |
//! | `reparse_region` | a bounded run of top-level children, plus its two neighbours | the region |
//! | `reparse_section_window` | the previous top-level heading to EOF | the document tail |
//! | the suffix path of `reparse_ranges` | the enclosing block to EOF | the document tail |
//!
//! Each tier entry point is `#[inline(never)]` on purpose, so the cost of the
//! cheapest path cannot depend on which tiers sit below it: inlined, a tier's
//! locals land in the dispatcher's frame and every early return pays for it.
//!
//! # The guards
//!
//! Two decline for *cost* rather than soundness --- one per tier that has a cost
//! worth guarding, and both evaluated before any parse runs:
//!
//! * `MAX_WINDOW_SHARE_PERCENT` refuses a window covering nearly the whole
//!   document, because re-parsing 95% of a file and then splicing is a slower
//!   way to reach the tree a full parse produces.
//! * `REGION_MAX_FILE_DIVISOR` refuses a region (plus its neighbours) over a
//!   quarter of the document, because answering one costs roughly three
//!   region-sized parses.
//!
//! The rest decline for soundness, and they fall into three kinds. The
//! *structural* ones ask the old tree what it already proved --- a region is a
//! run of whole children, so nothing can be half-inside it. The *boundary
//! parses* re-parse a fragment against one neighbour and require the seam to
//! survive; that is what catches every construct which reaches forward
//! greedily. And a small number of *long-range* guards
//! (`long_range_pairing_lines`, `prefix_fence_state_is_stable`,
//! `edit_may_touch_refdefs`) exist because a few constructs pair at unbounded
//! distance and no bounded parse can see them. Each carries its counterexample
//! in its own doc comment, reproduced against the parser before it was written.
//!
//! # Edits
//!
//! [`Edit`] is the currency the caller speaks. Conversion from LSP `didChange`
//! content changes lives host-side; this crate only ever sees byte ranges.
//! [`diff_edit`] recovers a single contiguous edit from two whole texts, which
//! is how a caller with no edit information at all (a disk revert, a coalesced
//! `didChange` batch) still gets an incremental attempt.
//!
//! The oracle that enforces the invariant lives in
//! `crate::parser::verify`; `docs/development/lsp.qmd` covers how the host
//! stores and admits reparse bases.

use std::ops::Range;

use crate::options::{Dialect, ParserOptions};
use crate::parser::verify::assert_matches_full_parse;
use crate::parser::{Parser, SyntaxError, populate_refdef_labels};
use crate::range_utils::find_incremental_restart_offset;
use crate::syntax::{SyntaxKind, SyntaxNode};

/// A single contiguous text edit: replace `range` (a byte range in the *old*
/// text) with `insert`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub insert: String,
}

impl Edit {
    /// The signed length change this edit applies to text after `range`.
    pub fn delta(&self) -> isize {
        self.insert.len() as isize - (self.range.end - self.range.start) as isize
    }

    /// Apply the edit to `old`, producing the new text.
    pub fn apply(&self, old: &str) -> String {
        let mut out =
            String::with_capacity(old.len().saturating_sub(self.range.len()) + self.insert.len());
        out.push_str(&old[..self.range.start]);
        out.push_str(&self.insert);
        out.push_str(&old[self.range.end..]);
        out
    }

    fn new_range(&self) -> (usize, usize) {
        (self.range.start, self.range.start + self.insert.len())
    }
}

/// Recover a single contiguous [`Edit`] from a pair of whole texts by stripping
/// the common prefix and suffix. Multiple disjoint edits collapse into one
/// spanning edit -- still a correct transform, just coarser (and one the guard
/// cascade is more likely to decline). Boundaries are clamped to char
/// boundaries of both texts.
pub fn diff_edit(old: &str, new: &str) -> Edit {
    let ob = old.as_bytes();
    let nb = new.as_bytes();

    let mut prefix = 0;
    let max_prefix = ob.len().min(nb.len());
    while prefix < max_prefix && ob[prefix] == nb[prefix] {
        prefix += 1;
    }
    while prefix > 0 && !old.is_char_boundary(prefix) {
        prefix -= 1;
    }

    let mut suffix = 0;
    let max_suffix = (ob.len() - prefix).min(nb.len() - prefix);
    while suffix < max_suffix && ob[ob.len() - 1 - suffix] == nb[nb.len() - 1 - suffix] {
        suffix += 1;
    }
    while suffix > 0
        && (!old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix))
    {
        suffix -= 1;
    }

    Edit {
        range: prefix..(old.len() - suffix),
        insert: new[prefix..(new.len() - suffix)].to_string(),
    }
}

/// Which window a successful reparse re-derived.
///
/// [`ReparseStrategy::Token`] is the cheapest and re-derives no window at all,
/// replacing a single green `TEXT` token in place.
///
/// [`ReparseStrategy::Region`] parses a *bounded* fragment -- one run of
/// top-level `DOCUMENT` children -- and proves the seams with neighbour-sized
/// boundary parses. Its cost is a function of the region, not of where the
/// region sits in the document.
///
/// The two window strategies both parse their window to EOF, so their cost is a
/// function of window share and nothing else. They are the fallback for what the
/// region tier declines. The section window differs from the suffix window in
/// that it re-adopts the old suffix children when they come back structurally
/// equal, preserving their `Arc` identity; when they don't, it degrades to the
/// wholesale suffix splice and reports itself as
/// [`ReparseStrategy::SuffixWindow`].
///
/// **Both window tiers stay, and that is a measurement rather than caution.**
/// The roadmap expected the region tier to replace them; it does not. Over the
/// fuzzer's real-document corpus at 20x, of 1457 splices the region tier takes
/// 1120, the *section* window 225, the suffix window 79, and the token tier 33 —
/// so a seventh of real splices are still answered by the tier the roadmap
/// planned to delete, and it beats the suffix window there by three to one. On
/// the hazard snippets the suffix window is the workhorse instead (47 126
/// splices against the section window's 121), because those documents have no
/// headings to anchor a section.
///
/// The region tier does not subsume them because it declines for reasons a
/// window does not share: a region wider than a quarter of the document, and a
/// delimiter line whose pairing the edit moves. Where it declines, a window
/// still splices, and the section window is strictly better than the suffix one
/// when it fires — same parse, narrower splice, retained `Arc` identity.
/// `incremental_fuzz.rs` prints the histogram on every run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReparseStrategy {
    Token,
    Region,
    SectionWindow,
    SuffixWindow,
}

impl ReparseStrategy {
    /// The stable label used in logs and test assertions.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Token => "token",
            Self::Region => "region",
            Self::SectionWindow => "section_window",
            Self::SuffixWindow => "suffix_window",
        }
    }
}

/// A successful incremental reparse.
///
/// `errors` covers the **whole** document, not just the reparsed window: the
/// retained prefix's share is carried over from the caller-supplied previous
/// errors and the window's are shifted into document coordinates. The governing
/// invariant covers this vector as much as the tree.
pub struct Reparsed {
    pub green: rowan::GreenNode,
    pub errors: Vec<SyntaxError>,
    pub reparse_range: (usize, usize),
    pub strategy: ReparseStrategy,
}

/// Attempt an incremental reparse of `new_text` off the previous parse.
///
/// Returns [`None`] whenever any guard declines -- the caller must then full-
/// parse. A [`Some`] result is, by the governing invariant, byte-identical to
/// that full parse (tree and errors), which the debug oracle asserts on every
/// success.
///
/// `prev_errors` are the syntax errors of the parse that produced `prev_green`;
/// the retained prefix's share of them is carried into the result. Passing an
/// under-reported vector is a divergence the oracle will catch.
///
/// `options.refdef_labels` is scanned from `new_text` only when the caller has
/// not already supplied it; every caller holding a previous parse also holds a
/// cached refdef set, and passing it skips the scan.
pub fn reparse(
    prev_green: &rowan::GreenNode,
    prev_errors: &[SyntaxError],
    edit: &Edit,
    new_text: &str,
    options: &ParserOptions,
) -> Option<Reparsed> {
    reparse_with_cost_guards(
        prev_green,
        prev_errors,
        edit,
        new_text,
        options,
        CostGuards::Enforced,
    )
}

/// Whether the *cost* guards apply to a reparse attempt --- today just the
/// window-size cutoff ([`MAX_WINDOW_SHARE_PERCENT`]).
///
/// They are the only guards in the cascade that decline for cost rather than
/// for soundness, which makes them the ones a correctness harness wants out of
/// its way. `tests/incremental_fuzz.rs` works on hazard snippets tens of bytes
/// long, where almost every window covers most of the document: enforcing the
/// cutoff there declines two thirds of its edits before they reach a single
/// correctness guard, and the harness quietly stops testing the splice at all.
/// It therefore fuzzes the snippets with [`CostGuards::Ignored`] and its
/// real-document corpus with the production setting.
///
/// Production has exactly one reparse caller and it uses
/// [`CostGuards::Enforced`], which is what plain [`reparse`] passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostGuards {
    /// Decline a window wider than [`MAX_WINDOW_SHARE_PERCENT`].
    Enforced,
    /// Attempt the splice however wide the window is. Test-only.
    Ignored,
}

/// [`reparse`], with the cost guards under the caller's control.
///
/// See [`CostGuards`] for who wants the non-default and why.
pub fn reparse_with_cost_guards(
    prev_green: &rowan::GreenNode,
    prev_errors: &[SyntaxError],
    edit: &Edit,
    new_text: &str,
    options: &ParserOptions,
    cost_guards: CostGuards,
) -> Option<Reparsed> {
    let mut options = options.clone();
    populate_refdef_labels(new_text, &mut options);
    let prev_tree = SyntaxNode::new_root(prev_green.clone());

    if let Some(result) = reparse_token(&prev_tree, prev_errors, edit, new_text, &options) {
        assert_matches_full_parse(&result, new_text, &options);
        return Some(result);
    }

    let old_edit = (edit.range.start, edit.range.end);

    let new_edit = edit.new_range();
    let window_hopeless = window_is_too_wide(cost_guards, new_edit.0, new_text.len());
    let region_hopeless = region_is_too_wide(
        cost_guards,
        new_edit.1.saturating_sub(new_edit.0),
        new_text.len(),
    );
    if window_hopeless && region_hopeless {
        return None;
    }

    if edit_may_touch_refdefs(&prev_tree, old_edit, new_text, new_edit) {
        return None;
    }

    if let Some(result) = reparse_region(
        new_text,
        &options,
        &prev_tree,
        prev_errors,
        old_edit,
        new_edit,
        cost_guards,
    ) {
        return Some(result);
    }

    reparse_ranges(
        new_text,
        &options,
        &prev_tree,
        prev_errors,
        old_edit,
        new_edit,
        cost_guards,
    )
}

/// The token tier: an edit confined to the interior of one plain-prose `TEXT`
/// token replaces that token's green node and nothing else.
///
/// The other two strategies re-parse from a window start to EOF, so their cost
/// is a function of where the edit sits in the document. This one parses
/// nothing but the token, which is why it is the only thing that makes a
/// keystroke in a 300 KB document cost what a keystroke in a small one does.
///
/// Not quite O(token), and the difference is worth stating plainly:
/// [`rowan::cursor::SyntaxToken::replace_with`] rebuilds each ancestor with
/// `replace_child`, which materialises the whole child vector at every level.
/// At the root that is one `GreenNode` of the document's top-level arity. So
/// the cost is O(token) + O(top-level children) --- but that second term is
/// *already* paid by `splice_children` on the window paths, so this tier is
/// strictly cheaper than the one it displaces, and against a millisecond-scale
/// window parse it is noise.
///
/// The whole correctness argument is that nothing outside the token can
/// observe the change:
///
/// * the edit is strictly interior to the token's non-whitespace core, so the
///   bytes adjacent to *both* neighbouring tokens are untouched -- which is
///   what makes emphasis flanking, bare-URI left boundaries, and hard-break
///   trailing runs unable to move;
/// * no byte of the token, before or after, can open a construct
///   ([`token_tier_ban_mask`]), so no delimiter lives inside it that could pair
///   with one elsewhere in the paragraph;
/// * the token's text still lexes as exactly one `TEXT` token on its own
///   ([`relexes_as_one_text_token`]), which is what catches the constructs the
///   mask deliberately does not carry;
/// * the edit sits past its line's first word ([`edit_is_past_line_marker_zone`]),
///   so no block marker can be manufactured at the line start;
/// * and no syntax error touches the token, so the rest merely shift.
///
/// Every check is a decline. `None` costs the caller the window tiers it would
/// have tried anyway.
/// `#[inline(never)]`, like every tier entry point here, so that the cost of
/// the *cheapest* path cannot depend on which tiers sit below it. Inlined, a
/// tier's locals land in the dispatcher's stack frame, which every early return
/// pays for on the way out --- reordering the two window-class tiers alone cost
/// this one 20% (43x -> 35x on `pandoc_manual_midline_edit`) with nothing on
/// its own path changed.
#[inline(never)]
fn reparse_token(
    old_tree: &SyntaxNode,
    old_errors: &[SyntaxError],
    edit: &Edit,
    new_text: &str,
    options: &ParserOptions,
) -> Option<Reparsed> {
    if edit.insert.contains(['\n', '\r']) {
        return None;
    }

    let old_edit = normalize_range((edit.range.start, edit.range.end))?;
    if old_tree.kind() != SyntaxKind::DOCUMENT {
        return None;
    }
    if old_edit.1 > usize::from(old_tree.text_range().end()) {
        return None;
    }

    let token = covering_token(old_tree, old_edit)?;
    if token.kind() != SyntaxKind::TEXT {
        return None;
    }
    if token.parent().map(|parent| parent.kind()) != Some(SyntaxKind::PARAGRAPH) {
        return None;
    }
    if token
        .parent()
        .and_then(|parent| parent.parent())
        .map(|grandparent| grandparent.kind())
        != Some(SyntaxKind::DOCUMENT)
    {
        return None;
    }

    let range = token.text_range();
    let (t0, t1) = (usize::from(range.start()), usize::from(range.end()));
    let old_leaf = token.text();

    let leading = old_leaf.len() - old_leaf.trim_start_matches([' ', '\t']).len();
    let trailing = old_leaf.len() - old_leaf.trim_end_matches([' ', '\t']).len();
    if old_edit.0 <= t0 + leading || old_edit.1 >= t1.saturating_sub(trailing) {
        return None;
    }

    let (lo, hi) = (old_edit.0 - t0, old_edit.1 - t0);
    if !old_leaf.is_char_boundary(lo) || !old_leaf.is_char_boundary(hi) {
        return None;
    }
    let mut new_leaf = String::with_capacity(old_leaf.len() - (hi - lo) + edit.insert.len());
    new_leaf.push_str(&old_leaf[..lo]);
    new_leaf.push_str(&edit.insert);
    new_leaf.push_str(&old_leaf[hi..]);

    let allowed = token_tier_allowed_mask(options);
    let inert = |text: &str| text.bytes().all(|byte| allowed[byte as usize]);
    if !inert(old_leaf) || !inert(&new_leaf) {
        return None;
    }

    if !relexes_as_one_text_token(old_leaf, options)
        || !relexes_as_one_text_token(&new_leaf, options)
    {
        return None;
    }

    if !edit_is_past_line_marker_zone(new_text, old_edit.0) {
        return None;
    }

    if old_errors
        .iter()
        .any(|error| usize::from(error.range.start()) <= t1 && usize::from(error.range.end()) >= t0)
    {
        return None;
    }
    let delta = edit.delta();
    let errors = old_errors
        .iter()
        .map(|error| {
            if usize::from(error.range.start()) >= t1 {
                SyntaxError {
                    range: shift_range(error.range, delta),
                    ..error.clone()
                }
            } else {
                error.clone()
            }
        })
        .collect();

    let green = token.replace_with(rowan::GreenToken::new(SyntaxKind::TEXT.into(), &new_leaf));
    Some(Reparsed {
        green,
        errors,
        reparse_range: (t0, t0 + new_leaf.len()),
        strategy: ReparseStrategy::Token,
    })
}

fn covering_token(
    old_tree: &SyntaxNode,
    edit: (usize, usize),
) -> Option<crate::syntax::SyntaxToken> {
    let start = rowan::TextSize::new(edit.0 as u32);
    if edit.0 == edit.1 {
        match old_tree.token_at_offset(start) {
            rowan::TokenAtOffset::Single(token) => Some(token),
            _ => None,
        }
    } else {
        let range = rowan::TextRange::new(start, rowan::TextSize::new(edit.1 as u32));
        old_tree.covering_element(range).into_token()
    }
}

fn shift_range(range: rowan::TextRange, delta: isize) -> rowan::TextRange {
    let moved = |size: rowan::TextSize| {
        rowan::TextSize::new((usize::from(size) as isize + delta).max(0) as u32)
    };
    rowan::TextRange::new(moved(range.start()), moved(range.end()))
}

/// Bytes a token the tier splices is allowed to contain.
///
/// This is an **allowlist**, and the direction is the whole point. A ban list
/// derived from the grammar would extend itself for a new *inline* construct
/// (that is what [`structural_byte_mask_without_uri_schemes`] buys) but would
/// silently admit a new *block* construct, because the inline mask has no
/// reason to carry a line-leading byte. An allowlist fails closed in both
/// directions: a construct added tomorrow is excluded unless someone
/// deliberately adds its trigger byte here.
///
/// The seed is then narrowed by the inline mask, so the two mechanisms
/// compose --- a byte has to be both hand-vetted as inert *and* not structural
/// under the caller's extensions.
///
/// [`structural_byte_mask_without_uri_schemes`]: crate::parser::inlines::core::structural_byte_mask_without_uri_schemes
fn token_tier_allowed_mask(options: &ParserOptions) -> [bool; 256] {
    let structural =
        crate::parser::inlines::core::structural_byte_mask_without_uri_schemes(options);

    let mut allowed = [false; 256];
    for byte in TOKEN_TIER_ALPHABET_SEED {
        allowed[*byte as usize] = true;
    }
    allowed[0x80..=0xFF].fill(true);

    for byte in 0..=u8::MAX {
        if structural[byte as usize] {
            allowed[byte as usize] = false;
        }
    }
    allowed
}

const TOKEN_TIER_ALPHABET_SEED: &[u8] = b"abcdefghijklmnopqrstuvwxyz\
                                          ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                                          0123456789 \t.,;?'\"/&()";

fn relexes_as_one_text_token(text: &str, options: &ParserOptions) -> bool {
    use crate::parser::inlines::sink::InlineSink;

    #[derive(Default)]
    struct SingleTextProbe {
        tokens: usize,
        bytes: usize,
        plain: bool,
    }

    impl InlineSink for SingleTextProbe {
        fn token(&mut self, kind: rowan::SyntaxKind, text: &str) {
            self.tokens += 1;
            self.bytes += text.len();
            if kind != SyntaxKind::TEXT.into() {
                self.plain = false;
            }
        }

        fn start_node(&mut self, _kind: rowan::SyntaxKind) {
            self.plain = false;
        }

        fn finish_node(&mut self) {
            self.plain = false;
        }
    }

    if text.is_empty() {
        return false;
    }
    let mut probe = SingleTextProbe {
        plain: true,
        ..SingleTextProbe::default()
    };
    crate::parser::inlines::core::parse_inline_text_recursive(&mut probe, text, options, false);
    probe.plain && probe.tokens == 1 && probe.bytes == text.len()
}

/// Whether `offset` sits past the zone of its line where a block marker is
/// decided.
///
/// Every block construct is recognised from the head of its line: a marker
/// (`-`, `1.`, `>`, `:::`, a fence, an indent) runs from the first non-space
/// byte to the first space. So an edit that is already past a space on its
/// line cannot create one --- the bytes that decide the line all sit before the
/// edit and interiority leaves them alone.
///
/// This is what covers the marker shapes the ban mask deliberately allows:
/// `12 apples` plus an interior `.` is a list, and so is `12.apples` plus an
/// interior space, and both are refused here rather than by banning `.` from
/// prose.
fn edit_is_past_line_marker_zone(text: &str, offset: usize) -> bool {
    let line_start = text[..offset]
        .rfind(['\n', '\r'])
        .map_or(0, |index| index + 1);
    let head = &text[line_start..offset];
    let indent = head.len() - head.trim_start_matches(' ').len();
    // Four spaces is an indented code block; the guard has nothing to say
    // about a line whose head it cannot read as prose.
    indent <= 3 && head[indent..].contains(' ')
}

fn merge_incremental_errors(
    old_errors: &[SyntaxError],
    seam: usize,
    window_errors: Vec<SyntaxError>,
) -> Option<Vec<SyntaxError>> {
    let seam = rowan::TextSize::new(seam as u32);
    let mut merged = Vec::with_capacity(old_errors.len() + window_errors.len());

    for error in old_errors {
        if error.range.end() <= seam {
            merged.push(error.clone());
        } else if error.range.start() < seam {
            debug_assert!(
                false,
                "syntax error {:?} straddles the splice seam at {seam:?}",
                error.range
            );
            return None;
        }
    }

    for error in window_errors {
        merged.push(SyntaxError {
            range: rowan::TextRange::new(error.range.start() + seam, error.range.end() + seam),
            ..error
        });
    }

    debug_assert!(
        merged
            .windows(2)
            .all(|w| w[0].range.start() <= w[1].range.start()),
        "merged syntax errors are out of document order: {merged:?}"
    );
    Some(merged)
}

#[inline(never)]
fn reparse_ranges(
    input: &str,
    config: &ParserOptions,
    old_tree: &SyntaxNode,
    old_errors: &[SyntaxError],
    old_edit_range: (usize, usize),
    new_edit_range: (usize, usize),
    cost_guards: CostGuards,
) -> Option<Reparsed> {
    let input_len = input.len();

    let old_edit = normalize_range(old_edit_range)?;
    let new_edit = normalize_range(new_edit_range)?;
    if new_edit.1 > input_len {
        return None;
    }

    // Window-size cutoff, cheap half. Every window this function can choose
    // starts at or before the edit, so an edit that already leaves more than
    // the threshold share of the document downstream cannot produce a narrow
    // enough one -- and this is the only place the answer is known *before*
    // touching the old tree at all. A whole-document replacement lands here,
    // which is what keeps it at the price of the full parse the caller runs
    // next instead of that plus a walk of the old tree.
    if window_is_too_wide(cost_guards, new_edit.0, input_len) {
        return None;
    }

    if old_tree.kind() != SyntaxKind::DOCUMENT {
        return None;
    }

    // The edit is described in the *old* text's coordinates, so an edit range
    // past the old tree's end means the caller's tree and text have gone out
    // of sync. Bail rather than let the offset reach rowan, which panics on a
    // range it cannot resolve.
    if old_edit.1 > usize::from(old_tree.text_range().end()) {
        return None;
    }

    // The refdef guard that used to sit here is hoisted into
    // `reparse_with_cost_guards`, so it covers the region tier too and is paid
    // for once.

    // A section window is anchored at the previous top-level heading, which can
    // sit far earlier than the edit's own block -- so a too-wide section window
    // declines *this strategy* and falls through to the suffix window below,
    // which starts at the enclosing block and may well be narrow enough.
    if let Some(section_window) =
        find_top_level_heading_section_window(old_tree, old_edit, new_edit, input_len)
        && !window_is_too_wide(cost_guards, section_window.new_start, input_len)
        && let Some(result) =
            reparse_section_window(input, config, old_tree, old_errors, section_window)
    {
        assert_matches_full_parse(&result, input, config);
        return Some(result);
    }

    let restart = find_incremental_restart_offset(old_tree, old_edit.0, old_edit.1);
    let old_restart = align_to_document_child_start(old_tree, restart);

    // The retained prefix is `old_tree`'s bytes up to the restart, so the
    // restart must not lie past the edit start: a later restart would keep
    // stale pre-edit bytes and drop the edit from the spliced tree. (An
    // edit at a blank line between blocks can produce such a restart — the
    // enclosing-block lookup resolves to the *following* block.)
    if old_restart > old_edit.0 {
        return None;
    }

    let new_restart = map_old_offset_to_new(old_restart, old_edit, new_edit, input_len);
    if !input.is_char_boundary(new_restart) {
        return None;
    }

    // Window-size cutoff, precise half. The restart can land arbitrarily far
    // before the edit -- one top-level block spanning most of the document is
    // enough -- so the edit-start check above does not subsume this one.
    // Ahead of the seam guards and the window parse, so a decline here costs
    // the cascade it already walked and nothing more.
    if window_is_too_wide(cost_guards, new_restart, input_len) {
        return None;
    }

    // Seam decoupling: the suffix is parsed as a standalone document, so
    // nothing may couple across the splice seam in the edited text.
    // Backward: the retained prefix must end at a blank line (or the seam
    // is the document start), else the suffix's first line could continue
    // a prefix paragraph, turn it into a setext heading, or attach lazily
    // to a prefix container. Forward-into-prefix: an indented suffix start
    // can be absorbed by a trailing prefix list, footnote definition, or
    // indented code block even across blank lines, so it also bails.
    if !seam_is_decoupled(input, new_restart)
        || !prefix_ends_structurally_decoupled(old_tree, old_restart)
    {
        return None;
    }

    // Fence pairing is global: an edit in the suffix can pair with (or
    // orphan) a fence-capable line retained in the prefix, flipping the
    // prefix's interpretation.
    if !prefix_fence_state_is_stable(&input[..new_restart]) {
        return None;
    }

    let suffix_text = &input[new_restart..];

    // The window is parsed standalone, so its first line looks like the
    // document's first line to the block dispatcher.
    if new_restart > 0 && window_start_manufactures_document_start_construct(suffix_text, config) {
        return None;
    }

    // A list (or definition-list) block continues across blank lines when
    // the next non-blank line carries a compatible marker, so a suffix
    // whose first line looks like a list continuation must not be parsed
    // standalone after a retained trailing list.
    if last_retained_block_can_absorb_marker(old_tree, old_restart)
        && first_nonblank_line_is_container_marker(suffix_text)
    {
        return None;
    }

    // Two backward couplings the seam's blank line does not break, because
    // pandoc permits a blank line in both: a `:`/`~` line after a paragraph
    // promotes it to a definition-list `TERM`, and the same line after a table
    // becomes that table's caption. Parsed standalone the suffix is only a
    // paragraph starting with a colon, so the splice keeps two blocks where a
    // full parse has one.
    //
    // A retained thematic break is the mirror image: `---` is also a
    // multiline-table border, so suffix content can turn the retained rule into
    // the top rule of a table that swallows the seam. `prefix_fence_state_is_stable`
    // cannot carry this one — dash runs are far too common for a parity count.
    if last_retained_block_kind(old_tree, old_restart)
        .is_some_and(last_retained_block_absorbs_colon_line)
        && first_nonblank_line_is_definition_marker(suffix_text)
    {
        return None;
    }

    // The mirror image: a bare dash run is a multiline-table rule as well as a
    // thematic break, and table rules pair across blank lines like a fence. A
    // dash run retained as a `HORIZONTAL_RULE` can therefore be re-read as the
    // *top* rule of a table that swallows the seam once the window supplies a
    // partner. `prefix_fence_state_is_stable` cannot carry this one: dash runs
    // are far too common for a parity count, so the guard reads the old tree
    // for a rule that actually parsed as one, and only bails when the window
    // has a partner to offer.
    if retained_prefix_has_thematic_break(old_tree, old_restart) && has_dash_rule_line(suffix_text)
    {
        return None;
    }

    // A retained `DEFINITION_LIST` absorbs a definition list the window starts:
    // pandoc pairs a term with any number of definitions and consecutive items
    // join into one list node, so a full parse has one `DEFINITION_LIST` where
    // the standalone splice has two adjacent ones.
    if last_retained_block_kind(old_tree, old_restart) == Some(SyntaxKind::DEFINITION_LIST)
        && has_definition_marker_line(suffix_text)
    {
        return None;
    }

    let (suffix_tree, suffix_errors) = Parser::new(suffix_text, config).parse_with_errors();
    let errors = merge_incremental_errors(old_errors, new_restart, suffix_errors)?;

    // Splice on the green tree directly: retain the prefix children verbatim
    // (rowan's structural sharing keeps their `Arc` identity) and replace
    // everything from the restart boundary onward with the reparsed suffix.
    let old_green = old_tree.green();
    let split = first_child_ending_after(old_green, old_restart);
    let suffix_green = suffix_tree.green();
    let new_green = old_green.splice_children(
        split..,
        suffix_green.children().map(|child| child.to_owned()),
    );

    let len: usize = new_green.text_len().into();
    let result = Reparsed {
        green: new_green,
        errors,
        reparse_range: (new_restart, len),
        strategy: ReparseStrategy::SuffixWindow,
    };
    assert_matches_full_parse(&result, input, config);
    Some(result)
}

fn normalize_range(range: (usize, usize)) -> Option<(usize, usize)> {
    (range.0 <= range.1).then_some(range)
}

/// The largest share of the document, as a percentage, that a window may leave
/// downstream and still be worth splicing.
///
/// Both strategies parse their window to EOF, so this share *is* the parse work
/// a splice does relative to a full parse, and the splice pays a 5-10%
/// surcharge on top of it: walking the old tree, the guard cascade, and
/// rebuilding the green root. Past roughly this point the surcharge stops being
/// repaid and the incremental path is a slower way to reach the same tree.
/// `benches/lsp_incremental.rs` prices the crossover -- 0.9x at an 87.5%
/// window, 0.2x at 100% on a whole-document replace, against 2.7x-5.6x where
/// the window is a fifth of the document or less.
///
/// Declining here is the ordinary refusal-first contract: the caller full-
/// parses, which is exactly what it would have done before the feature existed.
/// The region tier has its own bail (`REGION_MAX_FILE_DIVISOR`) rather than
/// sharing this one, because the two measure different work: this is the share
/// of the document left downstream of a window, that is the fraction of it a
/// region plus its neighbours covers.
const MAX_WINDOW_SHARE_PERCENT: usize = 85;

/// Whether a window starting at `window_start` leaves more than
/// [`MAX_WINDOW_SHARE_PERCENT`] of the document downstream of it.
///
/// Widened to `u64` before multiplying: `usize` is 32 bits on the wasm target,
/// where `input_len * 100` would otherwise overflow on a large document.
fn window_is_too_wide(cost_guards: CostGuards, window_start: usize, input_len: usize) -> bool {
    if cost_guards == CostGuards::Ignored {
        return false;
    }
    let window = input_len.saturating_sub(window_start) as u64;
    window * 100 > input_len as u64 * MAX_WINDOW_SHARE_PERCENT as u64
}

const REFDEF_SCAN_WINDOW: usize = 512;

/// Whether the edit could add, remove, or alter a reference or footnote
/// definition, judged by cheap textual evidence: a `]:` occurrence within a
/// bounded window around the edit, in the old text or the new. False
/// positives (a literal `]:` in prose) only cost a full reparse.
///
/// Only the *new* text is windowed. The old text is read across the edited
/// span alone, and that is an equivalence rather than a weakening: outside the
/// edit the two texts are byte-identical, so the old window's two flanks are
/// the very bytes the new window already covers. What the old text can still
/// hide is the *deleted* span, which has no image in the new text at all --- so
/// that is what gets read, plus one byte on each side, which is enough for a
/// two-byte `]:` straddling either boundary.
///
/// The union of bytes examined is therefore exactly what it was, and the cost
/// stops depending on the size of the document: reading a 1 KB window out of
/// the tree meant walking every token in it, because a window that wide spans
/// several top-level children and `covering_element` resolves to the root.
/// Measured on the 300 KB pandoc manual, that was ~800 us on the path of every
/// reparse attempt, accepted or declined.
fn edit_may_touch_refdefs(
    old_tree: &SyntaxNode,
    old_edit: (usize, usize),
    input: &str,
    new_edit: (usize, usize),
) -> bool {
    let new_start = floor_char_boundary(input, new_edit.0.saturating_sub(REFDEF_SCAN_WINDOW));
    let new_end = floor_char_boundary(
        input,
        new_edit
            .1
            .saturating_add(REFDEF_SCAN_WINDOW)
            .min(input.len()),
    );
    if new_start < new_end && input[new_start..new_end].contains("]:") {
        return true;
    }

    // Snap to token boundaries before slicing: a range edge landing inside a
    // multi-byte token is not a char boundary, and slicing panics on one.
    // Snapping outward only ever widens the read, which is the safe direction
    // for a conservative guard.
    let old_len: usize = old_tree.text_range().end().into();
    let old_start = snap_out_to_token_boundary(old_tree, old_edit.0.saturating_sub(1), false);
    let old_end =
        snap_out_to_token_boundary(old_tree, old_edit.1.saturating_add(1).min(old_len), true);
    old_start < old_end
        && covering_text(
            old_tree,
            rowan::TextRange::new((old_start as u32).into(), (old_end as u32).into()),
        )
        .contains("]:")
}

fn covering_text(tree: &SyntaxNode, range: rowan::TextRange) -> String {
    match tree.covering_element(range) {
        rowan::NodeOrToken::Node(node) => {
            let base = node.text_range().start();
            node.text()
                .slice(rowan::TextRange::new(
                    range.start() - base,
                    range.end() - base,
                ))
                .to_string()
        }
        rowan::NodeOrToken::Token(token) => {
            let base = token.text_range().start();
            let start = usize::from(range.start() - base);
            let end = usize::from(range.end() - base);
            token.text()[start..end].to_owned()
        }
    }
}

fn snap_out_to_token_boundary(tree: &SyntaxNode, offset: usize, upward: bool) -> usize {
    let len: usize = tree.text_range().end().into();
    let clamped = offset.min(len);
    // A one-byte probe through `covering_element`, rather than
    // `token_at_offset`. The two agree at every offset --- pinned by
    // `snapping_agrees_with_a_linear_token_scan` --- and differ only in cost:
    // rowan's `token_at_offset` filters `children_with_tokens()` linearly at
    // each level (its own source carries a TODO saying so), while
    // `child_or_token_at_range` binary-searches the green children. On a
    // document with a few thousand top-level children that is the difference
    // between `O(children)` and `O(log children)`, and this runs twice on the
    // path of every reparse attempt.
    //
    // The probe is taken on the side we are snapping *away* from, so that an
    // offset already sitting on a boundary snaps outward rather than staying
    // put: upward looks at the byte after it, downward at the byte before.
    if upward {
        if clamped >= len {
            return len;
        }
        let probe = rowan::TextRange::at((clamped as u32).into(), 1.into());
        usize::from(tree.covering_element(probe).text_range().end()).min(len)
    } else {
        if clamped == 0 {
            return 0;
        }
        let probe = rowan::TextRange::at(((clamped - 1) as u32).into(), 1.into());
        usize::from(tree.covering_element(probe).text_range().start())
    }
}

fn floor_char_boundary(text: &str, mut pos: usize) -> usize {
    while pos > 0 && !text.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

/// Whether the fence-pairing state of a retained prefix is visibly stable.
///
/// Fence-like delimiters (backtick/tilde code fences, `:::` div fences,
/// `$$` display math, HTML comments) pair across blank lines, so their
/// interpretation is global: an edit after the prefix can supply or remove
/// a pairing partner and flip how prefix lines parse. This is a parity
/// heuristic — every delimiter-capable line in the prefix must have a
/// partner — not a proof; delimiter-looking lines in prose (e.g. docs
/// about Markdown) can fool it in both directions. The debug oracle
/// backstops false negatives; false positives only cost a full reparse.
/// The precise check --- asking the old tree whether each candidate line actually
/// parsed as a closed fence delimiter --- is still unwritten; `TODO.md` tracks it
/// under "Incremental Parsing -> Tier coverage", together with the same
/// coarseness in `long_range_pairing_lines`.
fn prefix_fence_state_is_stable(prefix: &str) -> bool {
    let mut backtick = 0usize;
    let mut tilde = 0usize;
    let mut colon = 0usize;
    let mut math = 0usize;
    for line in prefix.lines() {
        let trimmed = line.trim_start_matches(' ');
        if line.len() - trimmed.len() > 3 {
            continue;
        }
        if trimmed.starts_with("```") {
            backtick += 1;
        } else if trimmed.starts_with("~~~") {
            tilde += 1;
        } else if trimmed.starts_with(":::") {
            colon += 1;
        } else if trimmed.starts_with("$$") {
            math += 1;
        }
    }
    backtick.is_multiple_of(2)
        && tilde.is_multiple_of(2)
        && colon.is_multiple_of(2)
        && math.is_multiple_of(2)
        && prefix.matches("<!--").count() == prefix.matches("-->").count()
}

/// Whether the retained prefix is *structurally* decoupled at `boundary`:
/// the last retained top-level child must be a `BLANK_LINE` node (or
/// nothing is retained). The textual blank-line check in
/// [`seam_is_decoupled`] is blind to containment — an unterminated
/// container (open `:::` div, fence, comment) can hold the seam's blank
/// line *inside* itself and still absorb anything appended after it, which
/// this check catches because the blank line is then not a top-level
/// child.
fn prefix_ends_structurally_decoupled(old_tree: &SyntaxNode, boundary: usize) -> bool {
    old_tree
        .children_with_tokens()
        .take_while(|child| usize::from(child.text_range().end()) <= boundary)
        .last()
        .is_none_or(|child| child.kind() == SyntaxKind::BLANK_LINE)
}

fn last_retained_block_can_absorb_marker(old_tree: &SyntaxNode, boundary: usize) -> bool {
    old_tree
        .children()
        .take_while(|child| usize::from(child.text_range().end()) <= boundary)
        .filter(|child| child.kind() != SyntaxKind::BLANK_LINE)
        .last()
        .is_some_and(|child| {
            matches!(
                child.kind(),
                SyntaxKind::LIST | SyntaxKind::DEFINITION_LIST | SyntaxKind::BLOCK_QUOTE
            )
        })
}

fn last_retained_block_kind(old_tree: &SyntaxNode, boundary: usize) -> Option<SyntaxKind> {
    old_tree
        .children()
        .take_while(|child| usize::from(child.text_range().end()) <= boundary)
        .filter(|child| child.kind() != SyntaxKind::BLANK_LINE)
        .last()
        .map(|child| child.kind())
}

fn last_retained_block_absorbs_colon_line(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PARAGRAPH
            | SyntaxKind::SIMPLE_TABLE
            | SyntaxKind::MULTILINE_TABLE
            | SyntaxKind::PIPE_TABLE
            | SyntaxKind::GRID_TABLE
    )
}

fn retained_prefix_has_thematic_break(old_tree: &SyntaxNode, boundary: usize) -> bool {
    old_tree
        .children()
        .take_while(|child| usize::from(child.text_range().end()) <= boundary)
        .any(|child| child.kind() == SyntaxKind::HORIZONTAL_RULE)
}

fn has_dash_rule_line(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.len() >= 3 && trimmed.bytes().all(|b| b == b'-')
    })
}

/// Whether the first non-blank line of `text` is a definition-list marker
/// line (`:` or `~` followed by space or end of line).
///
/// Distinct from [`first_nonblank_line_is_container_marker`] in what it
/// couples to: a container marker continues an open *container*, while a
/// definition marker reaches back and rewrites the preceding *paragraph*.
/// `:::` and `~~~` are fence openers, not markers, so the rest of the run must
/// not be another delimiter character.
fn first_nonblank_line_is_definition_marker(text: &str) -> bool {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .is_some_and(is_definition_marker_line)
}

fn is_definition_marker_line(line: &str) -> bool {
    line.trim_start()
        .strip_prefix([':', '~'])
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t'))
}

fn has_definition_marker_line(text: &str) -> bool {
    text.lines().any(is_definition_marker_line)
}

/// Whether the first non-blank line of `text` starts with a marker that
/// could continue a preceding container: bullet or ordered list markers
/// (including pandoc's fancy single-letter forms), a definition-list `:`,
/// or a blockquote `>`.
fn first_nonblank_line_is_container_marker(text: &str) -> bool {
    let Some(line) = text.lines().find(|line| !line.trim().is_empty()) else {
        return false;
    };
    let trimmed = line.trim_start();
    // A blockquote marker needs no following space.
    if trimmed.starts_with('>') {
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix(['-', '*', '+', ':']) {
        return rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t');
    }
    if trimmed.starts_with('(') {
        return true;
    }
    let marker_len = trimmed
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .count();
    if marker_len > 0 && marker_len <= 9 {
        let rest = &trimmed[marker_len..];
        if let Some(rest) = rest.strip_prefix(['.', ')']) {
            return rest.is_empty() || rest.starts_with(' ');
        }
    }
    false
}

/// Whether the first line of a window would be read as a construct only a
/// document's *first* line can produce.
///
/// A window is parsed as a standalone document, so `at_document_start` is true
/// at its first line. Three constructs test that flag with no
/// `|| ctx.has_blank_before` escape, and each would therefore be manufactured
/// where a full parse refuses:
///
/// - a pandoc `%` title block (`PandocTitleBlockParser::detect_prepared`),
/// - a MultiMarkdown title block (`MmdTitleBlockParser::detect_prepared`),
/// - under the CommonMark dialect, a YAML metadata block: those readers
///   recognize YAML only as frontmatter, and read a body `---` as a thematic
///   break. This one also manufactures *syntax errors*, malformed YAML being
///   the only source of them.
///
/// Every other `at_document_start` consumer is `||`-ed with `has_blank_before`,
/// which the seam guard already guarantees, so they agree either way. Textual
/// and deliberately over-eager: a false positive costs one full parse. The
/// The principled fix --- separating "byte 0 of the document" from "blank-line
/// separated fragment start" --- landed as `ParseOrigin`, which is what lets the
/// region tier answer these shapes instead of refusing them. This guard remains
/// because the *window* tiers have no fragment context to parse with.
fn window_start_manufactures_document_start_construct(
    window: &str,
    config: &ParserOptions,
) -> bool {
    let Some(first) = window.lines().next() else {
        return false;
    };
    if config.extensions.pandoc_title_block && first.trim_start().starts_with('%') {
        return true;
    }
    if config.extensions.mmd_title_block && !first.trim().is_empty() && first.contains(':') {
        return true;
    }
    config.dialect == Dialect::CommonMark && first.trim() == "---"
}

fn strip_line_ending(text: &str) -> Option<&str> {
    text.strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
}

fn ends_with_blank_line(prefix: &str) -> bool {
    strip_line_ending(prefix).is_some_and(|rest| strip_line_ending(rest).is_some())
}

fn seam_is_decoupled(text: &str, seam: usize) -> bool {
    if seam != 0 && !ends_with_blank_line(&text[..seam]) {
        return false;
    }
    let first_nonblank_indented = text[seam..]
        .lines()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| line.starts_with(' ') || line.starts_with('\t'));
    !first_nonblank_indented
}

fn align_to_document_child_start(tree: &SyntaxNode, offset: usize) -> usize {
    for child in tree.children_with_tokens() {
        let range = child.text_range();
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        if offset <= start {
            return start;
        }
        if offset < end {
            return start;
        }
    }
    let len: usize = tree.text_range().end().into();
    len
}

fn map_old_offset_to_new(
    old_offset: usize,
    old_edit: (usize, usize),
    new_edit: (usize, usize),
    new_len: usize,
) -> usize {
    if old_offset <= old_edit.0 {
        return old_offset;
    }
    if old_offset >= old_edit.1 {
        let old_span = old_edit.1 - old_edit.0;
        let new_span = new_edit.1 - new_edit.0;
        let delta = new_span as isize - old_span as isize;
        return old_offset.saturating_add_signed(delta).min(new_len);
    }
    new_edit.1.min(new_len)
}

fn first_child_ending_after(green: &rowan::GreenNodeData, pos: usize) -> usize {
    let mut offset = 0usize;
    for (index, child) in green.children().enumerate() {
        let len: usize = child.text_len().into();
        offset += len;
        if offset > pos {
            return index;
        }
    }
    green.children().count()
}

fn first_child_starting_at_or_after(green: &rowan::GreenNodeData, pos: usize) -> usize {
    let mut offset = 0usize;
    for (index, child) in green.children().enumerate() {
        if offset >= pos {
            return index;
        }
        let len: usize = child.text_len().into();
        offset += len;
    }
    green.children().count()
}

#[derive(Debug, Clone, Copy)]
struct SectionWindow {
    old_start: usize,
    old_end: usize,
    new_start: usize,
    new_end: usize,
}

fn find_top_level_heading_section_window(
    old_tree: &SyntaxNode,
    old_edit: (usize, usize),
    new_edit: (usize, usize),
    new_len: usize,
) -> Option<SectionWindow> {
    let old_len: usize = old_tree.text_range().end().into();
    let mut previous_heading: Option<(usize, usize)> = None;
    let mut next_heading: Option<(usize, usize)> = None;

    for child in old_tree.children() {
        if child.kind() != SyntaxKind::HEADING {
            continue;
        }

        let range = child.text_range();
        let start: usize = range.start().into();
        let end: usize = range.end().into();

        if start <= old_edit.0 {
            previous_heading = Some((start, end));
        } else {
            next_heading = Some((start, end));
            break;
        }
    }

    let (previous_start, previous_end) = previous_heading?;
    let (next_start, next_end) = next_heading.unwrap_or((old_len, old_len));

    if ranges_intersect(old_edit, (previous_start, previous_end))
        || ranges_intersect(old_edit, (next_start, next_end))
    {
        return None;
    }

    if old_edit.0 <= previous_end || old_edit.1 >= next_start {
        return None;
    }

    let new_start = map_old_offset_to_new(previous_start, old_edit, new_edit, new_len);
    let new_end = map_old_offset_to_new(next_start, old_edit, new_edit, new_len);
    if new_start >= new_end || new_end > new_len {
        return None;
    }

    Some(SectionWindow {
        old_start: previous_start,
        old_end: next_start,
        new_start,
        new_end,
    })
}

fn ranges_intersect(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

fn reparse_section_window(
    input: &str,
    config: &ParserOptions,
    old_tree: &SyntaxNode,
    old_errors: &[SyntaxError],
    section_window: SectionWindow,
) -> Option<Reparsed> {
    if !input.is_char_boundary(section_window.new_start)
        || !input.is_char_boundary(section_window.new_end)
    {
        return None;
    }

    if !seam_is_decoupled(input, section_window.new_start)
        || !prefix_ends_structurally_decoupled(old_tree, section_window.old_start)
        || !prefix_fence_state_is_stable(&input[..section_window.new_start])
    {
        return None;
    }

    let window_text = &input[section_window.new_start..];

    if section_window.new_start > 0
        && window_start_manufactures_document_start_construct(window_text, config)
    {
        return None;
    }

    if last_retained_block_can_absorb_marker(old_tree, section_window.old_start)
        && first_nonblank_line_is_container_marker(window_text)
    {
        return None;
    }
    if last_retained_block_kind(old_tree, section_window.old_start)
        .is_some_and(last_retained_block_absorbs_colon_line)
        && first_nonblank_line_is_definition_marker(window_text)
    {
        return None;
    }
    if retained_prefix_has_thematic_break(old_tree, section_window.old_start)
        && has_dash_rule_line(window_text)
    {
        return None;
    }
    if last_retained_block_kind(old_tree, section_window.old_start)
        == Some(SyntaxKind::DEFINITION_LIST)
        && has_definition_marker_line(window_text)
    {
        return None;
    }

    let (tail_tree, tail_errors) =
        Parser::new(&input[section_window.new_start..], config).parse_with_errors();
    let errors = merge_incremental_errors(old_errors, section_window.new_start, tail_errors)?;
    let tail_green = tail_tree.green();
    let window_len = section_window.new_end - section_window.new_start;

    let old_green = old_tree.green();
    let start_idx = first_child_ending_after(old_green, section_window.old_start);
    let end_idx = first_child_starting_at_or_after(old_green, section_window.old_end);

    let mut offset = 0usize;
    let mut boundary_idx = None;
    for (index, child) in tail_green.children().enumerate() {
        if offset == window_len {
            boundary_idx = Some(index);
            break;
        }
        if offset > window_len {
            break;
        }
        offset += usize::from(child.text_len());
    }
    if boundary_idx.is_none() && offset == window_len {
        boundary_idx = Some(tail_green.children().count());
    }

    if let Some(boundary_idx) = boundary_idx {
        let tail_matches_old_suffix = tail_green.children().count() - boundary_idx
            == old_green.children().count() - end_idx
            && tail_green
                .children()
                .skip(boundary_idx)
                .zip(old_green.children().skip(end_idx))
                .all(|(new_child, old_child)| new_child == old_child);
        if tail_matches_old_suffix {
            let new_green = old_green.splice_children(
                start_idx..end_idx,
                tail_green
                    .children()
                    .take(boundary_idx)
                    .map(|child| child.to_owned()),
            );
            return Some(Reparsed {
                green: new_green,
                errors,
                reparse_range: (section_window.new_start, section_window.new_end),
                strategy: ReparseStrategy::SectionWindow,
            });
        }
    }

    let new_green = old_green.splice_children(
        start_idx..,
        tail_green.children().map(|child| child.to_owned()),
    );
    let len: usize = new_green.text_len().into();
    Some(Reparsed {
        green: new_green,
        errors,
        reparse_range: (section_window.new_start, len),
        strategy: ReparseStrategy::SuffixWindow,
    })
}

type GreenChild = rowan::NodeOrToken<rowan::GreenNode, rowan::GreenToken>;

/// A region wider than `1 / REGION_MAX_FILE_DIVISOR` of the document costs more
/// to answer than the full parse it is avoiding. A divisor rather than a
/// fraction, so `4` here means a quarter.
///
/// The quarter is the tier's own arithmetic rather than a tuned number: it
/// answers a region with a fragment parse *plus* up to two boundary parses that
/// each carry a neighbour, so roughly three region-sized parses. Three quarters
/// of a full parse is where that stops being worth doing.
///
/// Unlike fatou, Panache has no always-try floor. Sharing the block parser
/// registry removes one fixed cost from each boundary parse, but measurement
/// still puts the 74-byte `multi_change_utf16_4` region splice below a full
/// parse. Charging the fraction on every document keeps that case from losing.
const REGION_MAX_FILE_DIVISOR: usize = 4;

/// Whether a region is too wide to be worth attempting.
///
/// Measured over the region **plus its two neighbours**, not the region alone,
/// which is where this departs from fatou's version. The tier answers a region
/// with a fragment parse of it plus up to two boundary parses that each carry a
/// whole neighbour, and a panache neighbour can be a 19 KB grid table where a
/// fatou one is a single statement. Charging only the region would let a narrow
/// region between two enormous blocks cost more than the full parse.
///
/// Evaluated from `TextRange`s before any parse runs, so a decline costs the
/// tree walk that found the region and nothing more.
///
/// A performance guard only: like every other bail here it returns the caller to
/// a full parse, so it cannot affect what a reparse yields. [`CostGuards`] turns
/// it off for the fuzz harness, whose snippets are tens of bytes long and would
/// otherwise never reach this tier at all.
fn region_is_too_wide(cost_guards: CostGuards, parsed_len: usize, text_len: usize) -> bool {
    if cost_guards == CostGuards::Ignored {
        return false;
    }
    parsed_len > text_len / REGION_MAX_FILE_DIVISOR
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Region {
    start: usize,
    end: usize,
    prev: Option<(usize, usize)>,
    next: Option<(usize, usize)>,
}

fn select_region(old_tree: &SyntaxNode, old_edit: (usize, usize)) -> Option<Region> {
    let blank: rowan::SyntaxKind = SyntaxKind::BLANK_LINE.into();
    let mut offset = 0usize;
    let spans: Vec<(bool, usize, usize)> = old_tree
        .green()
        .children()
        .map(|child| {
            let start = offset;
            offset += usize::from(child.text_len());
            (child.kind() == blank, start, offset)
        })
        .collect();
    if spans.is_empty() {
        return None;
    }

    let touches =
        |&(_, start, end): &(bool, usize, usize)| start <= old_edit.1 && end >= old_edit.0;
    let mut first = spans.iter().position(touches)?;
    let mut last = spans.len() - 1 - spans.iter().rev().position(touches)?;

    while first > 0 && !spans[first - 1].0 {
        first -= 1;
    }
    while last + 1 < spans.len() && !spans[last + 1].0 {
        last += 1;
    }

    if first == 0 && last == spans.len() - 1 {
        return None;
    }

    if spans[first..=last].iter().all(|&(is_blank, _, _)| is_blank) {
        return None;
    }

    let span = |&(_, start, end): &(bool, usize, usize)| (start, end);
    Some(Region {
        start: spans[first].1,
        end: spans[last].2,
        prev: spans[..first].iter().rfind(|s| !s.0).map(span),
        next: spans[last + 1..].iter().find(|s| !s.0).map(span),
    })
}

fn no_straddle(tree: &SyntaxNode, seam: usize) -> bool {
    let seam = rowan::TextSize::new(seam as u32);
    tree.children_with_tokens()
        .all(|child| !(child.text_range().start() < seam && seam < child.text_range().end()))
}

fn split_children_at(tree: &SyntaxNode, seam: usize) -> Option<(Vec<GreenChild>, Vec<GreenChild>)> {
    if !no_straddle(tree, seam) {
        return None;
    }
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut offset = 0usize;
    for child in tree.green().children() {
        let len = usize::from(child.text_len());
        if offset + len <= seam {
            before.push(child.to_owned());
        } else {
            after.push(child.to_owned());
        }
        offset += len;
    }
    Some((before, after))
}

fn green_children(tree: &SyntaxNode) -> Vec<GreenChild> {
    tree.green()
        .children()
        .map(|child| child.to_owned())
        .collect()
}

fn parse_fragment_at(
    text: &str,
    offset: usize,
    config: &ParserOptions,
) -> (SyntaxNode, Vec<SyntaxError>) {
    if offset == 0 {
        Parser::new(text, config).parse_with_errors()
    } else {
        Parser::new_fragment(text, config).parse_with_errors()
    }
}

/// Lines whose reading can pair with a partner *arbitrarily far away* in the
/// document, in document order and verbatim.
///
/// This is the tier's answer to the one hazard class no bounded boundary parse
/// can catch. Absorptive constructs -- an unclosed `:::`, a matched-pair HTML
/// block -- swallow forward greedily, so whatever the fragment leaves open eats
/// its immediate next sibling and the forward boundary parse sees it. But two
/// families only *form* when a partner is found at unbounded distance, and
/// without the partner they degrade to something perfectly innocent-looking:
///
/// * Fence pairing. Under the pandoc dialect an unclosed ` ``` ` is literal
///   paragraph text, not a code block, so a paragraph three siblings back can be
///   a live opener while looking inert. Appending a closing fence inside the
///   region collapses every child between them into one `CODE_BLOCK`, and no
///   parse of the region plus one neighbour contains both delimiters.
/// * Multiline-table borders. A `----` run pairs with a closing run across
///   arbitrarily many blank-line-separated blocks, swallowing everything
///   between; without the closer it is a thematic break.
///
/// The guard is a *comparison*, not a ban: when the region's old and new text
/// carry the same pairing lines, the whole document's sequence of them is
/// unchanged (the prefix and suffix are byte-identical), so every pairing
/// anywhere resolves exactly as it did. That admits the common case -- editing
/// prose, or a table cell, or a line inside a code block -- and declines only an
/// edit that actually adds, removes, or alters a delimiter. It costs O(region)
/// and, unlike the prefix scan it replaces, nothing at all in the document's
/// size, which is what keeps a keystroke's cost independent of where it lands.
fn long_range_pairing_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim_start_matches([' ', '\t']);
            if line.len() - trimmed.len() <= 3
                && (trimmed.starts_with("```")
                    || trimmed.starts_with("~~~")
                    || trimmed.starts_with(":::")
                    || trimmed.starts_with("$$"))
            {
                return true;
            }
            if line.contains("<!--") || line.contains("-->") {
                return true;
            }
            let body = trimmed.trim_end();
            !body.is_empty()
                && body.chars().all(|c| matches!(c, '-' | '=' | ' ' | '\t'))
                && body.chars().any(|c| matches!(c, '-' | '='))
        })
        .collect()
}

/// Merge the previous errors with a bounded region reparse's own.
///
/// This is the **third bucket** [`merge_incremental_errors`] does not have and
/// its doc comment promised to the region tier: a bounded region leaves a live
/// suffix whose errors survive and must move by the edit delta.
///
/// * Before the region: kept verbatim. Region selection guarantees
///   `region.start <= edit.start`, so those offsets did not move.
/// * Inside the region: dropped; the fragment parse re-derives them.
/// * After the region: shifted by `delta`.
///
/// `SyntaxErrorSource::Yaml` is the only error source and both emit sites lie
/// strictly inside their owning top-level child, so an error cannot straddle a
/// region boundary. That stays a `debug_assert!` plus a bail rather than a
/// guess, exactly as the seam merge treats its own impossible case.
fn merge_region_errors(
    old_errors: &[SyntaxError],
    region: (usize, usize),
    frag_errors: Vec<SyntaxError>,
    delta: isize,
) -> Option<Vec<SyntaxError>> {
    let shift = |offset: rowan::TextSize, by: isize| -> Option<rowan::TextSize> {
        let shifted = usize::from(offset) as isize + by;
        (shifted >= 0).then(|| rowan::TextSize::new(shifted as u32))
    };
    let region_start = rowan::TextSize::new(region.0 as u32);
    let region_end = rowan::TextSize::new(region.1 as u32);

    let mut merged = Vec::with_capacity(old_errors.len() + frag_errors.len());
    for error in old_errors {
        if error.range.end() <= region_start {
            merged.push(error.clone());
        }
    }
    for error in frag_errors {
        merged.push(SyntaxError {
            range: rowan::TextRange::new(
                error.range.start() + region_start,
                error.range.end() + region_start,
            ),
            ..error
        });
    }
    for error in old_errors {
        if error.range.start() >= region_end {
            merged.push(SyntaxError {
                range: rowan::TextRange::new(
                    shift(error.range.start(), delta)?,
                    shift(error.range.end(), delta)?,
                ),
                ..error.clone()
            });
        } else if error.range.end() <= region_start {
        } else if error.range.start() < region_start || error.range.end() > region_end {
            debug_assert!(
                false,
                "syntax error {:?} straddles region {region:?}",
                error.range
            );
            return None;
        }
    }

    debug_assert!(
        merged
            .windows(2)
            .all(|w| w[0].range.start() <= w[1].range.start()),
        "merged syntax errors are out of document order: {merged:?}"
    );
    Some(merged)
}

/// The region tier: reparse one run of top-level `DOCUMENT` children as a
/// bounded fragment and splice it in place.
///
/// The other window strategies re-parse from a window start to EOF, so their
/// cost is a function of where the edit sits. This one parses the region, plus
/// up to two neighbour-sized boundary parses, and nothing else.
///
/// The correctness argument has three parts, and the first is what makes the
/// other two finite:
///
/// * **Everything outside the region is byte-identical, and a region is a run of
///   whole children.** So every construct in the old parse is entirely inside
///   the region or entirely outside it, the container stack and fence state are
///   empty at both seams (that is what being top-level children *means*), and
///   the only hazards left are couplings the edit newly creates.
/// * **Absorptive lookahead is caught by the boundary parses.** Anything the
///   fragment leaves open swallows greedily, so it eats the immediate neighbour
///   first and cannot hide: the neighbour then straddles the seam or fails to
///   come back byte-identical.
/// * **Terminator-seeking lookahead is caught by
///   [`long_range_pairing_lines`],** which is the only class that reaches past
///   one neighbour.
///
/// Every check is a decline. `None` costs the caller the window tiers it would
/// have tried anyway.
#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn reparse_region(
    input: &str,
    config: &ParserOptions,
    old_tree: &SyntaxNode,
    old_errors: &[SyntaxError],
    old_edit: (usize, usize),
    new_edit: (usize, usize),
    cost_guards: CostGuards,
) -> Option<Reparsed> {
    if old_tree.kind() != SyntaxKind::DOCUMENT {
        return None;
    }
    if old_edit.1 > usize::from(old_tree.text_range().end()) {
        return None;
    }

    if region_is_too_wide(
        cost_guards,
        new_edit.1.saturating_sub(new_edit.0),
        input.len(),
    ) {
        return None;
    }

    let region = select_region(old_tree, old_edit)?;

    debug_assert!(region.start <= old_edit.0, "region starts past the edit");
    if region.start > old_edit.0 {
        return None;
    }

    let delta = new_edit.1 as isize - old_edit.1 as isize;
    let new_region_end = (region.end as isize + delta) as usize;
    if new_region_end > input.len()
        || !input.is_char_boundary(region.start)
        || !input.is_char_boundary(new_region_end)
    {
        return None;
    }

    let context_len = region.prev.map_or(0, |(s, e)| e - s) + region.next.map_or(0, |(s, e)| e - s);
    if region_is_too_wide(
        cost_guards,
        (new_region_end - region.start) + context_len,
        input.len(),
    ) {
        return None;
    }

    let old_text = old_tree.text().to_string();
    let fragment = &input[region.start..new_region_end];
    if long_range_pairing_lines(&old_text[region.start..region.end])
        != long_range_pairing_lines(fragment)
    {
        return None;
    }

    let (frag_tree, frag_errors) = parse_fragment_at(fragment, region.start, config);
    let frag_children = green_children(&frag_tree);
    let old_children = green_children(old_tree);

    if let Some((prev_start, prev_end)) = region.prev {
        let guard = format!("{}{}", &old_text[prev_start..region.start], fragment);
        let seam = region.start - prev_start;
        let (parsed, _) = parse_fragment_at(&guard, prev_start, config);
        let (before, after) = split_children_at(&parsed, seam)?;
        if after != frag_children {
            return None;
        }
        if before != children_covering(old_tree, &old_children, prev_start, region.start) {
            return None;
        }
        debug_assert!(prev_end <= region.start);
    }

    let tail_end = region.next.map_or(old_text.len(), |(_, end)| end);
    if tail_end > region.end {
        let guard = format!("{}{}", fragment, &old_text[region.end..tail_end]);
        let seam = fragment.len();
        let (parsed, _) = parse_fragment_at(&guard, region.start, config);
        let (before, after) = split_children_at(&parsed, seam)?;
        if before != frag_children {
            return None;
        }
        if after != children_covering(old_tree, &old_children, region.end, tail_end) {
            return None;
        }
    }

    let errors = merge_region_errors(old_errors, (region.start, region.end), frag_errors, delta)?;

    let old_green = old_tree.green();
    let start_idx = first_child_ending_after(old_green, region.start);
    let end_idx = first_child_starting_at_or_after(old_green, region.end);
    let new_green = old_green.splice_children(start_idx..end_idx, frag_children);

    let result = Reparsed {
        green: new_green,
        errors,
        reparse_range: (region.start, new_region_end),
        strategy: ReparseStrategy::Region,
    };
    assert_matches_full_parse(&result, input, config);
    Some(result)
}

fn children_covering(
    tree: &SyntaxNode,
    children: &[GreenChild],
    start: usize,
    end: usize,
) -> Vec<GreenChild> {
    let green = tree.green();
    let from = first_child_ending_after(green, start);
    let to = first_child_starting_at_or_after(green, end);
    children[from.min(children.len())..to.min(children.len())].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{SyntaxErrorSource, parse, parse_with_errors};

    fn edit(range: Range<usize>, insert: &str) -> Edit {
        Edit {
            range,
            insert: insert.to_string(),
        }
    }

    #[test]
    fn diff_edit_recovers_a_noop() {
        assert_eq!(diff_edit("# Title\n", "# Title\n"), edit(8..8, ""));
    }

    #[test]
    fn diff_edit_recovers_an_insertion() {
        let e = diff_edit("# Title\n", "# Titles\n");
        assert_eq!(e, edit(7..7, "s"));
        assert_eq!(e.apply("# Title\n"), "# Titles\n");
        assert_eq!(e.delta(), 1);
        assert_eq!(e.new_range(), (7, 8));
    }

    #[test]
    fn diff_edit_recovers_a_deletion() {
        let e = diff_edit("# Titles\n", "# Title\n");
        assert_eq!(e, edit(7..8, ""));
        assert_eq!(e.apply("# Titles\n"), "# Title\n");
        assert_eq!(e.delta(), -1);
        assert_eq!(e.new_range(), (7, 7));
    }

    #[test]
    fn diff_edit_recovers_a_replacement() {
        let e = diff_edit("a *b* c\n", "a *z* c\n");
        assert_eq!(e, edit(3..4, "z"));
        assert_eq!(e.apply("a *b* c\n"), "a *z* c\n");
        assert_eq!(e.delta(), 0);
    }

    #[test]
    fn diff_edit_collapses_disjoint_edits_into_one_span() {
        let e = diff_edit("a b c\n", "z b z\n");
        assert_eq!(e, edit(0..5, "z b z"));
        assert_eq!(e.apply("a b c\n"), "z b z\n");
    }

    #[test]
    fn diff_edit_handles_whole_replacement_and_empty_texts() {
        assert_eq!(diff_edit("", "# Title\n"), edit(0..0, "# Title\n"));
        assert_eq!(diff_edit("# Title\n", ""), edit(0..8, ""));
        assert_eq!(diff_edit("abc", "xyz"), edit(0..3, "xyz"));
    }

    #[test]
    fn diff_edit_clamps_to_char_boundaries() {
        let e = diff_edit("\u{3B1} text\n", "\u{3B2} text\n");
        assert_eq!(e, edit(0..2, "\u{3B2}"));
        assert_eq!(e.apply("\u{3B1} text\n"), "\u{3B2} text\n");

        let old = "x\u{3AC}";
        let new = "x\u{1FB6}";
        let e = diff_edit(old, new);
        assert!(old.is_char_boundary(e.range.start));
        assert!(old.is_char_boundary(e.range.end));
        assert_eq!(e.apply(old), new);
    }

    #[test]
    fn reparse_splices_a_paragraph_edit() {
        let old_text = "# One\n\nAlpha.\n\n# Two\n\nBeta.\n";
        let new_text = "# One\n\nAlpha.\n\n# Two\n\nBetas.\n";
        let options = ParserOptions::default();
        let (old_tree, old_errors) =
            super::super::Parser::new(old_text, &options).parse_with_errors();

        let edit = diff_edit(old_text, new_text);
        let result = reparse(
            &old_tree.green().to_owned(),
            &old_errors,
            &edit,
            new_text,
            &options,
        )
        .expect("a plain paragraph edit must splice");

        assert_eq!(
            crate::parser::fingerprint(&SyntaxNode::new_root(result.green)),
            crate::parser::fingerprint(&super::super::parse(new_text, Some(options))),
        );
    }

    #[test]
    fn window_share_cutoff_is_inclusive_at_the_threshold() {
        assert_eq!(MAX_WINDOW_SHARE_PERCENT, 85);
        let enforced = |start, len| window_is_too_wide(CostGuards::Enforced, start, len);
        assert!(!enforced(15, 100), "an 85% window is accepted");
        assert!(enforced(14, 100), "an 86% window is declined");
        assert!(enforced(0, 100), "a 100% window is declined");
        assert!(!enforced(0, 0));
        assert!(!enforced(9, 4));
        assert!(!window_is_too_wide(CostGuards::Ignored, 0, 100));
    }

    #[test]
    fn reparse_declines_a_whole_document_replacement() {
        let old_text = "# One\n\nAlpha.\n\n# Two\n\nBeta.\n";
        let new_text = "# Replaced\n\nAll new text.\n";
        let options = ParserOptions::default();
        let (old_tree, old_errors) = Parser::new(old_text, &options).parse_with_errors();

        assert!(
            reparse(
                &old_tree.green().to_owned(),
                &old_errors,
                &diff_edit(old_text, new_text),
                new_text,
                &options,
            )
            .is_none()
        );
    }

    #[test]
    fn reparse_declines_an_early_edit_and_accepts_a_late_one() {
        let mut input = String::new();
        for index in 0..20 {
            input.push_str(&format!(
                "## Section {index:02}\n\nbody {index:02} text\n\n"
            ));
        }
        let options = ParserOptions::default();
        let (old_tree, old_errors) = Parser::new(&input, &options).parse_with_errors();

        let splice_at = |needle: &str| {
            let at = input.find(needle).expect("marker in test input");
            let updated = apply_edit(&input, (at, at + 4), "BODY");
            reparse_ranges(
                &updated,
                &options,
                &old_tree,
                &old_errors,
                (at, at + 4),
                (at, at + 4),
                CostGuards::Enforced,
            )
            .map(|reparsed| {
                let full = super::super::parse(&updated, Some(options.clone()));
                assert_eq!(
                    crate::parser::fingerprint(&SyntaxNode::new_root(reparsed.green.clone())),
                    crate::parser::fingerprint(&full),
                    "an accepted splice must match a full parse"
                );
                reparsed.reparse_range.0
            })
        };

        assert!(
            splice_at("body 02").is_none(),
            "a window leaving ~90% of the document downstream must decline"
        );
        let late_start = splice_at("body 16").expect("a late edit must still splice");
        assert!(
            input.len() - late_start < input.len() * MAX_WINDOW_SHARE_PERCENT / 100,
            "the accepted window must sit under the cutoff"
        );
    }

    /// The section window is anchored at the previous top-level heading, which
    /// can be much earlier than the edited block. When that anchor is too wide
    /// the cutoff must decline *the strategy*, not the reparse: the suffix
    /// window starts at the enclosing block and is narrower here.
    #[test]
    fn a_too_wide_section_window_falls_through_to_the_suffix_window() {
        let mut input = String::from("# Only heading\n\n");
        for index in 0..20 {
            input.push_str(&format!("paragraph {index:02} of the body text\n\n"));
        }
        let at = input.find("paragraph 17").expect("marker in test input");

        let inc = window_cascade_incrementally(&input, at + 10, "`x`", ParserOptions::default());
        assert_eq!(inc.strategy, "suffix_window");
        assert!(
            inc.reparse_range.0 > 0,
            "the suffix window must retain a real prefix"
        );
    }

    #[test]
    fn reparse_declines_an_edit_past_the_previous_tree() {
        let old_text = "Alpha.\n";
        let options = ParserOptions::default();
        let (old_tree, old_errors) =
            super::super::Parser::new(old_text, &options).parse_with_errors();

        let stale = Edit {
            range: 50..60,
            insert: String::new(),
        };
        assert!(
            reparse(
                &old_tree.green().to_owned(),
                &old_errors,
                &stale,
                "Alpha.\n",
                &options,
            )
            .is_none()
        );
    }

    struct Spliced {
        tree: SyntaxNode,
        errors: Vec<SyntaxError>,
        reparse_range: (usize, usize),
        strategy: &'static str,
    }

    fn full_parse(input: &str, options: &ParserOptions) -> Spliced {
        let mut options = options.clone();
        populate_refdef_labels(input, &mut options);
        let (tree, errors) = Parser::new(input, &options).parse_with_errors();
        let len: usize = tree.text_range().end().into();
        Spliced {
            tree,
            errors,
            reparse_range: (0, len),
            strategy: "full_reparse",
        }
    }

    fn reparse_or_full(
        input: &str,
        options: Option<ParserOptions>,
        old_tree: &SyntaxNode,
        old_errors: &[SyntaxError],
        old_edit: (usize, usize),
        new_edit: (usize, usize),
    ) -> Spliced {
        let options = options.unwrap_or_default();
        let Some(insert) = input
            .get(new_edit.0..new_edit.1)
            .filter(|_| new_edit.0 == old_edit.0)
        else {
            return full_parse(input, &options);
        };
        let edit = Edit {
            range: old_edit.0..old_edit.1,
            insert: insert.to_string(),
        };
        match reparse(
            &old_tree.green().to_owned(),
            old_errors,
            &edit,
            input,
            &options,
        ) {
            Some(reparsed) => Spliced {
                tree: SyntaxNode::new_root(reparsed.green),
                errors: reparsed.errors,
                reparse_range: reparsed.reparse_range,
                strategy: reparsed.strategy.as_str(),
            },
            None => full_parse(input, &options),
        }
    }

    fn apply_edit(text: &str, old: (usize, usize), insert: &str) -> String {
        let mut out = String::with_capacity(text.len() - (old.1 - old.0) + insert.len());
        out.push_str(&text[..old.0]);
        out.push_str(insert);
        out.push_str(&text[old.1..]);
        out
    }

    fn flavor_options(flavor: crate::options::Flavor) -> ParserOptions {
        ParserOptions {
            flavor,
            extensions: crate::options::Extensions::for_flavor(flavor),
            dialect: crate::options::Dialect::for_flavor(flavor),
            ..Default::default()
        }
    }

    fn quarto_options() -> ParserOptions {
        flavor_options(crate::options::Flavor::Quarto)
    }

    /// Apply `insert` at `at` (a pure insertion) and reparse incrementally.
    /// Drive the *window* cascade directly, skipping the tiers above it.
    ///
    /// Some cases below are about which window a shape reaches and which guard
    /// declines it. Routed through the public entry point they would measure
    /// the token and region tiers instead, which answer most of those shapes
    /// first -- and a test that quietly changed subject is worse than one that
    /// fails, because it goes on passing.
    fn window_cascade_incrementally(
        input: &str,
        at: usize,
        insert: &str,
        options: ParserOptions,
    ) -> Spliced {
        let mut options = options;
        populate_refdef_labels(input, &mut options);
        let (old_tree, old_errors) = Parser::new(input, &options).parse_with_errors();
        let updated = apply_edit(input, (at, at), insert);
        match reparse_ranges(
            &updated,
            &options,
            &old_tree,
            &old_errors,
            (at, at),
            (at, at + insert.len()),
            CostGuards::Enforced,
        ) {
            Some(reparsed) => Spliced {
                tree: SyntaxNode::new_root(reparsed.green),
                errors: reparsed.errors,
                reparse_range: reparsed.reparse_range,
                strategy: reparsed.strategy.as_str(),
            },
            None => full_parse(&updated, &options),
        }
    }

    fn insert_incrementally(
        input: &str,
        at: usize,
        insert: &str,
        options: ParserOptions,
    ) -> Spliced {
        let (old_tree, old_errors) = parse_with_errors(input, Some(options.clone()));
        let updated = apply_edit(input, (at, at), insert);
        reparse_or_full(
            &updated,
            Some(options),
            &old_tree,
            &old_errors,
            (at, at),
            (at, at + insert.len()),
        )
    }

    #[test]
    fn parse_with_errors_reports_malformed_hashpipe_yaml() {
        let input = "```{r}\n#| echo: [\n1 + 1\n```\n";
        let (_tree, errors) = parse_with_errors(input, Some(quarto_options()));
        assert_eq!(errors.len(), 1, "expected one yaml error, got {errors:?}");
        assert_eq!(errors[0].source, SyntaxErrorSource::Yaml);
        let start: usize = errors[0].range.start().into();
        assert_eq!(start, input.find('[').unwrap());
    }

    #[test]
    fn parse_with_errors_reports_malformed_frontmatter_yaml() {
        let input = "---\ntitle: [\n---\n";
        let (_tree, errors) = parse_with_errors(input, None);
        assert_eq!(errors.len(), 1, "expected one yaml error, got {errors:?}");
        assert_eq!(errors[0].source, SyntaxErrorSource::Yaml);
        let start: usize = errors[0].range.start().into();
        assert_eq!(start, input.find('[').unwrap());
    }

    #[test]
    fn parse_with_errors_empty_for_valid_document() {
        let input = "---\ntitle: ok\n---\n\n```{r}\n#| echo: false\n1\n```\n";
        let (_tree, errors) = parse_with_errors(input, Some(quarto_options()));
        assert!(
            errors.is_empty(),
            "valid document should have no errors: {errors:?}"
        );
    }

    fn yaml_error(start: u32, end: u32) -> SyntaxError {
        SyntaxError {
            range: rowan::TextRange::new(start.into(), end.into()),
            message: format!("error at {start}"),
            source: SyntaxErrorSource::Yaml,
        }
    }

    #[test]
    fn merge_keeps_prefix_errors_and_shifts_window_errors() {
        let old = vec![yaml_error(3, 9)];
        let merged = merge_incremental_errors(&old, 20, vec![yaml_error(2, 5)]).unwrap();
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0], old[0]);
        assert_eq!(merged[1].range, rowan::TextRange::new(22.into(), 25.into()));
        assert_eq!(merged[1].message, "error at 2");
    }

    #[test]
    fn merge_drops_old_errors_the_window_reparses() {
        let old = vec![yaml_error(3, 9), yaml_error(30, 40)];
        let merged = merge_incremental_errors(&old, 20, vec![yaml_error(10, 20)]).unwrap();
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0], old[0]);
        assert_eq!(merged[1].range, rowan::TextRange::new(30.into(), 40.into()));
    }

    #[test]
    fn merge_of_error_free_parses_is_empty() {
        assert!(
            merge_incremental_errors(&[], 12, Vec::new())
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "straddles the splice seam")]
    fn merge_refuses_an_error_straddling_the_seam() {
        merge_incremental_errors(&[yaml_error(15, 25)], 20, Vec::new());
    }

    #[test]
    fn refdef_guard_survives_a_scan_window_edge_inside_a_multibyte_token() {
        let mut input = String::from("para one\n\n");
        input.push_str(&"x".repeat(REFDEF_SCAN_WINDOW - 12));
        input.push_str("\u{2705}\n\npara two\n");
        let edit_at = input.find("para two").unwrap();

        let (old_tree, old_errors) = parse_with_errors(&input, None);
        let updated = apply_edit(&input, (edit_at, edit_at + 8), "para three");
        let inc = reparse_or_full(
            &updated,
            None,
            &old_tree,
            &old_errors,
            (edit_at, edit_at + 8),
            (edit_at, edit_at + 10),
        );
        assert_eq!(inc.tree.text().to_string(), updated);
    }

    #[test]
    fn incremental_bails_when_the_edit_range_exceeds_the_old_tree() {
        let input = "para one\n\npara two\n";
        let (old_tree, old_errors) = parse_with_errors(input, None);
        let past_end = usize::from(old_tree.text_range().end()) + 5;
        let inc = reparse_or_full(
            input,
            None,
            &old_tree,
            &old_errors,
            (past_end, past_end + 4),
            (0, 0),
        );
        assert_eq!(inc.strategy, "full_reparse");
    }

    /// A window is parsed standalone, so its first line is a document's first
    /// line to the block dispatcher, and `% Title` would become a pandoc title
    /// block. The window path can only decline that, via a textual guard.
    ///
    /// This document is far too small for the region tier's width bail, so the
    /// answer here is a full parse. The padded twin below is the region-tier
    /// version, which *answers* the same shape instead of refusing it.
    #[test]
    fn a_window_must_not_manufacture_a_pandoc_title_block() {
        let input = "intro para\n\n Title\n% Author\n\ntail para\n";
        let at = input.find(" Title").unwrap();
        let inc = insert_incrementally(
            input,
            at,
            "%",
            flavor_options(crate::options::Flavor::Pandoc),
        );
        assert_eq!(inc.strategy, "full_reparse");
    }

    /// The same shape on a document long enough to reach the region tier.
    ///
    /// The tier *answers* it rather than declining, because its fragment parse
    /// is a `ParseOrigin::Fragment` and so has no document start to offer. The
    /// debug oracle checks every splice against a full parse, so `"region"`
    /// here is a correctness claim and not just a routing one --- if the
    /// fragment had manufactured a title block, this test would panic inside
    /// the reparse rather than fail its assertion.
    #[test]
    fn a_region_reparse_must_not_manufacture_a_pandoc_title_block() {
        let mut input = String::from("intro para\n\n Title\n% Author\n\n");
        for index in 0..200 {
            input.push_str(&format!("Filler paragraph {index}.\n\n"));
        }
        let at = input.find(" Title").unwrap();
        let inc = insert_incrementally(
            &input,
            at,
            "%",
            flavor_options(crate::options::Flavor::Pandoc),
        );
        assert_eq!(inc.strategy, "region");
    }

    #[test]
    fn suffix_window_must_not_manufacture_mid_document_yaml_under_commonmark() {
        let input = "intro para\n\n--\nkey: value\n---\n\ntail para\n";
        let at = input.find("--\nkey").unwrap() + 2;
        let inc = insert_incrementally(input, at, "-", flavor_options(crate::options::Flavor::Gfm));
        assert_eq!(inc.strategy, "full_reparse");
    }

    #[test]
    fn a_window_must_not_manufacture_an_mmd_title_block() {
        let input = "intro para\n\nKey value\nOther: thing\n\ntail para\n";
        let at = input.find("Key value").unwrap() + 3;
        let inc = insert_incrementally(
            input,
            at,
            ":",
            flavor_options(crate::options::Flavor::MultiMarkdown),
        );
        assert_eq!(inc.strategy, "full_reparse");
    }

    #[test]
    fn a_region_reparse_must_not_manufacture_an_mmd_title_block() {
        let mut input = String::from("intro para\n\nKey value\nOther: thing\n\n");
        for index in 0..200 {
            input.push_str(&format!("Filler paragraph {index}.\n\n"));
        }
        let at = input.find("Key value").unwrap() + 3;
        let inc = insert_incrementally(
            &input,
            at,
            ":",
            flavor_options(crate::options::Flavor::MultiMarkdown),
        );
        assert_eq!(inc.strategy, "region");
    }

    #[test]
    fn suffix_window_still_reuses_when_the_construct_is_not_live() {
        let input = "intro para\n\nKey value\nOther: thing\n\ntail para\n";
        let at = input.find("Key value").unwrap() + 3;
        let inc = insert_incrementally(
            input,
            at,
            ":",
            flavor_options(crate::options::Flavor::Pandoc),
        );
        assert_eq!(inc.strategy, "suffix_window");
    }

    #[test]
    fn incremental_suffix_matches_full_parse_for_tail_edit() {
        let input = "# H\n\npara one\n\npara two\n\npara three\n";
        let old_tree = parse(input, None);
        let old_edit = (30, 35);
        let updated = apply_edit(input, old_edit, "tail section");
        let new_edit = (30, 42);

        let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit).tree;
        let full = parse(&updated, None);
        assert_eq!(inc.to_string(), full.to_string());
    }

    #[test]
    fn incremental_suffix_matches_full_parse_for_middle_edit() {
        let input = "# H\n\n- a\n- b\n\nfinal para\n";
        let old_tree = parse(input, None);
        let old_edit = (10, 11);
        let updated = apply_edit(input, old_edit, "alpha");
        let new_edit = (10, 15);

        let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit).tree;
        let full = parse(&updated, None);
        assert_eq!(inc.to_string(), full.to_string());
    }

    #[test]
    fn incremental_suffix_matches_full_parse_for_setext_transition() {
        let input = "Intro\nSecond\n\nTail\n";
        let old_tree = parse(input, None);
        let old_edit = (5, 5);
        let updated = apply_edit(input, old_edit, "\n-----");
        let new_edit = (5, 11);

        let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit).tree;
        let full = parse(&updated, None);
        assert_eq!(inc.to_string(), full.to_string());
    }

    #[test]
    fn incremental_suffix_matches_full_parse_for_lazy_blockquote_change() {
        let input = "> quoted\nlazy\n\nnext\n";
        let old_tree = parse(input, None);
        let old_edit = (9, 13);
        let updated = apply_edit(input, old_edit, "> line");
        let new_edit = (9, 15);

        let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit).tree;
        let full = parse(&updated, None);
        assert_eq!(inc.to_string(), full.to_string());
    }

    #[test]
    fn incremental_uses_heading_section_window_when_available() {
        let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta section\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let start = input.find("beta").expect("beta in test input");
        let old_edit = (start, start + 4);
        let updated = apply_edit(input, old_edit, "BETA");
        let new_edit = (start, start + 4);

        let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert!(
            inc.reparse_range.0 > 0,
            "section reparse should not start at 0"
        );
        assert!(
            inc.reparse_range.1 < updated.len(),
            "section reparse should stop before EOF"
        );
    }

    #[test]
    fn incremental_uses_section_window_for_last_section() {
        let input = "# Intro\n\nalpha\n\n# Last\n\nbeta section\n";
        let old_tree = parse(input, None);
        let start = input.find("beta").expect("beta in test input");
        let old_edit = (start, start + 4);
        let updated = apply_edit(input, old_edit, "BETA");
        let new_edit = (start, start + 4);

        let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert!(
            inc.reparse_range.0 > 0,
            "last section should start at the last heading boundary"
        );
        assert_eq!(
            inc.reparse_range.1,
            updated.len(),
            "last section should end at EOF"
        );
    }

    #[test]
    fn incremental_does_not_use_section_window_when_edit_touches_heading() {
        let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let middle_start = input
            .find("# Middle")
            .expect("middle heading in test input");
        let old_edit = (middle_start, middle_start + 1);
        let updated = apply_edit(input, old_edit, "#");
        let new_edit = (middle_start, middle_start + 1);

        let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert_eq!(
            inc.reparse_range.1,
            updated.len(),
            "edits on headings should avoid section-window reparsing"
        );
    }

    #[test]
    fn incremental_does_not_use_section_window_when_edit_crosses_next_heading() {
        let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let beta_start = input.find("beta").expect("beta in test input");
        let end_start = input.find("# End").expect("end heading in test input");
        let old_edit = (beta_start, end_start + 2);
        let updated = apply_edit(input, old_edit, "beta\n\n# ");
        let new_edit = (beta_start, beta_start + 8);

        let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert_eq!(
            inc.reparse_range.1,
            updated.len(),
            "cross-heading edits should avoid section-window reparsing"
        );
    }

    #[test]
    fn incremental_ignores_nested_headings_for_window_boundaries() {
        let input =
            "# Intro\n\nprelude para\n\n# Middle\n\n> ## Nested\n> quote body\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let quote_start = input.find("quote body").expect("quote body in test input");
        let old_edit = (quote_start, quote_start + 5);
        let updated = apply_edit(input, old_edit, "QUOTE");
        let new_edit = (quote_start, quote_start + 5);

        let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert_eq!(
            inc.reparse_range.0,
            updated
                .find("# Middle")
                .expect("middle heading in test input"),
            "the window must anchor on the top-level heading, not the nested one"
        );
        assert!(
            inc.reparse_range.1 < updated.len(),
            "window boundary should be the next top-level heading, not nested heading"
        );
    }

    #[test]
    fn incremental_section_window_handles_list_tight_loose_transition() {
        let input = "# Intro\n\nprelude\n\n# Middle\n\n- one\n- two\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let two_start = input.find("- two").expect("list item in test input");
        let old_edit = (two_start, two_start);
        let updated = apply_edit(input, old_edit, "\n");
        let new_edit = (two_start, two_start + 1);

        let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert!(
            inc.reparse_range.0 > 0 && inc.reparse_range.1 < updated.len(),
            "list transition inside section should remain section-bounded"
        );
    }

    #[test]
    fn incremental_section_window_handles_blockquote_lazy_transition() {
        let input = "# Intro\n\nprelude\n\n# Middle\n\n> quoted\nlazy line\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let lazy_start = input.find("lazy line").expect("lazy line in test input");
        let old_edit = (lazy_start, lazy_start);
        let updated = apply_edit(input, old_edit, "> ");
        let new_edit = (lazy_start, lazy_start + 2);

        let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert!(
            inc.reparse_range.0 > 0 && inc.reparse_range.1 < updated.len(),
            "blockquote continuation change inside section should remain section-bounded"
        );
    }

    #[test]
    fn incremental_section_window_handles_fenced_div_with_nested_heading() {
        let input = "# Intro\n\nprelude\n\n# Middle\n\n::: {.callout-note}\n## Nested\nbody text\n:::\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let body_start = input.find("body text").expect("body text in test input");
        let old_edit = (body_start, body_start + 4);
        let updated = apply_edit(input, old_edit, "BODY");
        let new_edit = (body_start, body_start + 4);

        let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert!(
            inc.reparse_range.0 > 0 && inc.reparse_range.1 < updated.len(),
            "fenced div edits should use top-level heading boundaries"
        );
    }

    #[test]
    fn incremental_handles_inserting_heading_inside_section_window() {
        let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let beta_start = input.find("beta").expect("beta in test input");
        let old_edit = (beta_start, beta_start);
        let updated = apply_edit(input, old_edit, "## Inserted\n\n");
        let new_edit = (beta_start, beta_start + "## Inserted\n\n".len());

        let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert_eq!(
            inc.strategy, "section_window",
            "heading insertions within a bounded section should remain section-window mode"
        );
    }

    #[test]
    fn incremental_falls_back_when_deleting_next_heading_boundary() {
        let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let end_start = input.find("# End\n").expect("end heading in test input");
        let old_edit = (end_start, end_start + "# End\n\n".len());
        let updated = apply_edit(input, old_edit, "");
        let new_edit = (end_start, end_start);

        let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert_ne!(
            inc.strategy, "section_window",
            "heading deletions across boundaries should avoid section-window mode"
        );
    }

    #[test]
    fn incremental_falls_back_when_editing_blank_line_after_heading() {
        let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let boundary = input
            .find("# Middle\n\n")
            .expect("middle heading boundary in test input");
        let blank_line_start = boundary + "# Middle\n".len();
        let old_edit = (blank_line_start, blank_line_start + 1);
        let updated = apply_edit(input, old_edit, "");
        let new_edit = (blank_line_start, blank_line_start);

        let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert_ne!(
            inc.strategy, "section_window",
            "heading-adjacent blank line edits should avoid section-window mode"
        );
    }

    #[test]
    fn incremental_handles_frontmatter_to_first_heading_edit() {
        let input = "---\ntitle: Demo\n---\n\n# Intro\n\nalpha\n\n# Next\n\nomega\n";
        let old_tree = parse(input, None);
        let title_start = input.find("Demo").expect("frontmatter value in test input");
        let old_edit = (title_start, title_start + 4);
        let updated = apply_edit(input, old_edit, "Updated Demo");
        let new_edit = (title_start, title_start + "Updated Demo".len());

        let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert_ne!(
            inc.strategy, "section_window",
            "frontmatter edits before first heading should use conservative mode"
        );
    }

    #[test]
    fn incremental_handles_frontmatter_delimiter_edit() {
        let input = "---\ntitle: Demo\n---\n\n# Intro\n\nalpha\n";
        let old_tree = parse(input, None);
        let first_delim_start = 0;
        let old_edit = (first_delim_start, first_delim_start + 3);
        let updated = apply_edit(input, old_edit, "----");
        let new_edit = (first_delim_start, first_delim_start + 4);

        let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert_ne!(
            inc.strategy, "section_window",
            "frontmatter delimiter edits should stay in conservative mode"
        );
    }

    #[test]
    fn a_blank_line_is_recognized_under_every_line_ending() {
        assert!(ends_with_blank_line("a\n\n"));
        assert!(ends_with_blank_line("a\r\n\r\n"));
        assert!(ends_with_blank_line("a\n\r\n"));
        assert!(ends_with_blank_line("a\r\n\n"));
        assert!(!ends_with_blank_line("a\n"));
        assert!(!ends_with_blank_line("a\r\n"));
        assert!(!ends_with_blank_line("a"));
        assert!(!ends_with_blank_line(""));
        assert!(!ends_with_blank_line("a\n\r"));
    }

    /// A CRLF document must reach the splice at all. Every seam test in the
    /// cascade is textual, so a `"\n\n"`-only blank-line check refuses one
    /// silently: correct, and a total loss of the feature for anything authored
    /// on Windows.
    #[test]
    fn a_crlf_document_splices_like_its_lf_twin() {
        let lf =
            "# Intro\n\nalpha body here\n\n# Middle\n\nbeta body here\n\n# End\n\nomega body\n";
        let crlf = lf.replace('\n', "\r\n");

        for document in [lf.to_string(), crlf] {
            let at = document.find("beta").expect("marker in test input");
            let old_tree = parse(&document, None);
            let updated = apply_edit(&document, (at, at + 4), "BETA");

            let inc = reparse_or_full(
                &updated,
                None,
                &old_tree,
                &[],
                (at, at + 4),
                (at, at + "BETA".len()),
            );
            assert_eq!(
                inc.strategy,
                "section_window",
                "line ending decided the strategy: {:?}",
                &document[..8]
            );
            assert_eq!(
                crate::parser::fingerprint(&inc.tree),
                crate::parser::fingerprint(&parse(&updated, None)),
            );
        }
    }

    #[test]
    fn a_section_window_declines_a_retained_thematic_break_with_a_dash_partner() {
        let mut input = String::from("intro para\n\n---\n\n");
        for index in 0..20 {
            input.push_str(&format!("filler {index:02} paragraph text\n\n"));
        }
        input.push_str("# Section\n\nbody text\n");
        let at = input.find("body text").expect("marker in test input");

        let accepted =
            window_cascade_incrementally(&input, at, "edited ", ParserOptions::default());
        assert_eq!(accepted.strategy, "section_window");

        let declined =
            window_cascade_incrementally(&input, at, "a b\n---\n1 2\n", ParserOptions::default());
        assert_ne!(
            declined.strategy, "section_window",
            "a dash-rule partner in the window must decline the section splice"
        );
    }

    /// Splice `insert` in over `range` and return the strategy that ran, having
    /// first checked the governing invariant on any splice that happened.
    ///
    /// The invariant check is unconditional and not an assertion about the
    /// tier: a test that pins a *decline* still proves the fallback is right,
    /// and a test that pins the token tier proves the splice is right. Every
    /// case below gets both for free.
    fn token_tier_strategy(before: &str, range: Range<usize>, insert: &str) -> &'static str {
        let options = ParserOptions::default();
        let after = apply_edit(before, (range.start, range.end), insert);
        let (old_tree, old_errors) = parse_with_errors(before, None);

        let spliced = reparse_or_full(
            &after,
            Some(options),
            &old_tree,
            &old_errors,
            (range.start, range.end),
            (range.start, range.start + insert.len()),
        );

        let (full, full_errors) = parse_with_errors(&after, None);
        assert_eq!(
            crate::parser::fingerprint(&spliced.tree),
            crate::parser::fingerprint(&full),
            "strategy {} diverged from a full parse of {after:?}",
            spliced.strategy,
        );
        assert_eq!(
            spliced.errors, full_errors,
            "strategy {} diverged from a full parse on errors",
            spliced.strategy,
        );
        spliced.strategy
    }

    fn assert_token_tier(before: &str, range: Range<usize>, insert: &str) {
        let strategy = token_tier_strategy(before, range.clone(), insert);
        assert_eq!(
            strategy, "token",
            "inserting {insert:?} at {range:?} of {before:?} should reach the token tier"
        );
    }

    fn assert_not_token_tier(before: &str, range: Range<usize>, insert: &str) {
        let strategy = token_tier_strategy(before, range.clone(), insert);
        assert_ne!(
            strategy, "token",
            "inserting {insert:?} at {range:?} of {before:?} must not reach the token tier"
        );
    }

    #[test]
    fn a_prose_edit_inside_a_paragraph_reaches_the_token_tier() {
        let before = "# Title\n\nSome ordinary prose here.\n\nAnd a second paragraph.\n";
        let at = before.find("ordinary").expect("marker in test input") + 3;
        assert_token_tier(before, at..at, "X");
    }

    /// The tier must not need a document big enough to clear the window-size
    /// cutoff: it is meant to run *before* that guard, which is what lets it
    /// skip the fixed cascade overhead too.
    #[test]
    fn a_prose_edit_in_a_tiny_document_reaches_the_token_tier() {
        let before = "alpha beta gamma\n";
        assert_token_tier(before, 8..8, "X");
    }

    #[test]
    fn an_interior_edit_that_manufactures_a_list_marker_declines() {
        let before = "12 apples here\n";
        assert_not_token_tier(before, 2..2, ".");
    }

    #[test]
    fn an_interior_edit_that_completes_a_list_marker_declines() {
        let before = "12.apples here\n";
        assert_not_token_tier(before, 3..3, " ");
    }

    #[test]
    fn an_interior_space_that_manufactures_a_hard_break_declines() {
        let before = "alpha beta \nsecond line\n";
        assert_not_token_tier(before, 10..10, " ");
    }

    #[test]
    fn an_edit_beside_an_unmatched_delimiter_declines() {
        let before = "abc*def ghi\n";
        assert_not_token_tier(before, 9..9, "*");
    }

    #[test]
    fn a_deletion_that_frees_a_delimiter_to_pair_declines() {
        let before = "a*b*c*d word\n";
        assert_not_token_tier(before, 3..4, "");
    }

    /// A bare URI typed into prose is a construct the byte mask cannot see
    /// (its scheme has no leading-byte gate), so the tier answers it by
    /// re-parsing the token's text in isolation.
    #[test]
    fn an_edit_that_manufactures_a_bare_uri_declines() {
        let before = "see docs now\n";
        let options = flavor_options(crate::options::Flavor::Gfm);
        let after = apply_edit(before, (4, 8), "https://example.com");
        let (old_tree, old_errors) =
            crate::parser::Parser::new(before, &options).parse_with_errors();
        let spliced = reparse_or_full(
            &after,
            Some(options.clone()),
            &old_tree,
            &old_errors,
            (4, 8),
            (4, 4 + "https://example.com".len()),
        );
        assert_ne!(
            spliced.strategy, "token",
            "a bare URI is a construct; the token tier must not splice past it"
        );
        assert_eq!(
            crate::parser::fingerprint(&spliced.tree),
            crate::parser::fingerprint(&crate::parser::Parser::new(&after, &options).parse()),
        );
    }

    #[test]
    fn an_edit_inside_a_code_block_declines() {
        let before = "```\nsome code here\n```\n";
        let at = before.find("code").expect("marker in test input") + 1;
        assert_not_token_tier(before, at..at, "X");
    }

    fn every_flavor() -> Vec<(&'static str, ParserOptions)> {
        use crate::options::Flavor;
        [
            ("pandoc", Flavor::Pandoc),
            ("quarto", Flavor::Quarto),
            ("rmarkdown", Flavor::RMarkdown),
            ("gfm", Flavor::Gfm),
            ("commonmark", Flavor::CommonMark),
            ("multimarkdown", Flavor::MultiMarkdown),
            ("mdsvex", Flavor::Mdsvex),
            ("myst", Flavor::Myst),
        ]
        .into_iter()
        .map(|(name, flavor)| (name, flavor_options(flavor)))
        .collect()
    }

    #[test]
    fn the_token_tier_alphabet_admits_no_structural_byte() {
        for (name, options) in every_flavor() {
            let structural =
                crate::parser::inlines::core::structural_byte_mask_without_uri_schemes(&options);
            let allowed = token_tier_allowed_mask(&options);
            for byte in 0..=u8::MAX {
                assert!(
                    !(allowed[byte as usize] && structural[byte as usize]),
                    "{name}: byte {:?} is both allowed by the token tier and \
                     structural to the inline grammar",
                    byte as char,
                );
            }
        }
    }

    /// Two bans the tier cannot survive losing, stated separately from the
    /// seed so that deleting them from it fails loudly rather than quietly.
    ///
    /// Line terminators would let a splice move block boundaries. Brackets are
    /// what keeps reference definitions out of reach: `]:` cannot appear in a
    /// token, so the tier needs none of the refdef machinery the window tiers
    /// carry.
    #[test]
    fn the_token_tier_alphabet_bans_terminators_and_brackets() {
        for (name, options) in every_flavor() {
            let allowed = token_tier_allowed_mask(&options);
            for &byte in b"\n\r[]" {
                assert!(
                    !allowed[byte as usize],
                    "{name}: byte {:?} must never be admitted",
                    byte as char,
                );
            }
        }
    }

    /// The block-side check, stated for exactly what it proves and no more.
    ///
    /// `structural_byte_mask` speaks only for the *inline* dispatcher, so it has
    /// nothing to say about the 27-parser block registry. This puts every
    /// admitted byte mid-line in a paragraph and checks the parser agrees it is
    /// inert there: one top-level `PARAGRAPH`, the line still one coalesced
    /// `TEXT`.
    ///
    /// **Mid-line is the whole claim.** A block trigger is only a trigger at the
    /// head of a line, and this says nothing about that position --- adding `#`
    /// to the seed would not fail here, because `a # b` really is inert prose.
    /// What keeps line heads safe is positional, not alphabetic: strict
    /// interiority pins the token's leading bytes and
    /// [`edit_is_past_line_marker_zone`] pins the line's. The block bytes are
    /// excluded from the seed as defence in depth, not because this test
    /// demands it.
    #[test]
    fn every_admitted_byte_is_inert_mid_line() {
        for (name, options) in every_flavor() {
            let allowed = token_tier_allowed_mask(&options);
            for byte in 0x20..=0x7Eu8 {
                if !allowed[byte as usize] {
                    continue;
                }
                let ch = byte as char;
                let input = format!("lead in words {ch} and more prose\n");
                let tree = crate::parser::Parser::new(&input, &options).parse();

                let children: Vec<_> = tree.children().collect();
                assert_eq!(
                    children.len(),
                    1,
                    "{name}: {ch:?} split the document into {} blocks",
                    children.len(),
                );
                assert_eq!(
                    children[0].kind(),
                    SyntaxKind::PARAGRAPH,
                    "{name}: {ch:?} produced a {:?}, not a paragraph",
                    children[0].kind(),
                );
                let texts = children[0]
                    .children_with_tokens()
                    .filter(|element| element.kind() == SyntaxKind::TEXT)
                    .count();
                assert_eq!(
                    texts, 1,
                    "{name}: {ch:?} split the paragraph's prose into {texts} TEXT tokens",
                );
            }
        }
    }

    /// A hazard no token-local or line-local proof can see: `foo bar` above a
    /// `--- | ---` line is a paragraph, and adding one `|` makes the pair a
    /// `PIPE_TABLE`. The deciding byte is on a *different line* of the same
    /// block.
    ///
    /// This is the case that rules out ever admitting `|`, and the reason the
    /// alphabet is a vetted allowlist rather than the inline mask's complement:
    /// `structural_byte_mask` never sets `|` under any configuration, because
    /// pipe tables are a block construct and the mask speaks for inline
    /// dispatch only.
    #[test]
    fn a_pipe_inserted_above_a_delimiter_row_declines() {
        let before = "foo bar\n--- | ---\n";
        let tree = parse(before, None);
        assert_eq!(
            tree.children().next().map(|child| child.kind()),
            Some(SyntaxKind::PARAGRAPH),
        );
        let after = apply_edit(before, (4, 4), "| ");
        assert_eq!(
            parse(&after, None).children().next().map(|c| c.kind()),
            Some(SyntaxKind::PIPE_TABLE),
        );

        assert_not_token_tier(before, 4..4, "| ");
    }

    #[test]
    fn an_edit_that_widens_a_line_indent_declines() {
        let before = "- item\n\n foo bar\n";
        let tree = parse(before, None);
        assert_eq!(
            tree.children().last().map(|child| child.kind()),
            Some(SyntaxKind::PARAGRAPH),
            "the paragraph must start out top-level for this to be a hazard",
        );
        assert_not_token_tier(before, 8..8, "   ");
    }

    /// The property the debug oracle enforces at runtime, hoisted into a test
    /// that runs in release builds too: for every printable byte, typed into
    /// prose, the tier either declines or produces exactly what a full parse
    /// produces. There is no third outcome.
    ///
    /// This is the check that would catch an alphabet admitting a byte whose
    /// hazard nobody thought of, because it does not encode anyone's idea of
    /// what the hazards are.
    #[test]
    fn inserting_any_byte_into_prose_declines_or_matches_a_full_parse() {
        for (name, options) in every_flavor() {
            for byte in 0x20..=0x7Eu8 {
                let ch = byte as char;
                for before in [
                    "lead in words here and more prose\n",
                    "# Title\n\nlead in words here and more prose\n\ntail para\n",
                    "12 apples in a basket here\n",
                    "a*b c*d and more words\n",
                    "trailing spaces live here \nsecond line\n",
                ] {
                    let at = before
                        .find("words")
                        .or_else(|| before.find("apples"))
                        .unwrap_or(4)
                        + 2;
                    let insert = ch.to_string();
                    let after = apply_edit(before, (at, at), &insert);

                    let (old_tree, old_errors) =
                        crate::parser::Parser::new(before, &options).parse_with_errors();
                    let attempt = reparse(
                        &old_tree.green().to_owned(),
                        &old_errors,
                        &edit(at..at, &insert),
                        &after,
                        &options,
                    );

                    let Some(spliced) = attempt else {
                        continue; // Declining is always allowed.
                    };
                    let (full, full_errors) =
                        crate::parser::Parser::new(&after, &options).parse_with_errors();
                    assert_eq!(
                        crate::parser::fingerprint(&SyntaxNode::new_root(spliced.green)),
                        crate::parser::fingerprint(&full),
                        "{name}: inserting {ch:?} into {before:?} spliced a tree a full \
                         parse disagrees with ({} strategy)",
                        spliced.strategy.as_str(),
                    );
                    assert_eq!(
                        spliced.errors, full_errors,
                        "{name}: inserting {ch:?} into {before:?} spliced errors a full \
                         parse disagrees with",
                    );
                }
            }
        }
    }

    #[test]
    fn the_bench_paragraph_shape_reaches_the_token_tier() {
        let line = "Paragraph 042 alpha beta gamma delta epsilon zeta eta theta.\n";
        let before = format!("# Benchmark Document\n\n{line}\n{line}\n");
        let at = before.find("alpha").expect("marker in test input");

        assert_token_tier(&before, at..at + 5, "ALPHA");

        assert_not_token_tier(&before, at..at + 5, "`ALPHA`");
    }

    #[test]
    fn typing_a_word_character_by_character_stays_at_the_token_tier() {
        let mut text = String::from("# Title\n\nThe quick brown fox jumps.\n\nMore prose.\n");
        let mut at = text.find("brown").expect("marker in test input") + 2;

        for ch in "increment".chars() {
            let insert = ch.to_string();
            assert_token_tier(&text, at..at, &insert);
            text = apply_edit(&text, (at, at), &insert);
            at += insert.len();
        }
    }

    const REGION_DOC: &str =
        "# Title\n\nAlpha para.\n\n- one\n- two\n\nGamma para.\n\nDelta para.\n";

    fn region_of(input: &str, old_edit: (usize, usize)) -> Option<Region> {
        let options = ParserOptions::default();
        let tree = Parser::new(input, &options).parse();
        select_region(&tree, old_edit)
    }

    #[test]
    fn select_region_takes_the_edited_child_alone() {
        let at = REGION_DOC.find("Gamma").expect("marker") + 2;
        let region = region_of(REGION_DOC, (at, at)).expect("a region");
        assert_eq!(
            &REGION_DOC[region.start..region.end],
            "Gamma para.\n",
            "the region is exactly the edited paragraph"
        );
        assert_eq!(
            region.prev.map(|(s, e)| &REGION_DOC[s..e]),
            Some("- one\n- two\n"),
            "prev skips the blank line to the nearest real child"
        );
        assert_eq!(
            region.next.map(|(s, e)| &REGION_DOC[s..e]),
            Some("Delta para.\n")
        );
    }

    #[test]
    fn select_region_widens_at_a_child_boundary() {
        let at = REGION_DOC.find("Gamma").expect("marker");
        let region = region_of(REGION_DOC, (at, at)).expect("a region");
        assert_eq!(
            &REGION_DOC[region.start..region.end],
            "- one\n- two\n\nGamma para.\n",
            "an insertion abutting the blank line takes the child on each side of it"
        );
    }

    /// Deleting the blank line between two blocks must select *both*, or the
    /// splice would keep two children where a full parse has one. Closed-interval
    /// touch is what gives this.
    #[test]
    fn select_region_spans_a_deleted_blank_line() {
        let blank = REGION_DOC.find("\n\nGamma").expect("marker") + 1;
        let region = region_of(REGION_DOC, (blank, blank + 1)).expect("a region");
        assert_eq!(
            &REGION_DOC[region.start..region.end],
            "- one\n- two\n\nGamma para.\n",
            "both neighbours and the blank line between them are in the region"
        );
    }

    #[test]
    fn select_region_declines_when_it_would_cover_the_whole_document() {
        let input = "Only para.\n";
        assert_eq!(region_of(input, (0, input.len())), None);
        assert_eq!(region_of(REGION_DOC, (0, REGION_DOC.len())), None);
        assert_eq!(region_of("", (0, 0)), None);
    }

    #[test]
    fn select_region_never_starts_past_the_edit() {
        let options = ParserOptions::default();
        let tree = Parser::new(REGION_DOC, &options).parse();
        for at in 0..=REGION_DOC.len() {
            if let Some(region) = select_region(&tree, (at, at)) {
                assert!(
                    region.start <= at,
                    "region {region:?} starts past an edit at {at}"
                );
            }
        }
    }

    /// Snapping is a pure optimization too, so the same rule applies: it must
    /// agree with the linear `token_at_offset` scan it replaced at *every*
    /// offset, in both directions. The old implementation is inlined here as
    /// the oracle, which is the only way this stays honest if rowan's
    /// `covering_element` ever changes its boundary bias.
    #[test]
    fn snapping_agrees_with_a_linear_token_scan() {
        fn by_linear_scan(tree: &SyntaxNode, offset: usize, upward: bool) -> usize {
            let len: usize = tree.text_range().end().into();
            let clamped = offset.min(len);
            let at = tree.token_at_offset((clamped as u32).into());
            let snapped = if upward {
                at.right_biased()
                    .map(|token| usize::from(token.text_range().end()))
            } else {
                at.left_biased()
                    .map(|token| usize::from(token.text_range().start()))
            };
            snapped.unwrap_or(if upward { len } else { 0 }).min(len)
        }

        let options = ParserOptions::default();
        for input in [
            "# Title\n\nAlpha para.\n\n- one\n- two\n\nGamma para.\n",
            "para\n\n[x]: /url\n\nmore para\n",
            "héllo wörld\n\n```\ncode\n```\n\nπαρά κείμενο\n",
            "a\r\nb\r\n\r\nc\r\n",
            "",
        ] {
            let tree = Parser::new(input, &options).parse();
            let len = usize::from(tree.text_range().end());
            for offset in 0..=len + 2 {
                for upward in [false, true] {
                    assert_eq!(
                        snap_out_to_token_boundary(&tree, offset, upward),
                        by_linear_scan(&tree, offset, upward),
                        "snapping diverged at {offset} (upward {upward}) in {input:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn covering_text_agrees_with_a_whole_tree_slice() {
        let options = ParserOptions::default();
        for input in [
            "# Title\n\nAlpha para.\n\n- one\n- two\n\nGamma para.\n",
            "para\n\n[x]: /url\n\nmore para\n",
            "héllo wörld\n\n```\ncode\n```\n\nπαρά κείμενο\n",
            "",
        ] {
            let tree = Parser::new(input, &options).parse();
            let len = usize::from(tree.text_range().end());
            for start in 0..=len {
                for end in start..=len {
                    if !input.is_char_boundary(start) || !input.is_char_boundary(end) {
                        continue;
                    }
                    if start == end {
                        continue;
                    }
                    let range = rowan::TextRange::new(
                        rowan::TextSize::new(start as u32),
                        rowan::TextSize::new(end as u32),
                    );
                    assert_eq!(
                        covering_text(&tree, range),
                        tree.text().slice(range).to_string(),
                        "covering_text diverged at {start}..{end} in {input:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn no_straddle_finds_a_boundary_or_says_there_is_none() {
        let options = ParserOptions::default();
        let tree = Parser::new("Para one.\n\nPara two.\n", &options).parse();
        assert!(no_straddle(&tree, 0), "the document start is a boundary");
        assert!(no_straddle(&tree, 10), "a child boundary");
        assert!(no_straddle(&tree, 21), "EOF is a boundary");
        assert!(!no_straddle(&tree, 5), "mid-paragraph is not");

        let fused = Parser::new("Para one.\n=========\n", &options).parse();
        assert!(!no_straddle(&fused, 10), "a setext heading spans the seam");
    }

    #[test]
    fn region_width_bail_charges_the_neighbours_too() {
        assert_eq!(REGION_MAX_FILE_DIVISOR, 4);
        let enforced = |parsed, len| region_is_too_wide(CostGuards::Enforced, parsed, len);
        assert!(!enforced(5000, 100_000), "a twentieth of the document");
        assert!(!enforced(25_000, 100_000), "exactly a quarter is accepted");
        assert!(enforced(25_001, 100_000), "one byte over is declined");
        assert!(enforced(50, 74), "a small document gets no exemption");
        assert!(!region_is_too_wide(CostGuards::Ignored, 30_000, 100_000));
        assert!(!enforced(0, 0));
    }

    #[test]
    fn long_range_pairing_lines_ignore_prose_and_catch_delimiters() {
        assert_eq!(
            long_range_pairing_lines("just prose\nmore prose\n"),
            Vec::<&str>::new()
        );
        assert_eq!(long_range_pairing_lines("a\n```\nb\n"), vec!["```"]);
        assert_eq!(long_range_pairing_lines("a\n::: note\n"), vec!["::: note"]);
        assert_eq!(long_range_pairing_lines("a\n----\n"), vec!["----"]);
        assert_eq!(
            long_range_pairing_lines("a\n<!-- c -->\n"),
            vec!["<!-- c -->"]
        );
        assert_eq!(
            long_range_pairing_lines("a\n| - | - |\n"),
            Vec::<&str>::new()
        );
        assert_eq!(
            long_range_pairing_lines("a\n+----+----+\n"),
            Vec::<&str>::new()
        );
        assert_eq!(
            long_range_pairing_lines("```\nlet x = 1;\n```\n"),
            long_range_pairing_lines("```\nlet x = 12;\n```\n")
        );
        assert_ne!(
            long_range_pairing_lines("prose\n"),
            long_range_pairing_lines("```\n")
        );
    }

    #[test]
    fn merge_region_errors_keeps_before_drops_inside_and_shifts_after() {
        let err = |start: u32, end: u32| SyntaxError {
            range: rowan::TextRange::new(start.into(), end.into()),
            message: "boom".to_owned(),
            source: SyntaxErrorSource::Yaml,
        };
        let old = vec![err(2, 4), err(20, 24), err(60, 64)];
        let merged = merge_region_errors(&old, (10, 50), vec![err(1, 3)], 5).expect("a merge");
        assert_eq!(
            merged,
            vec![err(2, 4), err(11, 13), err(65, 69)],
            "before kept verbatim, inside dropped, fragment shifted to the region, after shifted by the delta"
        );
    }

    #[test]
    fn merge_region_errors_shifts_a_trailing_error_backwards_on_a_deletion() {
        let err = |start: u32, end: u32| SyntaxError {
            range: rowan::TextRange::new(start.into(), end.into()),
            message: "boom".to_owned(),
            source: SyntaxErrorSource::Yaml,
        };
        let merged =
            merge_region_errors(&[err(60, 64)], (10, 50), Vec::new(), -7).expect("a merge");
        assert_eq!(merged, vec![err(53, 57)]);
    }

    fn assert_region_tier(input: &str, old_edit: (usize, usize), insert: &str) -> Spliced {
        let new_text = apply_edit(input, old_edit, insert);
        let options = ParserOptions::default();
        let (old_tree, old_errors) = Parser::new(input, &options).parse_with_errors();
        let spliced = reparse_or_full(
            &new_text,
            Some(options),
            &old_tree,
            &old_errors,
            old_edit,
            (old_edit.0, old_edit.0 + insert.len()),
        );
        assert_eq!(
            spliced.strategy, "region",
            "expected the region tier for {insert:?} at {old_edit:?} in {input:?}"
        );
        spliced
    }

    #[test]
    fn region_tier_answers_an_early_edit_the_window_cutoff_declines() {
        let input = long_document_with_early_paragraph();
        let at = input.find("Alpha").expect("marker") + 2;
        let spliced = assert_region_tier(&input, (at, at), "X");
        assert!(
            spliced.reparse_range.1 - spliced.reparse_range.0 < 32,
            "the region is one short paragraph, not a window to EOF: {:?}",
            spliced.reparse_range
        );
    }

    #[test]
    fn region_tier_declines_a_fence_that_pairs_beyond_the_neighbour() {
        let input = "```\naaa\n\nbbb\n\nccc\n\nddd\n";
        let at = input.find("ccc").expect("marker") + 3;
        let new_text = apply_edit(input, (at, at), "\n```");
        let options = ParserOptions::default();
        let (old_tree, old_errors) = Parser::new(input, &options).parse_with_errors();
        let spliced = reparse_or_full(
            &new_text,
            Some(options),
            &old_tree,
            &old_errors,
            (at, at),
            (at, at + 4),
        );
        assert_ne!(
            spliced.strategy, "region",
            "a new fence delimiter must not reach the region tier"
        );
    }

    /// The mirror image: a multiline-table border pairs across arbitrarily many
    /// blank-line-separated blocks. Verified against pandoc --
    /// `----\nx\nfoo\n\nbar\n\n----\n` is one table.
    #[test]
    fn region_tier_declines_a_table_border_that_pairs_beyond_the_neighbour() {
        let input = "intro\n\nx\nfoo\n\nbar\n\n----\n\ntail\n";
        let at = input.find("intro").expect("marker");
        let new_text = apply_edit(input, (at, at + 5), "----");
        let options = ParserOptions::default();
        let (old_tree, old_errors) = Parser::new(input, &options).parse_with_errors();
        let spliced = reparse_or_full(
            &new_text,
            Some(options),
            &old_tree,
            &old_errors,
            (at, at + 5),
            (at, at + 4),
        );
        assert_ne!(
            spliced.strategy, "region",
            "a new table border must not reach the region tier"
        );
    }

    /// Typing into an early paragraph, keystroke by keystroke, chaining each
    /// splice onto the previous one. A tier that works once and then splices
    /// against its own stale output fails here and nowhere else.
    ///
    /// The document has to be long enough that the window tiers decline every
    /// keystroke, because the region tier is tried behind them: on a short
    /// document a window would answer first and this would pin the wrong tier.
    #[test]
    fn typing_into_a_region_stays_correct_across_keystrokes() {
        let mut text = long_document_with_early_paragraph();
        let mut at = text.find("Alpha").expect("marker") + 2;
        for ch in "increment".chars() {
            let insert = ch.to_string();
            let spliced = assert_region_tier(&text, (at, at), &insert);
            let updated = apply_edit(&text, (at, at), &insert);
            let full = full_parse(&updated, &ParserOptions::default());
            assert_eq!(
                crate::parser::fingerprint(&spliced.tree),
                crate::parser::fingerprint(&full.tree),
                "keystroke {ch:?} diverged from a full parse"
            );
            text = updated;
            at += insert.len();
        }
    }

    fn long_document_with_early_paragraph() -> String {
        let mut input = String::from("# Title\n\nAlpha para.\n\n");
        for index in 0..400 {
            input.push_str(&format!("Filler paragraph number {index}.\n\n"));
        }
        input
    }
}
