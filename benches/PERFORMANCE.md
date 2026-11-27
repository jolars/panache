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

### Pandoc line, title, and raw TeX block checkpoint

Both release builds ran on logical CPU 4 of an Intel Core Ultra 7 155U with
Rust 1.98.1. The runs used the previous checkpoint's documents and options
and added four Pandoc corpus fixtures under Quarto defaults. The baseline
rebuild used revision `70678ae5` so both builds could run on the same host.
Each cell gives the median and p95 in microseconds, with the incumbent on
the left and the planner on the right.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| Project README | 456.5 / 493.9 → 267.3 / 292.2 | 259.1 / 271.3 → 218.0 / 232.7 | 5.9 / 6.1 → 5.7 / 6.0 | 4.7 / 5.1 → 2.7 / 3.0 |
| Pandoc manual | 9289.9 / 10271.6 → 5173.8 / 5710.4 | 2045.5 / 2305.0 → 2040.4 / 2268.6 | 82.8 / 96.6 → 83.1 / 94.4 | 9.2 / 12.8 → 7.1 / 9.0 |
| Parser README | 34.1 / 37.6 → 18.6 / 19.2 | 28.9 / 32.1 → 18.6 / 20.6 | 17.2 / 18.1 → 10.6 / 11.8 | 3.1 / 3.3 → 1.3 / 1.5 |
| Integrations | 352.1 / 395.7 → 215.3 / 241.7 | 168.2 / 189.7 → 143.3 / 160.3 | 74.9 / 83.6 → 62.7 / 69.4 | 5.6 / 6.3 → 2.8 / 3.2 |
| Getting Started | 631.0 / 656.3 → 345.5 / 371.6 | 178.0 / 199.9 → 161.9 / 182.5 | 78.7 / 88.1 → 70.7 / 79.2 | 4.8 / 5.4 → 2.4 / 2.8 |
| Performance | 1898.7 / 2113.2 → 1568.9 / 1755.6 | 344.8 / 372.0 → 320.9 / 336.5 | 178.9 / 192.7 → 158.5 / 169.9 | 6.3 / 7.9 → 3.1 / 3.8 |
| Tables | 1451.9 / 1500.2 → 936.8 / 979.7 | 385.5 / 405.8 → 364.9 / 383.3 | 189.9 / 202.5 → 175.0 / 187.7 | 6.2 / 7.1 → 3.1 / 3.5 |
| Math | 964.1 / 1074.4 → 602.8 / 678.2 | 318.6 / 352.7 → 304.6 / 337.3 | 139.6 / 154.9 → 129.9 / 146.8 | 5.1 / 5.8 → 2.6 / 2.9 |
| Math with grid tables disabled | 957.2 / 1078.4 → 599.8 / 674.3 | 314.9 / 353.3 → 301.0 / 342.0 | 138.4 / 154.8 → 129.7 / 150.6 | 5.2 / 6.0 → 2.6 / 3.0 |

The eight previous Pandoc fixtures and all four fixtures for this port
reached timing. The latter four failed preflight before the port. A dash
marks a fixture with fewer than two blank lines, which leaves the edit
benchmark without an edit location.

  | Workload                                         | Full parse                | Line-start edit           | Typing edit               | Whole-file replacement |
  | ---                                              | ---:                      | ---:                      | ---:                      | ---:                   |
  | 0123, caption before a multiline table           |   19.1 / 21.6 → 8.4 / 9.6 | 22.6 / 23.7 → 12.3 / 13.6 | 20.9 / 23.1 → 10.1 / 11.2 |  4.0 / 4.3 → 1.7 / 1.8 |
  | 0124, caption after a multiline table            |   11.6 / 13.2 → 7.6 / 8.7 | 18.2 / 20.3 → 12.2 / 13.8 |     6.6 / 7.4 → 4.3 / 4.9 |  3.8 / 4.3 → 1.5 / 1.8 |
  | 0541, simple table in a blockquote               |     6.3 / 7.2 → 3.9 / 4.6 |                         — |                         — |                      — |
  | 0199, nested native divs                         | 11.1 / 11.5 → 12.7 / 13.1 | 14.2 / 15.8 → 16.1 / 18.2 | 13.4 / 14.9 → 15.3 / 17.3 |  3.9 / 4.2 → 1.7 / 1.9 |
  | 0481, comment trailing prose in a fenced div     |     7.2 / 7.5 → 6.2 / 6.4 |                         — |                         — |                      — |
  | 0495, unclosed div in a list item                | 12.0 / 12.4 → 10.7 / 11.1 |                         — |                         — |                      — |
  | 0610, quoted div stops before the next list item | 12.8 / 13.3 → 11.3 / 12.0 |                         — |                         — |                      — |
  | 0611, comment trailing prose in a quoted item    | 11.7 / 12.3 → 10.6 / 11.0 |                         — |                         — |                      — |
  | 0098, line blocks                                |     5.0 / 5.1 → 2.3 / 2.5 |                         — |                         — |                      — |
  | 0130, title block                                |   21.3 / 21.8 → 7.5 / 8.2 | 25.0 / 29.7 → 12.4 / 13.6 |     1.3 / 1.5 → 1.2 / 1.5 |  4.2 / 4.7 → 1.8 / 2.1 |
  | 0141, raw TeX commands                           |   12.0 / 12.6 → 4.2 / 4.6 |   12.8 / 14.6 → 7.4 / 8.4 |     0.8 / 0.9 → 0.9 / 1.0 |  3.9 / 4.4 → 1.8 / 2.0 |
  | 0095, LaTeX environment                          |    8.7 / 11.4 → 4.2 / 5.6 |   13.1 / 14.8 → 8.1 / 9.1 |     1.3 / 1.4 → 1.2 / 1.4 |  3.9 / 4.3 → 1.8 / 2.0 |

