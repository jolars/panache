#!/usr/bin/env bash

# Synthetic-corpus lint benchmark for tracking the panache vs rumdl gap on
# multi-file batches.
#
# Builds a self-contained corpus by replicating documents already vendored
# under benches/documents/ N times each (with numbered suffixes), so the
# comparison is reproducible without network access. Also includes a
# single-file baseline (pandoc_manual.md) so single-doc regressions are
# visible in the same JSON.
#
# Compares:
#   * panache lint
#   * rumdl check (if installed)
#
# Usage:
#   bash benches/compare_lint_corpus.sh                 # JSON to default path
#   bash benches/compare_lint_corpus.sh --out PATH      # custom output
#   PANACHE_BENCH_LINT_REPLICAS=16 bash benches/...     # override replicas

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOCS_DIR="$REPO_ROOT/benches/documents"
JSON_OUT="$REPO_ROOT/docs/guide/performance_lint_corpus_data.json"
CORPUS_PREFIX="${PANACHE_BENCH_LINT_CORPUS:-/var/tmp/panache-bench-lint-corpus}"
REPLICAS="${PANACHE_BENCH_LINT_REPLICAS:-8}"
HYPERFINE_MIN_RUNS=3

while [[ $# -gt 0 ]]; do
    case "$1" in
        --out)
            JSON_OUT="$2"
            shift 2
            ;;
        --replicas)
            REPLICAS="$2"
            shift 2
            ;;
        -h|--help)
            sed -n '3,18p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

log() { echo "$@" >&2; }
json_escape() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

HAVE_HYPERFINE=$(command -v hyperfine >/dev/null 2>&1 && echo yes || echo no)
HAVE_JQ=$(command -v jq >/dev/null 2>&1 && echo yes || echo no)
HAVE_RUMDL=$(command -v rumdl >/dev/null 2>&1 && echo yes || echo no)

if [[ "$HAVE_HYPERFINE" != yes || "$HAVE_JQ" != yes ]]; then
    log "compare_lint_corpus.sh requires hyperfine and jq"
    exit 1
fi

log "Building panache (release)..."
(cd "$REPO_ROOT" && cargo build --release --bin panache --quiet) >&2
PANACHE="$REPO_ROOT/target/release/panache"

PANACHE_VER=$("$PANACHE" --version | awk '{print $2}')
RUMDL_VER=""
[[ "$HAVE_RUMDL" == yes ]] && RUMDL_VER=$(rumdl --version | awk '{print $2}')

HOST_OS=$(uname -s | tr '[:upper:]' '[:lower:]')
HOST_ARCH=$(uname -m)
HOST_CPU=""
[[ -f /proc/cpuinfo ]] && HOST_CPU=$(grep -m1 "model name" /proc/cpuinfo | sed 's/.*: //')
HOST_CORES=$(nproc 2>/dev/null || echo 0)

# Source documents: small/medium/large mix to stress per-file overhead and
# also keep parser/format work realistic.
SOURCES=(
    "small.qmd"
    "medium_quarto.qmd"
    "tables.qmd"
    "math.qmd"
    "large_authoring.qmd"
    "pandoc_testsuite.md"
    "pandoc_manual.md"
)

# Verify all sources exist; bail with a hint if downloads are missing.
MISSING=0
for src in "${SOURCES[@]}"; do
    if [[ ! -f "$DOCS_DIR/$src" ]]; then
        log "Missing $DOCS_DIR/$src"
        MISSING=1
    fi
done
if (( MISSING )); then
    log "Run 'cd benches/documents && ./download.sh' to fetch corpus docs."
    exit 1
fi

CORPUS_ROOT=$(mktemp -d "${CORPUS_PREFIX}.XXXXXX")
CORPUS_DIR="$CORPUS_ROOT/files"
SINGLE_FILE_DIR="$CORPUS_ROOT/single"
cleanup() {
    rm -rf "$CORPUS_ROOT"
}
trap cleanup EXIT

mkdir -p "$CORPUS_DIR" "$SINGLE_FILE_DIR"

# Replicate each source REPLICAS times with numbered suffix. Numbered names
# avoid identical-byte coincidences in either tool's interning.
TOTAL_BYTES=0
for src in "${SOURCES[@]}"; do
    base="${src%.*}"
    ext="${src##*.}"
    sz=$(wc -c < "$DOCS_DIR/$src")
    for ((i=1; i<=REPLICAS; i++)); do
        idx=$(printf '%03d' "$i")
        cp "$DOCS_DIR/$src" "$CORPUS_DIR/${base}_${idx}.${ext}"
        TOTAL_BYTES=$((TOTAL_BYTES + sz))
    done
done

