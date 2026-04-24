#!/bin/bash
# onchainos — Pinata template build script
# 1. Installs the onchainos CLI binary
# 2. Downloads the latest skills + workflows from the source repo (git preferred, curl fallback)
# 3. Symlinks skills into OpenClaw's discovery path
# 4. Symlinks workflows into the workspace
# Runs once during the build phase; no action needed from the user.

set -e

# ── 1. Install onchainos CLI ─────────────────────────────────────────────────

echo "[onchainos] Installing onchainos CLI..."

curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh

export PATH="$HOME/.local/bin:$PATH"

echo "[onchainos] Verifying installation..."
onchainos --version

# ── 2. Download latest skills + workflows from source repo ────────────────────────
# Single source of truth: okx/onchainos-skills contains skills/ and workflows/
# Prefers git (supports incremental updates) with curl+tar fallback.

REPO_URL="https://github.com/okx/onchainos-skills.git"
TARBALL_URL="https://github.com/okx/onchainos-skills/archive/refs/heads/main.tar.gz"
REPO_DIR="$HOME/.openclaw/onchainos-skills"

if command -v git &>/dev/null; then
  if [ -d "$REPO_DIR/.git" ]; then
    echo "[onchainos] Updating via git pull..."
    cd "$REPO_DIR" && git pull --ff-only && cd -
  else
    echo "[onchainos] Cloning via git..."
    rm -rf "$REPO_DIR"
    git clone --depth 1 "$REPO_URL" "$REPO_DIR"
  fi
else
  echo "[onchainos] git not available — downloading tarball..."
  rm -rf "$REPO_DIR"
  mkdir -p "$REPO_DIR"
  curl -sSL "$TARBALL_URL" | tar xz --strip-components=1 -C "$REPO_DIR"
fi

echo "[onchainos] Skills + workflows at $REPO_DIR"

# ── 3. Symlink skills into OpenClaw's discovery path ─────────────────────────

SKILLS_LINK="$HOME/.agents/skills/onchainos-skills"
mkdir -p "$(dirname "$SKILLS_LINK")"
rm -f "$SKILLS_LINK"
ln -s "$REPO_DIR/skills" "$SKILLS_LINK"
echo "[onchainos] Skills linked → $SKILLS_LINK"

# ── 4. Symlink workflows into workspace ──────────────────────────────────────
# AGENTS.md references workflows/INDEX.md — this symlink makes that path resolve.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKFLOWS_LINK="$SCRIPT_DIR/workspace/workflows"

if [ -d "$REPO_DIR/workflows" ]; then
  rm -rf "$WORKFLOWS_LINK"
  ln -s "$REPO_DIR/workflows" "$WORKFLOWS_LINK"
  echo "[onchainos] Workflows linked → $WORKFLOWS_LINK"
else
  echo "[onchainos] Workflows not found in repo — using bundled version if available"
fi

echo ""
echo "[onchainos] Setup complete."
echo "  Skills + workflows: from okx/onchainos-skills (latest)"
echo "  Read-only research: available immediately — no login needed."
echo "  Live trading: run 'onchainos wallet login' in a chat session."
echo ""
