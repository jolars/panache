#!/usr/bin/env bash

# Directory-scale benchmark: how each formatter handles a directory of many
# Markdown files in a single invocation.
#
# Compares:
#   * panache --jobs 1   (serial outer loop, today's behavior pre-parallelism)
#   * panache --jobs 0   (auto, parallel across files)
#   * prettier --write   (single Node process, internally serial)
#   * rumdl fmt          (single process)
#
# Pandoc is excluded — it has no batch mode, so a fair comparison would mean
# spawning N processes from a shell loop, which mostly measures the loop.
#
# Corpus = every .md file in this repo, excluding build artifacts, vendored
# deps, and ad-hoc scratch dirs. Restricted to .md (not .qmd/.Rmd) because
# prettier won't infer a parser from those extensions and rumdl is markdown-
# only — keeps the comparison apples-to-apples.
#
# The corpus is rebuilt fresh per hyperfine sample (formatters write in place,
# so the second sample would otherwise format already-formatted files).
#
# Usage:
#   bash benches/compare_multifile.sh                    # progress to stderr,
#                                                       # JSON to default path
#   bash benches/compare_multifile.sh --out PATH         # custom output

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JSON_OUT="${REPO_ROOT}/docs/guide/performance_multifile_data.json"
CORPUS_ROOT="${PANACHE_BENCH_MULTIFILE_DIR:-/var/tmp/panache-bench-multifile}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --out)
            JSON_OUT="$2"
            shift 2
            ;;
        -h|--help)
            sed -n '3,28p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

log() { echo "$@" >&2; }

HAVE_HYPERFINE=$(command -v hyperfine >/dev/null 2>&1 && echo yes || echo no)
HAVE_JQ=$(command -v jq >/dev/null 2>&1 && echo yes || echo no)
HAVE_PRETTIER=$(command -v prettier >/dev/null 2>&1 && echo yes || echo no)
HAVE_RUMDL=$(command -v rumdl >/dev/null 2>&1 && echo yes || echo no)

if [[ "$HAVE_HYPERFINE" != yes || "$HAVE_JQ" != yes ]]; then
    log "compare_multifile.sh requires hyperfine and jq"
    exit 1
fi

# Build panache (release)
log "Building panache (release)..."
(cd "$REPO_ROOT" && cargo build --release --bin panache --quiet) >&2
PANACHE="$REPO_ROOT/target/release/panache"

PANACHE_VER=$("$PANACHE" --version | awk '{print $2}')
PRETTIER_VER=""
RUMDL_VER=""
[[ "$HAVE_PRETTIER" == yes ]] && PRETTIER_VER=$(prettier --version)
[[ "$HAVE_RUMDL" == yes ]]   && RUMDL_VER=$(rumdl --version | awk '{print $2}')

HOST_OS=$(uname -s | tr '[:upper:]' '[:lower:]')
HOST_ARCH=$(uname -m)
HOST_CPU=""
[[ -f /proc/cpuinfo ]] && HOST_CPU=$(grep -m1 "model name" /proc/cpuinfo | sed 's/.*: //')
HOST_CORES=$(nproc 2>/dev/null || echo 0)

json_escape() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

# Source corpus: every .md file under the repo, minus noise.
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

mkdir -p "$CORPUS_ROOT"
CORPUS_DIR="$CORPUS_ROOT/files"
PREPARE_SCRIPT="$CORPUS_ROOT/prepare.sh"

# Build the prepare script that hyperfine runs before each sample.
{
    echo "#!/usr/bin/env bash"
    echo "set -e"
    echo "rm -rf '$CORPUS_DIR'"
    echo "mkdir -p '$CORPUS_DIR'"
    i=0
    for src in "${SOURCE_FILES[@]}"; do
        # Strip leading "./" and replace separators so flat copy doesn't collide.
        flat="${src#./}"
        flat="${flat//\//__}"
        echo "cp '$REPO_ROOT/$src' '$CORPUS_DIR/$flat'"
        i=$((i + 1))
    done
} > "$PREPARE_SCRIPT"