Across the nine shared document workloads, the planner's full-parse
medians went from 53–82% of the incumbent's to 55–83%. Line-start edit
medians went from 63–98% to 64–100%, typing medians from 60–99% to 62–100%,
and whole-file replacement medians from 40–72% to 42–77%. The existing CST
or diagnostic mismatch excluded `large_authoring.qmd` in both runs.

A separate run with `line-blocks`, `pandoc-title-block`, and `raw-tex`
disabled in the resolved Quarto config passed source, CST, and diagnostic
preflight for all four fixtures added at this checkpoint.

### Display and fenced math checkpoint

Both release builds ran on logical CPU 4 of the Intel Core Ultra 7 155U
with Rust 1.98.1. The comparison used baseline revision `81e4f00e` and
the same documents and resolved options as the previous checkpoint.
Each cell gives the median and p95 in microseconds from the second run,
with the incumbent on the left and the planner on the right.

  | Workload                       | Full parse                          | Line-start edit                   | Typing edit                   | Whole-file replacement    |
  | ---                            | ---:                                | ---:                              | ---:                          | ---:                      |
  | Project README                 |       617.5 / 663.4 → 371.8 / 411.2 |     318.2 / 342.3 → 267.4 / 285.5 |         7.6 / 8.4 → 7.4 / 8.1 |     6.4 / 7.3 → 3.8 / 4.2 |
  | Pandoc manual                  | 14006.9 / 14620.7 → 8170.2 / 8712.9 | 3506.2 / 3810.0 → 3274.8 / 3737.8 | 120.5 / 137.3 → 120.4 / 136.5 | 31.9 / 38.3 → 25.3 / 31.6 |
  | Parser README                  |           47.0 / 51.1 → 26.0 / 28.1 |         40.4 / 48.1 → 26.3 / 30.8 |     24.5 / 28.2 → 15.1 / 17.5 |     4.4 / 5.1 → 2.0 / 2.3 |
  | Integrations                   |       499.0 / 525.8 → 301.6 / 328.8 |     242.1 / 276.6 → 205.0 / 236.2 |  110.2 / 126.4 → 89.7 / 108.7 |    8.8 / 10.0 → 4.4 / 5.3 |
  | Getting Started                |       796.4 / 867.6 → 454.6 / 497.1 |     252.0 / 282.1 → 226.7 / 249.5 |  111.4 / 128.6 → 99.2 / 115.7 |     7.4 / 8.4 → 3.7 / 4.4 |
  | Performance                    |   2746.7 / 3038.7 → 2245.9 / 2500.4 |     503.2 / 579.1 → 460.0 / 537.5 | 261.3 / 301.0 → 228.6 / 269.0 |   10.9 / 12.7 → 5.7 / 7.2 |
  | Tables                         |   2005.1 / 2308.7 → 1447.0 / 1669.1 |     560.6 / 615.6 → 527.0 / 587.9 | 275.0 / 318.0 → 252.9 / 285.8 |   10.5 / 12.5 → 5.4 / 6.4 |
  | Math                           |    1485.9 / 1678.0 → 934.5 / 1059.7 |     486.2 / 560.8 → 458.6 / 530.7 | 216.2 / 250.2 → 200.3 / 235.2 |    8.8 / 10.4 → 4.9 / 5.9 |
  | Math with grid tables disabled |    1555.2 / 1755.5 → 968.5 / 1115.5 |     482.2 / 580.5 → 450.2 / 537.3 | 213.8 / 253.9 → 194.8 / 231.7 |    9.0 / 10.5 → 5.1 / 6.0 |

