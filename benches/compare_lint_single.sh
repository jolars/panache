#!/usr/bin/env bash

# Single-document lint benchmark comparing panache against other markdown
# linters on the benchmark corpus under benches/documents.
#
# Usage:
#   bash benches/compare_lint_single.sh --out PATH

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOCS_DIR="$REPO_ROOT/benches/documents"
JSON_OUT="$REPO_ROOT/docs/guide/performance_lint_single_data.json"
HYPERFINE_MIN_RUNS=3

while [[ $# -gt 0 ]]; do
    case "$1" in
        --out)
            JSON_OUT="$2"
            shift 2
            ;;
        -h|--help)
            sed -n '3,8p' "$0" | sed 's/^# \{0,1\}//'
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
HAVE_MADO=$(command -v mado >/dev/null 2>&1 && echo yes || echo no)
HAVE_MARKDOWNLINT=$(command -v markdownlint >/dev/null 2>&1 && echo yes || echo no)
HAVE_MARKDOWNLINT_CLI2=$(command -v markdownlint-cli2 >/dev/null 2>&1 && echo yes || echo no)

if [[ "$HAVE_HYPERFINE" != yes || "$HAVE_JQ" != yes ]]; then
    log "compare_lint_single.sh requires hyperfine and jq"
    exit 1
fi

log "Building panache (release)..."
(cd "$REPO_ROOT" && cargo build --release --bin panache --quiet) >&2
PANACHE="$REPO_ROOT/target/release/panache"

PANACHE_VER=$("$PANACHE" --version | awk '{print $2}')
RUMDL_VER=""
MADO_VER=""
MARKDOWNLINT_VER=""
MARKDOWNLINT_CLI2_VER=""
[[ "$HAVE_RUMDL" == yes ]] && RUMDL_VER=$(rumdl --version | awk '{print $2}')
[[ "$HAVE_MADO" == yes ]] && MADO_VER=$(mado --version | awk '{print $2}')
[[ "$HAVE_MARKDOWNLINT" == yes ]] && MARKDOWNLINT_VER=$(markdownlint --version)
[[ "$HAVE_MARKDOWNLINT_CLI2" == yes ]] && MARKDOWNLINT_CLI2_VER=$(markdownlint-cli2 --version 2>&1 | awk 'NR==1{gsub(/^v/,"",$2); print $2}')

HOST_OS=$(uname -s | tr '[:upper:]' '[:lower:]')
HOST_ARCH=$(uname -m)
HOST_CPU=""
[[ -f /proc/cpuinfo ]] && HOST_CPU=$(grep -m1 "model name" /proc/cpuinfo | sed 's/.*: //')

DOCUMENTS_JSON=()
RESULTS_JSON=()

run_one_json() {
    local cmd="$1"
    local tmp
    tmp=$(mktemp)
    hyperfine --warmup 1 --min-runs "$HYPERFINE_MIN_RUNS" \
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
}

