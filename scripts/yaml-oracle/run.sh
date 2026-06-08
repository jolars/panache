#!/usr/bin/env bash
# Regenerate the YAML consumer-oracle audit (scripts/yaml-oracle/oracle.json
# and oracle-discrepancies.md) from the vendored yaml-test-suite fixtures.
#
# Requires: node (+ js-yaml resolvable), ruby (psych/libyaml), pandoc.
# These external tools are not available in CI, so this is a documented manual
# step — run it after any yaml-test-suite refresh and review the diff.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec node "${SCRIPT_DIR}/audit.mjs" "$@"
