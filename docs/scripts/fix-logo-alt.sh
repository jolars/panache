#!/usr/bin/env bash
set -euo pipefail

# Quarto 1.8 does not apply `website.navbar.logo-alt` to the light/dark logo
# variants, so both navbar <img> tags render with alt="" on every page. Bing
# Webmaster Tools (and accessibility checkers) then flag every page for a
# missing alt attribute. Inject the alt text in post-render until upstream
# applies logo-alt to the logo variants.

out_dir="${QUARTO_PROJECT_OUTPUT_DIR:-_site}"

[ -d "$out_dir" ] || exit 0

find "$out_dir" -name '*.html' -print0 \
  | xargs -0 --no-run-if-empty \
      sed -i 's/alt="" class="navbar-logo/alt="Panache logo" class="navbar-logo/g'