All 12 previously timed Pandoc fixtures still reached timing. Four new
display-math fixtures reached timing under Quarto defaults, as did the
quoted-list fence fixture under CommonMark with `tex-math-gfm` enabled.
The baseline planner declined all five math fixtures. A dash marks a
fixture with fewer than two blank lines for the edit sequence.

  | Workload                     | Full parse                | Line-start edit           | Typing edit               | Whole-file replacement |
  | ---                          | ---:                      | ---:                      | ---:                      | ---:                   |
  | 0123                         | 26.8 / 28.7 → 11.6 / 13.4 | 38.4 / 45.1 → 19.6 / 23.6 | 35.2 / 39.4 → 16.4 / 18.4 |  7.8 / 9.2 → 3.2 / 3.7 |
  | 0124                         | 17.4 / 18.4 → 11.3 / 12.7 | 26.5 / 30.0 → 16.8 / 19.0 |   10.0 / 10.9 → 6.3 / 7.0 |  5.8 / 6.3 → 2.4 / 2.7 |
  | 0541                         |     9.0 / 9.7 → 5.5 / 6.1 |                         — |                         — |                      — |
  | 0199                         | 17.2 / 17.9 → 19.7 / 21.6 | 24.0 / 26.0 → 26.9 / 29.2 | 22.7 / 23.8 → 25.7 / 27.3 |  6.7 / 7.2 → 3.1 / 3.5 |
  | 0481                         | 12.2 / 14.7 → 10.7 / 13.3 |                         — |                         — |                      — |
  | 0495                         | 17.5 / 19.9 → 15.9 / 18.1 |                         — |                         — |                      — |
  | 0610                         | 18.4 / 22.8 → 17.0 / 19.2 |                         — |                         — |                      — |
  | 0611                         | 17.3 / 18.5 → 15.9 / 17.2 |                         — |                         — |                      — |
  | 0098                         |     7.2 / 7.7 → 3.4 / 3.8 |                         — |                         — |                      — |
  | 0130                         | 28.9 / 31.8 → 10.5 / 11.9 | 33.9 / 35.2 → 15.8 / 16.8 |     1.7 / 1.9 → 1.6 / 1.8 |  5.9 / 6.3 → 2.6 / 2.8 |
  | 0141                         |   19.4 / 20.8 → 6.8 / 7.5 | 20.7 / 24.4 → 11.2 / 12.6 |     1.3 / 1.6 → 1.4 / 1.6 |  6.3 / 7.4 → 3.0 / 3.4 |
  | 0095                         |   13.4 / 15.6 → 6.3 / 7.9 | 19.5 / 21.8 → 11.8 / 12.6 |     1.9 / 2.1 → 1.9 / 2.1 |  5.9 / 6.5 → 2.8 / 3.1 |
  | 0047                         | 44.5 / 53.3 → 18.6 / 22.6 | 44.7 / 55.4 → 24.5 / 27.6 |     1.7 / 2.1 → 1.8 / 2.1 |  6.6 / 7.7 → 3.2 / 4.0 |
  | 0048                         | 24.1 / 26.9 → 10.4 / 12.1 | 23.8 / 28.7 → 14.3 / 16.4 | 22.4 / 27.7 → 13.0 / 15.6 |  5.7 / 6.5 → 2.5 / 2.9 |
  | 0049                         | 35.3 / 39.5 → 18.4 / 20.5 | 33.0 / 38.2 → 21.4 / 24.9 |     1.4 / 1.6 → 1.4 / 1.6 |  6.0 / 6.8 → 2.8 / 3.2 |
  | 0050                         |   10.0 / 10.6 → 5.2 / 5.7 |                         — |                         — |                      — |
  | Fenced math in a quoted list |     4.7 / 5.3 → 4.1 / 4.6 |                         — |                         — |                      — |

Across the nine shared document workloads in the second run, planner
full-parse medians went from 54–82% of the incumbent's to 55–82%.
Line-start edits went from 63–94% to 65–94%, typing edits from 61–100%
to 62–100%, and whole-file replacement from 42–78% to 45–79%.
Absolute timings varied between runs. On `tables.qmd`, the planner's
full-parse median rose from 64–68% of the incumbent's to 71–72% across
both runs. The two `math.qmd` configurations stayed within about one
percentage point of baseline. `large_authoring.qmd` failed CST or
diagnostic preflight in both builds.

After the math boundary fixes from review, all 26 timed workloads still
passed preflight. The existing mismatch for `large_authoring.qmd` remained.
Across the nine shared documents, planner medians were 55–85% of the
incumbent's for full parses, 65–95% for line-start edits, 61–99% for typing
edits, and 43–76% for whole-file replacement.

After the second review, the same 26 workloads reached timing. Across the
nine shared documents, planner medians were 56–86% of the incumbent's
for full parses, 65–99% for line-start edits, 62–99% for typing edits,
and 45–78% for whole-file replacement. The existing preflight mismatch
for `large_authoring.qmd` remained.

After the third review, the same 26 workloads reached timing. Across the
nine shared documents, planner medians were 56–83% of the incumbent's
for full parses, 66–98% for line-start edits, 63–100% for typing edits,
and 43–73% for whole-file replacement. The existing preflight mismatch
for `large_authoring.qmd` remained.

A separate Quarto scaling check placed 10,000 and 20,000 Setext-like
lines inside quoted display math. The planner's full-parse median and p95
were 9.54 / 10.08 ms and 18.66 / 20.16 ms. The incumbent measured
10.55 / 11.14 ms and 20.63 / 22.31 ms. The two engines passed source,
CST, and diagnostic preflight on both inputs.

### Pandoc, R Markdown, and MultiMarkdown checkpoint

