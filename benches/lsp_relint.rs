//! Measurement harness for the "re-lint all open documents per quiescent settle"
//! LSP model (`TODO.md` rust-analyzer divergence).
//!
//! The question this bench answers: is re-linting **every** open document on each
//! debounce settle cheap enough to replace the current per-document model (lint
//! the changed doc + its dependents)? The hypothesis is that re-linting an
//! *unchanged* document is mostly a `built_in_lint_plan` salsa memo hit, so the
//! per-settle cost stays negligible.
//!
//! Two measurements per scenario cell, both over a single snapshot:
//!   * **A (candidate):** `relint_all_open_documents()` — lint all N open docs.
//!   * **B (baseline):**  `relint_with_dependents(dirty)` — what one `didChange`
//!     costs today (the changed doc plus its project-graph dependents).
//!
//! and two memo states:
//!   * **warm:** all memos primed (nothing changed since the last pass).
//!   * **dirty:** one document edited just before the timed call (the realistic
//!     "one changed, the rest memo-hit" case — the actual hypothesis).
//!
//! The headline number is `A_dirty / B_dirty` at each N: if it grows
//! sub-linearly the memo dominates (GO); if it tracks N every doc pays full
//! freight (NO-GO / needs mitigation).
//!
//! Run: `cargo bench --bench lsp_relint` (honors `PANACHE_RELINT_BENCH_ITERS`).

use std::env;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use lsp_types::TextDocumentContentChangeEvent;
use panache::lsp::LspTester;

/// Open-document counts to sweep. 100 deliberately exceeds the `built_in_lint_plan`
/// LRU (64) so we see whether eviction, not memo cost, becomes the bottleneck.
const DOC_COUNTS: [usize; 7] = [1, 10, 32, 50, 64, 80, 100];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// N independent documents, no cross-document edges. Worst case for the
    /// all-docs model: a single `didChange` lints 1 doc today, but the candidate
    /// model lints all N.
    Independent,
    /// One shared child included by every other document. The current per-doc
    /// model already lints the child + all dependents, so the two models
    /// converge here.
    SharedInclude,
}

impl Shape {
    fn label(self) -> &'static str {
        match self {
            Shape::Independent => "independent",
            Shape::SharedInclude => "shared-include",
        }
    }
}

#[derive(Clone, Copy)]
struct DocSize {
    label: &'static str,
    filler_lines: usize,
}

const SMALL: DocSize = DocSize {
    label: "small",
    filler_lines: 45,
};
const LARGE: DocSize = DocSize {
    label: "large",
    filler_lines: 1995,
};

struct Stats {
    median_us: f64,
    mean_us: f64,
    p95_us: f64,
}

fn summarize(samples: &[Duration]) -> Stats {
    let mut us: Vec<f64> = samples
        .iter()
        .map(|d| d.as_nanos() as f64 / 1000.0)
        .collect();
    us.sort_by(f64::total_cmp);
    let len = us.len();
    let median = if len == 0 {
        0.0
    } else if len.is_multiple_of(2) {
        (us[len / 2 - 1] + us[len / 2]) / 2.0
    } else {
        us[len / 2]
    };
    let p95_idx = ((len as f64 - 1.0) * 0.95).round() as usize;
    let p95 = us.get(p95_idx).copied().unwrap_or(0.0);
    let mean = if len == 0 {
        0.0
    } else {
        us.iter().sum::<f64>() / len as f64
    };
    Stats {
        median_us: median,
        mean_us: mean,
        p95_us: p95,
    }
}

/// A document body with a heading-hierarchy violation (h1 → h3) so every doc
/// yields at least one diagnostic, exercising the un-memoized `convert_diagnostic`
/// path. `tag` lets callers vary the text to force a real salsa invalidation.
fn doc_body(size: DocSize, include_child: Option<&str>, tag: usize) -> String {
    let mut out = String::with_capacity(size.filler_lines * 48 + 128);
    out.push_str("# Title\n\n### Skipped heading\n\n");
    if let Some(child) = include_child {
        out.push_str(&format!("{{{{< include {child} >}}}}\n\n"));
    }
    for i in 0..size.filler_lines {
        out.push_str(&format!(
            "Paragraph {i:04}-{tag} alpha beta gamma delta epsilon zeta eta.\n"
        ));
    }
    out
}

fn file_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

fn full_change(text: String) -> Vec<TextDocumentContentChangeEvent> {
    vec![TextDocumentContentChangeEvent {
        range: None,
        range_length: None,
        text,
    }]
}

/// One scenario cell: a temp dir of fixtures, an initialized server with every
/// doc open and lint memos primed, and the URI we dirty for the "dirty" runs.
struct Fixture {
    _dir: PathBuf,
    tester: LspTester,
    uris: Vec<String>,
    dirty_uri: String,
    dirty_size: DocSize,
}

