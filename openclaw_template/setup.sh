#!/bin/bash
# onchainos — OpenClaw template build script
# Installs the onchainos CLI + workflows (via install.sh)
# Skills are installed separately by the agent — see https://github.com/okx/onchainos-skills/blob/main/.openclaw/INSTALL.md

set -e

curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh

# ── Ensure onchainos is on PATH ──────────────────────────────
INSTALL_DIR="$HOME/.local/bin"

# Symlink to a directory that's already on PATH (works in sandboxes
# where shell profiles aren't sourced)
if [ -f "$INSTALL_DIR/onchainos" ]; then
  for bin_dir in /usr/local/bin /usr/bin; do
    if [ -d "$bin_dir" ] && [ -w "$bin_dir" ]; then
      ln -sf "$INSTALL_DIR/onchainos" "$bin_dir/onchainos"
      break
    fi
  done
fi

# Also add to PATH for the current session
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) export PATH="$INSTALL_DIR:$PATH" ;;
esac

# Persist to shell profiles for environments that do source them
EXPORT_LINE='export PATH="$HOME/.local/bin:$PATH"'
for profile in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.zshrc"; do
  if [ -f "$profile" ] || [ "$profile" = "$HOME/.profile" ]; then
    if ! grep -qF '.local/bin' "$profile" 2>/dev/null; then
      echo "" >> "$profile"
      echo "# Added by onchainos setup" >> "$profile"
      echo "$EXPORT_LINE" >> "$profile"
    fi
  fi
done

# ── Verify ───────────────────────────────────────────────────
if command -v onchainos >/dev/null 2>&1; then
  echo "onchainos $(onchainos --version) is ready"
else
  echo "WARNING: onchainos installed to $INSTALL_DIR but not found on PATH."
  echo "Run: export PATH=\"$INSTALL_DIR:\$PATH\""
  exit 1
fi