CORPUS_FILES=$(( ${#SOURCES[@]} * REPLICAS ))
log "Corpus: $CORPUS_FILES files (${REPLICAS}× ${#SOURCES[@]} sources), $TOTAL_BYTES bytes"

# Single-file baseline: pandoc_manual.md (largest doc, most informative).
SINGLE_SRC="pandoc_manual.md"
cp "$DOCS_DIR/$SINGLE_SRC" "$SINGLE_FILE_DIR/$SINGLE_SRC"
SINGLE_BYTES=$(wc -c < "$SINGLE_FILE_DIR/$SINGLE_SRC")

run_one_json() {
    local cmd="$1"
    local tmp
    tmp=$(mktemp)
    if ! hyperfine --warmup 1 --min-runs "$HYPERFINE_MIN_RUNS" \
        --export-json "$tmp" --style=none \
        "$cmd" >/dev/null 2>&1; then
        rm -f "$tmp"
        return 1
    fi
    local mean stddev min max runs
    mean=$(jq -r '.results[0].mean' "$tmp")
    stddev=$(jq -r '.results[0].stddev' "$tmp")
    min=$(jq -r '.results[0].min' "$tmp")
    max=$(jq -r '.results[0].max' "$tmp")
    runs=$(jq -r '.results[0].times | length' "$tmp")
    rm -f "$tmp"
    awk -v m="$mean" -v s="$stddev" -v lo="$min" -v hi="$max" -v r="$runs" \
        'BEGIN { printf "%.4f %.4f %.4f %.4f %d\n", m*1000, s*1000, lo*1000, hi*1000, r }'
}

declare -a SCENARIO_RESULTS=()

run_scenario() {
    local scenario_id="$1"
    local target="$2"
    local label="$3"

    log "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    log "$label"
    log "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    declare -A TOOL_CMD=(
        [panache]="$PANACHE lint --isolated --no-cache --quiet '$target' >/dev/null 2>&1"
    )
    local tools=(panache)
    if [[ "$HAVE_RUMDL" == yes ]]; then
        TOOL_CMD[rumdl]="rumdl check --isolated --no-cache --silent --fail-on never '$target' >/dev/null 2>&1"
        tools+=(rumdl)
    fi

    local tool cmd mean stddev min max runs
    for tool in "${tools[@]}"; do
        cmd="${TOOL_CMD[$tool]}"
        log "  benchmarking $tool ..."
        if read -r mean stddev min max runs < <(run_one_json "$cmd"); then
            SCENARIO_RESULTS+=("$(printf '{"scenario":"%s","tool":"%s","mean_ms":%s,"stddev_ms":%s,"min_ms":%s,"max_ms":%s,"runs":%d,"failed":false}' \
                "$scenario_id" "$tool" "$mean" "$stddev" "$min" "$max" "$runs")")
        else
            log "  $tool failed; recording null"
            SCENARIO_RESULTS+=("$(printf '{"scenario":"%s","tool":"%s","mean_ms":null,"stddev_ms":null,"min_ms":null,"max_ms":null,"runs":0,"failed":true}' \
                "$scenario_id" "$tool")")
        fi
    done
    log
}

run_scenario "synthetic_corpus" "$CORPUS_DIR" \
    "Synthetic corpus ($CORPUS_FILES files, ${TOTAL_BYTES} bytes)"
run_scenario "single_pandoc_manual" "$SINGLE_FILE_DIR/$SINGLE_SRC" \
    "Single file baseline (pandoc_manual.md, ${SINGLE_BYTES} bytes)"

mkdir -p "$(dirname "$JSON_OUT")"
{
    printf '{\n'
    printf '  "schema_version": 1,\n'
    printf '  "meta": {\n'
    printf '    "generated_at": "%s",\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf '    "host": {"os": "%s", "arch": "%s", "cpu": "%s", "cores": %s},\n' \
        "$(json_escape "$HOST_OS")" "$(json_escape "$HOST_ARCH")" \
        "$(json_escape "$HOST_CPU")" "$HOST_CORES"
    printf '    "min_runs": %d,\n' "$HYPERFINE_MIN_RUNS"
    printf '    "replicas_per_source": %d,\n' "$REPLICAS"
    printf '    "tools": {\n'
    printf '      "panache": {"version": "%s"}' "$(json_escape "$PANACHE_VER")"
    [[ -n "$RUMDL_VER" ]] && printf ',\n      "rumdl":   {"version": "%s"}' "$(json_escape "$RUMDL_VER")"
    printf '\n    }\n'
    printf '  },\n'
    printf '  "scenarios": [\n'
    printf '    {"id":"synthetic_corpus","kind":"directory","file_count":%d,"total_bytes":%d,"sources":[' \
        "$CORPUS_FILES" "$TOTAL_BYTES"
    for ((i=0; i<${#SOURCES[@]}; i++)); do
        printf '"%s"' "${SOURCES[$i]}"
        if (( i < ${#SOURCES[@]} - 1 )); then printf ','; fi
    done
    printf ']},\n'
    printf '    {"id":"single_pandoc_manual","kind":"single_file","file":"%s","size_bytes":%d}\n' \
        "$SINGLE_SRC" "$SINGLE_BYTES"
    printf '  ],\n'
    printf '  "results": [\n'
    for ((i=0; i<${#SCENARIO_RESULTS[@]}; i++)); do
        printf '    %s' "${SCENARIO_RESULTS[$i]}"
        if (( i < ${#SCENARIO_RESULTS[@]} - 1 )); then printf ','; fi
        printf '\n'
    done
    printf '  ]\n'
    printf '}\n'
} > "$JSON_OUT"

log "Wrote $JSON_OUT"
