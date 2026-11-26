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
decisions before CST emission. It uses Panache's inline, YAML, raw HTML,
and table emitters. The comparison checks source round-trips, green trees,
and diagnostics before timing either engine.

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

  | Document        | Full parse, current | Full parse, prototype | Region edit, current | Region edit, prototype |
  | ---             | ---:                | ---:                  | ---:                 | ---:                   |
  | Project README  |             214–304 |               120–168 |              139–247 |                120–207 |
  | Parser README   |               19–33 |                 10–17 |                17–30 |                  12–21 |
  | Integrations    |             168–198 |               104–126 |               93–102 |                  81–89 |
  | Getting Started |             272–278 |               148–154 |              106–110 |                 97–101 |
  | Performance     |           1090–1311 |              910–1086 |              204–230 |                190–214 |

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

The first mixed-container slice raised CommonMark parity to 590 examples.
The sweep reports 61 explicit gaps and the same source mismatch in example
143. It prints the example number, construct, and source byte offset for
each gap. The release comparison reaches the Pandoc manual's nested block
at byte 155207. With default Quarto options, grid tables are still the
first unsupported construct in the large authoring, tables, and math
benchmarks. The math benchmark passes preflight with the checked-in
configuration that disables grid tables.

### Shared list and blockquote planner checkpoint

On the AMD Ryzen 9 7900 with Rust 1.98.1, the release comparison reached
timing for all six workloads that qualified at base revision `c39990bf`.
The Pandoc manual also qualified under CommonMark. The table gives median
and p95 in microseconds for each operation, with incumbent timings on the
left of the arrow and experimental timings on the right.

  | Workload                       | Full parse                        | Line-start edit                   | Typing edit                  | Whole-file replacement |
  | ---                            | ---:                              | ---:                              | ---:                         | ---:                   |
  | Project README                 |     223.5 / 255.7 → 134.9 / 150.8 |     136.4 / 149.6 → 116.0 / 124.2 |        2.7 / 3.0 → 2.6 / 2.8 |  2.0 / 2.2 → 1.1 / 1.2 |
  | Pandoc manual                  | 5123.4 / 5254.0 → 2826.3 / 2920.9 | 1379.3 / 1414.4 → 1375.4 / 1404.2 |    43.6 / 48.7 → 44.3 / 52.2 |  3.7 / 5.2 → 2.9 / 3.4 |
  | Parser README                  |          17.7 / 18.3 → 9.9 / 10.2 |         16.1 / 16.4 → 11.1 / 11.4 |        9.5 / 9.7 → 6.3 / 6.5 |  1.5 / 1.5 → 0.7 / 0.8 |
  | Integrations                   |      158.8 / 164.7 → 97.1 / 102.8 |         90.2 / 97.5 → 77.7 / 83.6 |    41.6 / 45.8 → 35.0 / 38.6 |  2.5 / 2.6 → 1.1 / 1.2 |
  | Getting Started                |     260.1 / 276.7 → 149.3 / 160.4 |       102.5 / 109.3 → 93.4 / 99.9 |    45.9 / 50.6 → 41.4 / 47.2 |  2.3 / 2.4 → 1.1 / 1.2 |
  | Performance                    |   1055.6 / 1075.0 → 898.1 / 920.9 |     199.2 / 212.5 → 185.4 / 193.5 |  101.2 / 107.3 → 91.4 / 97.2 |  2.7 / 2.9 → 1.3 / 1.4 |
  | Math with grid tables disabled |     484.0 / 505.5 → 314.2 / 328.0 |     190.4 / 200.1 → 180.9 / 190.6 |    83.1 / 89.5 → 77.6 / 82.9 |  2.5 / 2.6 → 1.2 / 1.3 |

Across the six comparable workloads, the experimental full-parse medians
were 56–85% of the incumbent medians, versus 54–85% at the base revision.
The experimental line-start edit medians were 69–95% of the incumbent
medians, versus 72–95% at the base revision.

With default Quarto options, `large_authoring.qmd`, `tables.qmd`, and
`math.qmd` stopped at grid tables at bytes 990, 3539, and 28452,
respectively.

In release scaling checks, increasing repeated quoted pipe tables from
4,000 to 16,000 increased median parse time from 6.4 ms to 26.2 ms.
Increasing consecutive blank lines inside a list from 16,000 to 32,000
increased the median from 1.0 ms to 2.0 ms. These checks measured the
experimental parser over 15 samples per input.

### Definition list and footnote body checkpoint

