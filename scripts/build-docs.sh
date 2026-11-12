#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_dir"

wasm-pack build crates/panache-wasm --target web
rm -rf docs/playground/pkg
cp -r crates/panache-wasm/pkg docs/playground/
rm -f docs/playground/pkg/.gitignore

quarto render docs

install -m 0644 scripts/panache-installer.sh docs/_site/install
install -m 0644 scripts/panache-installer.ps1 docs/_site/install.ps1
install -m 0644 funding.json docs/_site/funding.json
install -m 0644 workers/_headers docs/_site/_headers
cmp scripts/panache-installer.sh docs/_site/install
cmp scripts/panache-installer.ps1 docs/_site/install.ps1
cmp docs/panache.schema.json docs/_site/panache.schema.json
cmp funding.json docs/_site/funding.json
sh -n docs/_site/install
