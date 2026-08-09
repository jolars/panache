//! Measurement harness for LSP incremental reparsing: what a `didChange`
//! costs with the reparse side channel versus what it costs without one.
//!
//! # The model this mirrors
//!
//! The harness reproduces [`panache::salsa::parsed_document`] rather than
//! approximating it, because the approximation is what made the old numbers
//! wrong:
//!
//! * **One combined edit per notification.** The host keeps no staged edit
//!   chain; it diffs whole texts (`diff_edit`) and hands the parser a single
//!   contiguous edit. A notification carrying four content changes therefore
//!   still gets an incremental *attempt*, over the span from the first change
//!   to the last. The old harness declined outright on `changes.len() != 1`
//!   and reported the resulting full parse as if it were the incremental
//!   strategy, so every multi-change case measured full-parse-vs-full-parse.
//! * **A chained base.** Step *n* splices against the tree step *n-1*
//!   produced, not against the original parse. That is what makes a typing
//!   stream a stream and not *n* independent one-shot edits.
//! * **The host's refdef check, then the parser's.** `refdef_set` is a salsa
//!   query keyed on the text, so a changed document rescans; the incremental
//!   path is charged for that scan exactly as the full path is. A changed
//!   *set* declines host-side, before the parser is called at all.
//! * **`refdef_set` backdates, so both paths pay its comparison.** Salsa
//!   compares the recomputed set against the old one and hands back the *same*
//!   `Arc` when they are equal, which is what makes `parsed_document`'s
//!   `prev.refdefs != refdefs` a pointer compare rather than a walk of the set.
//!   Modelling the query as a bare scan instead charged that walk to the
//!   incremental path alone. [`refdef_query`] models the backdating and both
//!   step functions call it, so the comparison lands on both paths as it does
//!   in production. (This costs the synthetic cases nothing --- their documents
//!   carry no reference definitions, so the sets are empty either way. It is
//!   the refdef-carrying documents where a bare scan would have lied.)
//!
//! Applying the client's changes to the text buffer is *not* timed: it happens
//! on the LSP main thread and costs both strategies the same, so the step
//! texts are precomputed and the timed region is parse work only.
//!
//! # What the numbers mean
//!
//! Per step (one `didChange`), for each of two strategies:
//!
//! * **full** --- refdef scan + full parse, i.e. what the query cost before
//!   incremental parsing existed.
//! * **incremental** --- refdef scan + diff + [`reparse_with_refdefs`], plus a
//!   full parse when it declines. A declined step is strictly *more*
//!   expensive than a full parse; that surcharge is the bail cost.
//!
//! and three accounting numbers the speedup alone hides:
//!
//! * **fallback rate** --- declined steps / total steps. Deterministic per
//!   case, so it is a property of the edit shape, not noise.
//! * **bail cost** --- mean wall time of a `reparse` call that returned
//!   `None`, and that time as a fraction of a full parse. This is the guard
//!   cascade's price, paid on top of the full parse the caller then runs.
//! * **window vs spliced bytes** --- *both* strategies parse their window to
//!   EOF (list-item buffering needs unbounded lookahead), so `window %` is the
//!   share of the document actually re-parsed and is what predicts the
//!   speedup. `spliced %` is the smaller region whose green children were
//!   replaced; a section window's win over a suffix window is `Arc` identity
//!   for the retained tail, not parse time. Reading `spliced %` as work done
//!   is what made "7% reparsed, 0.98x speedup" look like a paradox.
//!
//! # Results
//!
//! `PANACHE_LSP_BENCH_ITERATIONS=80 cargo bench --bench lsp_incremental`, on an
//! AMD Ryzen 9 7900, rustc 1.94.1, `Config::default()` (pandoc flavor).
//! Microseconds per step; `full` and `incr` are means. Measured with the
//! window-size cutoff live (roadmap Phase 5b), which is why every case with a
//! window over 85% now reports a 100% fallback rate and no `window %`.
//!
//! ```text
//! case                               bytes  steps    full    incr  speedup  fallback   bail%  window%
//! single_change_small                 1620      1    41.8    31.7     1.3x      0.0%       -    59.4%
//! multi_change_small_4                1620      1    41.8    41.9     1.0x      0.0%       -    78.5%
//! multi_change_medium_4              15922      1   397.9   367.3     1.1x      0.0%       -    75.8%
//! multi_change_medium_clustered_4    15922      1   393.6   123.7     3.2x      0.0%       -    16.0%
//! multi_change_large_8               76542      1  1878.5  1945.5     1.0x    100.0%    0.0%        -
//! multi_change_utf16_4                  74      1     3.6     5.8     0.6x    100.0%   57.8%        -
//! full_replace                        1620      1     2.1     2.4     0.9x    100.0%    4.0%        -
//! typing_stream_medium               15922     14   394.6   139.2     2.8x      0.0%       -    20.0%
//! window_cutoff_accepted             15922      1   397.0   365.9     1.1x      0.0%       -    79.9%
//! window_cutoff_declined             15922      1   401.2   395.9     1.0x    100.0%    0.0%        -
//! bail_refdef_edit                    2687      1    93.1   108.2     0.9x    100.0%   15.4%        -
//! pandoc_manual_early_edit          300856      1 10148.5 10220.1     1.0x    100.0%    0.0%        -
//! pandoc_manual_refdef_label_edit   300856      1 10263.2 10221.8     1.0x    100.0%       -        -
//! pandoc_manual_late_edit           300856      1 10378.2  1831.8     5.7x      0.0%       -     7.0%
//! pandoc_manual_typing_stream       300856     12 10291.1  1801.9     5.7x      0.0%       -     7.0%
//! large_authoring_single_edit        29858      1   606.8   617.8     1.0x    100.0%    0.1%        -
//! tables_single_edit                 25101      1   737.8   725.1     1.0x    100.0%    0.0%        -
//! math_single_edit                   30112      1   547.7   544.2     1.0x    100.0%    0.1%        -
//! ```
//!
//! What the table says:
//!
//! * **The speedup is a function of `window %` and nothing else.** Every case
//!   below ~25% wins outright; the ones that used to sit at or above ~90% no
//!   longer appear in that column at all, because the cutoff declines them
//!   before the window parse. `tables_single_edit` is the shape that made the
//!   point: it edits line 40 of a 25 KB document, used to re-parse 98% of it
//!   for 1.0x, and reported a 7.5% *spliced* share --- the number the old
//!   harness printed, and the reason "7% reparsed, 0.98x speedup" used to look
//!   like a paradox.
//! * **A wide reparse loses to a full parse even when it succeeds**, which is
//!   what the cutoff is for. Before it, `pandoc_manual_early_edit` accepted,
//!   re-parsed 97%, and paid 0.9x for the guard cascade and splice on top;
//!   `full_replace` was the extreme at 0.2x, walking the old 1.6 KB tree to
//!   splice a 27-byte document. Declining both up front costs a fraction of a
//!   microsecond (`bail%` 0.0% and 4.0%) and returns them to 1.0x and 0.9x.
//! * **`window_cutoff_accepted` and `window_cutoff_declined` bracket the
//!   threshold**: the same document and the same one-word edit, at a 79.9% and
//!   an 87.8% window. Move `MAX_WINDOW_SHARE_PERCENT` and exactly one of them
//!   flips its fallback rate between 0% and 100%.
//! * **Clustering matters; change count does not.** The two medium
//!   multi-change cases carry the same four changes. Scattering them over 150
//!   lines takes `diff_edit`'s span from one line to most of the document: 16%
//!   window to 76%, 3.2x to 1.1x.
//! * **The typing streams are the workload the feature exists for**, and they
//!   are where it pays: 2.8x on a 16 KB document, 5.7x on the 300 KB pandoc
//!   manual, with no step declining. Each stream agrees with its equivalent
//!   single edit (`pandoc_manual_late_edit`) to within noise, which is the
//!   evidence that chaining the base does not degrade across keystrokes.
//! * **Bail cost is small, and the fallback rate is what governs it.** A
//!   cutoff decline is under a microsecond even at 300 KB, because it is
//!   arithmetic on the edit offset. The correctness guards cost more: the
//!   parser's `]:`-proximity cascade prices at 15.4% of a full parse
//!   (`bail_refdef_edit`), and a host-side decline
//!   (`pandoc_manual_refdef_label_edit`, whose edit rewrites a refdef *label*)
//!   costs one extra refdef scan, inside the noise at 300 KB. Both land under
//!   the 20%-of-a-full-parse budget the default flip is gated on.
//!
//! ## The cases still under 1.0x
//!
//! Neither is the wide-window surcharge the cutoff removed, and neither is
//! reachable by tuning its threshold:
//!
//! * `bail_refdef_edit` (0.9x) exists to price a decline. A decline is a full
//!   parse plus the cascade that reached it, so it is *definitionally* slower;
//!   the number to read on this case is `bail%`, not the speedup. In absolute
//!   terms it is ~16 us, which is the largest a single cascade costs anywhere
//!   in the table and is where [`MAX_ABSOLUTE_OVERHEAD_US`] comes from.
//! * `multi_change_utf16_4` (0.6-0.7x) is 74 bytes, and an attempt has a fixed
//!   cost --- cloning the options, materializing a cursor root over the previous
//!   green tree, walking it for the window --- that is under 2 us against a
//!   3.6 us whole parse. It lost at 0.8x before the cutoff too, while
//!   successfully splicing. No window threshold fixes a fixed cost; roadmap
//!   Phase 7's token tier is what removes it. A document-size floor would, but
//!   it would also refuse the small documents with *narrow* windows that do win
//!   (`single_change_small`, 1.6 KB, 1.3x).
//!
//! `multi_change_large_8` used to be the third, and its ~95 us is now measured
//! rather than guessed at --- and it is not the host-side per-step work it was
//! assumed to be. On this case `diff_edit` costs 7.1 us, the config clone
//! 0.1 us, and the declined attempt 0.2 us: under 8 us of the ~95, and the base
//! text copy is not inside the timed region at all. What is left is the
//! *fallback full parse itself* running ~5% slower on the incremental path ---
//! 1861 us against 1963 us for the same call on the same text --- with the
//! previous green tree and the 64 KB edit buffer resident across it. That is
//! within the run-to-run spread of the same parse, which is why the case
//! straddles 0.95x instead of failing, and why it declares a 0.90 ceiling
//! naming the profile rather than being exempted by name.
//!
//! # Running
//!
//! ```text
//! cargo bench --bench lsp_incremental
//! PANACHE_LSP_BENCH_ITERATIONS=200 cargo bench --bench lsp_incremental
//! PANACHE_LSP_BENCH_OUTPUT_JSON=/tmp/lsp.json cargo bench --bench lsp_incremental
//! ```
//!
//! Run it in release (`cargo bench` does): in debug builds the parser's splice
//! oracle full-parses on every success, which measures the oracle.
//!
//! ## Asserting the thresholds
//!
//! ```text
//! task bench:incremental-gate                                     # corpus + gate
//! PANACHE_LSP_BENCH_ASSERT=1 cargo bench --bench lsp_incremental  # gate alone
//! ```
//!
//! Every case declares an [`Expect`]: whether it must splice on every step or
//! decline on every step, and what it claims for speed. The gate prints each
//! check with its margin and exits non-zero on a violation, so a threshold can
//! be watched drifting long before it fails.
//!
//! Two things about the rules are deliberate. **There is no global
//! fallback-rate threshold**, because since the window-size cutoff a decline is
//! the correct outcome for a wide-window edit and most of these cases fall back
//! on every step by design; the contract each case declares says which it is.
//! And **every ratio rule carries an absolute-microsecond escape**, because a
//! ratio on a 2 us baseline measures noise --- `full_replace` ends up parsing
//! 27 bytes, so 0.3 us of guard work reads as 0.9x.
//!
//! The gate refuses to run without the real-document corpus. Those cases carry
//! the strictest thresholds, their documents are gitignored, and
//! [`load_document`] skips a missing one silently --- so without that check a
//! gate run on a fresh checkout would pass by not measuring.
//!
//! Do not lower `PANACHE_LSP_BENCH_ITERATIONS` for a gate run. The default is
//! sized so the means are stable; `multi_change_large_8` fails at 4 iterations
//! and passes at 80, because its margin is a few percent on a 1.9 ms parse.
//! Raising the count is fine and makes the tight cases quieter.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

