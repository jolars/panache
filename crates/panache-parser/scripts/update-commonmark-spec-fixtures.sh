#!/usr/bin/env sh
set -eu

SPEC_REF="${1:-0.31.2}"
REPO_URL="https://github.com/commonmark/commonmark-spec"

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
FIXTURE_DIR="${ROOT_DIR}/crates/panache-parser/tests/fixtures/commonmark-spec"
TMP_DIR="$(mktemp -d)"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

echo "Cloning commonmark-spec at ${SPEC_REF}..."
git clone --depth 1 --branch "$SPEC_REF" "$REPO_URL" "${TMP_DIR}/repo" >/dev/null

mkdir -p "$FIXTURE_DIR"
rm -f "${FIXTURE_DIR}/spec.txt" "${FIXTURE_DIR}/.panache-source"

cp "${TMP_DIR}/repo/spec.txt" "${FIXTURE_DIR}/spec.txt"

COMMIT="$(git -C "${TMP_DIR}/repo" rev-parse HEAD)"
cat > "${FIXTURE_DIR}/.panache-source" <<EOF
repo=${REPO_URL}
ref=${SPEC_REF}
commit=${COMMIT}
source=git-tag
EOF

echo "Updated CommonMark spec fixtures in ${FIXTURE_DIR}"
echo "  ref=${SPEC_REF}"
echo "  commit=${COMMIT}"