The release comparison used the same AMD Ryzen 9 7900, Rust 1.98.1,
documents, and parser options as the baseline run at `7f4282f6`.
Seven workloads passed preflight. The table records median and p95 in
microseconds, with the incumbent on the left of each arrow and the planner
on the right.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| Project README | 217.9 / 312.6 → 132.5 / 184.3 | 139.6 / 153.7 → 118.8 / 138.5 | 2.7 / 3.2 → 2.6 / 3.0 | 2.1 / 2.3 → 1.1 / 1.4 |
| Pandoc manual | 5188.8 / 5786.2 → 2883.4 / 3314.3 | 1439.2 / 1595.4 → 1417.5 / 1616.8 | 45.2 / 50.9 → 44.5 / 51.2 | 5.9 / 9.9 → 3.9 / 7.3 |
| Parser README | 18.5 / 19.5 → 10.5 / 12.8 | 16.9 / 19.6 → 11.9 / 14.1 | 10.0 / 10.8 → 6.8 / 7.1 | 1.5 / 1.6 → 0.7 / 0.8 |
| Integrations | 167.5 / 190.3 → 103.0 / 118.0 | 95.2 / 112.8 → 82.1 / 94.2 | 43.5 / 49.6 → 37.1 / 42.3 | 2.5 / 3.4 → 1.2 / 1.4 |
| Getting Started | 272.7 / 358.7 → 158.4 / 221.2 | 109.6 / 134.4 → 98.5 / 115.7 | 49.2 / 62.4 → 43.6 / 50.1 | 2.4 / 3.6 → 1.2 / 1.4 |
| Performance | 1058.2 / 1270.8 → 909.9 / 1015.4 | 203.9 / 233.8 → 189.8 / 211.2 | 102.2 / 119.5 → 92.4 / 102.7 | 2.9 / 3.8 → 1.4 / 1.7 |
| Math with grid tables disabled | 500.6 / 570.5 → 330.4 / 458.1 | 203.0 / 292.3 → 193.6 / 245.6 | 88.3 / 123.1 → 83.6 / 113.3 | 2.6 / 4.0 → 1.3 / 1.8 |

Planner full-parse medians were 56–86% of the incumbent medians,
compared with 55–85% at the baseline. Line-start edit medians were
70–98% of the incumbent medians, compared with 70–101% at the
baseline. Both engines' p95 timings varied between runs when the host was busy.

With default Quarto options, `large_authoring.qmd`, `tables.qmd`, and
`math.qmd` stopped at grid tables at bytes 990, 3539, and 28452, respectively.

### Body boundary and table lookahead checkpoint

The release comparison used the same machine, documents, and options for
`319045e1` and the follow-up. All seven timed workloads passed preflight.
Each cell gives median and p95 in microseconds, with the incumbent on the
left and the planner on the right.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| Project README | 209.9 / 223.4 → 130.3 / 138.2 | 166.3 / 172.2 → 121.3 / 127.3 | 2.6 / 2.9 → 2.5 / 2.8 | 2.0 / 2.2 → 1.1 / 1.2 |
| Pandoc manual | 5035.0 / 5090.1 → 2824.2 / 2874.5 | 2155.9 / 2192.3 → 1433.6 / 1476.1 | 42.9 / 47.9 → 42.6 / 46.8 | 3.8 / 5.7 → 2.8 / 3.4 |
| Parser README | 18.0 / 18.4 → 10.5 / 10.8 | 19.9 / 20.4 → 11.8 / 12.6 | 9.7 / 9.9 → 6.7 / 6.9 | 1.5 / 1.6 → 0.8 / 0.9 |
| Integrations | 159.2 / 174.4 → 97.5 / 105.7 | 89.7 / 106.5 → 78.1 / 88.7 | 41.7 / 47.6 → 35.6 / 40.9 | 2.5 / 3.3 → 1.1 / 1.4 |
| Getting Started | 264.3 / 274.1 → 151.5 / 158.4 | 101.9 / 108.4 → 92.7 / 98.0 | 45.9 / 50.6 → 41.6 / 45.9 | 2.3 / 2.4 → 1.1 / 1.2 |
| Performance | 1026.4 / 1051.0 → 874.1 / 892.4 | 196.7 / 222.3 → 184.0 / 201.8 | 100.2 / 117.6 → 90.3 / 108.8 | 2.8 / 3.2 → 1.4 / 1.6 |
| Math with grid tables disabled | 483.9 / 502.9 → 319.4 / 329.3 | 190.9 / 207.3 → 182.0 / 196.5 | 84.4 / 93.3 → 78.2 / 85.8 | 2.4 / 2.7 → 1.2 / 1.3 |

Planner full-parse medians were 56–85% of the incumbent medians
after the follow-up and 56–86% at `319045e1`. For line-start edits
the ratios were 59–95% after the follow-up and 73–99% at the baseline.
With default Quarto options, `large_authoring.qmd`, `tables.qmd`, and
`math.qmd` stopped at grid tables at bytes 990, 3539, and 28452, respectively.

A separate in-process probe measured full parses of a Quarto footnote
containing repeated blank-separated paragraphs. The paragraphs contained
either a literal pipe or a dash-prefixed continuation line. Both builds
used the workspace release parser and ran on the same logical CPU. The
probe checked source, CST, and diagnostics against the incumbent before
collecting five samples. Timings are median and range in milliseconds.

| Paragraphs | Literal pipe at `319045e1` | Literal pipe after | Dash at `319045e1` | Dash after |
| ---: | ---: | ---: | ---: | ---: |
| 4,000 | 497.34 (492.03–500.91) | 2.23 (2.20–3.16) | 130.73 (130.63–132.41) | 3.13 (3.10–4.20) |
| 8,000 | 1965.15 (1961.84–1980.51) | 4.42 (4.40–6.42) | 548.75 (546.37–549.08) | 6.23 (6.15–8.43) |

### Grid table checkpoint

