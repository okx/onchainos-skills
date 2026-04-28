#!/usr/bin/env bash
set -euo pipefail

# ──────────────────────────────────────────────────────────────
# Agent Task system packager — builds onchainos + bundles the
# okx-agent-task skill into agent-task.zip
#
# Usage:
#   ./pack-agent-task.sh [output-zip-path]
#
# Default output: <repo>/scripts/agent-task.zip
# (matches the default zip path looked up by install-agent-task.sh)
#
# Output zip layout:
#   onchainos                        ← debug-built binary
#   skills/okx-agent-task/...        ← skill folder
# ──────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
CARGO_DIR="$REPO_DIR/cli"
SKILL_DIR="$REPO_DIR/skills/okx-agent-task"
OUT_ZIP="${1:-$SCRIPT_DIR/agent-task.zip}"

[ -d "$CARGO_DIR" ] || { echo "✗ cli/ not found at $CARGO_DIR" >&2; exit 1; }
[ -d "$SKILL_DIR" ] || { echo "✗ skills/okx-agent-task not found at $SKILL_DIR" >&2; exit 1; }
command -v cargo >/dev/null || { echo "✗ missing required command: cargo" >&2; exit 1; }
command -v zip   >/dev/null || { echo "✗ missing required command: zip" >&2; exit 1; }

# 1. Build debug binary
echo "→ building onchainos (cargo build)"
( cd "$CARGO_DIR" && cargo build )

BIN_PATH="$CARGO_DIR/target/debug/onchainos"
[ -f "$BIN_PATH" ] || { echo "✗ build did not produce $BIN_PATH" >&2; exit 1; }

# 2. Stage with the layout expected by install-agent-task.sh
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

install -m 0755 "$BIN_PATH" "$STAGE/onchainos"
mkdir -p "$STAGE/skills"
cp -R "$SKILL_DIR" "$STAGE/skills/okx-agent-task"

# Strip macOS metadata files
find "$STAGE" -name '.DS_Store' -delete 2>/dev/null || true

# 3. Zip (overwrite if exists)
rm -f "$OUT_ZIP"
mkdir -p "$(dirname "$OUT_ZIP")"
( cd "$STAGE" && zip -qr "$OUT_ZIP" onchainos skills )

# 4. Report
bin_version="$("$BIN_PATH" --version 2>/dev/null | awk '{print $NF}')"
size_kb=$(( $(wc -c < "$OUT_ZIP") / 1024 ))
echo
echo "✓ packaged: $OUT_ZIP (${size_kb} KB)"
echo "  onchainos version: ${bin_version:-unknown}"
echo "  layout:"
echo "    onchainos"
echo "    skills/okx-agent-task/"
echo
echo "→ install on target machine:"
echo "    ./install-agent-task.sh $OUT_ZIP"
