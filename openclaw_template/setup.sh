#!/bin/sh
# onchainos — OpenClaw template build script
# 1. Installs the onchainos CLI + workflows (via install.sh)
# 2. Installs skills into the workspace

set -e

REPO_URL="https://github.com/okx/onchainos-skills"
REPO_DIR="$HOME/.openclaw/onchainos-skills"
INSTALL_DIR="$HOME/.local/bin"

# ── 1. Install CLI + workflows ──────────────────────────────
echo "[onchainos] Installing CLI + workflows..."

# Clear cache if binary is missing — forces install.sh to re-download
if [ ! -f "$INSTALL_DIR/onchainos" ]; then
  rm -f "$HOME/.onchainos/last_check"
fi

curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh

# ── Ensure onchainos is on PATH ──────────────────────────────

# Copy to a directory already on PATH (most reliable for sandboxes)
if [ -f "$INSTALL_DIR/onchainos" ]; then
  for bin_dir in /usr/local/bin /usr/bin; do
    if [ -d "$bin_dir" ] && [ -w "$bin_dir" ]; then
      cp -f "$INSTALL_DIR/onchainos" "$bin_dir/onchainos"
      echo "[onchainos] Copied to $bin_dir/onchainos"
      break
    fi
  done
fi

# Also add to PATH for the current session as fallback
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) export PATH="$INSTALL_DIR:$PATH"
     echo "[onchainos] Added $INSTALL_DIR to PATH" ;;
esac

# Create env file that can be sourced to set PATH
ENV_DIR="$HOME/.onchainos"
ENV_FILE="$ENV_DIR/env"
mkdir -p "$ENV_DIR"
cat > "$ENV_FILE" <<'ENVEOF'
# onchainos shell setup
export PATH="$HOME/.local/bin:$PATH"
ENVEOF

# Persist to shell profiles — include .zshenv for non-interactive zsh sessions
SOURCE_LINE=". \"$ENV_FILE\""
for profile in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.zshenv" "$HOME/.zshrc"; do
  # Always create .profile and .zshenv; others only if they already exist
  if [ -f "$profile" ] || [ "$profile" = "$HOME/.profile" ] || [ "$profile" = "$HOME/.zshenv" ]; then
    if ! grep -qF "$ENV_FILE" "$profile" 2>/dev/null; then
      echo "" >> "$profile"
      echo "# Added by onchainos setup" >> "$profile"
      echo "$SOURCE_LINE" >> "$profile"
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

SKILLS_DIR="$HOME/.onchainos/skills"
mkdir -p "$SKILLS_DIR"
cp -r "$REPO_DIR/skills/"* "$SKILLS_DIR/" 2>/dev/null || true

echo "[onchainos] Skills installed to $SKILLS_DIR/"
echo "[onchainos] Setup complete."