The release comparison used the same machine, documents, and options as the
previous checkpoint. Each cell gives median and p95 in microseconds, with
incumbent timings on the left and planner timings on the right.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| Project README | 208.4 / 220.7 → 126.3 / 141.5 | 136.4 / 141.7 → 115.2 / 121.1 | 2.7 / 2.7 → 2.5 / 2.6 | 2.0 / 2.1 → 1.1 / 1.2 |
| Pandoc manual | 5068.7 / 5155.4 → 2823.9 / 2926.3 | 1378.8 / 1404.7 → 1348.4 / 1365.7 | 43.4 / 47.7 → 43.2 / 47.5 | 3.6 / 4.0 → 2.7 / 2.9 |
| Parser README | 18.0 / 18.4 → 10.1 / 10.5 | 16.3 / 16.8 → 11.1 / 11.8 | 9.6 / 9.7 → 6.3 / 6.5 | 1.5 / 1.6 → 0.7 / 0.8 |
| Integrations | 161.7 / 236.6 → 99.6 / 151.8 | 90.3 / 96.3 → 78.8 / 83.6 | 41.0 / 45.0 → 34.8 / 37.7 | 2.5 / 2.7 → 1.1 / 1.2 |
| Getting Started | 260.0 / 266.2 → 151.0 / 158.0 | 102.3 / 107.8 → 93.9 / 101.9 | 45.1 / 49.8 → 41.1 / 45.8 | 2.3 / 2.4 → 1.1 / 1.1 |
| Performance | 1033.0 / 1043.5 → 882.7 / 894.1 | 196.0 / 205.1 → 186.2 / 194.2 | 98.7 / 105.2 → 89.9 / 95.0 | 2.8 / 3.0 → 1.3 / 1.4 |
| Tables | 665.4 / 679.4 → 476.9 / 492.1 | 218.6 / 238.7 → 209.8 / 225.3 | 106.2 / 120.0 → 99.1 / 112.3 | 2.7 / 3.1 → 1.3 / 1.5 |
| Math | 501.5 / 532.3 → 330.0 / 358.0 | 196.6 / 215.8 → 188.8 / 207.6 | 84.3 / 98.7 → 79.9 / 93.7 | 2.5 / 2.7 → 1.2 / 1.4 |
| Math with grid tables disabled | 488.3 / 511.0 → 318.8 / 330.8 | 190.3 / 199.9 → 180.6 / 190.0 | 82.4 / 88.1 → 76.2 / 81.8 | 2.5 / 2.6 → 1.2 / 1.3 |

In paired runs before and after the grid table port, the planner's full-parse
medians were 56–85% of the incumbent medians across the seven workloads that
reached timing in both runs. The planner's line-start edit medians were
71–99% of the incumbent medians before the port and 68–98% after it.

With default Quarto options, `tables.qmd` and `math.qmd` passed preflight
and reached timing. The grid tables in `large_authoring.qmd` matched the
incumbent CST. Preflight still found differences in fenced div nesting and
inline prefix placement outside the tables. The previous build also failed
preflight for that document with grid tables disabled.

### Parser review follow-up

The release comparison reran the grid table checkpoint before and after the
review fixes, on the same machine with the same documents and options. The
table gives median and p95 in microseconds after the fixes. Incumbent timings
are on the left and planner timings are on the right.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| Project README | 210.3 / 223.4 → 128.4 / 141.1 | 138.5 / 153.9 → 119.2 / 133.4 | 2.7 / 2.9 → 2.6 / 3.4 | 2.0 / 2.3 → 1.2 / 1.3 |
| Pandoc manual | 5131.9 / 5222.5 → 2860.2 / 2950.3 | 1395.0 / 1436.6 → 1408.0 / 1444.8 | 44.2 / 57.0 → 44.0 / 48.8 | 3.8 / 5.2 → 2.8 / 3.4 |
| Parser README | 17.7 / 18.6 → 10.1 / 10.6 | 16.1 / 17.8 → 11.4 / 11.9 | 9.5 / 9.8 → 6.5 / 6.7 | 1.5 / 1.6 → 0.7 / 0.8 |
| Integrations | 158.9 / 218.7 → 98.9 / 131.2 | 89.6 / 94.4 → 78.0 / 84.7 | 41.2 / 45.7 → 35.3 / 37.6 | 2.4 / 2.5 → 1.1 / 1.2 |
| Getting Started | 257.3 / 299.1 → 150.1 / 178.8 | 101.5 / 112.3 → 94.6 / 102.2 | 45.9 / 52.8 → 42.1 / 47.5 | 2.2 / 2.4 → 1.1 / 1.2 |
| Performance | 1024.3 / 1046.6 → 878.3 / 904.6 | 196.3 / 204.6 → 186.4 / 194.0 | 99.5 / 105.3 → 90.4 / 96.2 | 2.7 / 2.9 → 1.3 / 1.4 |
| Tables | 651.6 / 665.3 → 451.4 / 466.3 | 219.4 / 227.6 → 211.2 / 221.0 | 105.5 / 112.4 → 101.0 / 108.1 | 2.6 / 2.7 → 1.3 / 1.5 |
| Math | 484.8 / 493.5 → 319.0 / 328.1 | 194.4 / 202.4 → 187.4 / 197.5 | 83.5 / 89.7 → 79.9 / 84.9 | 2.4 / 2.5 → 1.2 / 1.3 |
| Math with grid tables disabled | 484.0 / 524.7 → 317.3 / 347.1 | 189.9 / 209.5 → 182.6 / 196.3 | 81.6 / 99.2 → 77.1 / 83.7 | 2.4 / 2.6 → 1.2 / 1.3 |

The planner's full-parse medians were 56–85% of the incumbent medians before
the fixes and 56–86% afterward. Its line-start edit medians were 71–101% in
both runs. Nine workloads reached timing in both runs. The default Quarto
run for `large_authoring.qmd` still failed CST preflight.

### Simple and multiline table checkpoint