fn build_fixture(shape: Shape, size: DocSize, n: usize) -> Fixture {
    let dir = env::temp_dir().join(format!(
        "panache_relint_{}_{}_{}_{}",
        shape.label(),
        size.label,
        n,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp fixture dir");
    fs::write(
        dir.join("panache.toml"),
        "flavor = \"quarto\"\ncache = false\n",
    )
    .expect("write panache.toml");

    let mut tester = LspTester::new();
    tester.initialize(&file_uri(&dir));

    let mut uris = Vec::with_capacity(n + 1);
    let include_child = matches!(shape, Shape::SharedInclude);
    let child_name = "_shared.qmd";

    if include_child {
        // The shared child every document includes; editing it is the realistic
        // "one change fans out to N dependents" case.
        let child_path = dir.join(child_name);
        fs::write(&child_path, doc_body(size, None, 0)).expect("write child");
        let child_uri = file_uri(&child_path);
        tester.open_document(&child_uri, &doc_body(size, None, 0), "quarto");
        uris.push(child_uri);
    }

    for i in 0..n {
        let path = dir.join(format!("doc{i:04}.qmd"));
        let child_ref = include_child.then_some(child_name);
        let body = doc_body(size, child_ref, 0);
        fs::write(&path, &body).expect("write doc");
        let uri = file_uri(&path);
        tester.open_document(&uri, &body, "quarto");
        uris.push(uri);
    }

    // Settle once: runs the dispatch write phase (loads referenced files for the
    // include graph) and primes every `built_in_lint_plan` memo.
    tester.pump(Duration::from_secs(30));

    // The doc we dirty for the "dirty" runs is `uris[0]`: in the shared-include
    // shape that is the child (editing it fans out to every parent dependent —
    // the realistic shared-content edit); in the independent shape it is the
    // first doc. Either way `uris[0]` carries no include directive of its own.
    let dirty_uri = uris[0].clone();

    Fixture {
        _dir: dir,
        tester,
        uris,
        dirty_uri,
        dirty_size: size,
    }
}

/// Re-write the dirty document with a fresh `tag` so salsa actually invalidates
/// its `FileText` (setting identical text is a no-op). Untimed.
fn dirty_one(fx: &mut Fixture, tag: usize) {
    let body = doc_body(fx.dirty_size, None, tag);
    fx.tester.edit_document(&fx.dirty_uri, full_change(body));
}

fn bench_cell(shape: Shape, size: DocSize, n: usize, iters: usize) {
    let mut fx = build_fixture(shape, size, n);
    let open_docs = fx.uris.len();

    // Warmups (untimed).
    for _ in 0..3 {
        black_box(fx.tester.relint_all_open_documents());
        black_box(fx.tester.relint_with_dependents(&fx.dirty_uri));
    }

    // A_warm: all memos primed, re-lint everything.
    let mut a_warm = Vec::with_capacity(iters);
    for _ in 0..iters {
        let start = Instant::now();
        black_box(fx.tester.relint_all_open_documents());
        a_warm.push(start.elapsed());
    }

    // A_dirty: one doc edited (untimed) before each timed all-docs re-lint.
    let mut a_dirty = Vec::with_capacity(iters);
    for it in 0..iters {
        dirty_one(&mut fx, it + 1);
        let start = Instant::now();
        black_box(fx.tester.relint_all_open_documents());
        a_dirty.push(start.elapsed());
    }

    // B_dirty: baseline — one doc edited, then lint it + dependents only.
    let mut b_dirty = Vec::with_capacity(iters);
    for it in 0..iters {
        dirty_one(&mut fx, iters + it + 1);
        let start = Instant::now();
        black_box(fx.tester.relint_with_dependents(&fx.dirty_uri));
        b_dirty.push(start.elapsed());
    }

    let aw = summarize(&a_warm);
    let ad = summarize(&a_dirty);
    let bd = summarize(&b_dirty);
    let ratio = if bd.median_us > 0.0 {
        ad.median_us / bd.median_us
    } else {
        0.0
    };

    println!(
        "{:<15} {:<6} N={:<4} open={:<4} | A_warm {:>9.1} | A_dirty {:>9.1} (p95 {:>9.1}) | B_dirty {:>9.1} | A/B {:>6.2}",
        shape.label(),
        size.label,
        n,
        open_docs,
        aw.median_us,
        ad.median_us,
        ad.p95_us,
        bd.median_us,
        ratio,
    );
    let _ = (aw.mean_us, ad.mean_us, bd.mean_us, bd.p95_us);
}

fn main() {
    let iters = env::var("PANACHE_RELINT_BENCH_ITERS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(60);

    println!("LSP re-lint-all-open-docs measurement (median microseconds)");
    println!("============================================================");
    println!(
        "A = relint all open docs (candidate per-settle cost); B = one doc + dependents (today)\n"
    );

    for shape in [Shape::Independent, Shape::SharedInclude] {
        for size in [SMALL, LARGE] {
            for n in DOC_COUNTS {
                // Cut iterations for the heaviest cells to keep runtime sane.
                let cell_iters = if n >= 100 || size.filler_lines > 1000 {
                    (iters / 3).max(8)
                } else {
                    iters
                };
                bench_cell(shape, size, n, cell_iters);
            }
            println!();
        }
    }
}
