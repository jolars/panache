# Benchmarking and Profiling Guide

## Quick Start

### Running Benchmarks

```bash
# Download test documents (first time only)
cd benches/documents && ./download.sh && cd ../..

# Run benchmarks
cargo bench --bench formatting

# Isolate the stable math formatter on a selected corpus document (`reflow` is
# the default; set `PANACHE_BENCH_FORMAT_MATH=0` for a verbatim comparison)
PANACHE_BENCH_DOC=math.qmd \
  cargo bench --bench formatting

# Run LSP incremental didChange benchmarks
cargo bench --bench lsp_incremental

# Same run, with every case checked against the contract it declares
task bench:incremental-gate

# Run LSP write-phase benchmarks (what a keystroke costs before any parse)
cargo bench --bench lsp_write_phase

# Same run, gated; `task bench:lsp-gate` runs both LSP gates
task bench:write-phase-gate

# Run the LSP settle benchmark (what publishing one document costs per settle)
cargo bench --bench lsp_settle

# Compare Panache and Marksman language-server memory on Linux
task bench:lsp-memory

# Run interned key impact benchmark
cargo bench --bench interned_keys

# Run CLI cache cold-vs-warm benchmark
cargo bench --bench cli_cache

# Generate docs + machine-readable JSON
./benches/generate_docs.sh
```

### Profiling

For line-level profiling with flame graphs:

```bash
# Install flamegraph
cargo install --locked flamegraph

# Profile the benchmark
cargo flamegraph --bench formatting

# Opens flamegraph.svg showing hotspots
```

For large-document bottlenecks (e.g. Pandoc MANUAL):

```bash
# Ensure pandoc_manual.md exists first
cd benches/documents && ./download.sh && cd ../..

# Profile only the selected document
PANACHE_BENCH_DOC=pandoc_manual.md PANACHE_BENCH_ITERATIONS=3 \
    cargo flamegraph --bench formatting

# LSP incremental benchmark knobs
# PANACHE_LSP_BENCH_ITERATIONS=80 (default)
# PANACHE_LSP_BENCH_OUTPUT_JSON=benches/lsp_incremental_results.json
# PANACHE_LSP_BENCH_ASSERT=1 (check thresholds; exit 1 on a violation)

# LSP write-phase benchmark knobs
# PANACHE_LSP_WRITE_BENCH_ITERATIONS=1.0 (scale factor on every row)
# PANACHE_LSP_WRITE_BENCH_OUTPUT_JSON=benches/lsp_write_phase_results.json
# PANACHE_LSP_WRITE_BENCH_ASSERT=1 (check thresholds; exit 1 on a violation)

# LSP settle benchmark knobs
# PANACHE_LSP_SETTLE_BENCH_ITERS=200 (default)
```

For more detailed profiling:

```bash
# Linux perf (CPU profiling)
perf record --call-graph dwarf cargo bench --bench formatting
perf report

# Line-level annotation for selected stress document
PANACHE_BENCH_DOC=pandoc_manual.md PANACHE_BENCH_ITERATIONS=3 \
    perf record --call-graph dwarf cargo bench --bench formatting
perf annotate

# Valgrind (memory profiling)
valgrind --tool=cachegrind cargo bench --bench formatting
```

## Benchmark Infrastructure

### Document Management

- **`benches/documents/`**: Test documents for benchmarking
  - `small.qmd`: Committed baseline (747 bytes)

  - `pandoc_manual.md`: Stress-test doc downloaded from upstream pandoc
    `MANUAL.txt`

  - Other files: Downloaded on-demand from Quarto docs

  - `.gitignore`: Excludes downloaded files from repo
- **`benches/documents/download.sh`**: Downloads real Quarto documents
  - Reproducible: same sources every time
  - Lightweight: doesn't bloat repo

### Benchmark Code

- **`benches/formatting.rs`**: Main benchmark suite
  - Tests parse, format, and full pipeline

  - Multiple document sizes and types

  - Reports throughput in KB/s
- **`benches/interned_keys.rs`**: Key interning measurement harness
  - Compares owned vs interned key map build costs
  - Reports repeated-byte potential from duplicated keys
- **`benches/cli_cache.rs`**: CLI cache warm-hit benchmark harness
  - Compares uncached (cold) vs cached (warm) runs for `format --check` and
    `lint`
  - Tunable with `PANACHE_CLI_CACHE_BENCH_FILES` and
    `PANACHE_CLI_CACHE_BENCH_ITERATIONS`
  - Optional JSON output via `PANACHE_CLI_CACHE_BENCH_OUTPUT_JSON`