The release comparison used the previous checkpoint's documents and options.
Both runs used the same logical CPU on the AMD Ryzen 9 7900. Each cell gives
median and p95 in microseconds, with incumbent timings on the left and planner
timings on the right.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| Project README | 216.8 / 232.4 → 133.0 / 147.3 | 141.0 / 155.5 → 119.3 / 125.5 | 2.6 / 2.9 → 2.6 / 2.8 | 2.0 / 2.2 → 1.2 / 1.3 |
| Pandoc manual | 5213.2 / 5303.5 → 2917.4 / 3101.9 | 1423.1 / 1477.5 → 1391.5 / 1429.2 | 44.9 / 49.5 → 44.8 / 49.4 | 4.0 / 7.3 → 3.0 / 5.1 |
| Parser README | 18.3 / 21.1 → 10.3 / 10.9 | 16.5 / 17.4 → 11.5 / 12.6 | 9.6 / 10.0 → 6.6 / 6.8 | 1.5 / 1.6 → 0.8 / 0.8 |
| Integrations | 166.0 / 181.1 → 103.0 / 111.3 | 95.9 / 105.2 → 82.8 / 89.4 | 42.9 / 47.8 → 38.4 / 41.8 | 2.6 / 3.0 → 1.3 / 1.4 |
| Getting Started | 278.8 / 293.0 → 164.2 / 176.5 | 108.3 / 115.9 → 97.8 / 103.7 | 47.3 / 51.8 → 45.1 / 49.7 | 2.4 / 2.6 → 1.2 / 1.3 |
| Performance | 1075.5 / 1109.4 → 914.9 / 938.8 | 204.6 / 213.4 → 189.5 / 204.9 | 102.0 / 108.4 → 93.6 / 99.0 | 3.0 / 3.2 → 1.5 / 1.6 |
| Tables | 708.2 / 735.5 → 511.8 / 535.9 | 230.2 / 241.8 → 219.7 / 231.4 | 110.9 / 118.6 → 106.0 / 114.0 | 2.9 / 3.0 → 1.5 / 1.6 |
| Math | 515.5 / 534.8 → 340.1 / 355.3 | 202.1 / 211.8 → 190.7 / 201.3 | 87.3 / 94.0 → 83.0 / 90.1 | 2.6 / 2.8 → 1.3 / 1.4 |
| Math with grid tables disabled | 505.0 / 523.1 → 332.5 / 347.6 | 196.8 / 211.3 → 186.5 / 195.6 | 84.8 / 90.7 → 78.8 / 84.6 | 2.6 / 2.8 → 1.3 / 1.4 |

The planner's full-parse medians were 56–85% of the incumbent medians in both
runs. Its line-start medians moved from 70–101% to 70–98%, and its typing
medians stayed within 68–100%. The planner's whole-file replacement medians
were 0.8–3.0 µs after the port. The same nine workloads reached timing in both
runs. `large_authoring.qmd` failed CST or diagnostic preflight in both runs.

Three Pandoc corpus fixtures also ran with Quarto defaults. All three failed
preflight before the port and reached timing afterward. Fixture 0541 has no
second blank line, so the edit benchmark has no edit location for it.

| Pandoc fixture | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| 0123, caption before a multiline table | 10.6 / 11.9 → 4.7 / 5.3 | 12.7 / 13.3 → 6.6 / 7.7 | 11.6 / 12.3 → 5.5 / 6.4 | 1.9 / 2.0 → 0.8 / 0.8 |
| 0124, caption after a multiline table | 6.7 / 7.0 → 4.6 / 4.8 | 9.7 / 9.9 → 6.5 / 6.6 | 3.1 / 3.1 → 2.0 / 2.1 | 1.9 / 1.9 → 0.8 / 0.9 |
| 0541, simple table in a blockquote | 3.5 / 3.7 → 2.2 / 2.4 | — | — | — |

### Simple and multiline table review follow-up

The release comparison used the same documents and options on the same
logical CPU. Each cell gives median and p95 in microseconds, with incumbent
timings on the left and planner timings on the right.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| Project README | 222.9 / 312.1 → 136.8 / 175.7 | 143.8 / 178.5 → 122.4 / 142.7 | 2.9 / 4.4 → 2.7 / 3.8 | 2.1 / 3.6 → 1.2 / 1.7 |
| Pandoc manual | 5308.9 / 7944.0 → 2993.8 / 4476.6 | 1438.1 / 1737.0 → 1404.0 / 1573.2 | 46.0 / 67.4 → 45.2 / 53.1 | 3.9 / 9.4 → 3.0 / 4.1 |
| Parser README | 18.5 / 19.0 → 10.9 / 11.3 | 16.8 / 22.6 → 11.9 / 17.0 | 10.0 / 14.1 → 6.8 / 8.6 | 1.5 / 1.7 → 0.8 / 0.9 |
| Integrations | 164.6 / 174.8 → 101.8 / 109.0 | 96.6 / 103.0 → 84.2 / 91.4 | 45.3 / 50.0 → 39.1 / 43.9 | 2.5 / 2.7 → 1.2 / 1.3 |
| Getting Started | 272.3 / 282.4 → 162.0 / 171.3 | 108.1 / 114.3 → 100.7 / 106.9 | 49.6 / 54.2 → 45.6 / 49.1 | 2.3 / 2.4 → 1.2 / 1.3 |
| Performance | 1078.5 / 1094.0 → 924.6 / 937.2 | 204.3 / 213.4 → 193.2 / 211.8 | 104.2 / 109.9 → 95.5 / 101.4 | 2.9 / 3.0 → 1.4 / 1.5 |
| Tables | 693.2 / 706.7 → 507.3 / 522.8 | 229.2 / 240.6 → 219.1 / 226.9 | 113.1 / 120.5 → 107.2 / 116.0 | 2.7 / 2.9 → 1.4 / 1.5 |
| Math | 510.8 / 523.0 → 340.6 / 353.4 | 203.9 / 213.2 → 195.2 / 209.2 | 89.2 / 94.4 → 84.6 / 89.7 | 2.5 / 2.6 → 1.3 / 1.4 |
| Math with grid tables disabled | 500.8 / 516.3 → 335.4 / 350.5 | 198.3 / 205.8 → 189.6 / 200.7 | 85.8 / 92.3 → 81.7 / 87.4 | 2.5 / 2.7 → 1.3 / 1.4 |

