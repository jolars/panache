#!/usr/bin/env bash
set -euo pipefail

# Quarto renders navbar `tools` icon links with an empty `aria-label=""`, so the
# GitHub icon link has no discernible name and accessibility checkers flag every
# page ("Links do not have a discernible name"). Inject a label in post-render
# until upstream emits a non-empty aria-label for icon-only tool links.

out_dir="${QUARTO_PROJECT_OUTPUT_DIR:-_site}"

[ -d "$out_dir" ] || exit 0

find "$out_dir" -name '*.html' -print0 \
  | xargs -0 --no-run-if-empty \
      sed -i 's#\(<a href="https://github.com/jolars/panache"[^>]*\)aria-label=""#\1aria-label="GitHub repository"#g'
