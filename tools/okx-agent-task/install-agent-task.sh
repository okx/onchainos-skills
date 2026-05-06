#!/usr/bin/env bash
set -euo pipefail

# ──────────────────────────────────────────────────────────────
# Agent Task system installer — onchainos binary + skills bundle
# (macOS / Linux)
#
# 用法：
#   ./install-agent-task.sh                    # 自动找 ~/Downloads 最新 tgz
#   ./install-agent-task.sh /path/to/x.tgz     # 指定 tgz
#   ./install-agent-task.sh                    # 解压后从解压目录跑也行
#
# SRC_ROOT 解析优先级：
#   1. 命令行传 tgz → tar 解到 tmp，从 tmp 取
#   2. 脚本同目录已包含 onchainos + skills/（用户已 tar -xzf 后跑）
#   3. ~/Downloads 下最新的 agent-task-*.tgz → tar 解到 tmp
#
# 安装动作：
#   1. `npx skills add <root> -g -s '*' --yes` — 全局注册所有 skills
#   2. install onchainos → $HOME/.local/bin/onchainos（写入 PATH）
#   3. Print `onchainos --version`
#
# 输出末尾会打：
#   NEW_SHA=<sha>      — 这次要装的 skills/ 内容指纹
#   OLD_SHA=<sha|empty>— 上次安装时记录的指纹
#   SKILLS_CHANGED=1|0 — 是否有变化
# 由调用方（agent / SKILL.md）决定要不要重启 openclaw + 写 mark 文件。
# ──────────────────────────────────────────────────────────────

INSTALL_DIR="$HOME/.local/bin"
BINARY="onchainos"
MARK="$HOME/.onchainos/agent-task-skills.sha"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# 把 $INSTALL_DIR 写入用户 shell rc，让新装的 onchainos 立刻可调用。
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

# 算 skills/ 目录内容指纹（路径 + 文件内容），改一个字符就会变。
hash_skills() {
  (cd "$1" && find . -type f -print0 | LC_ALL=C sort -z \
     | xargs -0 shasum 2>/dev/null | shasum | awk '{print $1}')
}

command -v npx >/dev/null || { echo "✗ missing required command: npx" >&2; exit 1; }

# ── 解析 SRC_ROOT ────────────────────────────────────────────
SRC_BIN=""
SRC_ROOT=""

resolve_root_from_dir() {
  local d="$1"
  if [ -f "$d/onchainos" ] && [ -d "$d/skills" ]; then
    SRC_BIN="$d/onchainos"
    SRC_ROOT="$d"
    return 0
  fi
  return 1
}

extract_to_tmp() {
  local archive="$1"
  command -v tar >/dev/null || { echo "✗ missing required command: tar" >&2; exit 1; }
  local tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  echo "→ extracting $archive"
  tar -xzf "$archive" -C "$tmp"
  if ! resolve_root_from_dir "$tmp"; then
    local inner
    inner="$(find "$tmp" -mindepth 1 -maxdepth 1 -type d | head -n1)"
    [ -n "$inner" ] && resolve_root_from_dir "$inner" || true
  fi
}

if [ -n "${1:-}" ]; then
  ARCHIVE_PATH="$1"
  [ -f "$ARCHIVE_PATH" ] || { echo "✗ archive not found: $ARCHIVE_PATH" >&2; exit 1; }
  extract_to_tmp "$ARCHIVE_PATH"
elif resolve_root_from_dir "$SCRIPT_DIR"; then
  :
else
  # 默认搜 ~/Downloads
  LATEST="$(ls -t "$HOME/Downloads"/agent-task-*.tgz 2>/dev/null | head -1)"
  if [ -n "$LATEST" ]; then
    echo "→ using latest archive in ~/Downloads: $LATEST"
    extract_to_tmp "$LATEST"
  fi
fi

if [ -z "$SRC_BIN" ] || [ -z "$SRC_ROOT" ]; then
  echo "✗ 没找到 onchainos 二进制或 skills/ 目录" >&2
  echo "  尝试顺序: 命令行参数 → 脚本同目录 → ~/Downloads/agent-task-*.tgz" >&2
  echo "  请先解压 tgz，或传 tgz 路径：$0 /path/to/agent-task.tgz" >&2
  exit 1
fi