Across the nine workloads timed in both runs, the planner's full-parse
medians were 56–86% of the incumbent medians before and after the fixes.
The planner's line-start medians moved from 70–98% to 71–98%, and its typing
medians moved from 68–100% to 68–98%. Whole-file replacement medians were
0.8–3.0 µs for the planner after the fixes. Preflight excluded
`large_authoring.qmd` in both runs. The three Pandoc table fixtures continued
to pass preflight.

A separate release probe parsed repeated headerless simple tables formed by
repeating `"---\nx\n---\n"`. It verified source, CST, and diagnostic parity
before timing. Each measurement is the median of seven parses in
milliseconds.

| Tables | Incumbent | Planner |
| ---: | ---: | ---: |
| 1,000 | 1.43 | 0.48 |
| 4,000 | 5.69 | 2.08 |
| 16,000 | 23.51 | 8.18 |

### Pandoc HTML block checkpoint

The release comparison reran the previous checkpoint's workloads with the
same parser options on logical CPU 4 of the AMD Ryzen 9 7900. Each cell gives
median and p95 in microseconds, with incumbent timings on the left and planner
timings on the right.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| Project README | 212.0 / 220.2 → 131.0 / 138.2 | 140.2 / 146.0 → 120.4 / 125.8 | 2.7 / 2.9 → 2.6 / 2.8 | 2.1 / 2.2 → 1.2 / 1.3 |
| Pandoc manual | 5123.8 / 5234.5 → 2873.7 / 2988.8 | 1403.1 / 1438.9 → 1402.1 / 1434.1 | 44.7 / 48.8 → 44.7 / 48.4 | 3.9 / 4.2 → 2.9 / 3.2 |
| Parser README | 18.2 / 19.9 → 10.4 / 10.8 | 16.5 / 17.6 → 11.5 / 12.4 | 9.7 / 9.9 → 6.5 / 6.7 | 1.5 / 1.6 → 0.8 / 0.8 |
| Integrations | 162.2 / 168.1 → 99.8 / 106.0 | 93.3 / 99.4 → 81.0 / 87.3 | 42.7 / 45.8 → 36.2 / 40.0 | 2.6 / 2.8 → 1.2 / 1.4 |
| Getting Started | 276.7 / 292.8 → 162.0 / 171.9 | 108.2 / 115.8 → 98.5 / 105.0 | 48.0 / 52.7 → 43.0 / 47.5 | 2.4 / 2.5 → 1.2 / 1.3 |
| Performance | 1075.2 / 1094.2 → 914.8 / 931.5 | 202.9 / 214.6 → 190.3 / 198.0 | 103.0 / 109.2 → 92.2 / 97.2 | 3.1 / 3.2 → 1.5 / 1.6 |
| Tables | 691.6 / 712.7 → 498.8 / 520.1 | 229.0 / 242.9 → 218.8 / 238.8 | 112.3 / 120.6 → 105.0 / 113.5 | 2.8 / 2.9 → 1.5 / 1.6 |
| Math | 510.9 / 517.9 → 335.2 / 345.4 | 201.8 / 209.6 → 193.2 / 200.4 | 88.1 / 94.2 → 82.8 / 88.1 | 2.6 / 2.7 → 1.3 / 1.3 |
| Math with grid tables disabled | 497.7 / 505.0 → 327.5 / 336.0 | 196.4 / 202.8 → 188.8 / 197.2 | 84.9 / 90.5 → 80.5 / 85.3 | 2.5 / 2.7 → 1.2 / 1.3 |

Across these nine workloads, the planner's full-parse medians were 56–86% of
the incumbent before the port and 56–85% afterward. Line-start edit medians
moved from 71–101% to 70–100%, and typing medians moved from 68–102% to
67–100%. The planner's whole-file replacement medians were 0.8–2.9 µs after
the port. `large_authoring.qmd` failed CST or diagnostic preflight in both
runs.