Both release builds ran on logical CPU 4 of the Intel Core Ultra 7 155U
with Rust 1.98.1. The comparison reran the display and fenced math
workloads with the same files and resolved options at baseline `f3e31594`
and with the three flavors enabled. Each cell gives the median and p95
in microseconds, with the incumbent on the left and the planner on the
right. A dash marks a fixture with fewer than two blank lines for the
edit sequence.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| Project README | 487.6 / 514.5 → 291.4 / 311.1 | 239.7 / 272.5 → 205.7 / 230.5 | 5.5 / 6.2 → 5.4 / 6.2 | 4.7 / 5.3 → 2.8 / 3.2 |
| Pandoc manual | 9312.4 / 10256.5 → 5294.2 / 5917.8 | 2088.3 / 2293.2 → 2057.7 / 2276.2 | 84.0 / 94.8 → 85.6 / 95.9 | 9.4 / 12.3 → 7.4 / 8.6 |
| Parser README | 33.3 / 36.6 → 18.6 / 19.5 | 28.8 / 32.2 → 19.0 / 21.2 | 17.5 / 19.4 → 11.0 / 11.9 | 3.1 / 3.4 → 1.4 / 1.6 |
| Integrations | 349.7 / 394.7 → 215.0 / 243.5 | 164.7 / 187.0 → 139.4 / 161.0 | 75.3 / 86.5 → 61.8 / 70.2 | 5.8 / 6.6 → 2.9 / 3.4 |
| Getting Started | 581.2 / 644.0 → 328.6 / 368.8 | 185.2 / 211.0 → 170.6 / 189.2 | 82.0 / 98.3 → 77.1 / 86.3 | 5.1 / 5.8 → 2.7 / 3.0 |
| Performance | 2083.6 / 2135.5 → 1717.2 / 1762.8 | 344.1 / 360.6 → 319.5 / 332.8 | 178.7 / 188.2 → 157.8 / 165.3 | 6.4 / 7.3 → 3.1 / 3.5 |
| Tables | 1345.2 / 1486.0 → 879.5 / 969.3 | 390.8 / 405.3 → 366.4 / 383.3 | 193.3 / 209.2 → 176.0 / 189.7 | 6.4 / 7.2 → 3.2 / 3.5 |
| Math | 974.2 / 1084.6 → 607.8 / 685.3 | 319.9 / 360.4 → 304.2 / 345.7 | 140.8 / 163.5 → 130.4 / 146.3 | 5.4 / 6.2 → 2.7 / 3.2 |
| Math with grid tables disabled | 949.4 / 1060.9 → 600.4 / 674.9 | 316.6 / 360.3 → 304.3 / 342.7 | 139.2 / 161.1 → 131.8 / 148.2 | 5.4 / 6.1 → 2.8 / 3.2 |
| 0123 | 18.6 / 21.1 → 8.2 / 9.3 | 22.4 / 25.4 → 11.7 / 13.1 | 20.7 / 23.4 → 9.9 / 11.2 | 4.2 / 4.9 → 1.8 / 2.0 |
| 0124 | 11.5 / 11.9 → 7.6 / 7.9 | 18.2 / 20.4 → 11.7 / 13.3 | 7.0 / 7.8 → 4.5 / 5.0 | 3.8 / 4.3 → 1.5 / 1.8 |
| 0541 | 6.4 / 7.0 → 3.9 / 4.2 | — | — | — |
| 0199 | 11.3 / 11.6 → 13.1 / 13.6 | 14.8 / 15.1 → 16.5 / 17.7 | 14.0 / 14.3 → 15.7 / 17.0 | 4.1 / 4.2 → 1.7 / 1.8 |
| 0481 | 7.6 / 8.3 → 6.5 / 7.4 | — | — | — |
| 0495 | 11.8 / 14.3 → 11.2 / 13.4 | — | — | — |
| 0610 | 12.6 / 17.1 → 11.9 / 16.6 | — | — | — |
| 0611 | 11.7 / 12.2 → 10.6 / 11.3 | — | — | — |
| 0098 | 4.8 / 5.0 → 2.3 / 2.5 | — | — | — |
| 0130 | 19.0 / 19.3 → 6.9 / 7.2 | 22.5 / 25.6 → 11.1 / 12.0 | 1.2 / 1.3 → 1.2 / 1.3 | 3.8 / 4.3 → 1.7 / 1.9 |
| 0141 | 11.8 / 12.0 → 4.3 / 4.5 | 12.8 / 14.2 → 7.3 / 8.0 | 0.9 / 1.0 → 0.9 / 1.0 | 3.8 / 4.0 → 1.9 / 2.1 |
| 0095 | 8.7 / 9.0 → 4.2 / 4.4 | 13.4 / 14.9 → 8.2 / 9.1 | 1.3 / 1.5 → 1.3 / 1.5 | 3.9 / 4.3 → 1.8 / 2.0 |
| 0047 | 27.6 / 28.3 → 12.2 / 12.9 | 27.8 / 28.3 → 15.8 / 16.4 | 1.1 / 1.2 → 1.1 / 1.2 | 4.1 / 4.3 → 2.0 / 2.2 |
| 0048 | 15.1 / 17.0 → 6.7 / 7.7 | 15.7 / 16.1 → 9.7 / 10.1 | 14.6 / 15.0 → 8.8 / 9.1 | 3.7 / 3.8 → 1.7 / 1.7 |
| 0049 | 20.2 / 23.5 → 11.2 / 12.8 | 23.0 / 27.1 → 15.4 / 18.3 | 0.9 / 1.1 → 0.9 / 1.0 | 4.1 / 4.8 → 1.9 / 2.2 |
| 0050 | 6.8 / 6.9 → 3.6 / 3.8 | — | — | — |
| fenced_math_in_list_blockquote | 3.5 / 4.0 → 2.9 / 3.5 | — | — | — |

