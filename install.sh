#!/bin/sh
set -e

# ──────────────────────────────────────────────────────────────
# onchainos installer / updater (macOS / Linux)
#
# Usage:
#   curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
#   curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh -s -- --beta
#
# Behavior:
#   - Default (stable): fetches latest stable release from GitHub,
#     compares with local version, installs/upgrades if needed.
#   - --beta: fetches all tags, finds the latest version (including
#     pre-releases) by semver, and installs it.
#   - Caches the last check timestamp. Skips GitHub API calls if
#     checked within the last 12 hours.
#
# Supported platforms:
#   macOS  : x86_64 (Intel), arm64 (Apple Silicon)
#   Linux  : x86_64, i686, aarch64, armv7l
#   Windows: see install.ps1 (PowerShell)
# ──────────────────────────────────────────────────────────────

REPO="okx/onchainos-skills"
BINARY="onchainos"
INSTALL_DIR="$HOME/.local/bin"
CACHE_DIR="$HOME/.onchainos"
CACHE_FILE="$CACHE_DIR/last_check"
CACHE_TTL=43200  # 12 hours in seconds

# ── Parse arguments ──────────────────────────────────────────
BETA_MODE=false
for arg in "$@"; do
  case "$arg" in
    --beta) BETA_MODE=true ;;
  esac
done

# ── Platform detection ───────────────────────────────────────
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

# ── Cache helpers ────────────────────────────────────────────
is_cache_fresh() {
  [ -f "$CACHE_FILE" ] || return 1
  cached_ts=$(head -1 "$CACHE_FILE" 2>/dev/null)
  [ -z "$cached_ts" ] && return 1
  now=$(date +%s)
  elapsed=$((now - cached_ts))
  [ "$elapsed" -lt "$CACHE_TTL" ]
}

write_cache() {
  mkdir -p "$CACHE_DIR"
  date +%s > "$CACHE_FILE"
}

# ── Version helpers ──────────────────────────────────────────
get_local_version() {
  if [ -x "$INSTALL_DIR/$BINARY" ]; then
    "$INSTALL_DIR/$BINARY" --version 2>/dev/null | awk '{print $2}'
  fi
}

# Strip pre-release suffix: "2.0.0-beta.0" -> "2.0.0"
strip_prerelease() {
  echo "$1" | sed 's/-.*//'
}

# Extract Nth dot-separated field: _ver_field "1.2.3" 2 -> "2"
_ver_field() {
  echo "$1" | cut -d. -f"$2"
}

# Semver greater-than: returns 0 (true) if $1 > $2, 1 (false) otherwise.
# Handles pre-release: 2.0.0 > 2.0.0-beta.0; 2.0.0-beta.1 > 2.0.0-beta.0
semver_gt() {
  base1=$(strip_prerelease "$1")
  base2=$(strip_prerelease "$2")
  pre1=$(echo "$1" | sed -n 's/[^-]*-//p')
  pre2=$(echo "$2" | sed -n 's/[^-]*-//p')

  # Compare base version fields (major.minor.patch)
  for i in 1 2 3; do
    f1=$(_ver_field "$base1" "$i")
    f2=$(_ver_field "$base2" "$i")
    f1=${f1:-0}
    f2=${f2:-0}
    [ "$f1" -gt "$f2" ] 2>/dev/null && return 0
    [ "$f1" -lt "$f2" ] 2>/dev/null && return 1
  done

  # Base versions equal — compare pre-release
  [ -z "$pre1" ] && [ -z "$pre2" ] && return 1  # equal, not gt
  [ -z "$pre1" ] && return 0  # stable > any pre-release
  [ -z "$pre2" ] && return 1  # pre-release < stable

  # Both have pre-release (e.g., beta.0 vs beta.1)
  num1=$(echo "$pre1" | grep -o '[0-9]*$')
  num2=$(echo "$pre2" | grep -o '[0-9]*$')
  num1=${num1:-0}
  num2=${num2:-0}
  [ "$num1" -gt "$num2" ] 2>/dev/null && return 0
  return 1
}

# ── GitHub API helpers ───────────────────────────────────────