The three table fixtures passed preflight in both runs. Five HTML fixtures
that the planner previously declined reached timing after the port. A dash
means the fixture had no second blank line for the edit sequence.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| 0123, caption before a multiline table | 10.6 / 11.2 → 4.7 / 5.2 | 13.1 / 13.6 → 6.9 / 7.2 | 12.0 / 12.5 → 5.8 / 6.7 | 1.9 / 1.9 → 0.8 / 0.8 |
| 0124, caption after a multiline table | 6.7 / 7.0 → 4.5 / 4.7 | 10.0 / 10.2 → 6.8 / 7.0 | 3.4 / 3.4 → 2.3 / 2.4 | 1.8 / 1.9 → 0.8 / 0.8 |
| 0541, simple table in a blockquote | 3.4 / 3.6 → 2.1 / 2.3 | — | — | — |
| 0199, nested native divs | 6.1 / 6.3 → 6.9 / 7.5 | 7.5 / 7.7 → 8.5 / 8.7 | 7.1 / 7.3 → 8.1 / 8.5 | 1.9 / 1.9 → 0.8 / 0.8 |
| 0481, comment trailing prose in a fenced div | 3.5 / 3.6 → 3.0 / 3.1 | — | — | — |
| 0495, unclosed div in a list item | 5.6 / 5.8 → 4.8 / 5.0 | — | — | — |
| 0610, quoted div stops before the next list item | 5.6 / 5.8 → 5.5 / 5.7 | — | — | — |
| 0611, comment trailing prose in a quoted item | 5.0 / 5.1 → 4.8 / 5.0 | — | — | — |

For the nested div fixture, the planner took about 1 µs more than the incumbent
for a full parse and for each region edit. The other four HTML fixtures had
planner full-parse medians within 0.8 µs of the incumbent.

### Pandoc HTML review follow-up

Both release runs used the previous checkpoint's workloads, parser options,
and logical CPU. Each cell gives median and p95 in microseconds, with
incumbent timings on the left and planner timings on the right.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| Project README | 218.3 / 245.6 → 135.2 / 156.1 | 143.7 / 152.0 → 122.8 / 133.4 | 2.8 / 3.0 → 2.7 / 2.9 | 2.1 / 2.2 → 1.3 / 1.3 |
| Pandoc manual | 5297.2 / 5410.6 → 2950.2 / 3059.6 | 1457.0 / 1500.7 → 1455.4 / 1504.2 | 46.5 / 50.8 → 46.4 / 50.9 | 3.8 / 4.6 → 3.0 / 3.4 |
| Parser README | 19.0 / 19.5 → 10.7 / 11.0 | 17.3 / 17.8 → 12.3 / 13.0 | 10.3 / 10.7 → 7.2 / 7.5 | 1.5 / 1.6 → 0.8 / 0.9 |
| Integrations | 165.0 / 173.9 → 102.5 / 109.2 | 96.0 / 104.8 → 83.3 / 89.0 | 44.8 / 49.3 → 38.4 / 42.6 | 2.5 / 2.8 → 1.2 / 1.4 |
| Getting Started | 273.1 / 288.4 → 163.3 / 172.5 | 110.6 / 117.6 → 101.2 / 110.6 | 50.8 / 55.4 → 46.0 / 50.0 | 2.4 / 2.5 → 1.2 / 1.3 |
| Performance | 1089.3 / 1133.0 → 936.8 / 967.8 | 212.9 / 231.3 → 198.0 / 217.5 | 109.0 / 118.3 → 98.7 / 107.2 | 3.0 / 3.3 → 1.5 / 1.6 |
| Tables | 703.5 / 725.5 → 509.8 / 531.8 | 237.9 / 256.1 → 226.1 / 243.0 | 118.3 / 124.9 → 108.7 / 116.7 | 2.8 / 2.9 → 1.5 / 1.6 |
| Math | 517.7 / 538.4 → 346.0 / 362.2 | 210.4 / 238.2 → 199.4 / 228.1 | 93.4 / 106.4 → 87.0 / 100.8 | 2.6 / 4.1 → 1.4 / 1.7 |
| Math with grid tables disabled | 513.7 / 547.1 → 341.0 / 365.5 | 203.5 / 216.5 → 193.3 / 206.9 | 89.1 / 96.1 → 84.1 / 90.5 | 2.7 / 2.8 → 1.3 / 1.4 |

Planner full-parse medians were 55–85% of incumbent medians before the
fixes and 56–86% afterward. The ranges were 69–98% and 71–100% for
line-start edits and 67–100% and 70–100% for typing edits.
`large_authoring.qmd` failed CST or diagnostic preflight in both runs.

The same eight Pandoc fixtures reached timing in both runs.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| 0123, caption before a multiline table | 10.6 / 12.9 → 4.7 / 5.5 | 13.1 / 13.6 → 6.9 / 7.2 | 11.9 / 12.3 → 5.8 / 6.7 | 1.8 / 1.9 → 0.8 / 0.8 |
| 0124, caption after a multiline table | 6.7 / 7.0 → 4.5 / 4.6 | 10.2 / 10.8 → 6.9 / 7.6 | 3.5 / 3.5 → 2.4 / 2.5 | 1.8 / 1.9 → 0.8 / 0.9 |
| 0541, simple table in a blockquote | 3.5 / 3.7 → 2.2 / 2.4 | — | — | — |
| 0199, nested native divs | 6.1 / 6.3 → 7.0 / 7.4 | 7.8 / 8.0 → 8.8 / 9.7 | 7.4 / 7.5 → 8.4 / 9.1 | 1.9 / 1.9 → 0.8 / 0.8 |
| 0481, comment trailing prose in a fenced div | 3.5 / 3.6 → 3.0 / 3.2 | — | — | — |
| 0495, unclosed div in a list item | 5.5 / 5.9 → 5.0 / 5.2 | — | — | — |
| 0610, quoted div stops before the next list item | 5.6 / 5.8 → 5.5 / 5.7 | — | — | — |
| 0611, comment trailing prose in a quoted item | 5.1 / 5.3 → 4.9 / 5.1 | — | — | — |