All 26 workloads that reached timing at the baseline still reached timing.
Across the nine shared documents, planner full-parse medians stayed at
56–82% of the incumbent's. Line-start edit medians went from 65–100% to
66–99%, typing medians from 62–101% to 63–102%, and whole-file replacement
medians from 42–77% to 45–79%. The existing CST or diagnostic mismatch
excluded `large_authoring.qmd` in both runs.

The following fixtures passed preflight with the newly enabled flavor
defaults. Numbered fixtures come from the Pandoc conformance corpus.
The other fixtures come from the parser golden suite.

| Flavor | Fixture | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | --- | ---: | ---: | ---: | ---: |
| Pandoc | 0098 | 5.2 / 8.6 → 2.5 / 4.3 | — | — | — |
| Pandoc | 0130 | 21.2 / 21.9 → 7.8 / 8.4 | 25.1 / 25.7 → 12.2 / 12.8 | 1.3 / 1.4 → 1.2 / 1.3 | 4.3 / 4.5 → 1.9 / 2.1 |
| Pandoc | 0141 | 12.0 / 16.1 → 4.4 / 5.9 | 12.6 / 14.3 → 7.2 / 8.1 | 0.9 / 1.0 → 1.0 / 1.1 | 3.9 / 4.3 → 1.9 / 2.1 |
| R Markdown | bookdown | 34.6 / 39.2 → 16.9 / 17.7 | 30.7 / 32.4 → 18.2 / 19.5 | 28.7 / 31.1 → 15.8 / 16.7 | 4.1 / 4.2 → 1.8 / 1.9 |
| R Markdown | bookdown_math_labels_host | 35.3 / 40.4 → 20.9 / 23.9 | 35.7 / 39.1 → 22.3 / 24.4 | 1.2 / 1.3 → 1.2 / 1.4 | 5.0 / 5.3 → 2.3 / 2.5 |
| R Markdown | bookdown_math_labels_nested | 19.9 / 23.1 → 11.0 / 12.9 | 16.8 / 17.1 → 12.1 / 12.5 | 15.2 / 15.7 → 10.0 / 10.3 | 4.0 / 4.1 → 1.7 / 1.8 |
| R Markdown | display_math_content_on_fence_line | 27.3 / 30.0 → 13.7 / 15.6 | 29.3 / 30.2 → 17.8 / 18.2 | 1.1 / 1.2 → 1.1 / 1.2 | 4.2 / 4.3 → 1.9 / 2.0 |
| R Markdown | issue_186_list_blockquote_lazy_idempotency | 31.6 / 34.8 → 14.4 / 15.4 | 35.9 / 40.6 → 19.0 / 21.8 | 1.1 / 1.3 → 1.1 / 1.2 | 4.8 / 5.3 → 2.2 / 2.5 |
| R Markdown | issue_195_blockquote_lazy_continuation_shape | 31.7 / 33.7 → 16.2 / 17.7 | — | — | — |
| R Markdown | issue_314_advance_columns_multibyte | 10.2 / 10.5 → 5.6 / 5.9 | 8.6 / 9.8 → 5.4 / 6.2 | 0.8 / 0.9 → 0.8 / 0.9 | 4.2 / 4.8 → 1.9 / 2.2 |
| R Markdown | rmarkdown_math | 34.2 / 40.0 → 22.9 / 27.8 | 37.1 / 41.3 → 28.0 / 31.4 | 33.3 / 37.4 → 23.6 / 27.0 | 4.1 / 4.6 → 1.8 / 2.0 |
| R Markdown | unresolved_reference_intraword_underscore_pandoc | 6.5 / 6.8 → 4.9 / 5.3 | — | — | — |
| MultiMarkdown | mmd_title_block | 3.4 / 3.6 → 1.9 / 2.0 | — | — | — |
| MultiMarkdown | mmd_link_attributes | 7.5 / 8.5 → 4.5 / 5.0 | — | — | — |
| MultiMarkdown | mmd_header_identifiers | 3.4 / 3.5 → 2.0 / 2.2 | — | — | — |
| MultiMarkdown | atx_interrupts_paragraph_multimarkdown | 4.2 / 4.7 → 1.6 / 1.8 | — | — | — |

