#!/usr/bin/env sh
set -eu

DATA_TAG="${1:-data-2022-01-17}"
REPO_URL="https://github.com/yaml/yaml-test-suite"

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
FIXTURE_DIR="${ROOT_DIR}/crates/panache-parser/tests/fixtures/yaml-test-suite"
TMP_DIR="$(mktemp -d)"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

echo "Cloning yaml-test-suite ${DATA_TAG}..."
git clone --depth 1 --branch "$DATA_TAG" "$REPO_URL" "${TMP_DIR}/repo" >/dev/null

rm -rf "$FIXTURE_DIR"
mkdir -p "$FIXTURE_DIR"

# data-* branches use one directory per test case at repository root.
# Exclude VCS metadata to avoid creating an accidental nested git repository.
find "${TMP_DIR}/repo" -mindepth 1 -maxdepth 1 -type d ! -name ".git" -exec cp -R {} "$FIXTURE_DIR/" \;

cat > "${FIXTURE_DIR}/.panache-source" <<EOF
repo=${REPO_URL}
tag=${DATA_TAG}
source=git-branch
layout=data-branch-root
EOF

echo "Updated YAML test fixtures in ${FIXTURE_DIR}"
