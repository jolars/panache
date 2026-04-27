#!/usr/bin/env bash

# Benchmark formatting or linting across a curated set of full repositories.
#
# Usage:
#   bash benches/compare_repo_suite.sh --mode format --track markdown --out PATH
#   bash benches/compare_repo_suite.sh --mode lint --track quarto --out PATH

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CACHE_ROOT="${PANACHE_BENCH_REPO_CACHE:-/var/tmp/panache-bench-repo-cache}"
WORK_PREFIX="${PANACHE_BENCH_REPO_WORK:-/var/tmp/panache-bench-repo-work}"
MODE=""
TRACK=""
JSON_OUT=""
HYPERFINE_MIN_RUNS=3

while [[ $# -gt 0 ]]; do
    case "$1" in
        --mode)
            MODE="$2"
            shift 2
            ;;
        --track)
            TRACK="$2"
            shift 2
            ;;
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

if [[ -z "$MODE" || -z "$TRACK" || -z "$JSON_OUT" ]]; then
    echo "Missing required arguments: --mode, --track, --out" >&2
    exit 2
fi

if [[ "$MODE" != "format" && "$MODE" != "lint" ]]; then
    echo "Unsupported mode: $MODE" >&2
    exit 2
fi

if [[ "$TRACK" != "markdown" && "$TRACK" != "quarto" ]]; then
    echo "Unsupported track: $TRACK" >&2
    exit 2
fi

log() { echo "$@" >&2; }
json_escape() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

HAVE_HYPERFINE=$(command -v hyperfine >/dev/null 2>&1 && echo yes || echo no)
HAVE_JQ=$(command -v jq >/dev/null 2>&1 && echo yes || echo no)
HAVE_PRETTIER=$(command -v prettier >/dev/null 2>&1 && echo yes || echo no)
HAVE_RUMDL=$(command -v rumdl >/dev/null 2>&1 && echo yes || echo no)

if [[ "$HAVE_HYPERFINE" != yes || "$HAVE_JQ" != yes ]]; then
    log "compare_repo_suite.sh requires hyperfine and jq"
    exit 1
fi

log "Building panache (release)..."
(cd "$REPO_ROOT" && cargo build --release --bin panache --quiet) >&2
PANACHE="$REPO_ROOT/target/release/panache"

PANACHE_VER=$("$PANACHE" --version | awk '{print $2}')
PRETTIER_VER=""
RUMDL_VER=""
[[ "$HAVE_PRETTIER" == yes ]] && PRETTIER_VER=$(prettier --version)
[[ "$HAVE_RUMDL" == yes ]] && RUMDL_VER=$(rumdl --version | awk '{print $2}')

HOST_OS=$(uname -s | tr '[:upper:]' '[:lower:]')
HOST_ARCH=$(uname -m)
HOST_CPU=""
[[ -f /proc/cpuinfo ]] && HOST_CPU=$(grep -m1 "model name" /proc/cpuinfo | sed 's/.*: //')
HOST_CORES=$(nproc 2>/dev/null || echo 0)

mkdir -p "$CACHE_ROOT"
WORK_ROOT=$(mktemp -d "${WORK_PREFIX}.XXXXXX")
cleanup() {
    rm -rf "$WORK_ROOT"
}
trap cleanup EXIT

declare -a REPOS=()
case "$TRACK" in
    markdown)
        FILE_GLOB='*.md'
        REPOS=(
            "jgm/pandoc"
            "rust-lang/book"
            "rust-lang/reference"
        )
        ;;
    quarto)
        FILE_GLOB='*.qmd'
        REPOS=(
            "quarto-dev/quarto-web"
            "mlr-org/mlr3book"
            "RohanAlexander/tswd"
            "jolars/panache"
        )
        ;;
esac

ensure_repo() {
    local repo="$1"
    local slug="${repo//\//__}"
    local repo_dir="$CACHE_ROOT/$slug"
    if [[ ! -d "$repo_dir/.git" ]]; then
        log "Cloning $repo ..."
        git clone --depth 1 "https://github.com/${repo}.git" "$repo_dir" >&2
    fi
    printf '%s\n' "$repo_dir"
}