The formatter fixture `issue_609_chunk_option_expressions_rmarkdown`
passed the original parse comparison and failed parity on the line-start
edit. The edit turns a chunk opener into an inline code span opener.
The planner treats the span's closing backticks as a new fence. The
baseline also failed on that edit with Quarto defaults.

### Inline code and nested fenced div checkpoint

Both release builds ran on logical CPU 4 of the Intel Core Ultra 7 155U
with Rust 1.98.1. The comparison reran the same files and resolved
options at baseline `7f781174` and after the fixes. All 42 previously
timed workloads and both previously excluded workloads passed preflight,
including the chained edits where the document had enough blank lines.

Across the nine previously timed documents, planner full-parse medians
went from 56–83% to 55–83% of the incumbent's. Line-start edit medians
went from 66–97% to 66–98%, typing medians from 62–97% to 64–97%, and
whole-file replacement medians from 44–72% to 45–76%.

Each cell gives the median and p95 in microseconds, with the incumbent
on the left and the planner on the right.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| `large_authoring.qmd` | 1172.6 / 1272.3 → 734.2 / 781.7 | 348.1 / 393.5 → 337.3 / 380.8 | 7.8 / 9.4 → 7.8 / 9.3 | 5.8 / 6.9 → 3.1 / 3.7 |
| `issue_609_chunk_option_expressions_rmarkdown` | 24.8 / 25.6 → 20.8 / 21.7 | 30.3 / 31.4 → 24.4 / 25.3 | 1.1 / 1.2 → 1.1 / 1.2 | 4.9 / 5.2 → 2.5 / 2.6 |

### GFM and Jupyter checkpoint

Both release builds ran on logical CPU 4 of the Intel Core Ultra 7 155U
with Rust 1.98.1. All 44 workloads from the inline code and nested fenced
div checkpoint passed source, CST, and diagnostic parity at baseline
`dde1282b` and with GFM and Jupyter enabled. The nine focused fixtures
also passed with their checked-in parser options. The baseline rejected
both flavors before timing. The comparison checked each chained edit on
fixtures with enough blank lines to run the edit sequence.

Across the ten shared documents, the planner took the following
percentages of the incumbent’s time.

| Operation | Median, baseline → current | p95, baseline → current |
| --- | ---: | ---: |
| Full parse | 55–83% → 57–83% | 56–82% → 54–83% |
| Line-start edit | 65–98% → 67–99% | 61–99% → 65–97% |
| Typing edit | 62–101% → 63–100% | 61–101% → 61–98% |
| Whole-file replacement | 42–75% → 47–80% | 44–87% → 45–78% |