- **`benches/compare_all.sh`**: Multi-formatter comparison
  - Compares panache, Prettier, Pandoc, rumdl, mdformat, and Yamark across six
    documents (Pandoc testsuite, tables, configuration, math, large, and the
    full Pandoc manual)
  - **Text mode (default)**: prints colored results and appends to
    `benchmark_results.txt`
  - **JSON mode**: `bash benches/compare_all.sh --json [--out PATH]` writes
    structured JSON consumed by `docs/guide/performance.qmd`. Default output
    path is `docs/guide/performance_data.json`.
  - Prefers [hyperfine](https://github.com/sharkdp/hyperfine) for stats when
    available (and `jq` for parsing); otherwise falls back to a simple shell
    timing loop emitting mean only (`stddev_ms`/`min_ms`/`max_ms` are `null`).
  - Drives `docs/guide/performance.qmd`. Refresh the JSON explicitly, then
    delete `docs/_freeze/guide/performance/` and re-render to display it.
- **`benches/compare_multifile.sh`**: Local Markdown corpus comparison
  - Compares panache, Prettier, rumdl, and Yamark in a single process per tool.
  - Restores the input files before every sample because tools format in place.
- **`benches/compare_repo_suite.sh`**: Repository formatting and linting
  comparisons
  - The formatting suite includes Yamark on both the Markdown and Quarto tracks.
  - Restores tracked documents before every sample. Failed runs are recorded
    with null timings and excluded from the performance plots.
- **`benches/compare_lsp_memory.sh`**: Linux language-server memory comparison
  - Checks out a pinned revision of the Rust Book into a gitignored directory.
  - Opens the five largest tracked Markdown files under `src/`, exercises
    diagnostics and navigation, and performs 1,000 reference-label edits.
  - Runs three fresh processes per server in alternating order and records the
    median whole-process-tree RSS and PSS at baseline, settled, edited, and peak
    milestones.
  - Launches Panache with an isolated GFM config and gives both servers isolated
    user config and cache directories.
  - Writes raw runs and aggregate comparisons to
    `docs/guide/performance_lsp_memory_data.json` by default. Override the run
    with the `PANACHE_LSP_MEMORY_RUNS`, `PANACHE_LSP_MEMORY_OPEN_FILES`,
    `PANACHE_LSP_MEMORY_EDITS`, `PANACHE_LSP_MEMORY_QUIET_SECONDS`, and
    `PANACHE_LSP_MEMORY_SETTLE_TIMEOUT` environment variables.
- **`benches/generate_docs.sh`**: Captures results for documentation
  - Generates `benches/benchmark_results.json` (machine-readable)
  - Renders `docs/benchmarks.qmd` from JSON
  - Deterministic output for CI checks

### Yamark

The formatting comparison scripts include
[Yamark](https://github.com/t-kalinowski/yamark) when `yamark` is on `PATH`. The
development environment provides a pinned build and sets its benchmark version
automatically. Outside `devenv`, install it before benchmarking:

```bash
uv tool install yamark==0.3.0
export PANACHE_BENCH_YAMARK_VERSION=0.3.0

bash benches/compare_all.sh --json
bash benches/compare_multifile.sh
bash benches/compare_repo_suite.sh --mode format --track markdown --out docs/guide/performance_repo_markdown_format_data.json
bash benches/compare_repo_suite.sh --mode format --track quarto --out docs/guide/performance_repo_quarto_format_data.json
```

Yamark 0.3.0 has no version command, so the scripts record
`PANACHE_BENCH_YAMARK_VERSION`, or `unknown` when it is unset.

Timed commands invoke `yamark` directly. They use `--config /dev/null` to avoid
ambient configuration and `--skip-embedded-formatters` to exclude external code
formatters. Single-document runs use stdin with `--stdin-file-path`; batch runs
format a fresh copy of the corpus in place. Yamark's default wrapping and style
settings apply. These comparisons measure each tool's formatting policy and
supported syntax, which differ across tools.

## What to Benchmark

Good targets for benchmarking: - **Full pipeline** (parse + format) - what users
experience - **Parse speed** - CST construction - **Format speed** - CST
traversal and output - **Document types** - simple text vs complex (tables,
math, divs) - **Document sizes** - small (1KB), medium (10-50KB), large (100KB+)

## Performance Tips

Current performance baseline: - \~20MB/s throughput on typical documents - \~1ms
to format a 30KB document - Parse takes \~30-40% of time, format \~60-70%

To improve performance, profile with flamegraph to find hotspots.

## Adding New Benchmarks

1. Add document to `benches/documents/` (or update `download.sh`)
2. Load in `benches/formatting.rs` with `load_document()`
3. Call `run_benchmark()` with appropriate iteration count
4. Run and verify results

## Integrating with Docs

After running benchmarks:

```bash
# Generate fresh benchmark page
./benches/generate_docs.sh

# Verify tracked artifacts are up to date (CI-friendly)
./benches/check_docs.sh

# Preview in Quarto
cd docs && quarto preview

# Commit to repo
git add benches/benchmark_results.json docs/benchmarks.qmd
git commit -m "docs: update benchmark results"
```
