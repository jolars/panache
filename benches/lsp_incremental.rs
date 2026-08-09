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
//! single_change_small                 1620      1    45.1    33.9     1.3x      0.0%       -    59.4%
//! multi_change_small_4                1620      1    42.9    43.7     1.0x      0.0%       -    78.5%
//! multi_change_medium_4              15922      1   399.9   371.2     1.1x      0.0%       -    75.8%
//! multi_change_medium_clustered_4    15922      1   415.1   134.9     3.1x      0.0%       -    16.0%
//! multi_change_large_8               76542      1  1912.5  2016.3     0.9x    100.0%    0.0%        -
//! multi_change_utf16_4                  74      1     3.9     5.7     0.7x    100.0%   43.0%        -
//! full_replace                        1620      1     2.2     2.4     0.9x    100.0%    4.2%        -
//! typing_stream_medium               15922     14   407.8   146.2     2.8x      0.0%       -    20.0%
//! window_cutoff_accepted             15922      1   405.4   369.8     1.1x      0.0%       -    79.9%
//! window_cutoff_declined             15922      1   408.5   405.9     1.0x    100.0%    0.0%        -
//! bail_refdef_edit                    2687      1    96.4   113.0     0.9x    100.0%   15.5%        -
//! pandoc_manual_early_edit          300856      1 10396.6 10431.1     1.0x    100.0%    0.0%        -
//! pandoc_manual_refdef_label_edit   300856      1 10880.1 10517.4     1.0x    100.0%       -        -
//! pandoc_manual_late_edit           300856      1 10590.3  1929.0     5.5x      0.0%       -     7.0%
//! pandoc_manual_typing_stream       300856     12 10623.0  1951.6     5.4x      0.0%       -     7.0%
//! large_authoring_single_edit        29858      1   649.5   640.1     1.0x    100.0%    0.1%        -
//! tables_single_edit                 25101      1   758.5   744.0     1.0x    100.0%    0.0%        -
//! math_single_edit                   30112      1   553.7   551.5     1.0x    100.0%    0.1%        -
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
//!   microsecond (`bail%` 0.0% and 4.2%) and returns them to 1.0x and 0.9x.
//! * **`window_cutoff_accepted` and `window_cutoff_declined` bracket the
//!   threshold**: the same document and the same one-word edit, at a 79.9% and
//!   an 87.8% window. Move `MAX_WINDOW_SHARE_PERCENT` and exactly one of them
//!   flips its fallback rate between 0% and 100%.
//! * **Clustering matters; change count does not.** The two medium
//!   multi-change cases carry the same four changes. Scattering them over 150
//!   lines takes `diff_edit`'s span from one line to most of the document: 16%
//!   window to 76%, 3.1x to 1.1x.
//! * **The typing streams are the workload the feature exists for**, and they
//!   are where it pays: 2.8x on a 16 KB document, 5.4x on the 300 KB pandoc
//!   manual, with no step declining. Each stream agrees with its equivalent
//!   single edit (`pandoc_manual_late_edit`) to within noise, which is the
//!   evidence that chaining the base does not degrade across keystrokes.
//! * **Bail cost is small, and the fallback rate is what governs it.** A
//!   cutoff decline is under a microsecond even at 300 KB, because it is
//!   arithmetic on the edit offset. The correctness guards cost more: the
//!   parser's `]:`-proximity cascade prices at 15.5% of a full parse
//!   (`bail_refdef_edit`), and a host-side decline
//!   (`pandoc_manual_refdef_label_edit`, whose edit rewrites a refdef *label*)
//!   costs one extra refdef scan, inside the noise at 300 KB. Both land under
//!   the 20%-of-a-full-parse budget the default flip is gated on.
//!
//! ## The three cases still under 1.0x
//!
//! None of them is the wide-window surcharge the cutoff removed, and none is
//! reachable by tuning its threshold:
//!
//! * `bail_refdef_edit` (0.9x) exists to price a decline. A decline is a full
//!   parse plus the cascade that reached it, so it is *definitionally* slower;
//!   the number to read on this case is `bail%`, not the speedup.
//! * `multi_change_utf16_4` (0.7x) is 74 bytes, and an attempt has a fixed cost
//!   --- cloning the options, materializing a cursor root over the previous
//!   green tree, walking it for the window --- that is 1.8 us against a 3.9 us
//!   whole parse. It lost at 0.8x before the cutoff too, while successfully
//!   splicing. No window threshold fixes a fixed cost; roadmap Phase 7's token
//!   tier is what removes it. A document-size floor would, but it would also
//!   refuse the small documents with *narrow* windows that do win
//!   (`single_change_small`, 1.6 KB, 1.3x).
//! * `multi_change_large_8` (0.9-1.0x, run to run) declines in under a
//!   microsecond --- `bail%` pins the parser's share --- and still costs ~100 us
//!   more than a full parse of the same 76 KB. That residual is host-side
//!   per-step work the parser never sees: the whole-text `diff_edit` and the
//!   67 KB `insert` it allocates for an edit spanning most of the document, the
//!   refdef-set clone, the base text copy. It is unattributed between those and
//!   wants a profile, not a threshold.
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

