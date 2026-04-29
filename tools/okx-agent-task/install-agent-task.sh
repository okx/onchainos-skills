#!/usr/bin/env bash
set -euo pipefail

# ──────────────────────────────────────────────────────────────
# Agent Task system installer — onchainos binary + skills bundle
# (macOS / Linux)
#
# Usage:
#   ./install-agent-task.sh [path/to/agent-task.zip]
#
# Default zip path: latest `agent-task-YYYYMMDDHHmm.zip` in this
# script's directory (falls back to plain `agent-task.zip`).
#
# Expected zip layout (either form is accepted):
#   onchainos                  ← binary
#   skills/<skill-name>/...
# or wrapped in a single top-level directory:
#   agent-task/onchainos
#   agent-task/skills/<skill-name>/...
#
# What it does:
#   1. Replace ~/.onchainos/onchainos with the bundled binary
#   2. Symlink it to /usr/local/bin/onchainos (sudo if needed)
#   3. Overwrite each skill folder under ~/.openclaw/skills/
#      (untouched skills stay)
#   4. Print `onchainos --version`
# ──────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ -n "${1:-}" ]; then
  ZIP_PATH="$1"
else
  # Pick the lexicographically-latest agent-task-*.zip (timestamp
  # format YYYYMMDDHHmm sorts chronologically).
  ZIP_PATH=""
  shopt -s nullglob
  for candidate in "$SCRIPT_DIR"/agent-task-*.zip; do
    if [ -z "$ZIP_PATH" ] || [[ "$candidate" > "$ZIP_PATH" ]]; then
      ZIP_PATH="$candidate"
    fi
  done
  shopt -u nullglob
  if [ -n "$ZIP_PATH" ]; then
    echo "→ using latest zip: $ZIP_PATH"
  else
    ZIP_PATH="$SCRIPT_DIR/agent-task.zip"
  fi
fi

if [ ! -f "$ZIP_PATH" ]; then
  echo "✗ zip not found: $ZIP_PATH" >&2
  echo "Usage: $0 [path/to/agent-task.zip]" >&2
  exit 1
fi

command -v unzip >/dev/null || { echo "✗ missing required command: unzip" >&2; exit 1; }

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

echo "→ extracting $ZIP_PATH"
unzip -q "$ZIP_PATH" -d "$TMPDIR"

# Resolve the binary and skills inside the extracted tree.
SRC_BIN=""
SRC_SKILLS=""
if [ -f "$TMPDIR/onchainos" ] && [ -d "$TMPDIR/skills" ]; then
  SRC_BIN="$TMPDIR/onchainos"
  SRC_SKILLS="$TMPDIR/skills"
else
  inner="$(find "$TMPDIR" -mindepth 1 -maxdepth 1 -type d | head -n1)"
  if [ -n "$inner" ] && [ -f "$inner/onchainos" ] && [ -d "$inner/skills" ]; then
    SRC_BIN="$inner/onchainos"
    SRC_SKILLS="$inner/skills"
  fi
fi

if [ -z "$SRC_BIN" ] || [ -z "$SRC_SKILLS" ]; then
  echo "✗ zip layout invalid: expected 'onchainos' binary and 'skills/' directory" >&2
  exit 1
fi

# 1. Install onchainos binary
#    Prefer the existing install location (resolved via `readlink -f`),
#    fall back to ~/.onchainos when no prior install or when the only
#    existing copy lives at the symlink path itself.
LINK_PATH="/usr/local/bin/onchainos"
ONCHAIN_DIR="$HOME/.onchainos"
if existing="$(command -v onchainos 2>/dev/null)" && [ -n "$existing" ]; then
  if resolved="$(readlink -f "$existing" 2>/dev/null)" \
     && [ -n "$resolved" ] && [ -f "$resolved" ] \
     && [ "$resolved" != "$LINK_PATH" ]; then
    ONCHAIN_DIR="$(dirname "$resolved")"
    echo "→ detected existing onchainos at $resolved"
  fi
fi
mkdir -p "$ONCHAIN_DIR"
echo "→ installing onchainos → $ONCHAIN_DIR/onchainos"
install -m 0755 "$SRC_BIN" "$ONCHAIN_DIR/onchainos"

# 2. Symlink to /usr/local/bin (LINK_PATH defined above)
LINK_DIR="$(dirname "$LINK_PATH")"
mkdir_cmd=""
ln_cmd="ln -sf"
if [ ! -d "$LINK_DIR" ]; then
  mkdir_cmd="mkdir -p"
fi
needs_sudo=0
if { [ -n "$mkdir_cmd" ] && [ ! -w "$(dirname "$LINK_DIR")" ]; } \
   || { [ -d "$LINK_DIR" ] && [ ! -w "$LINK_DIR" ]; } \
   || { [ -e "$LINK_PATH" ] && [ ! -w "$LINK_PATH" ]; }; then
  needs_sudo=1
fi

echo "→ linking $LINK_PATH"
if [ "$needs_sudo" = "1" ]; then
  echo "  (requires sudo for $LINK_DIR)"
  [ -n "$mkdir_cmd" ] && sudo $mkdir_cmd "$LINK_DIR"
  sudo $ln_cmd "$ONCHAIN_DIR/onchainos" "$LINK_PATH"
else
  [ -n "$mkdir_cmd" ] && $mkdir_cmd "$LINK_DIR"
  $ln_cmd "$ONCHAIN_DIR/onchainos" "$LINK_PATH"
fi

# 3. Install skills — overwrite per-skill, leave others untouched
SKILLS_DIR="$HOME/.openclaw/skills"
mkdir -p "$SKILLS_DIR"
echo "→ installing skills → $SKILLS_DIR"
shopt -s nullglob
count=0
for src in "$SRC_SKILLS"/*/; do
  name="$(basename "$src")"
  rm -rf "$SKILLS_DIR/$name"
  cp -R "$src" "$SKILLS_DIR/$name"
  echo "  ✓ $name"
  count=$((count + 1))
done
shopt -u nullglob
if [ "$count" = "0" ]; then
  echo "✗ no skill directories found in zip's skills/" >&2
  exit 1
fi

# 4. Verify
echo
echo "→ verifying onchainos --version"
if ! version_output="$(onchainos --version 2>&1)"; then
  echo "✗ onchainos failed to run:" >&2
  echo "$version_output" >&2
  exit 1
fi
echo "  $version_output"

# 5. Best-effort: restart openclaw gateway so it picks up new skills.
#    Failure here does NOT fail the install.
echo
echo "→ restarting openclaw gateway (best-effort)"
if openclaw gateway restart; then
  echo "  ✓ gateway restarted"
else
  echo "  ⚠ openclaw gateway restart failed or not available — skipping"
fi

echo
echo "✓ install complete ($count skills, binary at $LINK_PATH)"
