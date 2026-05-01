#!/usr/bin/env sh
# One-shot bulk import: copy Pandoc-flavor `input.md` files from the parser-
# golden fixture corpus into the pandoc-conformance corpus and generate the
# matching `expected.native` via the locally-installed pandoc.
#
# Filters (skip if any apply):
#   - case has parser-options.toml with flavor != pandoc and flavor present
#     (CommonMark/GFM/Quarto/RMarkdown/MultiMarkdown have different parser
#     behavior or unsupported constructs at the projector level)
#   - case name contains `commonmark`, `gfm`, `disabled`, or starts with
#     `crlf_`, `line_ending_`, `tab_` (lossless-bytes / extension-disabled
#     tests, not pandoc-native parity tests)
#   - case has no `input.md` (skip .qmd / .Rmd; those use Quarto/R extensions)
#   - input.md is empty or larger than 4 KB (whole-document fixtures don't
#     fit the per-case allowlist cadence)
#
# Imported cases land under
# `tests/fixtures/pandoc-conformance/corpus/<NNNN>-imported-<orig-name>/`.
# Existing imported/* cases are removed before the run so the import is
# idempotent.
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
PARSER_FIXTURES="${ROOT_DIR}/crates/panache-parser/tests/fixtures/cases"
CORPUS_DIR="${ROOT_DIR}/crates/panache-parser/tests/fixtures/pandoc-conformance/corpus"

if ! command -v pandoc >/dev/null 2>&1; then
  echo "error: pandoc not found on PATH" >&2
  exit 1
fi

# Highest existing non-imported id, so imported ids don't collide with the
# hand-curated seed range.
#
# IDs are zero-padded ("0025"). Avoid POSIX shell's octal interpretation in
# `$((...))` and `printf '%d'` by stripping leading zeros via sed before any
# numeric comparison.
strip_leading_zeros() {
  echo "$1" | sed 's/^0*//;s/^$/0/'
}

HIGHEST=0
for d in "$CORPUS_DIR"/*/; do
  [ -d "$d" ] || continue
  name="$(basename "$d")"
  case "$name" in
    *-imported-*) continue ;;
  esac
  id="${name%%-*}"
  case "$id" in
    [0-9]*)
      id_dec=$(strip_leading_zeros "$id")
      if [ "$id_dec" -gt "$HIGHEST" ]; then
        HIGHEST="$id_dec"
      fi
      ;;
  esac
done

# Wipe prior imports so re-runs are clean.
for d in "$CORPUS_DIR"/*/; do
  [ -d "$d" ] || continue
  name="$(basename "$d")"
  case "$name" in
    *-imported-*) rm -rf "$d" ;;
  esac
done

next_id=$((HIGHEST + 1))
imported=0
skipped=0

for src in "$PARSER_FIXTURES"/*/; do
  [ -d "$src" ] || continue
  name="$(basename "$src")"

  # Name-based skips.
  case "$name" in
    *commonmark*|*gfm*|*disabled*) skipped=$((skipped + 1)); continue ;;
    crlf_*|line_ending_*|tab_*) skipped=$((skipped + 1)); continue ;;
  esac

  input="${src}input.md"
  [ -f "$input" ] || { skipped=$((skipped + 1)); continue; }

  # Flavor skip.
  opts="${src}parser-options.toml"
  if [ -f "$opts" ]; then
    flavor="$(grep -E '^flavor[[:space:]]*=' "$opts" | head -1 | awk -F'"' '{print $2}')"
    case "$flavor" in
      ""|pandoc) ;;
      *) skipped=$((skipped + 1)); continue ;;
    esac
  fi

  # Size skip.
  size=$(wc -c < "$input")
  if [ "$size" -le 0 ] || [ "$size" -gt 4096 ]; then
    skipped=$((skipped + 1)); continue
  fi

  padded=$(printf "%04d" "$next_id")
  dest="${CORPUS_DIR}/${padded}-imported-${name}"
  mkdir -p "$dest"
  cp "$input" "${dest}/input.md"
  pandoc -f markdown -t native "${dest}/input.md" > "${dest}/expected.native"

  next_id=$((next_id + 1))
  imported=$((imported + 1))
done

echo "imported: $imported"
echo "skipped:  $skipped"
echo "id range: $((HIGHEST + 1))..$((next_id - 1))"