use panache::parser::{
    RefdefMap, SyntaxError, collect_refdef_labels, diff_edit, parse_with_refdefs_and_errors,
    reparse_with_refdefs,
};
use panache::{Config, SyntaxNode};
use panache_parser::Dialect;
use panache_parser::parser::fingerprint;
use serde::Serialize;

#[derive(Clone, Copy)]
struct BenchPosition {
    line: u32,
    character: u32,
}

#[derive(Clone, Copy)]
struct BenchRange {
    start: BenchPosition,
    end: BenchPosition,
}

/// One LSP content change. `range: None` is a whole-document replacement.
#[derive(Clone)]
struct BenchChange {
    range: Option<BenchRange>,
    text: String,
}

/// One `didChange` notification: every content change the client sent in it.
///
/// Changes apply in order, each to the document as the previous ones left it,
/// so a case with several changes on the *same* line must list them in
/// descending column order (which is what clients send, for this reason).
type Step = Vec<BenchChange>;

/// Whether a case exists to show reuse working or a guard firing.
///
/// Declared per case rather than checked against one global fallback-rate
/// threshold, because since the window-size cutoff landed a decline is the
/// *correct* outcome for a wide-window edit -- ten of the cases below fall back
/// on every step by design. A global "fallback below N%" rule cannot say that,
/// and a list of exemptions kept apart from the cases drifts from them.
#[derive(Clone, Copy)]
enum Reuse {
    /// Every step must splice. One declining step fails the case: a typing
    /// stream that degrades halfway is the regression this is here to catch.
    Always,
    /// Every step must decline --- the case prices a guard, or shows the cutoff
    /// refusing a shape that would lose if it were admitted.
    Never,
}

