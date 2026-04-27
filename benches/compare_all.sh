#!/usr/bin/env bash

# Comprehensive benchmark comparing panache against other markdown formatters.
#
# Usage:
#   bash benches/compare_all.sh                      # human-readable text output
#   bash benches/compare_all.sh --json [--out PATH]  # structured JSON output
#
# JSON mode prefers `hyperfine` for proper warmup + stddev/min/max stats and
# falls back to a simple shell timing loop when hyperfine is not installed.
# Both backends emit the same schema; only the per-result stat fields differ
# (the fallback leaves stddev/min/max as null).

set -e

DOCS_DIR="benches/documents"
RESULTS_FILE="benches/benchmark_results.txt"

JSON_MODE=0
JSON_OUT="docs/guide/performance_data.json"
HYPERFINE_MIN_RUNS=3

while [[ $# -gt 0 ]]; do
    case "$1" in
        --json)
            JSON_MODE=1
            shift
            ;;
        --out)
            JSON_OUT="$2"
            shift 2
            ;;
        -h|--help)
            sed -n '3,11p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

# Colors (only in text mode; JSON mode logs plainly to stderr)
if [ "$JSON_MODE" = "0" ]; then
    GREEN='\033[0;32m'
    BLUE='\033[0;34m'
    YELLOW='\033[1;33m'
    RED='\033[0;31m'
    NC='\033[0m'
else
    GREEN=''; BLUE=''; YELLOW=''; RED=''; NC=''
fi

HAVE_PRETTIER=$(command -v prettier >/dev/null 2>&1 && echo "yes" || echo "no")
HAVE_PANDOC=$(command -v pandoc >/dev/null 2>&1 && echo "yes" || echo "no")
HAVE_RUMDL=$(command -v rumdl >/dev/null 2>&1 && echo "yes" || echo "no")
HAVE_HYPERFINE=$(command -v hyperfine >/dev/null 2>&1 && echo "yes" || echo "no")
HAVE_JQ=$(command -v jq >/dev/null 2>&1 && echo "yes" || echo "no")

BACKEND="shell-loop"
if [ "$JSON_MODE" = "1" ] && [ "$HAVE_HYPERFINE" = "yes" ] && [ "$HAVE_JQ" = "yes" ]; then
    BACKEND="hyperfine"
fi

# In text mode, write banner to stdout. In JSON mode, route progress to stderr
# so stdout/JSON-out remains clean.
if [ "$JSON_MODE" = "0" ]; then
    LOG_FD=1
else
    LOG_FD=2
fi
log() { echo -e "$@" >&$LOG_FD; }

log "================================"
log "Multi-Formatter Benchmark"
log "================================"
log
log "Available formatters:"
log "  panache: yes (building...)"
[ "$HAVE_PRETTIER" = "yes" ] && log "  prettier: yes ($(prettier --version))"
[ "$HAVE_PANDOC" = "yes" ] && log "  pandoc: yes ($(pandoc --version | head -1 | cut -d' ' -f2))"
[ "$HAVE_RUMDL" = "yes" ] && log "  rumdl: yes ($(rumdl --version | awk '{print $2}'))"
if [ "$JSON_MODE" = "1" ]; then
    log "  backend: $BACKEND"
    if [ "$BACKEND" = "shell-loop" ] && [ "$HAVE_HYPERFINE" = "no" ]; then
        log "  (hint: install hyperfine for stddev/min/max stats)"
    fi
fi
log

# Build panache (release)
cargo build --release --quiet 2>&1 | grep -v "warning:" >&2 || true
PANACHE="./target/release/panache"

# Tool versions for meta
PANACHE_VER=$("$PANACHE" --version | awk '{print $2}')
PRETTIER_VER=""
PANDOC_VER=""
RUMDL_VER=""
[ "$HAVE_PRETTIER" = "yes" ] && PRETTIER_VER=$(prettier --version)
[ "$HAVE_PANDOC" = "yes" ] && PANDOC_VER=$(pandoc --version | head -1 | awk '{print $2}')
[ "$HAVE_RUMDL" = "yes" ] && RUMDL_VER=$(rumdl --version | awk '{print $2}')

# Host info
HOST_OS=$(uname -s | tr '[:upper:]' '[:lower:]')
HOST_ARCH=$(uname -m)
HOST_CPU=""
if [ -f /proc/cpuinfo ]; then
    HOST_CPU=$(grep -m1 "model name" /proc/cpuinfo | sed 's/.*: //')
fi

