#!/bin/bash
# onchainos — OpenClaw template build script
# Installs the onchainos CLI + workflows (via install.sh)
# Skills are installed separately by the agent — see https://github.com/okx/onchainos-skills/blob/main/.openclaw/INSTALL.md

set -e

curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh

# ── Ensure onchainos is on PATH for the current session ──────
# install.sh persists PATH to the shell profile, but the current
# session won't pick it up until we add it explicitly.
INSTALL_DIR="$HOME/.local/bin"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) export PATH="$INSTALL_DIR:$PATH" ;;
esac

# ── Verify ───────────────────────────────────────────────────
if command -v onchainos >/dev/null 2>&1; then
  echo "onchainos $(onchainos --version) is ready"
else
  echo "WARNING: onchainos installed to $INSTALL_DIR but still not on PATH."
  echo "Run: export PATH=\"$INSTALL_DIR:\$PATH\""
  exit 1
fi
