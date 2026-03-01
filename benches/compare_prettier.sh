#!/usr/bin/env bash

# Benchmark Prettier vs Panache on the same documents

set -e

DOCS_DIR="benches/documents"
ITERATIONS_SMALL=50
ITERATIONS_MEDIUM=20
ITERATIONS_LARGE=10

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "================================"
echo "Prettier vs Panache Benchmark"
echo "================================"
echo

# Build panache in release mode
echo "Building panache..."
cargo build --release --quiet
PANACHE="./target/release/panache"

benchmark_file() {
    local file=$1
    local iterations=$2
    local name=$3

    if [ ! -f "$DOCS_DIR/$file" ]; then
        echo "⚠️  Skipping $name - file not found"
        return
    fi

    local size=$(wc -c < "$DOCS_DIR/$file")
    local lines=$(wc -l < "$DOCS_DIR/$file")

    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo -e "${BLUE}$name${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "File: $file"
    echo "Size: $size bytes, $lines lines"
    echo "Iterations: $iterations"
    echo

    # Benchmark Prettier
    echo -e "${YELLOW}Prettier:${NC}"
    local prettier_start=$(date +%s%N)
    for ((i=1; i<=iterations; i++)); do
        prettier --parser markdown "$DOCS_DIR/$file" > /dev/null 2>&1
    done
    local prettier_end=$(date +%s%N)
    local prettier_time=$((prettier_end - prettier_start))
    local prettier_avg=$((prettier_time / iterations / 1000)) # microseconds
    local prettier_ms=$(echo "scale=2; $prettier_avg / 1000" | bc)
    local prettier_throughput=$(echo "scale=2; ($size / 1024) / ($prettier_avg / 1000000)" | bc)

    echo "  Total: $(echo "scale=2; $prettier_time / 1000000" | bc)ms"
    echo "  Average: ${prettier_avg}µs ($prettier_ms ms)"
    echo "  Throughput: $prettier_throughput KB/s"
    echo

    # Benchmark Panache
    echo -e "${GREEN}Panache:${NC}"
    local panache_start=$(date +%s%N)
    for ((i=1; i<=iterations; i++)); do
        $PANACHE format < "$DOCS_DIR/$file" > /dev/null 2>&1
    done
    local panache_end=$(date +%s%N)
    local panache_time=$((panache_end - panache_start))
    local panache_avg=$((panache_time / iterations / 1000)) # microseconds
    local panache_ms=$(echo "scale=2; $panache_avg / 1000" | bc)
    local panache_throughput=$(echo "scale=2; ($size / 1024) / ($panache_avg / 1000000)" | bc)

    echo "  Total: $(echo "scale=2; $panache_time / 1000000" | bc)ms"
    echo "  Average: ${panache_avg}µs ($panache_ms ms)"
    echo "  Throughput: $panache_throughput KB/s"
    echo

    # Comparison
    local speedup=$(echo "scale=2; $prettier_avg / $panache_avg" | bc)
    if (( $(echo "$speedup > 1" | bc -l) )); then
        echo -e "${GREEN}✓ Panache is ${speedup}x faster${NC}"
    elif (( $(echo "$speedup < 1" | bc -l) )); then
        local slower=$(echo "scale=2; $panache_avg / $prettier_avg" | bc)
        echo -e "${YELLOW}✗ Panache is ${slower}x slower${NC}"
    else
        echo "≈ Similar performance"
    fi
    echo
}

# Run benchmarks
benchmark_file "small.qmd" $ITERATIONS_SMALL "Small Document"
benchmark_file "medium_quarto.qmd" $ITERATIONS_MEDIUM "Medium Document (Quarto Tutorial)"
benchmark_file "tables.qmd" 50 "Tables Document"
benchmark_file "math.qmd" 50 "Math Document"
benchmark_file "large_authoring.qmd" $ITERATIONS_LARGE "Large Document"

echo "================================"
echo "Benchmark Complete"
echo "================================"
