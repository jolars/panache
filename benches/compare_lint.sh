#!/usr/bin/env bash

# Directory-scale benchmark: how each linter handles a directory of many
# Markdown files in a single invocation.
#
# Compares:
#   * panache lint            (default worker parallelism)
#   * rumdl check             (single process)
#
# Usage:
#   bash benches/compare_lint.sh                    # JSON to default path
#   bash benches/compare_lint.sh --out PATH         # custom output

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JSON_OUT="${REPO_ROOT}/docs/guide/performance_lint_data.json"
CORPUS_PREFIX="${PANACHE_BENCH_LINT_DIR:-/var/tmp/panache-bench-lint}"
HYPERFINE_MIN_RUNS=3

while [[ $# -gt 0 ]]; do
    case "$1" in
        --out)
            JSON_OUT="$2"
            shift 2
            ;;
        -h|--help)
            sed -n '3,14p' "$0" | sed 's/^# \{0,1\}//'
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
    log "compare_lint.sh requires hyperfine and jq"
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

mapfile -t SOURCE_FILES < <(
    cd "$REPO_ROOT" && find . -type f -name '*.md' \
        ! -path './target/*' \
        ! -path './node_modules/*' \
        ! -path '*/_freeze/*' \
        ! -path '*/.quarto/*' \
        ! -path './.git/*' \
        ! -path './editors/code/node_modules/*' \
        ! -path './docs/playground/node_modules/*' \
        ! -path './crates/panache-wasm/pkg/*' \
        ! -path './pandoc/*' \
        ! -path './quarto-web/*' 2>/dev/null
)
SRC_COUNT=${#SOURCE_FILES[@]}
log "Discovered $SRC_COUNT .md files in repo"

if (( SRC_COUNT < 10 )); then
    log "Too few source files ($SRC_COUNT) for a meaningful bench"
    exit 1
fi

CORPUS_ROOT=$(mktemp -d "${CORPUS_PREFIX}.XXXXXX")
CORPUS_DIR="$CORPUS_ROOT/files"
cleanup() {
    rm -rf "$CORPUS_ROOT"
}
trap cleanup EXIT

mkdir -p "$CORPUS_DIR"
for src in "${SOURCE_FILES[@]}"; do
    flat="${src#./}"
    flat="${flat//\//__}"
    cp "$REPO_ROOT/$src" "$CORPUS_DIR/$flat"
done

TOTAL_BYTES=0
for src in "${SOURCE_FILES[@]}"; do
    sz=$(wc -c < "$REPO_ROOT/$src")
    TOTAL_BYTES=$((TOTAL_BYTES + sz))
done

declare -A TOOL_CMD=(
    [panache]="$PANACHE lint --isolated --no-cache --quiet '$CORPUS_DIR' >/dev/null 2>&1"
)
TOOLS=(panache)
if [[ "$HAVE_RUMDL" == yes ]]; then
    TOOL_CMD[rumdl]="rumdl check --isolated --no-cache --silent --fail-on never '$CORPUS_DIR' >/dev/null 2>&1"
    TOOLS+=(rumdl)
fi

RESULTS_JSON=()
for tool in "${TOOLS[@]}"; do
    cmd="${TOOL_CMD[$tool]}"
    log "  benchmarking $tool ..."
    tmp=$(mktemp)
    hyperfine --warmup 1 --min-runs "$HYPERFINE_MIN_RUNS" \
        --export-json "$tmp" --style=none \
        "$cmd" >/dev/null 2>&1 || {
            log "  $tool failed; recording null"
            RESULTS_JSON+=("$(printf '{"tool":"%s","mean_ms":null,"stddev_ms":null,"min_ms":null,"max_ms":null,"runs":0,"failed":true}' "$tool")")
            rm -f "$tmp"
            continue
        }
    mean=$(jq -r '.results[0].mean' "$tmp")
    stddev=$(jq -r '.results[0].stddev' "$tmp")
    min=$(jq -r '.results[0].min' "$tmp")
    max=$(jq -r '.results[0].max' "$tmp")
    runs=$(jq -r '.results[0].times | length' "$tmp")
    rm -f "$tmp"
    read -r mean_ms stddev_ms min_ms max_ms < <(awk -v m="$mean" -v s="$stddev" -v lo="$min" -v hi="$max" \
        'BEGIN { printf "%.4f %.4f %.4f %.4f\n", m*1000, s*1000, lo*1000, hi*1000 }')
    RESULTS_JSON+=("$(printf '{"tool":"%s","mean_ms":%s,"stddev_ms":%s,"min_ms":%s,"max_ms":%s,"runs":%d,"failed":false}' \
        "$tool" "$mean_ms" "$stddev_ms" "$min_ms" "$max_ms" "$runs")")
done

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
    printf '    "tools": {\n'
    printf '      "panache": {"version": "%s"}' "$(json_escape "$PANACHE_VER")"
    [[ -n "$RUMDL_VER" ]] && printf ',\n      "rumdl":   {"version": "%s"}' "$(json_escape "$RUMDL_VER")"
    printf '\n    }\n'
    printf '  },\n'
    printf '  "corpus": {"file_count": %d, "total_bytes": %d, "extension": "md"},\n' \
        "$SRC_COUNT" "$TOTAL_BYTES"
    printf '  "results": [\n'
    for ((i=0; i<${#RESULTS_JSON[@]}; i++)); do
        printf '    %s' "${RESULTS_JSON[$i]}"
        if (( i < ${#RESULTS_JSON[@]} - 1 )); then printf ','; fi
        printf '\n'
    done
    printf '  ]\n'
    printf '}\n'
} > "$JSON_OUT"

log "Wrote $JSON_OUT"
