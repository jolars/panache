# Panache performance measurements

For parser work, use the `panache-parser` crate's `profile_parse` example
with `pandoc/MANUAL.txt` or a file that reproduces the reported behavior.
For formatter internals, select the input with `PANACHE_BENCH_DOC` and the
repeat count with `PANACHE_BENCH_ITERATIONS`. Use
`benches/documents/download.sh` if you need the pinned corpus.

```sh
CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release \
  --example profile_parse -p panache-parser
./target/release/examples/profile_parse pandoc/MANUAL.txt 200
PANACHE_BENCH_DOC=pandoc_manual.md PANACHE_BENCH_ITERATIONS=3 \
  cargo bench --bench formatting
```

For end-to-end formatting, build the release CLI and run it with the relevant
file and configuration. The devenv shell provides Hyperfine. Supply a shell
when timing a command with redirection, for example:

```sh
hyperfine --shell=default --warmup 3 \
  './target/release/panache format < pandoc/MANUAL.txt > /dev/null'
```

## Historical hotspot notes

These results describe prior inputs and builds. Re-measure on the current
revision before applying the same changes.

### Parser and formatter examples

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

## Experimental block planner

The `experimental-block-parser` feature exposes a second parser to tests and
the comparison example. The planner records block ranges and container
decisions before CST emission. It uses Panache's inline, YAML, raw HTML, and
pipe table emitters. The comparison checks source round-trips, green trees, and
diagnostics before timing either engine.

```sh
cargo build --release --features experimental-block-parser \
  --example compare_block_parsers
target/release/examples/compare_block_parsers --flavor commonmark \
  README.md benches/documents/pandoc_manual.md crates/panache-parser/README.md
target/release/examples/compare_block_parsers --flavor quarto \
  docs/guide/integrations.qmd docs/getting-started.qmd docs/guide/performance.qmd \
  benches/documents/{large_authoring,tables,math}.qmd
cargo test -p panache-parser --features experimental-block-parser \
  --test commonmark experimental_block_coverage -- --nocapture
```

The example resolves Panache project settings for each file. Run the full
Julia guide from the pinned benchmark corpus with the checked-in config.

```sh
target/release/examples/compare_block_parsers \
  --config benches/documents/block-parser.toml benches/documents/math.qmd
```

This config disables grid tables. The guide still exercises YAML metadata,
fenced divs, pipe tables, and executable chunks. The example checks both
engines against the same source, CST, and diagnostics for the original
document and each edit before collecting timings.

In one release run on the 30,112-byte guide, the full-parse median was
565 µs for the incumbent and 359 µs for the planner. The p95 was 613 µs
and 434 µs, respectively. The two region edits had medians of 221 µs and
96 µs on the incumbent and 214 µs and 91 µs on the planner. Both engines
submitted 1,940 and 1,255 fragment bytes for those edits.

On Linux with an AMD Ryzen 9 7900, using Rust 1.98.1 and the workspace release
profile at base revision `817e7ae0`, three runs collected 200 samples per engine
and case after 16 warmups. The table gives the range of run medians in
microseconds. The edit column measures the first region splice in a chain that
also types inside the region and replaces the whole file. It measures parser
latency with cached reference labels. The example reports throughput, p95, bytes replaced, and bytes
submitted to fragment parsers, including seam checks.

| Document | Full parse, current | Full parse, prototype | Region edit, current | Region edit, prototype |
| --- | ---: | ---: | ---: | ---: |
| Project README | 214–304 | 120–168 | 139–247 | 120–207 |
| Parser README | 19–33 | 10–17 | 17–30 | 12–21 |
| Integrations | 168–198 | 104–126 | 93–102 | 81–89 |
| Getting Started | 272–278 | 148–154 | 106–110 | 97–101 |
| Performance | 1090–1311 | 910–1086 | 204–230 | 190–214 |

The CommonMark sweep found 561 examples with CST, source, and rendered HTML
parity, 90 explicit coverage gaps, and one source mismatch. Example 143
exercises fence info with multiple spaces before the language. The existing
fence emitter used by both engines changes those bytes. The pinned Pandoc
manual and the three downloaded Quarto documents did not qualify for timing
in the prototype run with flavor defaults.
Mixed quote and list nesting and several Pandoc block extensions remain outside
the tested slice. Both engines submitted the same fragment sizes for the
measured edits. For example, the Performance guide’s first edit replaced 387
bytes and submitted 1,643 bytes across the region and its seam checks.

The planner keeps paragraph promotion, list continuation, and fenced div
closing ahead of emission. Top-level paragraphs retain line ranges that the
inline emitter reads directly. Pipe table recognition records prepared rows
and caption bounds, so the prototype emits each table directly into the CST.
The incumbent dispatcher builds a separate green subtree during detection
and copies it into the CST. Lists and quotes still use separate continuation
rules, and the code and HTML emitters still scan their bounded inputs. The experiment demonstrates the cost of
emitting equivalent CSTs for the supported slice. How much control flow the
model can remove with broader dialect coverage remains open.

The production LSP write-phase gate passed. The parse benchmark's
`large_authoring_single_edit` case missed the 3.80× threshold on both the
clean baseline (3.65×) and the experimental branch (3.70×).