benchmark_document() {
    local id="$1"
    local file="$2"
    local name="$3"

    if [[ ! -f "$DOCS_DIR/$file" ]]; then
        log "Skipping $name - file not found ($DOCS_DIR/$file)"
        return
    fi

    local size lines
    size=$(wc -c < "$DOCS_DIR/$file")
    lines=$(wc -l < "$DOCS_DIR/$file")

    log "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    log "$name"
    log "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    log "File: $file ($size bytes, $lines lines)"
    log "Minimum runs: $HYPERFINE_MIN_RUNS"
    log

    DOCUMENTS_JSON+=("$(printf '{"id":"%s","name":"%s","file":"%s","size_bytes":%d,"lines":%d}' \
        "$id" "$(json_escape "$name")" "$file" "$size" "$lines")")

    declare -A TOOL_CMD=(
        [panache]="$PANACHE lint --isolated --no-cache --quiet '$DOCS_DIR/$file' >/dev/null 2>&1"
    )
    local tools=(panache)
    if [[ "$HAVE_RUMDL" == yes ]]; then
        TOOL_CMD[rumdl]="rumdl check --isolated --no-cache --silent --fail-on never '$DOCS_DIR/$file' >/dev/null 2>&1"
        tools+=(rumdl)
    fi
    if [[ "$HAVE_MADO" == yes ]]; then
        TOOL_CMD[mado]="mado check --quiet '$DOCS_DIR/$file' >/dev/null 2>&1 || true"
        tools+=(mado)
    fi
    if [[ "$HAVE_MARKDOWNLINT" == yes ]]; then
        TOOL_CMD[markdownlint]="markdownlint --quiet '$DOCS_DIR/$file' >/dev/null 2>&1 || true"
        tools+=(markdownlint)
    fi
    if [[ "$HAVE_MARKDOWNLINT_CLI2" == yes ]]; then
        TOOL_CMD[markdownlint-cli2]="markdownlint-cli2 '$DOCS_DIR/$file' >/dev/null 2>&1 || true"
        tools+=(markdownlint-cli2)
    fi

    local tool cmd mean stddev min max runs
    for tool in "${tools[@]}"; do
        cmd="${TOOL_CMD[$tool]}"
        log "  $tool..."
        read -r mean stddev min max runs < <(run_one_json "$cmd")
        RESULTS_JSON+=("$(printf '{"document":"%s","tool":"%s","mean_ms":%s,"stddev_ms":%s,"min_ms":%s,"max_ms":%s,"runs":%d}' \
            "$id" "$tool" "$mean" "$stddev" "$min" "$max" "$runs")")
    done
    log
}

benchmark_document "pandoc_testsuite" "pandoc_testsuite.md" "Pandoc Testsuite Fixture (9 KB)"
benchmark_document "tables"           "tables.qmd"          "Tables Document (19 KB)"
benchmark_document "configuration"    "configuration.qmd"   "Configuration Guide (24 KB)"
benchmark_document "math"             "math.qmd"            "Math Document (29 KB)"
benchmark_document "large"            "large_authoring.qmd" "Large Document (30 KB)"
benchmark_document "pandoc_manual"    "pandoc_manual.md"    "Pandoc Manual (large reference doc)"

mkdir -p "$(dirname "$JSON_OUT")"
{
    printf '{\n'
    printf '  "schema_version": 1,\n'
    printf '  "meta": {\n'
    printf '    "generated_at": "%s",\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf '    "host": {"os": "%s", "arch": "%s", "cpu": "%s"},\n' \
        "$(json_escape "$HOST_OS")" "$(json_escape "$HOST_ARCH")" "$(json_escape "$HOST_CPU")"
    printf '    "min_runs": %d,\n' "$HYPERFINE_MIN_RUNS"
    printf '    "tools": {\n'
    printf '      "panache": {"version": "%s"}' "$(json_escape "$PANACHE_VER")"
    [[ -n "$RUMDL_VER" ]] && printf ',\n      "rumdl":   {"version": "%s"}' "$(json_escape "$RUMDL_VER")"
    [[ -n "$MADO_VER" ]] && printf ',\n      "mado":    {"version": "%s"}' "$(json_escape "$MADO_VER")"
    [[ -n "$MARKDOWNLINT_VER" ]] && printf ',\n      "markdownlint": {"version": "%s"}' "$(json_escape "$MARKDOWNLINT_VER")"
    [[ -n "$MARKDOWNLINT_CLI2_VER" ]] && printf ',\n      "markdownlint-cli2": {"version": "%s"}' "$(json_escape "$MARKDOWNLINT_CLI2_VER")"
    printf '\n    }\n'
    printf '  },\n'
    printf '  "documents": [\n'
    for ((i=0; i<${#DOCUMENTS_JSON[@]}; i++)); do
        printf '    %s' "${DOCUMENTS_JSON[$i]}"
        if (( i < ${#DOCUMENTS_JSON[@]} - 1 )); then printf ','; fi
        printf '\n'
    done
    printf '  ],\n'
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
