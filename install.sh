#!/bin/sh
# mcpdrain installer — downloads the latest prebuilt binary from GitHub Releases.
# Usage:  curl -fsSL https://raw.githubusercontent.com/patchwright/mcpdrain/main/install.sh | sh
# Env:    MCPDRAIN_INSTALL_DIR  (default: ~/.local/bin)
#         MCPDRAIN_VERSION       (default: latest release)
set -eu

OWNER="patchwright"
REPO="mcpdrain"
INSTALL_DIR="${MCPDRAIN_INSTALL_DIR:-${HOME}/.local/bin}"

uname_s=$(uname -s)
uname_m=$(uname -m)

case "$uname_s" in
  Linux)  os=unknown-linux-musl ;;
  Darwin) os=apple-darwin ;;
  *) echo "mcpdrain: unsupported OS '$uname_s' (Linux/macOS only in v0.1)" >&2; exit 1 ;;
esac
case "$uname_m" in
  x86_64|amd64)   arch=x86_64 ;;
  arm64|aarch64)  arch=aarch64 ;;
  *) echo "mcpdrain: unsupported arch '$uname_m'" >&2; exit 1 ;;
esac
target="${arch}-${os}"

if [ -n "${MCPDRAIN_VERSION:-}" ]; then
  tag="$MCPDRAIN_VERSION"
else
  tag=$(curl -fsSL "https://api.github.com/repos/${OWNER}/${REPO}/releases/latest" \
        | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n1)
  [ -n "$tag" ] || { echo "mcpdrain: could not determine latest release" >&2; exit 1; }
fi

url="https://github.com/${OWNER}/${REPO}/releases/download/${tag}/mcpdrain-${target}.tar.gz"
tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

echo "mcpdrain: downloading ${tag} for ${target}…"
curl -fsSL "$url" | tar -xzf - -C "$tmpdir"

mkdir -p "$INSTALL_DIR"
mv "$tmpdir/mcpdrain-${target}" "${INSTALL_DIR}/mcpdrain"
chmod +x "${INSTALL_DIR}/mcpdrain"

echo "mcpdrain: installed → ${INSTALL_DIR}/mcpdrain"
case ":$PATH:" in
  *":${INSTALL_DIR}:"*) ;;
  *) echo "mcpdrain: note: ${INSTALL_DIR} is not on your PATH" >&2 ;;
esac
