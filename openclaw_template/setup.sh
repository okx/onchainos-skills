#!/bin/bash
# onchainos — OpenClaw template build script
# 1. Installs the onchainos CLI + workflows (via install.sh)
# 2. Clones skills repo (as per .openclaw/INSTALL.md)
# Skills are copied into the workspace by the agent on first session (BOOTSTRAP.md).

set -e

# ── 1. Install onchainos CLI + workflows ─────────────────────────────────────

curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh

# Ensure onchainos is on PATH for the current session
[ -f "$HOME/.profile" ] && source "$HOME/.profile"

# ── 2. Clone skills repo (as per .openclaw/INSTALL.md) ───────────────────────

REPO_DIR="$HOME/.openclaw/onchainos-skills"

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

echo "[onchainos] Skills downloaded → $REPO_DIR/skills"
echo "[onchainos] Agent will copy skills into workspace on first session."
