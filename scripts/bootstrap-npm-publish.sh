#!/usr/bin/env bash
#
# One-time bootstrap: publish stub versions of all 9 npm packages so trusted
# publishers can be configured on them. Once trusted publishing is wired up
# for each package, the real publish-npm.yml workflow takes over and
# supersedes these 0.0.0-bootstrap stubs.
#
# Safe to re-run: existing packages are skipped.
#
# Usage:
#   scripts/bootstrap-npm-publish.sh
#
# Requirements:
#   - npmjs.com account with publish rights on the @panache-cli scope
#     (create the org first via the npmjs.com web UI).
#   - npm CLI installed.

set -euo pipefail

# Work around read-only ~/.npmrc on nix / home-manager setups.
if [[ -z "${NPM_CONFIG_USERCONFIG:-}" ]]; then
  NPM_CONFIG_USERCONFIG="$(mktemp -t npmrc.XXXXXX)"
  export NPM_CONFIG_USERCONFIG
  echo "Using NPM_CONFIG_USERCONFIG=$NPM_CONFIG_USERCONFIG (auto-cleaned on exit)"
  trap 'rm -f "$NPM_CONFIG_USERCONFIG"' EXIT
fi

if ! npm whoami >/dev/null 2>&1; then
  echo "Not logged in to npm; running 'npm login'..."
  npm login
fi
echo "Authenticated as: $(npm whoami)"
echo

NAMES=(
  @panache-cli/panache
  @panache-cli/linux-x64-gnu
  @panache-cli/linux-arm64-gnu
  @panache-cli/linux-x64-musl
  @panache-cli/linux-arm64-musl
  @panache-cli/darwin-x64
  @panache-cli/darwin-arm64
  @panache-cli/win32-x64
  @panache-cli/win32-arm64
)

WORKDIR="$(mktemp -d -t panache-bootstrap.XXXXXX)"
trap 'rm -rf "$WORKDIR"; rm -f "${NPM_CONFIG_USERCONFIG:-}"' EXIT
cd "$WORKDIR"

for name in "${NAMES[@]}"; do
  if npm view "$name" version >/dev/null 2>&1; then
    echo "[skip] $name already exists on npm"
    continue
  fi
  echo "[publish] $name"
  cat > package.json <<EOF
{
  "name": "$name",
  "version": "0.0.0-bootstrap",
  "description": "Bootstrap placeholder for trusted-publisher setup. Do not install.",
  "license": "MIT",
  "repository": "https://github.com/jolars/panache"
}
EOF
  npm publish --access public --tag bootstrap
done

cat <<'EOF'

All 9 packages bootstrapped.

Next steps:
  1. On npmjs.com, configure a trusted publisher for each of the 9 packages
     (Settings -> Publishing access -> Trusted publisher):
       repository:  jolars/panache
       workflow:    publish-npm.yml
       environment: release

  2. The next v* tag (e.g. via versionary's normal release flow) will
     trigger the "Publish npm" workflow. The real publish goes out at the
     workspace version and supersedes the 0.0.0-bootstrap stubs.

     If you want to force a publish without waiting for a new tag, re-run
     the workflow against the most recent v* tag from the Actions tab.
EOF
