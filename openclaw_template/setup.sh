#!/bin/sh
# onchainos — OpenClaw template build script
# 1. Installs the onchainos CLI + workflows (via install.sh)
# 2. Installs skills into the workspace

set -e

REPO_URL="https://github.com/okx/onchainos-skills"
REPO_DIR="$HOME/.openclaw/onchainos-skills"

# ── 1. Install CLI + workflows ──────────────────────────────
echo "[onchainos] Installing CLI + workflows..."

curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh

# ── Ensure onchainos is on PATH ──────────────────────────────
INSTALL_DIR="$HOME/.local/bin"

# Symlink to a directory that's already on PATH (works in sandboxes
# where shell profiles aren't sourced)
if [ -f "$INSTALL_DIR/onchainos" ]; then
  for bin_dir in /usr/local/bin /usr/bin; do
    if [ -d "$bin_dir" ] && [ -w "$bin_dir" ]; then
      ln -sf "$INSTALL_DIR/onchainos" "$bin_dir/onchainos"
      echo "[onchainos] Symlinked to $bin_dir/onchainos"
      break
    fi
  done
fi

# Also add to PATH for the current session
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) export PATH="$INSTALL_DIR:$PATH"
     echo "[onchainos] Added $INSTALL_DIR to PATH" ;;
esac

# Persist to shell profiles for environments that do source them
EXPORT_LINE='export PATH="$HOME/.local/bin:$PATH"'
for profile in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.zshrc"; do
  if [ -f "$profile" ] || [ "$profile" = "$HOME/.profile" ]; then
    if ! grep -qF '.local/bin' "$profile" 2>/dev/null; then
      echo "" >> "$profile"
      echo "# Added by onchainos setup" >> "$profile"
      echo "$EXPORT_LINE" >> "$profile"
      echo "[onchainos] Added PATH to $profile"
    fi
  fi
done

# Verify CLI
if command -v onchainos >/dev/null 2>&1; then
  echo "[onchainos] $(onchainos --version) is ready"
else
  echo "[onchainos] ERROR: onchainos not found on PATH after install."
  echo "[onchainos] Checked: $INSTALL_DIR/onchainos exists = $([ -f "$INSTALL_DIR/onchainos" ] && echo yes || echo no)"
  echo "[onchainos] PATH = $PATH"
  exit 1
fi

# ── 2. Install skills ───────────────────────────────────────
echo "[onchainos] Installing skills..."

if [ -d "$REPO_DIR/.git" ]; then
  git -C "$REPO_DIR" pull --ff-only || true
else
  git clone "$REPO_URL" "$REPO_DIR"
fi

mkdir -p skills
cp -r "$REPO_DIR/skills/"* skills/ 2>/dev/null || true

echo "[onchainos] Skills installed to skills/"
echo "[onchainos] Setup complete."
