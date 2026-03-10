#!/bin/sh
set -e

# ──────────────────────────────────────────────────────────────
# onchainos installer / updater (macOS / Linux)
#
# Usage:
#   curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
#
# Behavior:
#   - Fresh install: detect platform, download latest binary, verify, install.
#   - Already installed: skip if the same version was verified within the
#     last 12 hours (cache at ~/.onchainos/last_check). Otherwise, compare the
#     local version with the latest GitHub release and upgrade if needed.
#
# Supported platforms:
#   macOS  : x86_64 (Intel), arm64 (Apple Silicon)
#   Linux  : x86_64, i686, aarch64, armv7l
# ──────────────────────────────────────────────────────────────

REPO="okx/onchainos-skills"
BINARY="onchainos"
INSTALL_DIR="$HOME/.local/bin"
CACHE_DIR="$HOME/.onchainos"
CACHE_FILE="$CACHE_DIR/last_check"
CACHE_TTL=43200  # 12 hours in seconds

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
    *) echo "Unsupported OS" >&2; exit 1 ;;
  esac
}

is_cache_fresh() {
  [ -f "$CACHE_FILE" ] || return 1
  cached_ts=$(cat "$CACHE_FILE" 2>/dev/null | head -1)
  [ -z "$cached_ts" ] && return 1
  now=$(date +%s)
  elapsed=$((now - cached_ts))
  [ "$elapsed" -lt "$CACHE_TTL" ]
}

write_cache() {
  mkdir -p "$CACHE_DIR"
  date +%s > "$CACHE_FILE"
}

get_local_version() {
  if [ -x "$INSTALL_DIR/$BINARY" ]; then
    "$INSTALL_DIR/$BINARY" --version 2>/dev/null | awk '{print $2}'
  fi
}

normalize_tag() {
  echo "$1" | sed 's/^v//'
}

install_binary() {
  target=$(get_target)
  if [ -z "$target" ]; then
    exit 1
  fi
  tag="$1"

  binary_name="${BINARY}-${target}"
  url="https://github.com/${REPO}/releases/download/${tag}/${binary_name}"
  checksums_url="https://github.com/${REPO}/releases/download/${tag}/checksums.txt"

  echo "Installing ${BINARY} ${tag} (${target})..."

  tmpdir=$(mktemp -d)
  trap 'rm -rf "$tmpdir"' EXIT

  curl -sSL "$url" -o "$tmpdir/$binary_name"
  curl -sSL "$checksums_url" -o "$tmpdir/checksums.txt"

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

  mkdir -p "$INSTALL_DIR"
  mv "$tmpdir/$binary_name" "$INSTALL_DIR/$BINARY"
  chmod +x "$INSTALL_DIR/$BINARY"


  echo "Installed ${BINARY} ${tag} to ${INSTALL_DIR}/${BINARY}"
}

ensure_in_path() {
  # Check if INSTALL_DIR is already in PATH
  case ":$PATH:" in
    *":$INSTALL_DIR:"*) return 0 ;;
  esac

  EXPORT_LINE="export PATH=\"\$HOME/.local/bin:\$PATH\""

  # Detect shell and pick profile file
  shell_name=$(basename "$SHELL" 2>/dev/null || echo "sh")
  case "$shell_name" in
    zsh)  profile="$HOME/.zshrc" ;;
    bash)
      if [ -f "$HOME/.bash_profile" ]; then
        profile="$HOME/.bash_profile"
      elif [ -f "$HOME/.bashrc" ]; then
        profile="$HOME/.bashrc"
      else
        profile="$HOME/.profile"
      fi
      ;;
    *)    profile="$HOME/.profile" ;;
  esac

  # Skip if already present in profile
  if [ -f "$profile" ] && grep -qF '$HOME/.local/bin' "$profile" 2>/dev/null; then
    return 0
  fi

  echo "" >> "$profile"
  echo "# Added by onchainos installer" >> "$profile"
  echo "$EXPORT_LINE" >> "$profile"

  # Make it available in the current script process
  export PATH="$INSTALL_DIR:$PATH"

  echo ""
  echo "Added $INSTALL_DIR to PATH in $profile"
  echo "To start using '${BINARY}' now, run:"
  echo ""
  echo "  source $profile"
  echo ""
  echo "Or simply open a new terminal window."
}

main() {
  local_ver=$(get_local_version)

  # Fast path: already installed and checked within the last 12 hours
  if [ -n "$local_ver" ] && is_cache_fresh; then
    echo "${BINARY} ${local_ver} is already installed (update check skipped, checked recently)."
    return 0
  fi

  # Fetch latest release tag from GitHub API
  tag=$(curl -sS "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
  if [ -z "$tag" ]; then
    echo "Error: could not determine latest release" >&2
    exit 1
  fi

  latest_ver=$(normalize_tag "$tag")

  if [ -n "$local_ver" ] && [ "$local_ver" = "$latest_ver" ]; then
    echo "${BINARY} ${local_ver} is already up to date."
    write_cache
    return 0
  fi

  if [ -n "$local_ver" ]; then
    echo "Upgrading ${BINARY} from ${local_ver} to ${latest_ver}..."
  fi

  install_binary "$tag"
  write_cache
  ensure_in_path
  echo "Run '${BINARY} --help' to get started."
}

main
