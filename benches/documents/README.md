# Benchmark Documents

This directory contains documents used for benchmarking panache performance.

## Document Sources

The benchmark corpus mixes realistic project documents, upstream test fixtures,
and a few targeted stress cases. The setup script refreshes the copied or
downloaded files into this directory.

### Standard Benchmark Suite

The benchmark suite includes:

1. **Pandoc testsuite fixture** (\~9KB) - downloaded from upstream pandoc
   `test/testsuite.txt` as `pandoc_testsuite.md`
2. **Configuration guide** (\~24KB) - copied from `docs/guide/configuration.qmd`
3. **Table-heavy** (\~19KB) - Quarto tables documentation
4. **Math-heavy** (\~29KB) - Quarto computational document with extensive math
5. **Large authoring guide** (\~30KB) - Quarto markdown authoring guide
6. **Pandoc MANUAL stress doc** (\~8000 lines) - downloaded from upstream pandoc
   `MANUAL.txt` as `pandoc_manual.md`

## Setup

Download the benchmark documents:

```bash
./download.sh
```

Or manually:

```bash
# Local realistic doc + upstream fixture
cp ../../docs/guide/configuration.qmd configuration.qmd
curl -o pandoc_testsuite.md https://raw.githubusercontent.com/jgm/pandoc/main/test/testsuite.txt

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
├── configuration.qmd   # Copied from docs/guide/configuration.qmd
├── pandoc_testsuite.md # Downloaded from upstream pandoc testsuite
├── large_authoring.qmd # Downloaded - not in git
├── tables.qmd          # Downloaded - not in git
├── math.qmd            # Downloaded - not in git
└── pandoc_manual.md    # Downloaded from upstream pandoc MANUAL.txt - not in git
```
