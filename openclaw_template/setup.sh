#!/bin/bash
# onchainos — OpenClaw template build script
# 1. Installs the onchainos CLI + workflows (via install.sh)
# 2. Installs skills into the workspace
# Runs once during the build phase; no action needed from the user.

set -e

# ── 1. Install onchainos CLI + workflows ─────────────────────────────────────

curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh

# Ensure onchainos is on PATH for the current session
[ -f "$HOME/.profile" ] && source "$HOME/.profile"

# ── 2. Install skills into workspace ─────────────────────────────────────────
# Clone/update the repo, then copy skills into the workspace skills/ directory
# so OpenClaw discovers them at runtime.

REPO_DIR="$HOME/.openclaw/onchainos-skills"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_SKILLS="${OPENCLAW_WORKSPACE_DIR:-$SCRIPT_DIR/workspace}/skills"

if command -v git &>/dev/null; then
  if [ -d "$REPO_DIR/.git" ]; then
    cd "$REPO_DIR" && git pull --ff-only && cd -
  else
    git clone https://github.com/okx/onchainos-skills.git "$REPO_DIR"
  fi
else
  echo "Warning: git not available — downloading tarball..."
  rm -rf "$REPO_DIR"
  mkdir -p "$REPO_DIR"
  curl -sSL https://github.com/okx/onchainos-skills/archive/refs/heads/main.tar.gz \
    | tar xz --strip-components=1 -C "$REPO_DIR"
fi

# Copy skills into workspace (not symlink — OpenClaw reads from workspace)
mkdir -p "$WORKSPACE_SKILLS"
cp -r "$REPO_DIR/skills/"* "$WORKSPACE_SKILLS/"

echo "[onchainos] Skills installed → $WORKSPACE_SKILLS"