A separate release probe checked source round-trips for two quoted list
items containing 20 HTML comments each. The case with blank-separated
trailing prose took about 3.1 s in each engine before the fix. Afterward,
a full parse took 72 µs in the incumbent and 90 µs in the planner. The
case with a trailing ATX heading on every comment line went from about
1.9 s to 48 µs and 61 µs, respectively.

An apostrophe edit in prose after a closed div and 4,000 intervening
paragraphs used the planner's token tier after the fix. Median and p95
were 144 and 150 µs, compared with 1,061 and 1,084 µs for the previous
fallback to a full parse.

### Pandoc HTML second review follow-up

Both release runs used the same documents, parser options, and logical CPU
as the previous checkpoint. Each cell gives median and p95 in microseconds,
with incumbent timings on the left and planner timings on the right.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| Project README | 221.2 / 233.7 → 135.4 / 143.0 | 142.9 / 163.3 → 123.4 / 137.1 | 2.7 / 3.3 → 2.6 / 3.0 | 2.2 / 2.3 → 1.2 / 1.4 |
| Pandoc manual | 8647.8 / 10650.4 → 4561.3 / 5544.9 | 1466.2 / 1816.0 → 1454.5 / 1763.0 | 46.6 / 62.6 → 46.3 / 55.3 | 7.0 / 13.7 → 4.6 / 8.6 |
| Parser README | 18.9 / 19.4 → 10.9 / 11.2 | 17.0 / 17.9 → 11.9 / 12.3 | 10.1 / 10.3 → 6.8 / 7.0 | 1.6 / 1.7 → 0.8 / 0.8 |
| Integrations | 168.5 / 183.9 → 103.9 / 116.3 | 96.4 / 135.7 → 83.4 / 129.3 | 44.5 / 64.9 → 37.8 / 48.1 | 2.7 / 4.4 → 1.3 / 1.9 |
| Getting Started | 285.1 / 297.6 → 167.9 / 180.4 | 111.7 / 118.7 → 100.4 / 106.8 | 51.0 / 56.1 → 44.1 / 48.6 | 2.5 / 2.6 → 1.2 / 1.3 |
| Performance | 1088.7 / 1185.9 → 930.0 / 1024.4 | 209.4 / 226.7 → 194.3 / 214.2 | 104.8 / 116.3 → 94.3 / 102.6 | 3.2 / 3.5 → 1.5 / 1.7 |
| Tables | 722.5 / 768.5 → 520.8 / 563.1 | 241.5 / 338.7 → 229.7 / 325.0 | 117.0 / 174.4 → 110.0 / 153.0 | 2.9 / 4.7 → 1.5 / 2.1 |
| Math | 526.0 / 559.2 → 347.7 / 365.8 | 208.0 / 223.3 → 194.6 / 209.3 | 90.8 / 98.1 → 84.0 / 93.9 | 2.7 / 2.8 → 1.3 / 1.4 |
| Math with grid tables disabled | 513.0 / 534.8 → 338.2 / 352.0 | 199.1 / 215.4 → 190.8 / 201.0 | 87.0 / 93.0 → 82.9 / 89.3 | 2.6 / 2.8 → 1.3 / 1.5 |

Planner full-parse medians were 56–86% of incumbent medians before the
fixes and 53–85% afterward. Line-start edit medians ranged from 71–100%
before and 70–99% afterward. Typing medians ranged from 69–100% before
and 67–99% afterward. `large_authoring.qmd` failed CST or diagnostics
preflight in both runs.

The same eight Pandoc fixtures reached timing in both runs.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| 0123, caption before a multiline table | 10.4 / 10.9 → 4.6 / 4.8 | 13.0 / 13.6 → 6.9 / 7.2 | 12.0 / 12.5 → 5.8 / 6.0 | 1.9 / 1.9 → 0.8 / 0.8 |
| 0124, caption after a multiline table | 6.6 / 6.9 → 4.4 / 4.6 | 10.0 / 10.2 → 6.7 / 7.0 | 3.4 / 3.5 → 2.4 / 2.5 | 1.9 / 1.9 → 0.8 / 0.8 |
| 0541, simple table in a blockquote | 3.4 / 3.6 → 2.2 / 2.3 | — | — | — |
| 0199, nested native divs | 6.2 / 6.4 → 7.1 / 7.5 | 7.9 / 8.5 → 9.0 / 9.7 | 7.6 / 8.0 → 8.6 / 9.4 | 1.9 / 2.0 → 0.8 / 0.8 |
| 0481, comment trailing prose in a fenced div | 3.6 / 3.7 → 3.1 / 3.2 | — | — | — |
| 0495, unclosed div in a list item | 5.8 / 6.1 → 5.1 / 5.4 | — | — | — |
| 0610, quoted div stops before the next list item | 5.7 / 5.8 → 5.6 / 5.9 | — | — | — |
| 0611, comment trailing prose in a quoted item | 5.2 / 5.3 → 4.9 / 5.1 | — | — | — |

Two release probes compared the code before and after the fixes. The edit
inserted an apostrophe in prose after a fenced listing of repeated
`<div` lines and a final `>`. It used cached reference labels and the
planner's token tier in both runs. The parse probes checked source
round-trips for quoted list items with repeated
`<!--a--><div>x</div>` lines. Timings are median and p95 in microseconds.

