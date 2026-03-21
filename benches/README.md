# Benchmarking and Profiling Guide

## Quick Start

### Running Benchmarks

```bash
# Download test documents (first time only)
cd benches/documents && ./download.sh && cd ../..

# Run benchmarks
cargo bench --bench formatting

# Run LSP incremental didChange benchmarks
cargo bench --bench lsp_incremental

# Generate docs + machine-readable JSON
./benches/generate_docs.sh
```

### Profiling

For line-level profiling with flame graphs:

```bash
# Install flamegraph
cargo install flamegraph

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
# PANACHE_LSP_BENCH_CAP=8 (default)
# PANACHE_LSP_BENCH_ITERATIONS=80 (default)
# PANACHE_LSP_BENCH_OUTPUT_JSON=benches/lsp_incremental_results.json
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
- **`benches/compare_all.sh`**: Multi-formatter comparison
  - Compares panache vs Prettier vs Pandoc

  - Saves results to `benchmark_results.txt`

  - Use to update `docs/performance.qmd`
- **`benches/generate_docs.sh`**: Captures results for documentation
  - Generates `benches/benchmark_results.json` (machine-readable)
  - Renders `docs/benchmarks.qmd` from JSON
  - Deterministic output for CI checks

## What to Benchmark

Good targets for benchmarking: - **Full pipeline** (parse + format) - what
users experience - **Parse speed** - CST construction - **Format speed** - CST
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