/// What a case claims, checked by `PANACHE_LSP_BENCH_ASSERT=1`.
///
/// Every case declares one, so a new case cannot be added without saying what
/// it is for, and the regression ceiling applies to all of them without an
/// opt-in.
#[derive(Clone, Copy)]
struct Expect {
    reuse: Reuse,
    /// Floor on `speedup_vs_full`, where the case is a speed *claim* and not
    /// only a guard against regression. `None` leaves the ceiling every case
    /// carries.
    min_speedup: Option<f64>,
    /// A case-specific replacement for [`MIN_SPEEDUP_CEILING`], carrying the
    /// reason it is not the default one.
    ///
    /// The reason is not decoration: it is only legitimate to relax the ceiling
    /// for a case whose overhead has been profiled and attributed, and the
    /// string is printed on every run so the exemption stays visible instead of
    /// becoming a quiet floor.
    ceiling: Option<(f64, &'static str)>,
}

impl Expect {
    fn reuses() -> Self {
        Self {
            reuse: Reuse::Always,
            min_speedup: None,
            ceiling: None,
        }
    }

    fn declines() -> Self {
        Self {
            reuse: Reuse::Never,
            min_speedup: None,
            ceiling: None,
        }
    }

    fn min_speedup(mut self, min: f64) -> Self {
        self.min_speedup = Some(min);
        self
    }

