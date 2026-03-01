# Benchmark Documents

This directory contains documents used for benchmarking panache performance.

## Document Sources

Documents are not committed to the repository to keep it lightweight. Instead,
they are downloaded on-demand using the setup script.

### Standard Benchmark Suite

The benchmark suite includes:

1. **Small document** (\~1KB) - Simple mixed content
2. **Medium document** (\~50KB) - Real-world Quarto tutorial
3. **Large document** (\~200KB) - Complex academic paper with tables/math
4. **Table-heavy** (\~30KB) - Document with many complex tables
5. **Math-heavy** (\~20KB) - Document with extensive mathematical notation
6. **Pandoc MANUAL stress doc** (\~8000 lines) - downloaded from upstream pandoc
   `MANUAL.txt` as `pandoc_manual.md`

## Setup

Download the benchmark documents:

```bash
./download.sh
```

Or manually:

```bash
# Medium: Quarto tutorial
curl -o medium_quarto.qmd https://raw.githubusercontent.com/quarto-dev/quarto-web/main/docs/get-started/hello/rstudio.qmd

# Large: Quarto authoring guide (complex)
curl -o large_authoring.qmd https://raw.githubusercontent.com/quarto-dev/quarto-web/main/docs/authoring/markdown-basics.qmd

# Table-heavy: Tables documentation
curl -o tables.qmd https://raw.githubusercontent.com/quarto-dev/quarto-web/main/docs/authoring/tables.qmd

# Math-heavy: Julia computational documents
curl -o math.qmd https://raw.githubusercontent.com/quarto-dev/quarto-web/main/docs/computations/julia.qmd

# Stress-test document from upstream pandoc
curl -o pandoc_manual.md https://raw.githubusercontent.com/jgm/pandoc/refs/heads/main/MANUAL.txt
```

## Regenerating Benchmarks

To run benchmarks with the downloaded documents:

```bash
cargo bench --bench formatting
```

## Directory Structure

```
benches/documents/
├── README.md           # This file
├── download.sh         # Download script
├── small.qmd           # Committed - small synthetic document
├── medium_quarto.qmd   # Downloaded - not in git
├── large_authoring.qmd # Downloaded - not in git
├── tables.qmd          # Downloaded - not in git
├── math.qmd            # Downloaded - not in git
└── pandoc_manual.md    # Downloaded from upstream pandoc MANUAL.txt - not in git
```
