#!/usr/bin/env bash
# scripts/preflight.sh — onchainos session preflight (POSIX).
#
# Ships inside each skill's scripts/ folder. Invoked from SKILL.md via
# ${CLAUDE_SKILL_DIR}/scripts/preflight.sh (Claude Code substitutes the
# variable to the skill's absolute install path at load time). On other
# agents, the caller must provide an equivalent absolute-path mechanism.
#
# Pipeline:
#   1. Resolve the latest stable release tag (12h cache at ~/.onchainos/last_check).
#   2. Install or update the CLI via install.sh if missing or out-of-date.
#   3. Defer the skill-version drift check to `onchainos skills check`.

set -euo pipefail

SKILL_VERSION=""
for arg in "$@"; do
  case "$arg" in
    --skill-version=*) SKILL_VERSION="${arg#*=}" ;;
  esac
done
[ -n "$SKILL_VERSION" ] || { echo "warn: preflight.sh missing --skill-version arg" >&2; exit 2; }

REPO="okx/onchainos-skills"
CACHE_DIR="$HOME/.onchainos"
CACHE_FILE="$CACHE_DIR/last_check"
CACHE_TTL=43200  # 12h
INSTALL_URL="https://raw.githubusercontent.com/${REPO}/main/install.sh"
mkdir -p "$CACHE_DIR"

now=$(date +%s)
mtime=$({ stat -f %m "$CACHE_FILE" 2>/dev/null || stat -c %Y "$CACHE_FILE" 2>/dev/null; } || echo 0)
if [ -f "$CACHE_FILE" ] && [ $((now - mtime)) -lt "$CACHE_TTL" ]; then
  LATEST_TAG=$(tail -n 1 "$CACHE_FILE" 2>/dev/null || echo "")
else
  LATEST_TAG=$(curl -sSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
               | grep -m1 tag_name \
               | sed 's/.*"\(v[^"]*\)".*/\1/')
  if [ -n "$LATEST_TAG" ]; then
    printf '%s\n%s\n' "$now" "$LATEST_TAG" > "$CACHE_FILE"
  fi
fi

if ! command -v onchainos >/dev/null 2>&1; then
  curl -sSL "$INSTALL_URL" | sh
elif [ -n "$LATEST_TAG" ]; then
  INSTALLED="v$(onchainos --version | awk '{print $NF}')"
  if [ "$INSTALLED" != "$LATEST_TAG" ]; then
    curl -sSL "$INSTALL_URL" | sh
  fi
fi

onchainos skills check --expected-version="$SKILL_VERSION"