    fn ceiling(mut self, ceiling: f64, reason: &'static str) -> Self {
        self.ceiling = Some((ceiling, reason));
        self
    }
}

struct BenchCase {
    id: String,
    input: String,
    steps: Vec<Step>,
    iterations: usize,
    expect: Expect,
}

/// The previous parse the incremental strategy splices against --- the same
/// four fields [`panache::incremental::PrevParse`] keeps, minus the config,
/// which the bench never changes mid-stream.
struct ReparseBase {
    text: String,
    green: rowan::GreenNode,
    errors: Vec<SyntaxError>,
    refdefs: RefdefMap,
}

/// What one incremental step did, beside taking time.
enum StepOutcome {
    /// The guard cascade accepted and the tree was spliced.
    Reused {
        strategy: &'static str,
        /// Bytes actually re-parsed: the window start to EOF, for *both*
        /// strategies.
        window_bytes: usize,
        /// Bytes whose green children were replaced. Smaller than
        /// `window_bytes` only for a section window, which re-adopts the tail.
        spliced_bytes: usize,
    },
    /// Declined; the step paid a full parse on top of whatever the decline
    /// cost.
    Fallback {
        reason: &'static str,
        /// Wall time of the declined `reparse` call. `None` when the decline
        /// happened host-side and the parser was never called.
        bail: Option<Duration>,
    },
}

#[derive(Debug, Serialize)]
struct StrategyStats {
    mean_us: f64,
    median_us: f64,
    p95_us: f64,
}

#[derive(Debug, Serialize)]
struct CaseResult {
    id: String,
    document_size_bytes: usize,
    steps: usize,
    changes_total: usize,
    iterations: usize,
    /// Refdef scan + full parse, per step.
    full_parse: StrategyStats,
    /// Refdef scan + diff + reparse attempt (+ full parse when declined), per
    /// step.
    incremental: StrategyStats,
    speedup_vs_full: f64,
    /// Declined steps / total steps.
    fallback_rate: f64,
    /// Mean wall time of a declined `reparse` call, over the steps that
    /// reached the parser at all.
    bail_cost_us: Option<f64>,
    /// [`Self::bail_cost_us`] as a fraction of a full parse.
    bail_cost_ratio: Option<f64>,
    /// Mean re-parsed share of the document, over accepted steps.
    window_ratio: Option<f64>,
    /// Mean spliced share of the document, over accepted steps.
    spliced_ratio: Option<f64>,
    /// Accepted steps by strategy.
    strategy_counts: BTreeMap<String, usize>,
    /// Declined steps by reason.
    fallback_reasons: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct BenchmarkReport {
    schema_version: u32,
    results: Vec<CaseResult>,
}

/// The regression ceiling every case carries: incremental parsing is the
/// default path, so a case that is *slower* than the full parse it replaces is
/// a regression whatever else it proves. Nothing in the roadmap's original
/// thresholds was a ceiling --- they were all floors on the cases that win ---
/// and that is why `full_replace` sat at 0.2x unnoticed until the bench was
/// repaired.
const MIN_SPEEDUP_CEILING: f64 = 0.95;

/// The absolute escape both ratio rules carry.
///
/// A ratio on a microsecond baseline measures noise: `full_replace` ends up
/// parsing 27 bytes, so 0.2 us of guard work reads as 0.9x. Sized from the
/// largest measured cost of one guard cascade on the small documents
/// (`bail_refdef_edit`, ~16 us), so a case is forgiven its ratio only while its
/// absolute cost stays inside a single cascade --- which is the most any
/// declining step can add.
const MAX_ABSOLUTE_OVERHEAD_US: f64 = 20.0;

/// A declined step may not cost more than this share of the full parse it then
/// runs. The guard cascade is the price of admission for every edit that turns
/// out not to be spliceable.
const MAX_BAIL_RATIO: f64 = 0.20;

/// Documents [`real_document_cases`] needs, checked before anything runs.
///
/// They are gitignored and fetched by `benches/documents/download.sh`, and
/// [`load_document`] skips a case whose document is absent --- silently, and
/// precisely on the cases carrying the strictest thresholds. Without this
/// check a gate run on a fresh checkout passes by not measuring.
const REQUIRED_DOCUMENTS: &[&str] = &[
    "pandoc_manual.md",
    "large_authoring.qmd",
    "tables.qmd",
    "math.qmd",
];

fn position_to_offset_utf16(text: &str, position: BenchPosition) -> Option<usize> {
    let mut offset = 0;
    let mut current_line = 0;
    let bytes = text.as_bytes();

    for line in text.lines() {
        if current_line == position.line {
            let mut utf16_offset = 0;
            for (byte_idx, ch) in line.char_indices() {
                if utf16_offset >= position.character as usize {
                    return Some(offset + byte_idx);
                }
                utf16_offset += ch.len_utf16();
            }
            return Some(offset + line.len());
        }

        let line_end_offset = offset + line.len();
        let line_ending_len = if line_end_offset + 1 < text.len()
            && bytes[line_end_offset] == b'\r'
            && bytes[line_end_offset + 1] == b'\n'
        {
            2
        } else if line_end_offset < text.len() && bytes[line_end_offset] == b'\n' {
            1
        } else {
            0
        };

        offset += line.len() + line_ending_len;
        current_line += 1;
    }

    if current_line == position.line {
        return Some(offset);
    }

    None
}

/// Apply one content change. Panics on a position the document cannot resolve:
/// the real server validates client ranges before touching its buffer, so an
/// unresolvable one here is a typo in a case definition, not a scenario.
fn apply_change(text: &str, change: &BenchChange) -> String {
    let Some(range) = change.range else {
        return change.text.clone();
    };

    let resolve = |position: BenchPosition, what: &str| {
        position_to_offset_utf16(text, position).unwrap_or_else(|| {
            panic!(
                "unresolvable {what} position {}:{}",
                position.line, position.character
            )
        })
    };
    let start = resolve(range.start, "start");
    let end = resolve(range.end, "end");
    assert!(start <= end, "inverted change range {start}..{end}");

    let mut result = String::with_capacity(text.len() - (end - start) + change.text.len());
    result.push_str(&text[..start]);
    result.push_str(&change.text);
    result.push_str(&text[end..]);
    result
}

/// The document text after each notification, so the timed region is parse
/// work only.
fn step_texts(input: &str, steps: &[Step]) -> Vec<String> {
    let mut current = input.to_owned();
    steps
        .iter()
        .map(|changes| {
            for change in changes {
                current = apply_change(&current, change);
            }
            current.clone()
        })
        .collect()
}

/// Mirrors the `refdef_set` salsa query, which *both* strategies call: compute
/// the set, and backdate to the previous allocation when it is unchanged.
///
/// The backdating is what makes `parsed_document`'s own `prev.refdefs !=
/// refdefs` check a pointer compare rather than a hash-set walk. Modelling the
/// query as a bare scan instead charged that walk to the incremental path
/// alone, which is most of what used to look like ~100 us of unattributed
/// host-side work on `multi_change_large_8`.
fn refdef_query(prev: Option<&RefdefMap>, text: &str, config: &Config) -> RefdefMap {
    let refdefs = collect_refdef_labels(text, Dialect::for_flavor(config.flavor));
    match prev {
        Some(prev) if *prev == refdefs => prev.clone(),
        _ => refdefs,
    }
}

fn full_parse(
    text: &str,
    config: &Config,
    refdefs: RefdefMap,
) -> (rowan::GreenNode, Vec<SyntaxError>) {
    let (tree, errors) = parse_with_refdefs_and_errors(text, Some(config.clone()), refdefs);
    (tree.green().to_owned(), errors)
}

/// The baseline step: what `parsed_document` cost before the side channel.
///
/// Takes the previous step's refdef set for the same reason the incremental
/// step chains a base: the query it models is chained in production, and the
/// set comparison inside it is charged to whoever calls it -- which is both
/// strategies.
fn full_step(
    prev_refdefs: Option<&RefdefMap>,
    text: &str,
    config: &Config,
) -> (Duration, RefdefMap, rowan::GreenNode, Vec<SyntaxError>) {
    let start = Instant::now();
    let refdefs = refdef_query(prev_refdefs, text, config);
    let (green, errors) = full_parse(text, config, refdefs.clone());
    (start.elapsed(), refdefs, green, errors)
}

fn fresh_base(text: &str, config: &Config) -> ReparseBase {
    let refdefs = refdef_query(None, text, config);
    let (green, errors) = full_parse(text, config, refdefs.clone());
    ReparseBase {
        text: text.to_owned(),
        green,
        errors,
        refdefs,
    }
}

/// One incremental step, advancing `base` the way the side channel advances
/// its stored `PrevParse`.
fn incremental_step(
    base: &mut ReparseBase,
    new_text: &str,
    config: &Config,
) -> (Duration, StepOutcome) {
    let start = Instant::now();
    let refdefs = refdef_query(Some(&base.refdefs), new_text, config);

    // The host's exact set comparison runs ahead of the parser's textual
    // guard: retained blocks keep the reference resolution they were parsed
    // with, so a changed set invalidates them at a distance. An unchanged set
    // came back from the query as the same allocation, so this is the pointer
    // compare `parsed_document` performs, not a second walk of the set.
    if refdefs != base.refdefs {
        let (green, errors) = full_parse(new_text, config, refdefs.clone());
        let elapsed = start.elapsed();
        *base = ReparseBase {
            text: new_text.to_owned(),
            green,
            errors,
            refdefs,
        };
        return (
            elapsed,
            StepOutcome::Fallback {
                reason: "refdef_set_changed",
                bail: None,
            },
        );
    }

    let edit = diff_edit(&base.text, new_text);
    let attempt = Instant::now();
    let reparsed = reparse_with_refdefs(
        &base.green,
        &base.errors,
        &edit,
        new_text,
        Some(config.clone()),
        refdefs.clone(),
    );
    let attempt_elapsed = attempt.elapsed();

    match reparsed {
        Some(reparsed) => {
            let elapsed = start.elapsed();
            let outcome = StepOutcome::Reused {
                strategy: reparsed.strategy.as_str(),
                window_bytes: new_text.len().saturating_sub(reparsed.reparse_range.0),
                spliced_bytes: reparsed.reparse_range.1 - reparsed.reparse_range.0,
            };
            *base = ReparseBase {
                text: new_text.to_owned(),
                green: reparsed.green,
                errors: reparsed.errors,
                refdefs,
            };
            (elapsed, outcome)
        }
        None => {
            let (green, errors) = full_parse(new_text, config, refdefs.clone());
            let elapsed = start.elapsed();
            *base = ReparseBase {
                text: new_text.to_owned(),
                green,
                errors,
                refdefs,
            };
            (
                elapsed,
                StepOutcome::Fallback {
                    reason: "guard_declined",
                    bail: Some(attempt_elapsed),
                },
            )
        }
    }
}

/// The governing invariant, at every step of every case: a reused parse is
/// byte-identical to a full parse of the same text, tree *and* errors.
///
/// Untimed, and run once per case before the measurement loop: a bench that
/// reports a speedup for a splice that does not match a full parse is
/// measuring the wrong thing entirely. (Losing *reuse* is not a failure here
/// --- that shows up honestly in the fallback rate.)
fn verify_case(input: &str, texts: &[String], config: &Config, id: &str) {
    let mut base = fresh_base(input, config);
    for (index, text) in texts.iter().enumerate() {
        let (_, outcome) = incremental_step(&mut base, text, config);
        let path = match outcome {
            StepOutcome::Reused { strategy, .. } => strategy,
            StepOutcome::Fallback { reason, .. } => reason,
        };
        let (expected, expected_errors) =
            full_parse(text, config, refdef_query(None, text, config));
        assert_eq!(
            fingerprint(&SyntaxNode::new_root(base.green.clone())),
            fingerprint(&SyntaxNode::new_root(expected)),
            "case {id} step {index} ({path}) diverged from a full parse"
        );
        assert_eq!(
            base.errors, expected_errors,
            "case {id} step {index} ({path}) diverged from a full parse on syntax errors"
        );
    }
}

fn run_case(
    id: &str,
    input: &str,
    steps: &[Step],
    iterations: usize,
    config: &Config,
) -> CaseResult {
    let texts = step_texts(input, steps);
    verify_case(input, &texts, config, id);

    // Warm up both streams: the first parse of a document pays page faults and
    // branch-predictor cold start that no `didChange` in a live session does.
    for _ in 0..2 {
        let mut full_refdefs = refdef_query(None, input, config);
        for text in &texts {
            let (_, refdefs, green, errors) = full_step(Some(&full_refdefs), text, config);
            black_box((green, errors));
            full_refdefs = refdefs;
        }
        let mut base = fresh_base(input, config);
        for text in &texts {
            black_box(incremental_step(&mut base, text, config));
        }
    }

    let sample_count = iterations * texts.len();
    let mut full_samples = Vec::with_capacity(sample_count);
    let mut incremental_samples = Vec::with_capacity(sample_count);
    let mut bail_samples = Vec::new();
    let mut window_ratios = Vec::new();
    let mut spliced_ratios = Vec::new();
    let mut strategy_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut fallback_reasons: BTreeMap<String, usize> = BTreeMap::new();

    for _ in 0..iterations {
        // Seeded from the input, and chained across the stream, exactly as the
        // incremental side chains its base: a `didChange` always has a previous
        // revision's refdef set to backdate against.
        let mut full_refdefs = refdef_query(None, input, config);
        for text in &texts {
            let (elapsed, refdefs, green, errors) = full_step(Some(&full_refdefs), text, config);
            black_box((green, errors));
            full_refdefs = refdefs;
            full_samples.push(elapsed);
        }

        let mut base = fresh_base(input, config);
        for text in &texts {
            let (elapsed, outcome) = incremental_step(&mut base, text, config);
            incremental_samples.push(elapsed);
            match outcome {
                StepOutcome::Reused {
                    strategy,
                    window_bytes,
                    spliced_bytes,
                } => {
                    *strategy_counts.entry(strategy.to_owned()).or_default() += 1;
                    if !text.is_empty() {
                        window_ratios.push(window_bytes as f64 / text.len() as f64);
                        spliced_ratios.push(spliced_bytes as f64 / text.len() as f64);
                    }
                }
                StepOutcome::Fallback { reason, bail } => {
                    *fallback_reasons.entry(reason.to_owned()).or_default() += 1;
                    if let Some(bail) = bail {
                        bail_samples.push(bail);
                    }
                }
            }
        }
        black_box(&base.green);
    }

    let full_stats = summarize_samples(&full_samples);
    let incremental_stats = summarize_samples(&incremental_samples);
    let declined: usize = fallback_reasons.values().sum();
    let fallback_rate = if incremental_samples.is_empty() {
        0.0
    } else {
        declined as f64 / incremental_samples.len() as f64
    };
    let bail_cost_us = mean(&bail_samples.iter().map(duration_us).collect::<Vec<_>>());
    let bail_cost_ratio = bail_cost_us
        .filter(|_| full_stats.mean_us > 0.0)
        .map(|us| us / full_stats.mean_us);
    let speedup_vs_full = if incremental_stats.mean_us > 0.0 {
        full_stats.mean_us / incremental_stats.mean_us
    } else {
        0.0
    };

    CaseResult {
        id: id.to_owned(),
        document_size_bytes: input.len(),
        steps: steps.len(),
        changes_total: steps.iter().map(Vec::len).sum(),
        iterations,
        full_parse: full_stats,
        incremental: incremental_stats,
        speedup_vs_full,
        fallback_rate,
        bail_cost_us,
        bail_cost_ratio,
        window_ratio: mean(&window_ratios),
        spliced_ratio: mean(&spliced_ratios),
        strategy_counts,
        fallback_reasons,
    }
}

fn duration_us(duration: &Duration) -> f64 {
    duration.as_secs_f64() * 1_000_000.0
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn summarize_samples(samples: &[Duration]) -> StrategyStats {
    let mut micros: Vec<f64> = samples.iter().map(duration_us).collect();
    micros.sort_by(f64::total_cmp);

    let len = micros.len();
    let median = if len == 0 {
        0.0
    } else if len.is_multiple_of(2) {
        (micros[len / 2 - 1] + micros[len / 2]) / 2.0
    } else {
        micros[len / 2]
    };
    let p95_index = ((len as f64 - 1.0) * 0.95).round() as usize;
    let p95 = micros.get(p95_index).copied().unwrap_or(0.0);

    StrategyStats {
        mean_us: mean(&micros).unwrap_or(0.0),
        median_us: median,
        p95_us: p95,
    }
}

fn range_change(
    start_line: u32,
    start_char: u32,
    end_line: u32,
    end_char: u32,
    text: &str,
) -> BenchChange {
    BenchChange {
        range: Some(BenchRange {
            start: BenchPosition {
                line: start_line,
                character: start_char,
            },
            end: BenchPosition {
                line: end_line,
                character: end_char,
            },
        }),
        text: text.to_owned(),
    }
}

fn insert_change(line: u32, character: u32, text: &str) -> BenchChange {
    range_change(line, character, line, character, text)
}

fn full_change(text: &str) -> BenchChange {
    BenchChange {
        range: None,
        text: text.to_owned(),
    }
}

/// Typing `text` one character at a time, left to right, from `character` on
/// `line` --- one notification per keystroke, which is the workload the whole
/// feature exists for.
fn typing_stream(line: u32, character: u32, text: &str) -> Vec<Step> {
    let mut column = character;
    let mut steps = Vec::new();
    for ch in text.chars() {
        steps.push(vec![insert_change(line, column, &ch.to_string())]);
        column += ch.len_utf16() as u32;
    }
    steps
}

/// A blank-line separated document with a `##` heading every ten paragraphs.
///
/// The separation is load-bearing: a run of adjacent lines is a *single*
/// paragraph, and the seam guard needs a blank line to decouple at, so the
/// old unseparated generator gave every edit a window starting at byte 0 and
/// measured a full parse under an incremental name. The headings give the
/// section-window strategy something to find.
struct SyntheticDoc {
    text: String,
    /// Line index of each paragraph, so cases name paragraphs and never line
    /// arithmetic.
    paragraph_lines: Vec<u32>,
}

impl SyntheticDoc {
    fn new(paragraph_count: usize) -> Self {
        let mut text = String::from("# Benchmark Document\n\n");
        let mut line = 2u32;
        let mut paragraph_lines = Vec::with_capacity(paragraph_count);

        for index in 0..paragraph_count {
            if index.is_multiple_of(10) {
                text.push_str(&format!("## Section {:03}\n\n", index / 10));
                line += 2;
            }
            paragraph_lines.push(line);
            text.push_str(&format!(
                "Paragraph {index:03} alpha beta gamma delta epsilon zeta eta theta.\n\n"
            ));
            line += 2;
        }

        Self {
            text,
            paragraph_lines,
        }
    }

    fn line(&self, paragraph: usize) -> u32 {
        self.paragraph_lines[paragraph]
    }
}

/// Columns of the words in a generated paragraph line, so a case can say
/// "replace `beta`" without counting characters.
const ALPHA: (u32, u32) = (14, 19);
const BETA: (u32, u32) = (20, 24);
const GAMMA: (u32, u32) = (25, 30);
const DELTA: (u32, u32) = (31, 36);

fn word_change(line: u32, word: (u32, u32), text: &str) -> BenchChange {
    range_change(line, word.0, line, word.1, text)
}

/// Paragraphs in [`refdef_document`], and the line its first definition lands
/// on: two lines of title, then two lines per paragraph.
const REFDEF_PARAGRAPHS: usize = 40;
const REFDEF_LINE: u32 = 2 + 2 * REFDEF_PARAGRAPHS as u32;
/// End of `[one]: https://example.com/one`, where the edit appends.
const REFDEF_URL_END: u32 = 30;

/// A document whose reference definitions sit next to the edit, so the
/// parser's `]:`-proximity guard declines every step. This is the bail-cost
/// measurement: the case exists to price a decline, not to be fast.
fn refdef_document() -> String {
    let mut text = String::from("# Reference definitions\n\n");
    for index in 0..REFDEF_PARAGRAPHS {
        text.push_str(&format!(
            "Paragraph {index:03} referring to [one] and [two] and more prose here.\n\n"
        ));
    }
    text.push_str("[one]: https://example.com/one\n");
    text.push_str("[two]: https://example.com/two\n");
    text
}

fn load_document(name: &str) -> Option<String> {
    let path = Path::new("benches/documents").join(name);
    fs::read_to_string(path).ok()
}

fn synthetic_cases(default_iterations: usize) -> Vec<BenchCase> {
    let small = SyntheticDoc::new(25);
    let medium = SyntheticDoc::new(250);
    let large = SyntheticDoc::new(1200);
    let utf16_doc = "# UTF16\n\nemoji: 😀 rocket: 🚀\nRésumé café\nmath αβγ\nclosing line\n";
    let refdefs = refdef_document();

    vec![
        BenchCase {
            id: "single_change_small".to_owned(),
            steps: vec![vec![word_change(small.line(15), ALPHA, "ALPHA")]],
            input: small.text.clone(),
            iterations: default_iterations,
            expect: Expect::reuses(),
        },
        BenchCase {
            id: "multi_change_small_4".to_owned(),
            steps: vec![vec![
                word_change(small.line(5), ALPHA, "ALPHA"),
                word_change(small.line(10), BETA, "BETA"),
                word_change(small.line(15), GAMMA, "GAMMA"),
                word_change(small.line(20), DELTA, "DELTA"),
            ]],
            input: small.text.clone(),
            iterations: default_iterations,
            expect: Expect::reuses(),
        },
        BenchCase {
            id: "multi_change_medium_4".to_owned(),
            steps: vec![vec![
                word_change(medium.line(60), ALPHA, "ALPHA"),
                word_change(medium.line(110), BETA, "BETA"),
                word_change(medium.line(160), GAMMA, "GAMMA"),
                word_change(medium.line(210), DELTA, "DELTA"),
            ]],
            input: medium.text.clone(),
            iterations: default_iterations / 2,
            expect: Expect::reuses(),
        },
        // Multi-cursor inside one paragraph: the same change count as the
        // scattered case, but `diff_edit` spans one line instead of 150. The
        // pair is the whole argument that clustering, not count, is what the
        // incremental path cares about.
        BenchCase {
            id: "multi_change_medium_clustered_4".to_owned(),
            steps: vec![vec![
                word_change(medium.line(210), DELTA, "DELTA"),
                word_change(medium.line(210), GAMMA, "GAMMA"),
                word_change(medium.line(210), BETA, "BETA"),
                word_change(medium.line(210), ALPHA, "ALPHA"),
            ]],
            input: medium.text.clone(),
            iterations: default_iterations / 2,
            expect: Expect::reuses(),
        },
        BenchCase {
            id: "multi_change_large_8".to_owned(),
            steps: vec![vec![
                word_change(large.line(150), ALPHA, "A1"),
                word_change(large.line(300), BETA, "B2"),
                word_change(large.line(450), GAMMA, "C3"),
                word_change(large.line(600), DELTA, "D4"),
                word_change(large.line(750), ALPHA, "E5"),
                word_change(large.line(900), BETA, "F6"),
                word_change(large.line(1050), GAMMA, "G7"),
                word_change(large.line(1150), DELTA, "H8"),
            ]],
            input: large.text.clone(),
            iterations: default_iterations / 4,
            // Scattered over 1000 lines, so `diff_edit`'s span is most of the
            // document and the cutoff refuses it.
            //
            // The relaxed ceiling is profiled, not assumed. Of the ~95 us this
            // case costs over a full parse, `diff_edit` is 7 us and the config
            // clone 0.1 us; the rest is the *fallback full parse itself*
            // running slower, because the previous green tree and the 64 KB
            // edit buffer are resident across it. That residual sits within the
            // run-to-run spread of the same parse, which is why the case
            // straddles 0.95x from run to run rather than failing outright.
            expect: Expect::declines()
                .ceiling(0.90, "profiled: fallback-parse residency, not host work"),
        },
        BenchCase {
            id: "multi_change_utf16_4".to_owned(),
            input: utf16_doc.to_owned(),
            steps: vec![vec![
                range_change(2, 7, 2, 9, "😎"),
                range_change(2, 18, 2, 20, "🛰️"),
                range_change(3, 1, 3, 2, "e"),
                range_change(4, 5, 4, 7, "xyz"),
            ]],
            iterations: default_iterations,
            // 74 bytes: every window is a wide one, so the cutoff refuses.
            expect: Expect::declines(),
        },
        BenchCase {
            id: "full_replace".to_owned(),
            input: small.text.clone(),
            steps: vec![vec![full_change("# Replaced\n\nAll new text.\n")]],
            iterations: default_iterations,
            expect: Expect::declines(),
        },
        BenchCase {
            id: "typing_stream_medium".to_owned(),
            steps: typing_stream(medium.line(200), ALPHA.0, "incrementally "),
            input: medium.text.clone(),
            iterations: default_iterations / 2,
            // The workload the feature exists for: every keystroke must splice.
            expect: Expect::reuses().min_speedup(2.0),
        },
        // The window-size cutoff, one case per side. The same document and the
        // same single-word edit; only how far into the document it lands
        // differs, and with it the share of the document left downstream of the
        // window. `accepted` sits at ~80%, `declined` at ~88%, bracketing the
        // 85% threshold. The pair is what stops a threshold change from being
        // invisible: move it and exactly one of these two flips its fallback
        // rate between 0% and 100%.
        BenchCase {
            id: "window_cutoff_accepted".to_owned(),
            steps: vec![vec![word_change(medium.line(50), ALPHA, "ALPHA")]],
            input: medium.text.clone(),
            iterations: default_iterations / 2,
            expect: Expect::reuses(),
        },
        BenchCase {
            id: "window_cutoff_declined".to_owned(),
            steps: vec![vec![word_change(medium.line(30), ALPHA, "ALPHA")]],
            input: medium.text.clone(),
            iterations: default_iterations / 2,
            expect: Expect::declines(),
        },
        BenchCase {
            id: "bail_refdef_edit".to_owned(),
            // Appended to the first definition's URL: the label set is
            // unchanged, so the host admits the attempt and the parser's `]:`
            // proximity guard is what declines.
            steps: vec![vec![insert_change(REFDEF_LINE, REFDEF_URL_END, "/deep")]],
            input: refdefs,
            iterations: default_iterations,
            expect: Expect::declines(),
        },
    ]
}

fn real_document_cases(default_iterations: usize) -> Vec<BenchCase> {
    let mut cases = Vec::new();

    if let Some(doc) = load_document("pandoc_manual.md") {
        let iterations = (default_iterations / 16).max(3);
        // Prose deep inside a definition-list item, a third of the way in:
        // the worst shape for a suffix window at scale, since almost the whole
        // document is still downstream of the seam.
        cases.push(BenchCase {
            id: "pandoc_manual_early_edit".to_owned(),
            input: doc.clone(),
            steps: vec![vec![range_change(292, 4, 292, 13, "APPENDING")]],
            iterations,
            expect: Expect::declines(),
        });
        // Line 200 is `[`setspace`]: ...`, and the replacement rewrites the
        // *label*. The host's set comparison declines before the parser is
        // called at all, so this prices the cheapest decline there is: one
        // refdef scan, then the full parse that would have happened anyway.
        cases.push(BenchCase {
            id: "pandoc_manual_refdef_label_edit".to_owned(),
            input: doc.clone(),
            steps: vec![vec![range_change(200, 5, 200, 10, "manual")]],
            iterations,
            expect: Expect::declines(),
        });
        cases.push(BenchCase {
            id: "pandoc_manual_late_edit".to_owned(),
            input: doc.clone(),
            steps: vec![vec![insert_change(7600, 0, "NOTE: ")]],
            iterations,
            expect: Expect::reuses().min_speedup(5.0),
        });
        cases.push(BenchCase {
            id: "pandoc_manual_typing_stream".to_owned(),
            input: doc,
            steps: typing_stream(7600, 0, "NOTE: typing"),
            iterations,
            expect: Expect::reuses().min_speedup(5.0),
        });
    }

    let smaller: [(&str, &str, u32, u32, u32, u32, &str); 3] = [
        (
            "large_authoring_single_edit",
            "large_authoring.qmd",
            60,
            4,
            60,
            10,
            "AUTHORING",
        ),
        ("tables_single_edit", "tables.qmd", 40, 4, 40, 8, "TABLES"),
        ("math_single_edit", "math.qmd", 25, 3, 25, 8, "MATH"),
    ];

    // All three edit near the top of their document, so the nearest top-level
    // heading is close to byte 0 and the window the section strategy would
    // choose is over the cutoff. The region tier (roadmap Phase 8) is what
    // turns these into reuse; until then they are the shape that pays one
    // sub-microsecond decline and nothing else.
    for (id, file, sl, sc, el, ec, replacement) in smaller {
        if let Some(doc) = load_document(file) {
            cases.push(BenchCase {
                id: id.to_owned(),
                input: doc,
                steps: vec![vec![range_change(sl, sc, el, ec, replacement)]],
                iterations: (default_iterations / 2).max(8),
                expect: Expect::declines(),
            });
        }
    }

    cases
}

fn print_case(result: &CaseResult) {
    println!("\nCase: {}", result.id);
    println!("  Document size: {} bytes", result.document_size_bytes);
    println!(
        "  Steps: {} ({} content changes total)",
        result.steps, result.changes_total
    );
    println!("  Iterations: {}", result.iterations);
    println!(
        "  Full parse per step mean/median/p95: {:.2} / {:.2} / {:.2} us",
        result.full_parse.mean_us, result.full_parse.median_us, result.full_parse.p95_us
    );
    println!(
        "  Incremental per step mean/median/p95: {:.2} / {:.2} / {:.2} us",
        result.incremental.mean_us, result.incremental.median_us, result.incremental.p95_us
    );
    println!("  Speedup vs full: {:.2}x", result.speedup_vs_full);
    println!("  Fallback rate: {:.2}%", result.fallback_rate * 100.0);
    match (result.bail_cost_us, result.bail_cost_ratio) {
        (Some(us), Some(ratio)) => println!(
            "  Bail cost: {us:.2} us ({:.2}% of a full parse)",
            ratio * 100.0
        ),
        _ => println!("  Bail cost: n/a (no step reached the guard cascade and declined)"),
    }
    match (result.window_ratio, result.spliced_ratio) {
        (Some(window), Some(spliced)) => println!(
            "  Reused steps re-parsed {:.2}% of the document, spliced {:.2}%",
            window * 100.0,
            spliced * 100.0
        ),
        _ => println!("  Reused steps: none"),
    }
    if !result.strategy_counts.is_empty() {
        println!("  Strategies: {:?}", result.strategy_counts);
    }
    if !result.fallback_reasons.is_empty() {
        println!("  Fallback reasons: {:?}", result.fallback_reasons);
    }
}

/// The compact table the module doc carries. Printed last so a bench run can
/// be pasted straight into it.
fn print_summary(results: &[CaseResult]) {
    println!("\nSummary");
    println!("=======");
    println!(
        "{:<32} {:>7} {:>6} {:>7} {:>7} {:>8} {:>9} {:>7} {:>8}",
        "case", "bytes", "steps", "full", "incr", "speedup", "fallback", "bail%", "window%"
    );
    for result in results {
        let bail = match result.bail_cost_ratio {
            Some(ratio) => format!("{:.1}%", ratio * 100.0),
            None => "-".to_owned(),
        };
        let window = match result.window_ratio {
            Some(ratio) => format!("{:.1}%", ratio * 100.0),
            None => "-".to_owned(),
        };
        println!(
            "{:<32} {:>7} {:>6} {:>7.1} {:>7.1} {:>7.1}x {:>8.1}% {:>7} {:>8}",
            result.id,
            result.document_size_bytes,
            result.steps,
            result.full_parse.mean_us,
            result.incremental.mean_us,
            result.speedup_vs_full,
            result.fallback_rate * 100.0,
            bail,
            window
        );
    }
}

/// Check every case against the contract it declared, printing each check with
/// its margin so drift is visible well before it becomes a failure.
///
/// Returns the failures rather than panicking on the first, so one run tells
/// you everything that moved.
fn check_expectations(expectations: &[(String, Expect)], results: &[CaseResult]) -> Vec<String> {
    let mut failures = Vec::new();

    println!("\nThresholds");
    println!("==========");

    for (id, expect) in expectations {
        let Some(result) = results.iter().find(|result| &result.id == id) else {
            let failure = format!("{id}: declared but never ran");
            println!("  FAIL {id:<32} {failure}");
            failures.push(failure);
            continue;
        };

        let overhead_us = result.incremental.mean_us - result.full_parse.mean_us;
        let mut checks: Vec<(bool, String)> = Vec::new();

        // The reuse contract. Exact comparisons are deliberate: the rate is
        // declined steps over total steps, so "every step" and "no step" are
        // exactly 1.0 and 0.0, and one degraded keystroke in a stream of
        // fourteen must fail rather than round away.
        checks.push(match expect.reuse {
            Reuse::Always => (
                result.fallback_rate == 0.0,
                format!(
                    "splices every step (fallback {:.1}%)",
                    result.fallback_rate * 100.0
                ),
            ),
            Reuse::Never => (
                result.fallback_rate == 1.0,
                format!(
                    "declines every step (fallback {:.1}%)",
                    result.fallback_rate * 100.0
                ),
            ),
        });

        if let Some(min) = expect.min_speedup {
            checks.push((
                result.speedup_vs_full >= min,
                format!("speedup {:.2}x >= {min:.2}x", result.speedup_vs_full),
            ));
        }

        let (ceiling, why) = match expect.ceiling {
            Some((ceiling, reason)) => (ceiling, format!(" [{reason}]")),
            None => (MIN_SPEEDUP_CEILING, String::new()),
        };
        checks.push((
            result.speedup_vs_full >= ceiling || overhead_us <= MAX_ABSOLUTE_OVERHEAD_US,
            format!(
                "no regression: {:.2}x >= {ceiling:.2}x or {overhead_us:+.1} us <= {MAX_ABSOLUTE_OVERHEAD_US:.0} us{why}",
                result.speedup_vs_full
            ),
        ));

        if let (Some(ratio), Some(us)) = (result.bail_cost_ratio, result.bail_cost_us) {
            checks.push((
                ratio <= MAX_BAIL_RATIO || us <= MAX_ABSOLUTE_OVERHEAD_US,
                format!(
                    "bail {:.1}% <= {:.0}% or {us:.1} us <= {MAX_ABSOLUTE_OVERHEAD_US:.0} us",
                    ratio * 100.0,
                    MAX_BAIL_RATIO * 100.0
                ),
            ));
        }

        for (passed, description) in checks {
            println!(
                "  {} {id:<32} {description}",
                if passed { "ok  " } else { "FAIL" }
            );
            if !passed {
                failures.push(format!("{id}: {description}"));
            }
        }
    }

    failures
}

/// Fail before measuring anything if a document the gate depends on is absent.
fn check_required_documents() -> Vec<String> {
    REQUIRED_DOCUMENTS
        .iter()
        .filter(|name| !Path::new("benches/documents").join(name).is_file())
        .map(|name| format!("benches/documents/{name} is missing"))
        .collect()
}

fn main() {
    let config = Config::default();
    let default_iterations = env::var("PANACHE_LSP_BENCH_ITERATIONS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(80);

    // The gate. Off by default, so a run that only wants the numbers stays a
    // measurement and never fails the shell it was typed into.
    let assert_mode = matches!(
        env::var("PANACHE_LSP_BENCH_ASSERT").as_deref(),
        Ok("1") | Ok("true")
    );

    if assert_mode {
        let missing = check_required_documents();
        if !missing.is_empty() {
            eprintln!("PANACHE_LSP_BENCH_ASSERT=1 needs the real-document corpus:");
            for entry in &missing {
                eprintln!("  {entry}");
            }
            eprintln!("Run `benches/documents/download.sh` (or `task bench:incremental-gate`).");
            std::process::exit(1);
        }
    }

    let mut cases = synthetic_cases(default_iterations);
    cases.extend(real_document_cases(default_iterations));

    let expectations: Vec<(String, Expect)> = cases
        .iter()
        .map(|case| (case.id.clone(), case.expect))
        .collect();

    println!("LSP Incremental Benchmarks");
    println!("==========================");

    let mut results = Vec::new();
    for case in cases {
        let result = run_case(
            &case.id,
            &case.input,
            &case.steps,
            case.iterations.max(1),
            &config,
        );
        print_case(&result);
        results.push(result);
    }

    print_summary(&results);

    let failures = if assert_mode {
        check_expectations(&expectations, &results)
    } else {
        Vec::new()
    };

    // Written before the verdict: a failing gate is exactly when the numbers
    // are worth keeping.
    if let Ok(path) = env::var("PANACHE_LSP_BENCH_OUTPUT_JSON") {
        let report = BenchmarkReport {
            schema_version: 3,
            results,
        };
        let json = serde_json::to_string_pretty(&report)
            .expect("failed to serialize LSP benchmark JSON report");
        fs::write(&path, json)
            .unwrap_or_else(|e| panic!("failed to write benchmark JSON report to '{path}': {e}"));
        println!("\nWrote JSON report to {}", path);
    }

    if !failures.is_empty() {
        eprintln!("\n{} threshold(s) failed:", failures.len());
        for failure in &failures {
            eprintln!("  {failure}");
        }
        std::process::exit(1);
    }
}
