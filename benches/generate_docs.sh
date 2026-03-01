#!/usr/bin/env bash

# Script to generate benchmark results for documentation
# Output is saved to docs/benchmarks.qmd

set -e

OUTPUT_FILE="docs/benchmarks.qmd"
TIMESTAMP=$(date -u +"%Y-%m-%d %H:%M UTC")
COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")

echo "Running benchmarks..."
BENCH_OUTPUT=$(cargo bench --bench formatting 2>&1)

cat > "$OUTPUT_FILE" << EOF
---
title: "Performance Benchmarks"
description: "panache performance metrics on real-world documents"
---

::: {.callout-note}
Last updated: $TIMESTAMP
Commit: \`$COMMIT\`
:::

## Benchmark Suite

Benchmarks are run on real Quarto documents to measure realistic performance.

### Test Documents

- **Small**: Synthetic document (747 bytes) with mixed content
- **Medium**: Quarto tutorial from quarto-dev/quarto-web (~9KB)
- **Tables**: Table-heavy document from Quarto docs (~19KB)
- **Math**: Computation-heavy document with equations (~29KB)
- **Large**: Comprehensive authoring guide (~30KB)

### Methodology

- Each benchmark runs multiple iterations (1000 for small, 20-100 for larger)
- Measures three phases: full pipeline (parse+format), parse only, format only
- Reports average time per iteration and throughput (KB/s)

## Results

\`\`\`
$BENCH_OUTPUT
\`\`\`

## Interpretation

- **Full pipeline**: What users experience when running \`panache format\`
- **Parse only**: Time to build the CST (concrete syntax tree)
- **Format only**: Time to traverse CST and generate output
- **Throughput**: Processing speed in KB/second

## Reproducing

To reproduce these benchmarks:

\`\`\`bash
# Download test documents
cd benches/documents && ./download.sh

# Run benchmarks
cargo bench --bench formatting
\`\`\`
EOF

echo "✅ Benchmark results saved to $OUTPUT_FILE"
echo
echo "Add to your docs with:"
echo "  git add $OUTPUT_FILE"
echo "  git commit -m 'docs: update benchmark results'"