Timings are in microseconds. Each cell shows median and p95 for the
incumbent on the left and the planner on the right. A dash marks
a fixture with fewer than two blank lines for the edit sequence.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| `README.md` | 445.8 / 501.9 → 268.5 / 302.7 | 249.9 / 269.8 → 213.6 / 231.8 | 5.9 / 6.3 → 5.6 / 5.9 | 4.7 / 5.1 → 2.8 / 3.0 |
| `benches/documents/pandoc_manual.md` | 9152.3 / 10016.0 → 5294.0 / 5778.6 | 2079.9 / 2397.2 → 2065.9 / 2304.3 | 84.0 / 98.7 → 84.2 / 96.3 | 9.1 / 18.5 → 7.3 / 14.5 |
| `crates/panache-parser/README.md` | 32.6 / 36.1 → 18.8 / 19.5 | 28.4 / 30.1 → 18.9 / 19.7 | 17.2 / 19.1 → 10.9 / 11.6 | 3.0 / 3.3 → 1.4 / 1.5 |
| `docs/guide/integrations.qmd` | 349.6 / 394.7 → 216.3 / 244.9 | 166.5 / 186.4 → 140.3 / 158.2 | 75.3 / 88.2 → 62.5 / 71.1 | 5.4 / 6.2 → 2.7 / 3.0 |
| `docs/getting-started.qmd` | 565.8 / 639.3 → 320.0 / 364.0 | 178.1 / 196.7 → 161.2 / 181.9 | 79.0 / 88.7 → 70.8 / 79.6 | 4.8 / 5.3 → 2.4 / 2.6 |
| `docs/guide/performance.qmd` | 1854.9 / 2064.5 → 1543.9 / 1707.8 | 316.6 / 356.0 → 293.8 / 332.3 | 163.7 / 186.0 → 145.4 / 166.7 | 5.8 / 6.9 → 2.8 / 3.3 |
| `benches/documents/large_authoring.qmd` | 1118.7 / 1175.0 → 691.5 / 740.5 | 346.8 / 390.4 → 330.2 / 373.6 | 8.0 / 9.1 → 7.8 / 8.9 | 5.7 / 6.6 → 3.0 / 3.5 |
| `benches/documents/tables.qmd` | 1366.1 / 1487.2 → 896.8 / 974.9 | 358.0 / 403.7 → 338.1 / 379.5 | 176.1 / 204.3 → 163.9 / 185.1 | 5.7 / 6.6 → 2.9 / 3.4 |
| `benches/documents/math.qmd` | 961.6 / 1084.9 → 609.5 / 685.5 | 317.7 / 355.2 → 303.6 / 343.7 | 139.9 / 161.6 → 130.3 / 148.3 | 5.2 / 5.9 → 2.6 / 2.9 |
| `benches/documents/math.qmd (grid tables disabled)` | 999.6 / 1075.8 → 627.6 / 675.2 | 317.5 / 357.5 → 300.3 / 335.2 | 142.4 / 159.5 → 128.5 / 148.2 | 5.3 / 6.0 → 2.6 / 3.0 |
| `Quarto 0123` | 18.5 / 20.3 → 8.1 / 9.0 | 22.8 / 25.6 → 11.9 / 13.5 | 20.8 / 23.6 → 9.9 / 11.7 | 4.0 / 4.6 → 1.6 / 1.9 |
| `Quarto 0124` | 11.6 / 13.3 → 7.6 / 8.7 | 18.2 / 21.2 → 11.8 / 14.3 | 6.7 / 7.5 → 4.4 / 5.1 | 3.7 / 4.4 → 1.5 / 1.7 |
| `Quarto 0541` | 6.3 / 6.7 → 3.8 / 4.2 | — | — | — |
| `Quarto 0199` | 11.5 / 12.1 → 13.3 / 13.8 | 14.8 / 16.9 → 16.5 / 18.1 | 14.0 / 16.1 → 15.8 / 17.4 | 3.8 / 4.2 → 1.6 / 1.7 |
| `Quarto 0481` | 7.2 / 7.9 → 6.2 / 6.9 | — | — | — |
| `Quarto 0495` | 11.8 / 12.3 → 11.0 / 11.6 | — | — | — |
| `Quarto 0610` | 12.8 / 13.2 → 12.0 / 12.5 | — | — | — |
| `Quarto 0611` | 11.5 / 11.9 → 10.9 / 11.5 | — | — | — |
| `Quarto 0098` | 4.9 / 5.1 → 2.4 / 2.5 | — | — | — |
| `Quarto 0130` | 19.0 / 19.6 → 7.1 / 7.7 | 22.4 / 23.3 → 11.0 / 11.5 | 1.1 / 1.2 → 1.1 / 1.1 | 3.9 / 4.1 → 1.7 / 1.8 |
| `Quarto 0141` | 12.3 / 12.6 → 4.6 / 4.8 | 12.9 / 13.1 → 7.4 / 7.7 | 0.8 / 0.9 → 0.9 / 1.0 | 3.9 / 4.1 → 1.8 / 2.0 |
| `Quarto 0095` | 8.8 / 9.0 → 4.2 / 4.5 | 13.3 / 14.9 → 8.3 / 9.3 | 1.3 / 1.4 → 1.3 / 1.4 | 4.0 / 4.5 → 1.8 / 2.1 |
| `Quarto 0047` | 31.5 / 32.7 → 13.9 / 14.6 | 28.3 / 31.9 → 16.0 / 18.4 | 1.1 / 1.3 → 1.1 / 1.3 | 4.1 / 4.6 → 2.0 / 2.4 |
| `Quarto 0048` | 15.3 / 16.4 → 6.8 / 7.3 | 15.7 / 17.7 → 9.6 / 10.7 | 14.6 / 15.9 → 8.6 / 9.4 | 3.6 / 3.9 → 1.5 / 1.6 |
| `Quarto 0049` | 20.5 / 24.1 → 11.4 / 13.7 | 22.8 / 24.1 → 15.5 / 16.4 | 0.9 / 0.9 → 0.9 / 1.0 | 4.0 / 4.2 → 1.8 / 2.0 |
| `Quarto 0050` | 7.0 / 8.0 → 3.9 / 4.3 | — | — | — |
| `fenced_math_in_list_blockquote` | 3.6 / 3.9 → 3.1 / 3.4 | — | — | — |
| `Pandoc 0098` | 5.4 / 5.8 → 2.4 / 2.8 | — | — | — |
| `Pandoc 0130` | 20.9 / 21.6 → 7.7 / 8.5 | 22.5 / 25.5 → 11.0 / 12.6 | 1.1 / 1.3 → 1.1 / 1.2 | 3.9 / 4.5 → 1.7 / 2.0 |
| `Pandoc 0141` | 12.1 / 12.4 → 4.5 / 4.7 | 12.7 / 13.6 → 7.3 / 8.1 | 0.8 / 0.9 → 0.9 / 1.0 | 4.0 / 4.4 → 1.9 / 2.0 |
| `bookdown` | 35.0 / 38.7 → 16.8 / 18.8 | 30.7 / 34.7 → 18.0 / 20.4 | 28.6 / 32.3 → 15.4 / 17.4 | 3.9 / 4.4 → 1.7 / 2.0 |
| `bookdown_math_labels_host` | 39.0 / 41.7 → 23.1 / 24.9 | 35.9 / 40.8 → 22.7 / 25.4 | 1.2 / 1.4 → 1.1 / 1.3 | 4.9 / 5.5 → 2.3 / 2.5 |
| `bookdown_math_labels_nested` | 20.1 / 22.7 → 11.3 / 13.9 | 16.6 / 18.4 → 12.1 / 12.8 | 15.1 / 16.5 → 9.8 / 10.4 | 3.9 / 4.2 → 1.7 / 1.9 |
| `display_math_content_on_fence_line` | 27.8 / 31.4 → 13.7 / 15.4 | 29.3 / 32.8 → 17.7 / 19.5 | 1.0 / 1.1 → 1.0 / 1.1 | 4.1 / 4.6 → 1.9 / 2.1 |
| `issue_186_list_blockquote_lazy_idempotency` | 32.1 / 36.2 → 15.2 / 17.4 | 35.5 / 36.4 → 19.3 / 20.7 | 1.1 / 1.2 → 1.0 / 1.1 | 4.6 / 4.9 → 2.1 / 2.3 |
| `issue_195_blockquote_lazy_continuation_shape` | 29.4 / 31.7 → 15.4 / 16.7 | — | — | — |
| `issue_314_advance_columns_multibyte` | 9.3 / 9.6 → 5.3 / 5.5 | 8.6 / 10.6 → 5.6 / 6.3 | 0.8 / 0.9 → 0.7 / 0.8 | 4.1 / 5.0 → 1.9 / 2.1 |
| `rmarkdown_math` | 34.4 / 38.1 → 22.6 / 25.6 | 38.0 / 44.4 → 28.3 / 32.2 | 34.2 / 39.2 → 24.1 / 27.4 | 4.1 / 4.7 → 1.8 / 2.1 |
| `unresolved_reference_intraword_underscore_pandoc` | 7.3 / 7.5 → 5.3 / 5.7 | — | — | — |
| `issue_609_chunk_option_expressions_rmarkdown` | 25.6 / 26.9 → 21.1 / 22.0 | 30.5 / 31.5 → 24.5 / 25.5 | 1.1 / 1.1 → 1.0 / 1.1 | 4.8 / 5.1 → 2.4 / 2.6 |
| `mmd_title_block` | 3.4 / 3.5 → 1.8 / 1.9 | — | — | — |
| `mmd_link_attributes` | 7.4 / 8.4 → 4.4 / 5.1 | — | — | — |
| `mmd_header_identifiers` | 3.2 / 3.4 → 2.0 / 2.1 | — | — | — |
| `atx_interrupts_paragraph_multimarkdown` | 4.1 / 4.3 → 1.7 / 1.8 | — | — | — |

