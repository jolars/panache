#!/usr/bin/env sh
set -eu

# Vendor the upstream MyST conformance examples used as a losslessness +
# idempotency smoke corpus. The examples live in `docs/examples/*.yml`, each a
# list of cases carrying a `myst:` markdown input alongside `mdast`/`html`
# targets. Only the `myst` inputs are consumed (see tests/myst_corpus.rs); the
# `mdast`/`html` targets describe a JS AST and rendered HTML that do not map to
# Panache's CST, so we do not assert against them.
#
# The CommonMark-derived files (`cmark_spec_*.yml`, `commonmark.*.yml`) are
# skipped: they duplicate coverage already provided by the dedicated CommonMark
# spec.txt harness (tests/commonmark.rs) and carry quoted/escaped YAML scalar
# forms that the smoke extractor deliberately does not handle. The MyST-specific
# files use only `|-` block-literal `myst:` scalars.

SPEC_REF="${1:-v0.0.5}"
REPO_URL="https://github.com/jupyter-book/myst-spec"

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
FIXTURE_DIR="${ROOT_DIR}/crates/panache-parser/tests/fixtures/myst-spec"
TMP_DIR="$(mktemp -d)"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

echo "Cloning myst-spec at ${SPEC_REF}..."
git clone --depth 1 --branch "$SPEC_REF" "$REPO_URL" "${TMP_DIR}/repo" >/dev/null

rm -rf "$FIXTURE_DIR"
mkdir -p "$FIXTURE_DIR/examples"

# Copy the MyST-specific example fixtures verbatim so a refresh is a clean
# re-copy, skipping the CommonMark-derived files (see header note).
for f in "${TMP_DIR}/repo/docs/examples/"*.yml; do
  base="$(basename "$f")"
  case "$base" in
    cmark_spec_*.yml | commonmark.*.yml) continue ;;
  esac
  cp "$f" "${FIXTURE_DIR}/examples/"
done

COMMIT="$(git -C "${TMP_DIR}/repo" rev-parse HEAD)"
cat > "${FIXTURE_DIR}/.panache-source" <<EOF
repo=${REPO_URL}
ref=${SPEC_REF}
commit=${COMMIT}
source=git-tag
layout=docs-examples-yml
EOF

echo "Updated MyST spec fixtures in ${FIXTURE_DIR}"
echo "  ref=${SPEC_REF}"
echo "  commit=${COMMIT}"
echo "  files=$(ls -1 "${FIXTURE_DIR}/examples" | wc -l)"
