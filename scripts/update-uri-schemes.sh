#!/usr/bin/env sh
set -eu

# Refreshes the vendored IANA URI scheme registry used by the
# `autolink_bare_uris` extension. This only fetches the registry verbatim;
# all processing (parsing the CSV, folding in nonstandard schemes, sorting)
# happens at build time in crates/panache-parser/build.rs.
#
# Source: https://www.iana.org/assignments/uri-schemes/uri-schemes.xhtml

REGISTRY_URL="https://www.iana.org/assignments/uri-schemes/uri-schemes-1.csv"

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
OUT_FILE="${ROOT_DIR}/crates/panache-parser/src/parser/inlines/uri-schemes.csv"

echo "Downloading IANA URI scheme registry..."
curl -fsSL "$REGISTRY_URL" -o "$OUT_FILE"

echo "Updated ${OUT_FILE} ($(wc -l < "$OUT_FILE" | tr -d ' ') lines)"
