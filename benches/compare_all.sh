#!/usr/bin/env bash

# Comprehensive benchmark comparing panache against other markdown formatters

set -e

DOCS_DIR="benches/documents"
RESULTS_FILE="benches/benchmark_results.txt"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

# Check what formatters are available
HAVE_PRETTIER=$(command -v prettier >/dev/null 2>&1 && echo "yes" || echo "no")
HAVE_PANDOC=$(command -v pandoc >/dev/null 2>&1 && echo "yes" || echo "no")
HAVE_RUMDL=$(command -v rumdl >/dev/null 2>&1 && echo "yes" || echo "no")

echo "================================"
echo "Multi-Formatter Benchmark"
echo "================================"
echo
echo "Available formatters:"
echo "  panache: yes (building...)"
[ "$HAVE_PRETTIER" = "yes" ] && echo "  prettier: yes ($(prettier --version))"
[ "$HAVE_PANDOC" = "yes" ] && echo "  pandoc: yes ($(pandoc --version | head -1 | cut -d' ' -f2))"
[ "$HAVE_RUMDL" = "yes" ] && echo "  rumdl: yes ($(rumdl --version | awk '{print $2}'))"
echo

# Build panache
cargo build --release --quiet 2>&1 | grep -v "warning:" || true
PANACHE="./target/release/panache"

# Clear results file
> "$RESULTS_FILE"

benchmark_formatter() {
    local formatter=$1
    local file=$2
    local iterations=$3
    local cmd=$4

    local start=$(date +%s%N)
    for ((i=1; i<=iterations; i++)); do
        eval "$cmd" > /dev/null 2>&1
    done
    local end=$(date +%s%N)

    local total_time=$((end - start))
    local avg_time=$((total_time / iterations / 1000)) # microseconds

    echo "$avg_time"
}

benchmark_document() {
    local file=$1
    local name=$2
    local iterations=$3

    if [ ! -f "$DOCS_DIR/$file" ]; then
        echo "⚠️  Skipping $name - file not found"
        return
    fi

    local size=$(wc -c < "$DOCS_DIR/$file")
    local lines=$(wc -l < "$DOCS_DIR/$file")

    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo -e "${BLUE}$name${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "File: $file ($size bytes, $lines lines)"
    echo "Iterations: $iterations"
    echo

    # Benchmark panache
    echo -ne "${GREEN}panache:${NC}    "
    local panache_time=$(benchmark_formatter "panache" "$file" "$iterations" \
        "$PANACHE format < $DOCS_DIR/$file")
    local panache_ms=$(awk "BEGIN {printf \"%.2f\", $panache_time / 1000}")
    printf "%8s µs (%6s ms)\n" "$panache_time" "$panache_ms"

    # Benchmark prettier if available
    if [ "$HAVE_PRETTIER" = "yes" ]; then
        echo -ne "${YELLOW}prettier:${NC}  "
        local prettier_time=$(benchmark_formatter "prettier" "$file" "$iterations" \
            "prettier --parser markdown $DOCS_DIR/$file")
        local prettier_ms=$(awk "BEGIN {printf \"%.2f\", $prettier_time / 1000}")
        local speedup=$(awk "BEGIN {printf \"%.1f\", $prettier_time / $panache_time}")
        printf "%8s µs (%6s ms) - panache is %sx faster\n" "$prettier_time" "$prettier_ms" "$speedup"
    fi

    # Benchmark pandoc if available
    if [ "$HAVE_PANDOC" = "yes" ]; then
        echo -ne "${RED}pandoc:${NC}     "
        local pandoc_time=$(benchmark_formatter "pandoc" "$file" "$iterations" \
            "pandoc $DOCS_DIR/$file -f markdown -t markdown")
        local pandoc_ms=$(awk "BEGIN {printf \"%.2f\", $pandoc_time / 1000}")
        local speedup=$(awk "BEGIN {printf \"%.1f\", $pandoc_time / $panache_time}")
        printf "%8s µs (%6s ms) - panache is %sx faster\n" "$pandoc_time" "$pandoc_ms" "$speedup"
    fi

    # Benchmark rumdl if available
    if [ "$HAVE_RUMDL" = "yes" ]; then
        echo -ne "${BLUE}rumdl:${NC}      "
        local rumdl_time=$(benchmark_formatter "rumdl" "$file" "$iterations" \
            "rumdl fmt --fix --stdin --no-cache --silent < $DOCS_DIR/$file")
        local rumdl_ms=$(awk "BEGIN {printf \"%.2f\", $rumdl_time / 1000}")
        local speedup=$(awk "BEGIN {printf \"%.1f\", $rumdl_time / $panache_time}")
        printf "%8s µs (%6s ms) - panache is %sx faster\n" "$rumdl_time" "$rumdl_ms" "$speedup"
    fi

    echo

    # Save to results file
    {
        echo "### $name"
        echo "- panache: ${panache_time}µs (${panache_ms}ms)"
        [ "$HAVE_PRETTIER" = "yes" ] && echo "- prettier: ${prettier_time}µs (${prettier_ms}ms)"
        [ "$HAVE_PANDOC" = "yes" ] && echo "- pandoc: ${pandoc_time}µs (${pandoc_ms}ms)"
        [ "$HAVE_RUMDL" = "yes" ] && echo "- rumdl: ${rumdl_time}µs (${rumdl_ms}ms)"
        echo
    } >> "$RESULTS_FILE"
}

# Run benchmarks with appropriate iteration counts
benchmark_document "small.qmd" "Small Document (747 bytes)" 50
benchmark_document "medium_quarto.qmd" "Medium Document (9 KB)" 20
benchmark_document "tables.qmd" "Tables Document (19 KB)" 20
benchmark_document "math.qmd" "Math Document (29 KB)" 20
benchmark_document "large_authoring.qmd" "Large Document (30 KB)" 10

echo "================================"
echo "Benchmark Complete"
echo "================================"
echo
echo "Results saved to: $RESULTS_FILE"
