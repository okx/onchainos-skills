#!/usr/bin/env bash
set -euo pipefail

# ──────────────────────────────────────────────────────────────
# Agent Task system packager — builds onchainos + bundles the
# 整个 skills/ + install 脚本 into agent-task-*.tgz
#
# Usage:
#   ./pack-agent-task.sh [output-tgz-path]
#
# 默认输出: ~/Downloads/agent-task-YYYYMMDDHHmm.tgz
#
# tgz 内容:
#   onchainos                    ← debug 二进制(debug-log feature + OKX_BASE_URL=cnouxyex.org baked-in)
#   skills/...                   ← 整个 skills/ 目录
#   install-agent-task.sh        ← 安装脚本
# ──────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
CARGO_DIR="$REPO_DIR/cli"
SKILLS_DIR="$REPO_DIR/skills"
INSTALL_SCRIPT="$SCRIPT_DIR/install-agent-task.sh"
TIMESTAMP="$(date +%Y%m%d%H%M)"
DEFAULT_OUT="$HOME/Downloads/agent-task-${TIMESTAMP}.tgz"
OUT_TGZ="${1:-$DEFAULT_OUT}"

[ -d "$CARGO_DIR" ]      || { echo "✗ cli/ not found at $CARGO_DIR" >&2; exit 1; }
[ -d "$SKILLS_DIR" ]     || { echo "✗ skills/ not found at $SKILLS_DIR" >&2; exit 1; }
[ -f "$INSTALL_SCRIPT" ] || { echo "✗ install 脚本不存在: $INSTALL_SCRIPT" >&2; exit 1; }
command -v cargo >/dev/null || { echo "✗ missing required command: cargo" >&2; exit 1; }
command -v tar   >/dev/null || { echo "✗ missing required command: tar" >&2; exit 1; }
command -v node  >/dev/null || { echo "✗ missing required command: node" >&2; exit 1; }

# 1. Build & install onchainos to ~/.cargo/bin (debug profile + debug-log feature,
#    OKX_BASE_URL 通过 option_env!() 在编译期 baked into binary,运行时无需再设 env)
echo "→ installing onchainos (cargo install --debug, OKX_BASE_URL=cnouxyex.org, features=debug-log)"
( cd "$CARGO_DIR" && OKX_BASE_URL=https://www.cnouxyex.org cargo install --path . --force --debug --features debug-log )

BIN_PATH="$HOME/.cargo/bin/onchainos"
[ -f "$BIN_PATH" ] || { echo "✗ cargo install did not produce $BIN_PATH" >&2; exit 1; }

# 2. Stage with the layout expected by install-agent-task.sh
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

install -m 0755 "$BIN_PATH" "$STAGE/onchainos"
cp -R "$SKILLS_DIR" "$STAGE/skills"
install -m 0755 "$INSTALL_SCRIPT" "$STAGE/install-agent-task.sh"

# Strip macOS metadata files
find "$STAGE" -name '.DS_Store' -delete 2>/dev/null || true

# 3. tar 到 stage 内（先不直接落到 ~/Downloads —— macOS TCC 下 bash 可能无写权限，
#    后面交给 node 搬运，让 node 进程的权限决定能否落地）
TMP_TGZ="$STAGE/$(basename "$OUT_TGZ")"
( cd "$STAGE" && tar -czf "$TMP_TGZ" onchainos skills install-agent-task.sh )

# 4. node 搬到目标位置（~/Downloads 默认；macOS 上 node 通常有 TCC 授权）
mkdir -p "$(dirname "$OUT_TGZ")" 2>/dev/null || true
echo "→ 通过 node 写入 $OUT_TGZ"
node -e '
  const fs = require("node:fs");
  const path = require("node:path");
  const [src, dst] = process.argv.slice(1);
  fs.mkdirSync(path.dirname(dst), { recursive: true });
  fs.rmSync(dst, { force: true });
  fs.copyFileSync(src, dst);
  fs.chmodSync(dst, 0o644);
' "$TMP_TGZ" "$OUT_TGZ"

[ -f "$OUT_TGZ" ] || { echo "✗ node 搬运后 $OUT_TGZ 不存在" >&2; exit 1; }

# 5. Report
bin_version="$("$BIN_PATH" --version 2>/dev/null | awk '{print $NF}')"
size_mb=$(awk -v b="$(wc -c < "$OUT_TGZ")" 'BEGIN{printf "%.2f", b/1024/1024}')
skill_count=$(find "$STAGE/skills" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')
echo
echo "✓ packaged: $OUT_TGZ (${size_mb} MB)"
echo "  onchainos version: ${bin_version:-unknown}"
echo "  skills count:      $skill_count"
echo "  layout:"
echo "    onchainos"
echo "    skills/..."
echo "    install-agent-task.sh"
echo
echo "→ install on target machine:"
echo "    tar -xzf $(basename "$OUT_TGZ") && ./install-agent-task.sh"