skill_count=$(find "$SRC_ROOT/skills" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')
[ "$skill_count" = "0" ] && { echo "✗ skills/ 下没有任何 skill 目录" >&2; exit 1; }

# ── 算指纹（装之前就算好，给最后输出用）─────────────────────
new_sha="$(hash_skills "$SRC_ROOT/skills")"
old_sha="$(cat "$MARK" 2>/dev/null || true)"

# 1. Register skills via npx
echo "→ registering skills (npx skills add $SRC_ROOT -g -s '*' --yes)"
npx skills add "$SRC_ROOT" -g -s '*' --yes

# 2. 先把 $INSTALL_DIR 写进 shell rc + 加进当前 PATH —— 这样下面装完 onchainos
# 立刻能在 PATH 里找到，不依赖用户手动 source。
mkdir -p "$INSTALL_DIR"
ensure_in_path

# 3. Install onchainos → $INSTALL_DIR
echo "→ installing $BINARY → $INSTALL_DIR/$BINARY"
install -m 0755 "$SRC_BIN" "$INSTALL_DIR/$BINARY"

# macOS：剥掉 com.apple.quarantine xattr —— tgz 从 IM/网盘下载后解压出来的
# 二进制会带 quarantine 标记，首次执行触发 Gatekeeper 弹窗，用户即使点了允许，
# 当前脚本也会因 set -e 在下面 --version 检查时直接退出。这里主动清掉避免弹窗。
if [ "$(uname -s)" = "Darwin" ]; then
  xattr -dr com.apple.quarantine "$INSTALL_DIR/$BINARY" 2>/dev/null || true
fi

# 4. 自动迁移老装 —— 老版 install-agent-task.sh 落
# /usr/local/bin/onchainos（symlink → ~/.onchainos/onchainos），在 macOS 默认 PATH
# 顺序里排在 ~/.local/bin 前面，会让新装失效。这里直接清掉让 ~/.local/bin 接管。
# 系统目录的 symlink 需要 sudo（会提示输密码一次），失败就 fallback 给手动指引。
LEGACY_LINK="/usr/local/bin/$BINARY"
LEGACY_BIN="$HOME/.onchainos/$BINARY"

# 4a. 用户目录下的二进制副本，直接删
if [ -e "$LEGACY_BIN" ] || [ -L "$LEGACY_BIN" ]; then
  echo "→ 清理老装二进制 $LEGACY_BIN"
  rm -f "$LEGACY_BIN"
fi

# 4b. 系统目录下的 symlink，要 sudo
if [ -e "$LEGACY_LINK" ] || [ -L "$LEGACY_LINK" ]; then
  legacy_real="$(readlink -f "$LEGACY_LINK" 2>/dev/null || echo "$LEGACY_LINK")"
  target_real="$(readlink -f "$INSTALL_DIR/$BINARY" 2>/dev/null || echo "$INSTALL_DIR/$BINARY")"
  if [ "$legacy_real" != "$target_real" ]; then
    echo "→ 清理老装 symlink $LEGACY_LINK（需 sudo 一次性确认）"
    if ! sudo rm -f "$LEGACY_LINK" 2>/dev/null; then
      echo "  ⚠ sudo 删除失败/被拒，请手动跑：sudo rm -f $LEGACY_LINK" >&2
    fi
  fi
fi

# 5. Verify
echo
echo "→ verifying $BINARY --version"
if ! version_output="$("$INSTALL_DIR/$BINARY" --version 2>&1)"; then
  echo "✗ $BINARY 启动失败：" >&2
  echo "$version_output" >&2
  if [ "$(uname -s)" = "Darwin" ]; then
    echo >&2
    echo "若 macOS 弹出过『无法验证开发者』/『已被破坏』等弹窗，按以下任一处理：" >&2
    echo "  a) 打开 系统设置 → 隐私与安全性 → 点『仍要打开』允许 $BINARY" >&2
    echo "  b) 终端运行: xattr -dr com.apple.quarantine $INSTALL_DIR/$BINARY" >&2
    echo "  然后重跑本脚本（或直接 $INSTALL_DIR/$BINARY --version 验证）" >&2
  fi
  exit 1
fi
echo "  $version_output"

echo
echo "✓ install complete ($skill_count skills registered, binary at $INSTALL_DIR/$BINARY)"

# ── 把指纹结果输给调用方（不重启、不写 mark；交给 SKILL.md 决策）─
echo
echo "NEW_SHA=$new_sha"
echo "OLD_SHA=${old_sha:-}"
if [ "$new_sha" != "$old_sha" ]; then
  echo "SKILLS_CHANGED=1"
else
  echo "SKILLS_CHANGED=0"
fi