Focused fixtures from the parser golden suite.

| Workload | Full parse | Line-start edit | Typing edit | Whole-file replacement |
| --- | ---: | ---: | ---: | ---: |
| `gfm_alert_block_boundaries` | 16.8 / 17.8 → 10.3 / 11.0 | 10.1 / 10.8 → 7.7 / 8.4 | 8.0 / 8.4 → 5.8 / 6.1 | 3.9 / 4.2 → 2.0 / 2.1 |
| `gfm_task_block_boundaries` | 22.8 / 23.9 → 12.5 / 13.9 | 7.1 / 7.7 → 5.6 / 6.0 | 1.8 / 1.9 → 1.6 / 1.8 | 4.5 / 4.9 → 2.1 / 2.3 |
| `html_block_div_footnote_body_gfm` | 7.7 / 8.1 → 5.8 / 6.1 | — | — | — |
| `jupyter_markdown` | 39.6 / 44.2 → 28.1 / 29.5 | 42.0 / 44.4 → 33.0 / 35.4 | 2.2 / 2.5 → 2.1 / 2.4 | 5.2 / 5.7 → 3.0 / 3.4 |
| `jupyter_math_block_precedence` | 38.7 / 41.0 → 29.5 / 32.2 | 30.6 / 31.8 → 25.7 / 26.6 | 1.9 / 2.0 → 1.8 / 1.9 | 5.3 / 5.5 → 2.9 / 3.1 |
| `jupyter_math_headings` | 51.9 / 56.4 → 38.9 / 40.7 | 53.3 / 60.7 → 40.6 / 46.5 | 49.5 / 56.2 → 37.1 / 42.1 | 4.4 / 5.1 → 2.4 / 2.8 |
| `jupyter_math_multiline_tables` | 82.9 / 89.5 → 47.9 / 50.8 | 50.8 / 57.7 → 32.7 / 37.0 | 41.6 / 46.6 → 23.5 / 26.6 | 5.2 / 5.9 → 2.8 / 3.2 |
| `jupyter_math_setext` | 31.0 / 33.5 → 23.2 / 25.1 | 31.8 / 33.8 → 26.4 / 27.5 | 28.7 / 31.3 → 23.1 / 23.9 | 4.6 / 4.8 → 2.5 / 2.7 |
| `jupyter_math_setext_blank_before_header` | 6.3 / 6.8 → 3.6 / 4.0 | — | — | — |
