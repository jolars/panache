#!/usr/bin/env bash

set -e

DOCS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DOCS_DIR"

echo "Downloading benchmark documents..."
echo

# Medium: Quarto getting started tutorial
echo "📄 Downloading medium_quarto.qmd..."
curl -sL -o medium_quarto.qmd \
  https://raw.githubusercontent.com/quarto-dev/quarto-web/main/docs/get-started/hello/rstudio.qmd

# Large: Markdown basics (comprehensive)
echo "📄 Downloading large_authoring.qmd..."
curl -sL -o large_authoring.qmd \
  https://raw.githubusercontent.com/quarto-dev/quarto-web/main/docs/authoring/markdown-basics.qmd

# Table-heavy
echo "📄 Downloading tables.qmd..."
curl -sL -o tables.qmd \
  https://raw.githubusercontent.com/quarto-dev/quarto-web/main/docs/authoring/tables.qmd

# Math-heavy (using computational documents as they have more math)
echo "📄 Downloading math.qmd..."
curl -sL -o math.qmd \
  https://raw.githubusercontent.com/quarto-dev/quarto-web/main/docs/computations/julia.qmd

echo "📄 Downloading pandoc_manual.md..."
curl -sL -o pandoc_manual.md \
  https://raw.githubusercontent.com/jgm/pandoc/refs/heads/main/MANUAL.txt

echo
echo "✅ Benchmark documents downloaded successfully!"
echo
echo "File sizes:"
du -h *.qmd *.md 2>/dev/null || true
echo
echo "Run benchmarks with: cargo bench --bench formatting"