# Total bytes for the meta block.
TOTAL_BYTES=0
for src in "${SOURCE_FILES[@]}"; do
    sz=$(wc -c < "$REPO_ROOT/$src")
    TOTAL_BYTES=$((TOTAL_BYTES + sz))
done

# Tool definitions: name → command-string operating on $CORPUS_DIR.
declare -A TOOL_CMD=(
    [panache-jobs1]="$PANACHE format --jobs 1 --no-cache '$CORPUS_DIR' >/dev/null"
    [panache-jobs0]="$PANACHE format --jobs 0 --no-cache '$CORPUS_DIR' >/dev/null"
    [prettier]="prettier --write --log-level silent '$CORPUS_DIR'/*.md >/dev/null 2>&1 || true"
    [rumdl]="rumdl fmt --no-cache '$CORPUS_DIR' >/dev/null 2>&1 || true"
)
TOOLS=(panache-jobs1 panache-jobs0)
[[ "$HAVE_PRETTIER" == yes ]] && TOOLS+=(prettier)
[[ "$HAVE_RUMDL"   == yes ]] && TOOLS+=(rumdl)

RESULTS_JSON=()
for tool in "${TOOLS[@]}"; do
    cmd="${TOOL_CMD[$tool]}"
    log "  benchmarking $tool ..."
    tmp=$(mktemp)
    hyperfine --warmup 1 --runs 3 \
        --prepare "bash '$PREPARE_SCRIPT'" \
        --export-json "$tmp" --style=none \
        "$cmd" >/dev/null 2>&1 || {
            log "  $tool failed; recording null"
            RESULTS_JSON+=("$(printf '{"tool":"%s","mean_ms":null,"stddev_ms":null,"min_ms":null,"max_ms":null,"runs":0,"failed":true}' "$tool")")
            rm -f "$tmp"
            continue
        }
    mean=$(jq -r '.results[0].mean'   "$tmp")
    stddev=$(jq -r '.results[0].stddev' "$tmp")
    min=$(jq -r '.results[0].min'     "$tmp")
    max=$(jq -r '.results[0].max'     "$tmp")
    runs=$(jq -r '.results[0].times | length' "$tmp")
    rm -f "$tmp"
    read -r mean_ms stddev_ms min_ms max_ms < <(awk -v m="$mean" -v s="$stddev" -v lo="$min" -v hi="$max" \
        'BEGIN { printf "%.4f %.4f %.4f %.4f\n", m*1000, s*1000, lo*1000, hi*1000 }')
    RESULTS_JSON+=("$(printf '{"tool":"%s","mean_ms":%s,"stddev_ms":%s,"min_ms":%s,"max_ms":%s,"runs":%d,"failed":false}' \
        "$tool" "$mean_ms" "$stddev_ms" "$min_ms" "$max_ms" "$runs")")
done

# Tear down corpus.
rm -rf "$CORPUS_DIR" "$PREPARE_SCRIPT"

mkdir -p "$(dirname "$JSON_OUT")"
{
    printf '{\n'
    printf '  "schema_version": 1,\n'
    printf '  "meta": {\n'
    printf '    "generated_at": "%s",\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf '    "host": {"os": "%s", "arch": "%s", "cpu": "%s", "cores": %s},\n' \
        "$(json_escape "$HOST_OS")" "$(json_escape "$HOST_ARCH")" \
        "$(json_escape "$HOST_CPU")" "$HOST_CORES"
    printf '    "tools": {\n'
    printf '      "panache":  {"version": "%s"}' "$(json_escape "$PANACHE_VER")"
    [[ -n "$PRETTIER_VER" ]] && printf ',\n      "prettier": {"version": "%s"}' "$(json_escape "$PRETTIER_VER")"
    [[ -n "$RUMDL_VER"   ]] && printf ',\n      "rumdl":    {"version": "%s"}' "$(json_escape "$RUMDL_VER")"
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
