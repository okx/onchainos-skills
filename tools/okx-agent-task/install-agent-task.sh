#!/usr/bin/env bash
set -euo pipefail

# ──────────────────────────────────────────────────────────────
# Agent Task system installer — onchainos binary + skills bundle
# (macOS / Linux)
#
# 用法（解压后从解压目录跑）：
#   tar -xzf agent-task-YYYYMMDDHHmm.tgz
#   cd agent-task-YYYYMMDDHHmm           # 若 tar 解出了带名字的目录
#   ./install-agent-task.sh
#
# 也可以传一个未解压的 tgz：
#   ./install-agent-task.sh /path/to/agent-task.tgz
#
# 解压后期望脚本同目录有：
#   onchainos                  ← 二进制
#   skills/<skill-name>/...
#   install-agent-task.sh      ← 本脚本
#
# 安装动作：
#   1. `npx skills add <root> -g -s '*' --yes` — 全局注册所有 skills
#   2. install onchainos → $HOME/.local/bin/onchainos（写入 PATH）
#   3. Print `onchainos --version`
# ──────────────────────────────────────────────────────────────

INSTALL_DIR="$HOME/.local/bin"
BINARY="onchainos"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Mirror onchainos online installer (install.sh): 把 $INSTALL_DIR 写入用户
# shell rc，让新装的 onchainos 立刻可调用。已经在 PATH 里就跳过。
ensure_in_path() {
  case ":$PATH:" in
    *":$INSTALL_DIR:"*) return 0 ;;
  esac

  export_line="export PATH=\"\$HOME/.local/bin:\$PATH\""
  shell_name=$(basename "${SHELL:-sh}" 2>/dev/null || echo "sh")
  case "$shell_name" in
    zsh)  profile="$HOME/.zshrc" ;;
    bash)
      if [ -f "$HOME/.bash_profile" ]; then
        profile="$HOME/.bash_profile"
      elif [ -f "$HOME/.bashrc" ]; then
        profile="$HOME/.bashrc"
      else
        profile="$HOME/.profile"
      fi
      ;;
    *)    profile="$HOME/.profile" ;;
  esac

  if [ -f "$profile" ] && grep -qF '$HOME/.local/bin' "$profile" 2>/dev/null; then
    export PATH="$INSTALL_DIR:$PATH"
    return 0
  fi

  echo "" >> "$profile"
  echo "# Added by agent-task installer" >> "$profile"
  echo "$export_line" >> "$profile"
  export PATH="$INSTALL_DIR:$PATH"

  echo
  echo "→ 已写入 PATH 到 $profile"
  echo "  当前 shell 生效：source $profile"
}

command -v npx >/dev/null || { echo "✗ missing required command: npx" >&2; exit 1; }

# ── 解析 SRC_ROOT ────────────────────────────────────────────
# 优先级：
#   1. 命令行传 tgz → tar 解压到 tmp，从 tmp 取
#   2. 默认：脚本同目录已包含 onchainos + skills/（用户已 tar -xzf 后跑）
SRC_BIN=""
SRC_ROOT=""
TMPDIR=""

resolve_root_from_dir() {
  local d="$1"
  if [ -f "$d/onchainos" ] && [ -d "$d/skills" ]; then
    SRC_BIN="$d/onchainos"
    SRC_ROOT="$d"
    return 0
  fi
  return 1
}

if [ -n "${1:-}" ]; then
  ARCHIVE_PATH="$1"
  [ -f "$ARCHIVE_PATH" ] || { echo "✗ archive not found: $ARCHIVE_PATH" >&2; exit 1; }
  command -v tar >/dev/null || { echo "✗ missing required command: tar" >&2; exit 1; }
  TMPDIR="$(mktemp -d)"
  trap 'rm -rf "$TMPDIR"' EXIT
  echo "→ extracting $ARCHIVE_PATH"
  tar -xzf "$ARCHIVE_PATH" -C "$TMPDIR"
  if ! resolve_root_from_dir "$TMPDIR"; then
    inner="$(find "$TMPDIR" -mindepth 1 -maxdepth 1 -type d | head -n1)"
    [ -n "$inner" ] && resolve_root_from_dir "$inner" || true
  fi
else
  resolve_root_from_dir "$SCRIPT_DIR" || true
fi

if [ -z "$SRC_BIN" ] || [ -z "$SRC_ROOT" ]; then
  echo "✗ 没找到 onchainos 二进制或 skills/ 目录" >&2
  echo "  请确认你已经解压了 tgz，并在解压后的目录里执行本脚本：" >&2
  echo "    tar -xzf agent-task-*.tgz" >&2
  echo "    ./install-agent-task.sh" >&2
  echo "  或者直接传 tgz 路径：" >&2
  echo "    $0 /path/to/agent-task.tgz" >&2
  exit 1
fi

skill_count=$(find "$SRC_ROOT/skills" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')
[ "$skill_count" = "0" ] && { echo "✗ skills/ 下没有任何 skill 目录" >&2; exit 1; }

# 1. Register skills via npx
echo "→ registering skills (npx skills add $SRC_ROOT -g -s '*' --yes)"
npx skills add "$SRC_ROOT" -g -s '*' --yes

# 2. Install onchainos → $INSTALL_DIR
mkdir -p "$INSTALL_DIR"
echo "→ installing $BINARY → $INSTALL_DIR/$BINARY"
install -m 0755 "$SRC_BIN" "$INSTALL_DIR/$BINARY"

# 把 $INSTALL_DIR 加进 PATH（与线上 install.sh 一致）
ensure_in_path

# 3. Verify
echo
echo "→ verifying $BINARY --version"
if ! version_output="$("$INSTALL_DIR/$BINARY" --version 2>&1)"; then
  echo "✗ $BINARY failed to run:" >&2
  echo "$version_output" >&2
  exit 1
fi
echo "  $version_output"

echo
echo "✓ install complete ($skill_count skills registered, binary at $INSTALL_DIR/$BINARY)"
