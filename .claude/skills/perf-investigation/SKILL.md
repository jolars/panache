---
name: perf-investigation
description: Profile-driven performance work on the panache parser or
  formatter. Measure first with perf + the right harness; classify
  hotspots into one of a small set of buckets; apply the matching
  cheap fix; verify median wall-time moved before committing.
---

Use this skill when asked to "speed up parsing", "speed up formatting",
"look at the parser/formatter hotspots", "fix the regression after
<feature> landed", or anything else where the task is *measure
parser/formatter cost on a real input and recover wall-time*.

The buckets, workflow, and verification steps are shared across parser
and formatter; only the harness invocation and the hot-file map
differ. The "Harness" sections below have both — pick the one that
matches the target.

## Scope boundaries

- Verification is end-to-end test green + `commonmark_allowlist` green
  + clippy/fmt clean. Performance gains do not justify a snapshot diff
  or a regression in any test.
- The thread-local pool / scratch-bundle pattern in
  `inline_ir.rs::ScratchEvents` is the established shape for
  amortizing per-call allocations. Don't invent a new pattern; extend
  that one.
- Formatting must remain idempotent (`format(format(x)) == format(x)`).
  Any formatter perf change that touches emitter shape needs the
  golden-cases suite green.
- Parsing must remain CST-lossless. Any parser perf change that
  touches `builder.token()` / `builder.start_node()` shape needs the
  parser golden snapshots and the conformance allowlist green.

## Related rules to read first

- `.claude/rules/parser.md` — losslessness, dialect gating, no
  formatter policy in parser code, TEXT-coalescence-vs-structural
  rule.
