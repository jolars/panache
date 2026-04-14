#!/usr/bin/env sh
set -eu

REPO="${PANACHE_REPO:-jolars/panache}"
INSTALL_DIR="${PANACHE_INSTALL_DIR:-$HOME/.local/bin}"
TAG="${PANACHE_TAG:-}"

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
Linux)
  case "$arch" in
  x86_64 | amd64) target="x86_64-unknown-linux-gnu" ;;
  aarch64 | arm64) target="aarch64-unknown-linux-gnu" ;;
  *)
    echo "Unsupported Linux architecture: $arch" >&2
    exit 1
    ;;
  esac
  ;;
Darwin)
  case "$arch" in
  x86_64 | amd64) target="x86_64-apple-darwin" ;;
  arm64 | aarch64) target="aarch64-apple-darwin" ;;
  *)
    echo "Unsupported macOS architecture: $arch" >&2
    exit 1
    ;;
  esac
  ;;
*)
  echo "Unsupported operating system: $os" >&2
  exit 1
  ;;
esac

asset="panache-${target}.tar.gz"

resolve_download_url() {
  if [ -n "$TAG" ]; then
    printf '%s\n' "https://github.com/${REPO}/releases/download/${TAG}/${asset}"
    return 0
  fi

  api_url="https://api.github.com/repos/${REPO}/releases?per_page=100"
  resolved_url="$(
    curl --proto '=https' --tlsv1.2 -fsSL "$api_url" \
      | tr ',' '\n' \
      | grep 'browser_download_url' \
      | grep -F "/${asset}\"" \
      | sed -E 's/.*"browser_download_url"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/' \
      | sed 's#\\/#/#g' \
      | sed -n '1p'
  )"

  if [ -z "$resolved_url" ]; then
    echo "Could not find a release asset named ${asset} in ${REPO}" >&2
    exit 1
  fi

  printf '%s\n' "$resolved_url"
}

url="$(resolve_download_url)"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT INT TERM

echo "Downloading ${asset}..."
curl --proto '=https' --tlsv1.2 -fLsS "$url" -o "$tmpdir/$asset"

tar -xzf "$tmpdir/$asset" -C "$tmpdir"
mkdir -p "$INSTALL_DIR"
install -m 755 "$tmpdir/panache" "$INSTALL_DIR/panache"

echo "Installed panache to $INSTALL_DIR/panache"
case ":$PATH:" in
*":$INSTALL_DIR:"*) ;;
*)
  echo "Note: $INSTALL_DIR is not on PATH."
  ;;
esac
