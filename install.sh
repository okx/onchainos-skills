#!/bin/sh
set -e

# ──────────────────────────────────────────────────────────────
# onchainos one-line installer (macOS / Linux)
#
# Usage:
#   curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
#
# Steps:
#   1. Detect OS and CPU architecture
#   2. Download the matching binary from GitHub Releases
#   3. Verify SHA256 checksum
#   4. Install to /usr/local/bin
#
# Supported platforms:
#   macOS  : x86_64 (Intel), arm64 (Apple Silicon)
#   Linux  : x86_64, i686, aarch64, armv7l
# For Windows, use install.ps1
# ──────────────────────────────────────────────────────────────

REPO="okx/onchainos-skills"
BINARY="onchainos"
INSTALL_DIR="/usr/local/bin"

# Detect OS and CPU architecture, return matching Rust target triple
get_target() {
  os=$(uname -s)
  arch=$(uname -m)

  case "$os" in
    Darwin)
      case "$arch" in
        x86_64) echo "x86_64-apple-darwin" ;;
        arm64)  echo "aarch64-apple-darwin" ;;
        *) echo "Unsupported architecture: $arch" >&2; exit 1 ;;
      esac
      ;;
    Linux)
      case "$arch" in
        x86_64)  echo "x86_64-unknown-linux-gnu" ;;
        i686)    echo "i686-unknown-linux-gnu" ;;
        aarch64) echo "aarch64-unknown-linux-gnu" ;;
        armv7l)  echo "armv7-unknown-linux-gnueabihf" ;;
        *) echo "Unsupported architecture: $arch" >&2; exit 1 ;;
      esac
      ;;
    *) echo "Unsupported OS: $os (use install.ps1 for Windows)" >&2; exit 1 ;;
  esac
}

main() {
  target=$(get_target)

  # Fetch latest stable release tag from GitHub API (skip prerelease)
  tag=$(curl -sSf "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
  if [ -z "$tag" ]; then
    echo "Error: could not determine latest release" >&2
    exit 1
  fi

  # Download raw binary + checksum file
  binary_name="${BINARY}-${target}"
  url="https://github.com/${REPO}/releases/download/${tag}/${binary_name}"
  checksums_url="https://github.com/${REPO}/releases/download/${tag}/checksums.txt"

  echo "Installing ${BINARY} ${tag} (${target})..."

  tmpdir=$(mktemp -d)
  trap 'rm -rf "$tmpdir"' EXIT

  curl -sSfL "$url" -o "$tmpdir/$binary_name"
  curl -sSfL "$checksums_url" -o "$tmpdir/checksums.txt"

  # SHA256 verification: ensure downloaded file has not been tampered with
  expected_hash=$(grep "$binary_name" "$tmpdir/checksums.txt" | awk '{print $1}')
  if [ -z "$expected_hash" ]; then
    echo "Error: no checksum found for $binary_name" >&2
    exit 1
  fi

  if command -v sha256sum >/dev/null 2>&1; then
    actual_hash=$(sha256sum "$tmpdir/$binary_name" | awk '{print $1}')
  elif command -v shasum >/dev/null 2>&1; then
    actual_hash=$(shasum -a 256 "$tmpdir/$binary_name" | awk '{print $1}')
  else
    echo "Error: sha256sum or shasum is required to verify download" >&2
    exit 1
  fi

  if [ "$actual_hash" != "$expected_hash" ]; then
    echo "Error: checksum mismatch!" >&2
    echo "  Expected: $expected_hash" >&2
    echo "  Got:      $actual_hash" >&2
    echo "The downloaded file may have been tampered with. Aborting." >&2
    exit 1
  fi

  echo "Checksum verified."

  # Install to target directory (auto sudo if no write permission)
  if [ -w "$INSTALL_DIR" ]; then
    mv "$tmpdir/$binary_name" "$INSTALL_DIR/$BINARY"
  else
    sudo mv "$tmpdir/$binary_name" "$INSTALL_DIR/$BINARY"
  fi

  chmod +x "$INSTALL_DIR/$BINARY"

  echo "Installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"
  echo "Run '${BINARY} --help' to get started."
}

main