- `.claude/rules/integration-tests.md` — where parser vs formatter
  goldens live (don't mix them).

## Harness noise to ignore inside this skill

The runtime occasionally injects a `system-reminder` nudging you to
use `TaskCreate` / `TaskUpdate`. The workflow below is linear
(baseline → profile → classify → fix → measure → commit → repeat), so
task tools add overhead without value. Skip them unless the user
explicitly asks.

## Harness — parser

Stress doc: `pandoc/MANUAL.txt` (~300 KB). Small docs hide per-line
dispatcher cost behind allocator noise.

```
CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release \
    --example profile_parse -p panache-parser
for i in $(seq 1 12); do
  taskset -c 0 ./target/release/examples/profile_parse \
      pandoc/MANUAL.txt 200 2>&1 | tail -1
done
```

`taskset -c 0` pins to one core — without it, scheduling jitter on a
hybrid-core CPU swamps small wins. Discard the first 2-3 warmup runs;
take median of the remaining ~9-10. Per-run variance on a warm machine
is ~3-5%; demand at least that big a delta before declaring a fix
worked.

## Harness — formatter

The repo's formatting bench is `cargo bench --bench formatting`. For
focused hotspot work, set `PANACHE_BENCH_DOC` to the doc you're
investigating and a low `PANACHE_BENCH_ITERATIONS`:

```
cd benches/documents && ./download.sh && cd ../..   # first time only
PANACHE_BENCH_DOC=pandoc_manual.md PANACHE_BENCH_ITERATIONS=3 \
    cargo bench --bench formatting

# Or end-to-end on a single doc via the CLI binary, with hyperfine if
# available (more honest than ad-hoc shell loops):
CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release
hyperfine --warmup 3 \
    'taskset -c 0 ./target/release/panache format \
         < pandoc/MANUAL.txt > /dev/null'
```

Same warmup-discard rule applies.

## Capture a perf profile

```
perf record --call-graph=dwarf -F 999 -o /tmp/panache_perf.data -- \
    ./target/release/examples/profile_parse pandoc/MANUAL.txt 400
perf report --stdio -i /tmp/panache_perf.data \
    --no-children -g none --percent-limit 1.0 | head -40
```

Always read **`cpu_core` samples**, not `cpu_atom` — on a hybrid-core
CPU `cpu_atom` typically captures only a handful of samples and
percentages there are essentially noise. Use `--no-children` for the
flat self-time view; use `-g graph,caller,…` (or `,callee,…`) when
you need to find who calls a hot leaf. For inline-frame visibility
add `--inline`.

For flame graphs, the repo already integrates `cargo flamegraph`:

```
PANACHE_BENCH_DOC=pandoc_manual.md PANACHE_BENCH_ITERATIONS=3 \
    cargo flamegraph --bench formatting
```

## Classify each hotspot

Every parser/formatter hotspot recovered so far falls into one of
these buckets. Identify which one BEFORE editing:

- **Slice-pattern trim** — `s.trim_*_matches([' ', '\t'])` or similar
  ASCII-set trims show up as `core::str::trim_matches` /
  `trim_start_matches` with `MultiCharEqSearcher` /
  `CharPredicateSearcher::next_reject` in the call stack. Replace
  with byte-level helpers from `parser/utils/helpers.rs`
  (`trim_end_newlines`, `trim_start_spaces_tabs`,
  `trim_end_spaces_tabs`, `is_blank_line`).
- **`.trim().is_empty()` on every line** — Unicode whitespace
  iterator for what is always ASCII. Use `is_blank_line(s)` instead.
- **Per-line block parser invoked without leading-byte gate** —
  `try_parse_*` runs on every non-blank line; allocates / scans
  before realizing the line can't possibly be the construct. Add a
  cheap byte gate: `bytes after up to 3 spaces` matches the expected
  leading byte. Examples that paid off (parser): `[` for ref-def +
  footnote-def, `<` for HTML block, `:` for fenced-div + def-marker,
  `=`/`-` for setext underline (next line). Skip when the existing
  inner check is already byte-cheap (`count_blockquote_markers`
  already has one).
- **Per-call `String` allocation on a no-match path** — a
  `try_parse_*` function that returns `Option<(String, …)>` allocates
  the string even when the caller's outer guard rejects it. Change
  the signature to return `Option<usize>` (or `Option<&str>`) and
  have the caller build the `String` only on confirmed match.
- **Per-iteration `Vec::new()` in a hot loop** — `.collect::<Vec<_>>()`
  inside an inner loop, or fresh `Vec` per call to a function that's
  invoked per range/paragraph/line. Either pool via the scratch
  bundle pattern in `inline_ir.rs::ScratchBundle` or hoist +
  `clear()` + `extend()` so capacity is reused across iterations.
- **Per-call malloc for a discardable builder** —
  `GreenNodeBuilder::new()` in `detect_prepared` to "try a parse and
  throw it away" allocates a fresh NodeCache each call. The right
  fix is splitting the parser function into a separate `validate_*`
  that doesn't emit, *not* pooling the discardable builder (the
  cache holds Arcs across parses and pooling it across the benchmark
  loop creates an unrealistic flatter — each iteration after the
  first hits a warm cache that wouldn't exist in real CLI usage).
- **char-walk where bytes would do** — code-span / list-marker-like
  scanners stepping `pos += rest[pos..].chars().next()?.len_utf8()`
  byte-by-byte through plain ASCII. Replace with `memchr`-style
  `bytes.iter().position(|&b| b == NEEDLE)` (the compiler emits
  vectorized memchr). All Pandoc / CommonMark structural bytes are
  ASCII, so byte-level scans are losslessness-safe.
- **`to_uppercase()` / `to_lowercase()` on ASCII** — Unicode
  case-folding allocates a fresh `String`. For ASCII-only checks
  (e.g. Roman numeral validation), case-fold a byte at a time via
  `b & !0x20`.
- **Formatter wrapping / line-builder churn** — formatter-specific
  hotspots tend to live in wrapping
  (`crates/panache-formatter/src/formatter/wrapping.rs`), inline
  emission (`inlines.rs`), and table layout (`tables.rs`). Common
  shapes: `String` allocation per inline span, repeated width
  recalculation, per-line `Vec<String>` for column widths. Same
  buckets as above, just different files.
- **rowan internals (NodeCache::token, Arc::drop_slow,
  reserve_rehash, ThinArc::from_header_and_iter)** — these dominate
  the residual ~12-15% on parser benchmarks. Proportional to
  `builder.token()` / `builder.start_node()` call count and to the
  size of the resulting green tree. Reducing them means emitting
  fewer tokens (e.g. coalescing a per-line `TEXT + NEWLINE` pair in
  raw / code blocks into one `TEXT` token where the formatter
  doesn't need the split). This is invasive — verify CST snapshots
  and the formatter round-trip before changing emitter shape. Don't
  try to pool the NodeCache across parses; it holds Arc'd green
  nodes (memory leak) and warming it across benchmark iterations
  creates a misleading result.

## Apply the smallest matching fix

Rules that have paid off:

- **Don't theorize before measuring.** Multiple "should-have-helped"
  changes (pre-sizing line-split vecs via an LF count; byte-gate on
  `BlockQuoteParser::detect_prepared`) regressed wall time and were
  reverted. The intuition wasn't wrong; the cost model was. Always
  measure.
- **Verify with measurement, not perf-only.** A change can drop a
  symbol from the perf top-25 without moving median wall time
  (sample relocation, not work elimination). The wall-time median is
  the truth.
- **One change per commit.** Prevents one regression from masking the
  win of another. Re-run the test suite + clippy + fmt + a fresh
  `taskset -c 0` measurement cycle for each.
- **Revert promptly.** If 12 runs after the change don't show a
  median shift larger than the baseline noise (~3-5%), the fix
  doesn't pay; revert and pick a different lever. Don't ship
  pretty-but-flat refactors as perf.

## Verify and commit

For every commit:

```
cargo test --workspace --no-fail-fast
cargo test -p panache-parser --test commonmark commonmark_allowlist
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt -- --check
```

For formatter changes also verify the golden-cases suite explicitly:

```
cargo test --test golden_cases
```

Then a fresh measurement (taskset, 12 runs, median). The commit
message should name the bucket and quote the median delta:

```
perf(parser|formatter): <bucket> on <call site>

<one-paragraph rationale: profile pointed here, what specifically>
<was wasteful, what the fix replaces it with>

Median wall time on `<harness command>` (12 runs):
~X ms → ~Y ms (~Z%).
```

Cite the wall-time number even when it's "in the noise" — that's the
honest record, and a reviewer can decide whether to ship a noise-floor
change at all.

## Key files — parser

- `crates/panache-parser/examples/profile_parse.rs` — the harness.
- `crates/panache-parser/src/parser/utils/helpers.rs` — byte-level
  trim / blank-line helpers; first place to look for an existing
  helper before adding a new one.
- `crates/panache-parser/src/parser/inlines/inline_ir.rs` —
  `ScratchEvents` / `ScratchBundle` thread-local pool pattern;
  `build_full_plans` for per-paragraph IR work.
- `crates/panache-parser/src/parser/block_dispatcher.rs` — every
  block parser's `detect_prepared` lives here; this is the hot
  per-line dispatch site.
- `crates/panache-parser/src/parser/inlines/refdef_map.rs` —
  document-wide refdef pre-pass, called once per parse.
- `pandoc/MANUAL.txt` — 300 KB stress doc.

## Key files — formatter

- `benches/formatting.rs` — the bench harness; respects
  `PANACHE_BENCH_DOC` and `PANACHE_BENCH_ITERATIONS`.
- `benches/documents/` — set of stress docs (`small`,
  `medium_quarto`, `tables`, `math`, `large_authoring`,
  `pandoc_manual.md`).
- `crates/panache-formatter/src/formatter/` — split by concern
  (`wrapping`, `inlines`, `paragraphs`, `headings`, `lists`,
  `tables`, …). Match the file to the construct your hotspot
  involves.
- `crates/panache-formatter/src/formatter.rs` — top-level
  orchestration.
- `tests/fixtures/cases/` — formatter goldens (`UPDATE_EXPECTED=1`
  to refresh, but verify diffs carefully — the formatter's
  idempotency invariant means a wrong refresh is a silent
  regression).

## Don't redo / known traps

- **`split_lines_inclusive` LF pre-count regressed parser wall time.**
  The extra pass over the input cost more than the resize-grow it
  saved. Don't try this again unless you change the data structure
  (e.g. thread-local pooled `Vec<&'static str>` via lifetime
  transmute) — and even then, prove the win with measurement first.
- **`BlockQuoteParser::detect_prepared` byte-gate was a noise-level
  regression.** `count_blockquote_markers` already has its own
  internal byte-cheap check; layering another gate on top added a
  tiny cost without saving meaningful work.
- **Don't pool the rowan `NodeCache` across parses.** Holds Arc'd
  green nodes (LSP memory leak) and produces misleading benchmark
  numbers (warm cache after iter 1).
- **Don't trust `cpu_atom` perf samples on a hybrid-core CPU.** Read
  `cpu_core` data; atom is too few samples to be reliable.
- **Don't add a too-permissive byte-gate.** It has no effect. The
  gate must match the construct's actual first-byte set after
  indent; verify with the full test suite, not just by reading the
  parser code.
- **Don't add `String::new()` in a `detect_prepared` before the
  gate.** `ReferenceDefinitionParser` was the canonical example —
  the multi-line `String::new() + push_str` ran on every line; the
  byte gate is what unlocked the 15% wall-time jump.

## Report-back format

When done, report:

1. Hotspot (function + approximate %) addressed.
2. Bucket (from §Classify each hotspot).
3. Median wall-time delta on the relevant harness (12 runs,
   `taskset -c 0`).
4. Test / clippy / fmt status (all green or specific exception).
5. What was tried but reverted (with reason).
6. Suggested next hotspot, ranked by likely shared root cause.