# JSON helpers
json_escape() {
    # minimal: escape backslash and double-quote
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

# JSON-mode accumulators
DOCUMENTS_JSON=()
RESULTS_JSON=()

# Run a single (formatter, document) pair and emit "mean stddev min max runs"
# in milliseconds, where stddev/min/max may be the literal "null".
run_one_json() {
    local iterations="$1"
    local cmd="$2"

    if [ "$BACKEND" = "hyperfine" ]; then
        local tmp
        tmp=$(mktemp)
        # --style=none silences the progress UI. Hyperfine hides command
        # stdout/stderr by default; `--show-output` is an opt-in flag.
        hyperfine --warmup 1 \
            --min-runs "$HYPERFINE_MIN_RUNS" \
            --export-json "$tmp" --style=none \
            "$cmd" >/dev/null 2>&1
        local mean stddev min max runs
        mean=$(jq -r '.results[0].mean' "$tmp")
        stddev=$(jq -r '.results[0].stddev' "$tmp")
        min=$(jq -r '.results[0].min' "$tmp")
        max=$(jq -r '.results[0].max' "$tmp")
        runs=$(jq -r '.results[0].times | length' "$tmp")
        rm -f "$tmp"
        awk -v m="$mean" -v s="$stddev" -v lo="$min" -v hi="$max" -v r="$runs" \
            'BEGIN { printf "%.4f %.4f %.4f %.4f %d\n", m*1000, s*1000, lo*1000, hi*1000, r }'
    else
        local start end
        start=$(date +%s%N)
        local i
        for ((i=1; i<=iterations; i++)); do
            eval "$cmd" >/dev/null 2>&1
        done
        end=$(date +%s%N)
        awk -v t="$((end - start))" -v n="$iterations" \
            'BEGIN { printf "%.4f null null null %d\n", (t/n)/1e6, n }'
    fi
}

# Text-mode timer (mean microseconds, simple loop)
run_one_text() {
    local iterations="$1"
    local cmd="$2"
    local start end total avg
    start=$(date +%s%N)
    for ((i=1; i<=iterations; i++)); do
        eval "$cmd" >/dev/null 2>&1
    done
    end=$(date +%s%N)
    total=$((end - start))
    avg=$((total / iterations / 1000))  # microseconds
    echo "$avg"
}

benchmark_document() {
    local id="$1"
    local file="$2"
    local name="$3"
    local iterations="$4"

    if [ ! -f "$DOCS_DIR/$file" ]; then
        log "⚠️  Skipping $name - file not found ($DOCS_DIR/$file)"
        return
    fi

    local size lines
    size=$(wc -c < "$DOCS_DIR/$file")
    lines=$(wc -l < "$DOCS_DIR/$file")

    log "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    log "${BLUE}$name${NC}"
    log "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    log "File: $file ($size bytes, $lines lines)"
    if [ "$JSON_MODE" = "1" ] && [ "$BACKEND" = "hyperfine" ]; then
        log "Minimum runs: $HYPERFINE_MIN_RUNS"
    else
        log "Iterations: $iterations"
    fi
    log

    if [ "$JSON_MODE" = "1" ]; then
        DOCUMENTS_JSON+=("$(printf '{"id":"%s","name":"%s","file":"%s","size_bytes":%d,"lines":%d,"iterations":%d}' \
            "$id" "$(json_escape "$name")" "$file" "$size" "$lines" "$iterations")")
    fi

    declare -A FORMATTER_CMD=(
        [panache]="$PANACHE format --isolated --stdin-filename '$DOCS_DIR/$file' < '$DOCS_DIR/$file'"
        [prettier]="prettier --parser markdown $DOCS_DIR/$file"
        [pandoc]="pandoc $DOCS_DIR/$file -f markdown -t markdown"
        [rumdl]="rumdl fmt --fix --stdin --no-cache --silent < $DOCS_DIR/$file"
    )

    local formatters=("panache")
    [ "$HAVE_PRETTIER" = "yes" ] && formatters+=("prettier")
    [ "$HAVE_PANDOC" = "yes" ]  && formatters+=("pandoc")
    [ "$HAVE_RUMDL" = "yes" ]   && formatters+=("rumdl")

    local panache_us=""
    local fmt cmd
    for fmt in "${formatters[@]}"; do
        cmd="${FORMATTER_CMD[$fmt]}"
        log "  ${fmt}..."

        if [ "$JSON_MODE" = "1" ]; then
            local mean stddev min max runs
            read -r mean stddev min max runs < <(run_one_json "$iterations" "$cmd")
            RESULTS_JSON+=("$(printf '{"document":"%s","formatter":"%s","mean_ms":%s,"stddev_ms":%s,"min_ms":%s,"max_ms":%s,"runs":%d}' \
                "$id" "$fmt" "$mean" "$stddev" "$min" "$max" "$runs")")
        else
            local us ms
            us=$(run_one_text "$iterations" "$cmd")
            ms=$(awk "BEGIN {printf \"%.2f\", $us / 1000}")
            if [ "$fmt" = "panache" ]; then
                panache_us="$us"
                printf "  %s: %8s µs (%6s ms)\n" "$fmt" "$us" "$ms" >&1
            else
                local speedup
                speedup=$(awk "BEGIN {printf \"%.1f\", $us / $panache_us}")
                printf "  %s: %8s µs (%6s ms) - panache is %sx faster\n" \
                    "$fmt" "$us" "$ms" "$speedup" >&1
            fi
            # Append to results text file
            {
                if [ "$fmt" = "panache" ]; then echo "### $name"; fi
                echo "- $fmt: ${us}µs (${ms}ms)"
                if [ "$fmt" = "${formatters[-1]}" ]; then echo; fi
            } >> "$RESULTS_FILE"
        fi
    done

    log
}

# Reset text-mode results file at start
if [ "$JSON_MODE" = "0" ]; then
    : > "$RESULTS_FILE"
fi

# Document set
benchmark_document "pandoc_testsuite" "pandoc_testsuite.md" "Pandoc Testsuite Fixture (9 KB)"      20
benchmark_document "tables"           "tables.qmd"          "Tables Document (19 KB)"              20
benchmark_document "configuration"    "configuration.qmd"   "Configuration Guide (24 KB)"          10
benchmark_document "math"             "math.qmd"            "Math Document (29 KB)"                10
benchmark_document "large"            "large_authoring.qmd" "Large Document (30 KB)"               10
benchmark_document "pandoc_manual"    "pandoc_manual.md"    "Pandoc Manual (large reference doc)"  3

log "================================"
log "Benchmark Complete"
log "================================"

if [ "$JSON_MODE" = "1" ]; then
    # Assemble JSON
    mkdir -p "$(dirname "$JSON_OUT")"
    {
        printf '{\n'
        printf '  "schema_version": 1,\n'
        printf '  "meta": {\n'
        printf '    "generated_at": "%s",\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
        printf '    "host": {"os": "%s", "arch": "%s", "cpu": "%s"},\n' \
            "$(json_escape "$HOST_OS")" \
            "$(json_escape "$HOST_ARCH")" \
            "$(json_escape "$HOST_CPU")"
        printf '    "backend": "%s",\n' "$BACKEND"
        printf '    "min_runs": %d,\n' "$HYPERFINE_MIN_RUNS"
        printf '    "tools": {\n'
        printf '      "panache":  {"version": "%s"}' "$(json_escape "$PANACHE_VER")"
        if [ "$HAVE_PRETTIER" = "yes" ]; then
            printf ',\n      "prettier": {"version": "%s"}' "$(json_escape "$PRETTIER_VER")"
        fi
        if [ "$HAVE_PANDOC" = "yes" ]; then
            printf ',\n      "pandoc":   {"version": "%s"}' "$(json_escape "$PANDOC_VER")"
        fi
        if [ "$HAVE_RUMDL" = "yes" ]; then
            printf ',\n      "rumdl":    {"version": "%s"}' "$(json_escape "$RUMDL_VER")"
        fi
        printf '\n    }\n'
        printf '  },\n'

        printf '  "documents": [\n'
        for ((i=0; i<${#DOCUMENTS_JSON[@]}; i++)); do
            printf '    %s' "${DOCUMENTS_JSON[$i]}"
            if [ "$i" -lt $((${#DOCUMENTS_JSON[@]} - 1)) ]; then printf ','; fi
            printf '\n'
        done
        printf '  ],\n'

        printf '  "results": [\n'
        for ((i=0; i<${#RESULTS_JSON[@]}; i++)); do
            printf '    %s' "${RESULTS_JSON[$i]}"
            if [ "$i" -lt $((${#RESULTS_JSON[@]} - 1)) ]; then printf ','; fi
            printf '\n'
        done
        printf '  ]\n'
        printf '}\n'
    } > "$JSON_OUT"

    log
    log "JSON written to: $JSON_OUT"
else
    log
    log "Results saved to: $RESULTS_FILE"
fi
