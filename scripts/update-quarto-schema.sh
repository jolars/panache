#!/usr/bin/env sh
set -eu

# Refreshes the vendored Quarto schema used by the `quarto-schema` lint rule.
#
# Quarto compiles its machine-readable schema into a single resolved artifact,
# `all-schema-definitions.json`, committed in the quarto-cli repo. That file is
# ~2.9 MB and laden with editor-only metadata, so we distill it down to the
# validation-relevant shape (see src/bin/distill_quarto_schema.rs) and commit
# only the distilled form. The raw artifact is reproducible from the tag and
# commit recorded in .panache-source.
#
# Usage: scripts/update-quarto-schema.sh [vX.Y.Z]
#   Defaults to the latest quarto-cli release tag.
#
# Source: https://github.com/quarto-dev/quarto-cli
#   src/resources/editor/tools/yaml/all-schema-definitions.json

REPO="quarto-dev/quarto-cli"
ARTIFACT="src/resources/editor/tools/yaml/all-schema-definitions.json"

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
OUT_DIR="${ROOT_DIR}/assets/quarto-schema"
OUT_FILE="${OUT_DIR}/schema.json"
SOURCE_FILE="${OUT_DIR}/.panache-source"

TAG="${1:-}"
if [ -z "$TAG" ]; then
	echo "Resolving latest quarto-cli release..."
	TAG="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" |
		grep -m1 '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')"
fi
echo "Using quarto-cli ${TAG}"

COMMIT="$(curl -fsSL "https://api.github.com/repos/${REPO}/git/ref/tags/${TAG}" |
	grep -m1 '"sha"' | sed -E 's/.*"sha": *"([^"]+)".*/\1/')"

TMP_RAW="$(mktemp)"
trap 'rm -f "$TMP_RAW"' EXIT

echo "Downloading ${ARTIFACT}..."
curl -fsSL "https://raw.githubusercontent.com/${REPO}/${TAG}/${ARTIFACT}" -o "$TMP_RAW"

echo "Building distiller..."
cargo build --quiet --manifest-path "${ROOT_DIR}/Cargo.toml" --bin distill_quarto_schema

echo "Distilling..."
"${ROOT_DIR}/target/debug/distill_quarto_schema" "$TMP_RAW" "$TAG" "$OUT_FILE"

cat >"$SOURCE_FILE" <<EOF
repo=https://github.com/${REPO}
tag=${TAG}
commit=${COMMIT}
source=git-tag
artifact=${ARTIFACT}
generated=$(date -u +%Y-%m-%d)
EOF

echo "Updated ${OUT_FILE} ($(wc -c <"$OUT_FILE" | tr -d ' ') bytes) and ${SOURCE_FILE}"
