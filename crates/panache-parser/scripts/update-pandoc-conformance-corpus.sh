#!/usr/bin/env sh
# Regenerate every corpus/<NNNN>-<slug>/expected.native against the locally-
# installed pandoc. Pin the new version into .panache-source. Run this only
# when intentionally bumping the pandoc version the corpus is calibrated
# against — review the diff before committing.
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
FIXTURE_DIR="${ROOT_DIR}/crates/panache-parser/tests/fixtures/pandoc-conformance"
CORPUS_DIR="${FIXTURE_DIR}/corpus"

if ! command -v pandoc >/dev/null 2>&1; then
  echo "error: pandoc not found on PATH" >&2
  exit 1
fi

PANDOC_VERSION="$(pandoc --version | head -1 | awk '{print $2}')"
TODAY="$(date +%Y-%m-%d)"

count=0
for dir in "$CORPUS_DIR"/*/; do
  [ -d "$dir" ] || continue
  input="${dir}input.md"
  expected="${dir}expected.native"
  if [ ! -f "$input" ]; then
    echo "warning: missing $input, skipping" >&2
    continue
  fi
  pandoc -f markdown -t native "$input" > "$expected"
  count=$((count + 1))
done

cat > "${FIXTURE_DIR}/.panache-source" <<EOF
# Pinned pandoc version used to derive expected.native files in corpus/.
# Bumping this is an intentional act: re-run scripts/update-pandoc-conformance-corpus.sh
# and review the diff before committing.
pandoc_version=${PANDOC_VERSION}
generated=${TODAY}
EOF

echo "Regenerated ${count} expected.native files against pandoc ${PANDOC_VERSION}"
echo "  fixtures: ${FIXTURE_DIR}"