# Fetch latest stable version from GitHub releases API.
get_latest_stable_version() {
  response=$(curl -sSL --max-time 10 "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null) || true
  ver=$(echo "$response" | grep -o '"tag_name": *"v[^"]*"' | head -1 | sed 's/.*"v\([^"]*\)".*/\1/')
  if [ -z "$ver" ]; then
    echo "Error: could not fetch latest version from GitHub." >&2
    echo "Check your network connection or install manually from https://github.com/${REPO}" >&2
    exit 1
  fi
  echo "$ver"
}

# Fetch latest version including betas from tags API.
# Iterates all tags and returns the highest by semver (could be stable or beta).
get_latest_version_with_beta() {
  response=$(curl -sSL --max-time 10 "https://api.github.com/repos/${REPO}/tags?per_page=100" 2>/dev/null) || true
  versions=$(echo "$response" | grep -o '"name": *"v[^"]*"' | sed 's/.*"v\([^"]*\)".*/\1/')

  if [ -z "$versions" ]; then
    echo "Error: could not fetch tags from GitHub." >&2
    echo "Check your network connection or install manually from https://github.com/${REPO}" >&2
    exit 1
  fi

  best=""
  for v in $versions; do
    if [ -z "$best" ]; then
      best="$v"
    elif semver_gt "$v" "$best"; then
      best="$v"
    fi
  done

  if [ -z "$best" ]; then
    echo "Error: no valid versions found in tags." >&2
    exit 1
  fi

  echo "$best"
}

# ── Binary installer ─────────────────────────────────────────
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

# ── Workflow sync ────────────────────────────────────────────
sync_workflows() {
  local tag="$1"
  local workflows_dir="$CACHE_DIR/workflows"
  local tmpdir actual_hash expected_hash
  local workflows_url="https://github.com/${REPO}/releases/download/${tag}/workflows.tar.gz"
  local checksums_url="https://github.com/${REPO}/releases/download/${tag}/workflows-checksums.txt"

  echo "Syncing workflows (${tag})..."

  tmpdir=$(mktemp -d)

  if ! curl -sSL --max-time 30 "$workflows_url" -o "$tmpdir/workflows.tar.gz"; then
    echo "Warning: could not download workflows (non-fatal)" >&2
    rm -rf "$tmpdir"
    return 0
  fi

  # Verify checksum — fail closed: skip install if verification cannot complete
  if ! curl -sSL --max-time 10 "$checksums_url" -o "$tmpdir/workflows-checksums.txt" 2>/dev/null; then
    echo "Warning: could not download workflows checksum — skipping (non-fatal)" >&2
    rm -rf "$tmpdir"
    return 0
  fi

  expected_hash=$(grep "workflows.tar.gz" "$tmpdir/workflows-checksums.txt" | awk '{print $1}')
  if [ -z "$expected_hash" ]; then
    echo "Warning: no checksum found for workflows.tar.gz — skipping (non-fatal)" >&2
    rm -rf "$tmpdir"
    return 0
  fi

  actual_hash=""
  if command -v sha256sum >/dev/null 2>&1; then
    actual_hash=$(sha256sum "$tmpdir/workflows.tar.gz" | awk '{print $1}')
  elif command -v shasum >/dev/null 2>&1; then
    actual_hash=$(shasum -a 256 "$tmpdir/workflows.tar.gz" | awk '{print $1}')
  fi

  if [ -z "$actual_hash" ]; then
    echo "Warning: no sha256 tool found — skipping workflow install (non-fatal)" >&2
    rm -rf "$tmpdir"
    return 0
  fi

  if [ "$actual_hash" != "$expected_hash" ]; then
    echo "Warning: workflows checksum mismatch — skipping (non-fatal)" >&2
    rm -rf "$tmpdir"
    return 0
  fi

  if ! tar -xzf "$tmpdir/workflows.tar.gz" -C "$tmpdir"; then
    echo "Warning: could not extract workflows (non-fatal)" >&2
    rm -rf "$tmpdir"
    return 0
  fi

  if [ -d "$tmpdir/workflows" ]; then
    rm -rf "$workflows_dir"
    mkdir -p "$CACHE_DIR"
    mv "$tmpdir/workflows" "$workflows_dir"
    echo "Workflows synced to ${workflows_dir}"
  fi

  rm -rf "$tmpdir"
}

# ── PATH setup ───────────────────────────────────────────────
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

# ── Main ─────────────────────────────────────────────────────
main() {
  local_ver=$(get_local_version)

  if [ "$BETA_MODE" = true ]; then
    # ── Beta mode: find latest version including pre-releases ──
    target_ver=$(get_latest_version_with_beta)

    if [ "$local_ver" = "$target_ver" ]; then
      # Binary is current — but ensure workflows exist
      if [ ! -d "$CACHE_DIR/workflows" ]; then
        sync_workflows "v${local_ver}"
      fi
      write_cache
      return 0
    fi
  else
    # ── Stable mode ──

    # Fast path: binary exists and was checked recently — skip API call
    if [ -n "$local_ver" ] && is_cache_fresh; then
      # Ensure workflows exist even on cache-hit fast path
      if [ ! -d "$CACHE_DIR/workflows" ]; then
        sync_workflows "v${local_ver}"
      fi
      return 0
    fi

    latest_stable=$(get_latest_stable_version)

    if [ -z "$local_ver" ]; then
      # Not installed — install latest stable
      target_ver="$latest_stable"
    elif [ "$local_ver" = "$latest_stable" ]; then
      # Already on exact latest stable — but ensure workflows exist
      if [ ! -d "$CACHE_DIR/workflows" ]; then
        sync_workflows "v${local_ver}"
      fi
      write_cache
      return 0
    else
      if semver_gt "$latest_stable" "$local_ver"; then
        # Latest stable is newer than local (handles beta→stable upgrade too)
        target_ver="$latest_stable"
      else
        # Local is same or newer (e.g., on a beta ahead of stable)
        if [ ! -d "$CACHE_DIR/workflows" ]; then
          sync_workflows "v${local_ver}"
        fi
        write_cache
        return 0
      fi
    fi
  fi

  if [ -n "$local_ver" ]; then
    echo "Updating ${BINARY} from ${local_ver} to ${target_ver}..."
  fi

  install_binary "v${target_ver}"
  sync_workflows "v${target_ver}"
  write_cache
  ensure_in_path
}

main