| Workload | Before | After |
| --- | ---: | ---: |
| Token edit after 4,000 code lines | 76,578.7 / 77,269.2 | 0.4 / 4.9 |
| Token edit after 8,000 code lines | 311,774.7 / 376,238.4 | 0.4 / 1.4 |
| 250 comment tails, incumbent | 97,046.4 / 162,619.2 | 882.3 / 932.5 |
| 250 comment tails, planner | 97,934.2 / 103,245.9 | 988.9 / 1,053.9 |
| 500 comment tails, incumbent | 411,603.3 / 482,748.6 | 1,778.5 / 1,784.8 |
| 500 comment tails, planner | 398,624.8 / 666,382.3 | 1,962.7 / 2,296.4 |

### Pandoc HTML third review follow-up

Both release runs used the previous checkpoint's documents, parser options,
and logical CPU. Each cell gives median and p95 in microseconds, with
incumbent timings on the left and planner timings on the right.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| Project README | 391.3 / 424.6 → 235.9 / 261.1 | 238.6 / 254.4 → 200.7 / 216.8 | 4.3 / 5.0 → 4.3 / 4.9 | 3.5 / 4.0 → 1.9 / 2.2 |
| Pandoc manual | 9196.8 / 9798.4 → 4763.1 / 5112.8 | 2306.9 / 2748.5 → 2245.7 / 2532.6 | 68.9 / 78.2 → 68.8 / 78.5 | 21.3 / 28.0 → 15.4 / 23.0 |
| Parser README | 34.6 / 38.4 → 18.5 / 20.9 | 31.2 / 34.6 → 20.6 / 22.9 | 18.4 / 20.5 → 12.0 / 12.8 | 3.0 / 3.5 → 1.4 / 1.6 |
| Integrations | 328.1 / 355.0 → 198.4 / 218.9 | 171.5 / 190.6 → 147.2 / 161.5 | 75.9 / 84.7 → 64.2 / 71.8 | 5.0 / 6.2 → 2.4 / 3.0 |
| Getting Started | 315.3 / 462.6 → 186.6 / 257.9 | 127.9 / 187.6 → 112.8 / 172.2 | 55.9 / 84.8 → 50.1 / 75.0 | 2.7 / 4.6 → 1.3 / 2.1 |
| Performance | 1164.2 / 1838.6 → 987.9 / 1429.7 | 227.2 / 355.2 → 216.9 / 324.1 | 116.5 / 185.3 → 106.9 / 164.8 | 3.5 / 5.7 → 1.6 / 2.5 |
| Tables | 1156.2 / 1211.4 → 805.2 / 856.2 | 365.9 / 382.0 → 349.1 / 363.3 | 188.3 / 199.5 → 174.4 / 184.2 | 5.5 / 6.4 → 2.4 / 2.8 |
| Math | 964.8 / 989.1 → 591.8 / 609.8 | 305.3 / 341.3 → 298.5 / 323.2 | 138.7 / 155.3 → 128.3 / 145.2 | 4.5 / 5.9 → 2.1 / 2.7 |
| Math with grid tables disabled | 753.4 / 808.4 → 482.1 / 514.8 | 216.5 / 326.3 → 206.1 / 309.1 | 95.8 / 145.3 → 88.3 / 134.5 | 2.7 / 4.9 → 1.4 / 2.3 |

Planner full-parse medians ranged from 52–83% of incumbent medians before
the fixes and 52–85% afterward. Line-start edit medians ranged from 66–96%
before and 66–98% afterward. Typing medians ranged from 65–101% before
and 65–100% afterward. `large_authoring.qmd` failed CST or diagnostic
preflight in both runs.

The same eight Pandoc fixtures reached timing in both runs.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| 0123, caption before a multiline table | 17.6 / 19.4 → 7.9 / 8.6 | 13.8 / 14.4 → 7.3 / 8.0 | 12.6 / 12.9 → 6.1 / 6.9 | 1.9 / 2.0 → 0.8 / 0.8 |
| 0124, caption after a multiline table | 7.0 / 7.2 → 4.6 / 4.8 | 10.6 / 10.9 → 7.1 / 7.6 | 3.6 / 3.7 → 2.5 / 2.6 | 2.0 / 2.0 → 0.8 / 0.9 |
| 0541, simple table in a blockquote | 3.6 / 3.8 → 2.2 / 2.4 | — | — | — |
| 0199, nested native divs | 6.4 / 6.6 → 7.4 / 8.2 | 8.2 / 8.5 → 9.4 / 10.4 | 7.7 / 7.9 → 8.9 / 9.8 | 1.9 / 2.0 → 0.8 / 0.9 |
| 0481, comment trailing prose in a fenced div | 3.9 / 4.0 → 3.3 / 3.5 | — | — | — |
| 0495, unclosed div in a list item | 6.0 / 6.3 → 6.2 / 6.5 | — | — | — |
| 0610, quoted div stops before the next list item | 9.0 / 10.5 → 8.9 / 10.0 | — | — | — |
| 0611, comment trailing prose in a quoted item | 10.0 / 11.0 → 9.3 / 10.0 | — | — | — |

A separate release probe inserted an apostrophe in prose after an inline
`<em>` tag and a fenced listing of 4,000 lines. Both builds used cached
reference labels and reused the edited token. The first listing contained
plain text, and the second contained repeated `<div` lines and a final
`>`. The table gives median and p95 in microseconds.

| Listing | Before | After |
| --- | ---: | ---: |
| Plain text | 529.87 / 555.91 | 4.31 / 4.56 |
| HTML-looking code | 65.62 / 69.83 | 1.16 / 1.25 |