struct BenchCase {
    id: String,
    input: String,
    steps: Vec<Step>,
    iterations: usize,
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

fn scan_refdefs(text: &str, config: &Config) -> RefdefMap {
    collect_refdef_labels(text, Dialect::for_flavor(config.flavor))
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
fn full_step(text: &str, config: &Config) -> (Duration, rowan::GreenNode, Vec<SyntaxError>) {
    let start = Instant::now();
    let refdefs = scan_refdefs(text, config);
    let (green, errors) = full_parse(text, config, refdefs);
    (start.elapsed(), green, errors)
}

fn fresh_base(text: &str, config: &Config) -> ReparseBase {
    let refdefs = scan_refdefs(text, config);
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
    let refdefs = scan_refdefs(new_text, config);

    // The host's exact set comparison runs ahead of the parser's textual
    // guard: retained blocks keep the reference resolution they were parsed
    // with, so a changed set invalidates them at a distance.
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
        let (expected, expected_errors) = full_parse(text, config, scan_refdefs(text, config));
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
        for text in &texts {
            black_box(full_step(text, config));
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
        for text in &texts {
            let (elapsed, green, errors) = full_step(text, config);
            black_box((green, errors));
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
        },
        BenchCase {
            id: "full_replace".to_owned(),
            input: small.text.clone(),
            steps: vec![vec![full_change("# Replaced\n\nAll new text.\n")]],
            iterations: default_iterations,
        },
        BenchCase {
            id: "typing_stream_medium".to_owned(),
            steps: typing_stream(medium.line(200), ALPHA.0, "incrementally "),
            input: medium.text.clone(),
            iterations: default_iterations / 2,
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
        },
        BenchCase {
            id: "window_cutoff_declined".to_owned(),
            steps: vec![vec![word_change(medium.line(30), ALPHA, "ALPHA")]],
            input: medium.text.clone(),
            iterations: default_iterations / 2,
        },
        BenchCase {
            id: "bail_refdef_edit".to_owned(),
            // Appended to the first definition's URL: the label set is
            // unchanged, so the host admits the attempt and the parser's `]:`
            // proximity guard is what declines.
            steps: vec![vec![insert_change(REFDEF_LINE, REFDEF_URL_END, "/deep")]],
            input: refdefs,
            iterations: default_iterations,
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
        });
        cases.push(BenchCase {
            id: "pandoc_manual_late_edit".to_owned(),
            input: doc.clone(),
            steps: vec![vec![insert_change(7600, 0, "NOTE: ")]],
            iterations,
        });
        cases.push(BenchCase {
            id: "pandoc_manual_typing_stream".to_owned(),
            input: doc,
            steps: typing_stream(7600, 0, "NOTE: typing"),
            iterations,
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

    for (id, file, sl, sc, el, ec, replacement) in smaller {
        if let Some(doc) = load_document(file) {
            cases.push(BenchCase {
                id: id.to_owned(),
                input: doc,
                steps: vec![vec![range_change(sl, sc, el, ec, replacement)]],
                iterations: (default_iterations / 2).max(8),
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

fn main() {
    let config = Config::default();
    let default_iterations = env::var("PANACHE_LSP_BENCH_ITERATIONS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(80);

    let mut cases = synthetic_cases(default_iterations);
    cases.extend(real_document_cases(default_iterations));

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
}