run_hyperfine() {
    local cmd="$1"
    local prepare_cmd="$2"
    local tmp
    tmp=$(mktemp)
    if [[ -n "$prepare_cmd" ]]; then
        hyperfine --warmup 1 --min-runs "$HYPERFINE_MIN_RUNS" \
            --prepare "$prepare_cmd" \
            --export-json "$tmp" --style=none \
            "$cmd" >/dev/null 2>&1
    else
        hyperfine --warmup 1 --min-runs "$HYPERFINE_MIN_RUNS" \
            --export-json "$tmp" --style=none \
            "$cmd" >/dev/null 2>&1
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

REPOS_JSON=()
RESULTS_JSON=()

for repo in "${REPOS[@]}"; do
    repo_dir=$(ensure_repo "$repo")
    mapfile -d '' -t source_files < <(git -C "$repo_dir" ls-files -z -- "$FILE_GLOB")
    file_count=${#source_files[@]}
    if (( file_count == 0 )); then
        log "Skipping $repo - no files matching $FILE_GLOB"
        continue
    fi

    total_bytes=0
    for src in "${source_files[@]}"; do
        sz=$(wc -c < "$repo_dir/$src")
        total_bytes=$((total_bytes + sz))
    done

    repo_slug="${repo//\//__}"
    corpus_root="$WORK_ROOT/$repo_slug"
    corpus_dir="$corpus_root/files"
    prepare_script="$corpus_root/prepare.sh"
    mkdir -p "$corpus_root"

    {
        echo "#!/usr/bin/env bash"
        echo "set -e"
        echo "rm -rf '$corpus_dir'"
        echo "mkdir -p '$corpus_dir'"
        for src in "${source_files[@]}"; do
            flat="${src//\//__}"
            echo "cp '$repo_dir/$src' '$corpus_dir/$flat'"
        done
    } > "$prepare_script"

    REPOS_JSON+=("$(printf '{"id":"%s","name":"%s","file_count":%d,"total_bytes":%d,"extension":"%s"}' \
        "$repo_slug" "$(json_escape "$repo")" "$file_count" "$total_bytes" "${FILE_GLOB#*.}")")

    log "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    log "$repo ($file_count files, $total_bytes bytes)"
    log "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    declare -A TOOL_CMD=()
    declare -a TOOLS=()

    if [[ "$MODE" == "format" ]]; then
        TOOL_CMD[panache]="$PANACHE format --isolated --no-cache '$corpus_dir' >/dev/null"
        TOOLS=(panache)
        if [[ "$TRACK" == "markdown" && "$HAVE_PRETTIER" == yes ]]; then
            TOOL_CMD[prettier]="prettier --write --log-level silent '$corpus_dir'/*.md >/dev/null 2>&1 || true"
            TOOLS+=(prettier)
        fi
        if [[ "$HAVE_RUMDL" == yes ]]; then
            TOOL_CMD[rumdl]="rumdl fmt --isolated --no-cache '$corpus_dir' >/dev/null 2>&1 || true"
            TOOLS+=(rumdl)
        fi
    else
        TOOL_CMD[panache]="$PANACHE lint --isolated --no-cache --quiet '$corpus_dir' >/dev/null 2>&1"
        TOOLS=(panache)
        if [[ "$HAVE_RUMDL" == yes ]]; then
            TOOL_CMD[rumdl]="rumdl check --isolated --no-cache --silent --fail-on never '$corpus_dir' >/dev/null 2>&1"
            TOOLS+=(rumdl)
        fi
    fi

    for tool in "${TOOLS[@]}"; do
        log "  benchmarking $tool ..."
        cmd="${TOOL_CMD[$tool]}"
        if read -r mean stddev min max runs < <(run_hyperfine "$cmd" "bash '$prepare_script'"); then
            RESULTS_JSON+=("$(printf '{"repo":"%s","tool":"%s","mean_ms":%s,"stddev_ms":%s,"min_ms":%s,"max_ms":%s,"runs":%d,"failed":false}' \
                "$repo_slug" "$tool" "$mean" "$stddev" "$min" "$max" "$runs")")
        else
            log "  $tool failed; recording null"
            RESULTS_JSON+=("$(printf '{"repo":"%s","tool":"%s","mean_ms":null,"stddev_ms":null,"min_ms":null,"max_ms":null,"runs":0,"failed":true}' \
                "$repo_slug" "$tool")")
        fi
    done
    log
done

mkdir -p "$(dirname "$JSON_OUT")"
{
    printf '{\n'
    printf '  "schema_version": 1,\n'
    printf '  "meta": {\n'
    printf '    "mode": "%s",\n' "$MODE"
    printf '    "track": "%s",\n' "$TRACK"
    printf '    "generated_at": "%s",\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf '    "host": {"os": "%s", "arch": "%s", "cpu": "%s", "cores": %s},\n' \
        "$(json_escape "$HOST_OS")" "$(json_escape "$HOST_ARCH")" "$(json_escape "$HOST_CPU")" "$HOST_CORES"
    printf '    "min_runs": %d,\n' "$HYPERFINE_MIN_RUNS"
    printf '    "tools": {\n'
    printf '      "panache": {"version": "%s"}' "$(json_escape "$PANACHE_VER")"
    if [[ -n "$PRETTIER_VER" ]]; then
        printf ',\n      "prettier": {"version": "%s"}' "$(json_escape "$PRETTIER_VER")"
    fi
    if [[ -n "$RUMDL_VER" ]]; then
        printf ',\n      "rumdl": {"version": "%s"}' "$(json_escape "$RUMDL_VER")"
    fi
    printf '\n    }\n'
    printf '  },\n'
    printf '  "repos": [\n'
    for ((i=0; i<${#REPOS_JSON[@]}; i++)); do
        printf '    %s' "${REPOS_JSON[$i]}"
        if (( i < ${#REPOS_JSON[@]} - 1 )); then printf ','; fi
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
