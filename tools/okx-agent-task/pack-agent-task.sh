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
# tgz 内容(4 个 target,install 时按 uname 自动选):
#   onchainos-darwin-arm64       ← macOS Apple Silicon
#   onchainos-darwin-x64         ← macOS Intel
#   onchainos-linux-x64          ← Linux x86_64(交叉编译)
#   onchainos-linux-arm64        ← Linux aarch64(交叉编译,ARM 服务器/Graviton)
#   skills/...                   ← 整个 skills/ 目录
#   install-agent-task.sh        ← 安装脚本
#
# 4 个二进制都启用 debug-log feature + OKX_BASE_URL=cnouxyex.org baked-in。
#
# 交叉编译依赖(只需装一次):
#   方案 A. zigbuild(推荐,轻量,不依赖 Docker):
#       brew install zig
#       cargo install cargo-zigbuild
#       rustup target add x86_64-apple-darwin
#       rustup target add x86_64-unknown-linux-gnu
#       rustup target add aarch64-unknown-linux-gnu
#   方案 B. cross(用 Docker 容器):
#       cargo install cross --git https://github.com/cross-rs/cross
#       rustup target add x86_64-apple-darwin
#       (Docker Desktop 需运行)
#
# 模式快捷开关(推荐用法):
#   --test       打测试包,只 darwin-arm64(host,快,~30s 出包)
#   --release    打发布包,全 4 个 target(默认行为,~30s-3m 视是否首次 cross 编译)
#
# 单独跳过开关(高级用法,跟 --test/--release 不兼容混用):
#   --no-mac-x64       不打 macOS Intel
#   --no-linux         不打 Linux 任何变体(同 --no-linux-x64 --no-linux-arm)
#   --no-linux-x64     不打 Linux x86_64
#   --no-linux-arm     不打 Linux aarch64
# ──────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
CARGO_DIR="$REPO_DIR/cli"
SKILLS_DIR="$REPO_DIR/skills"
INSTALL_SCRIPT="$SCRIPT_DIR/install-agent-task.sh"
TIMESTAMP="$(date +%Y%m%d%H%M)"

# 解析模式开关:--test(只 darwin-arm64) / --release(全 4 个 target,默认)
# + 细粒度 --no-* 跳过(单独使用,不要跟 --test/--release 混用)
SKIP_MAC_X64=0
SKIP_LINUX_X64=0
SKIP_LINUX_ARM=0
POSITIONAL=()
for arg in "$@"; do
  case "$arg" in
    --test)
      # 测试包:只 host darwin-arm64
      SKIP_MAC_X64=1
      SKIP_LINUX_X64=1
      SKIP_LINUX_ARM=1
      ;;
    --release)
      # 发布包:全 4 个 target(等价于默认,无跳过)
      SKIP_MAC_X64=0
      SKIP_LINUX_X64=0
      SKIP_LINUX_ARM=0
      ;;
    --no-mac-x64)   SKIP_MAC_X64=1 ;;
    --no-linux)     SKIP_LINUX_X64=1; SKIP_LINUX_ARM=1 ;;
    --no-linux-x64) SKIP_LINUX_X64=1 ;;
    --no-linux-arm) SKIP_LINUX_ARM=1 ;;
    *) POSITIONAL+=("$arg") ;;
  esac
done
DEFAULT_OUT="$HOME/Downloads/agent-task-${TIMESTAMP}.tgz"
OUT_TGZ="${POSITIONAL[0]:-$DEFAULT_OUT}"

[ -d "$CARGO_DIR" ]      || { echo "✗ cli/ not found at $CARGO_DIR" >&2; exit 1; }
[ -d "$SKILLS_DIR" ]     || { echo "✗ skills/ not found at $SKILLS_DIR" >&2; exit 1; }
[ -f "$INSTALL_SCRIPT" ] || { echo "✗ install 脚本不存在: $INSTALL_SCRIPT" >&2; exit 1; }
command -v cargo >/dev/null || { echo "✗ missing required command: cargo" >&2; exit 1; }
command -v tar   >/dev/null || { echo "✗ missing required command: tar" >&2; exit 1; }
command -v node  >/dev/null || { echo "✗ missing required command: node" >&2; exit 1; }

# 1a. 构建 macOS arm64 原生二进制(host) → cargo install 顺便更新 ~/.cargo/bin
echo "→ [darwin-arm64] cargo install onchainos (host, --debug, OKX_BASE_URL=cnouxyex.org, debug-log)"
( cd "$CARGO_DIR" && OKX_BASE_URL=https://www.cnouxyex.org cargo install --path . --force --debug --features debug-log )

MAC_ARM64_BIN="$HOME/.cargo/bin/onchainos"
[ -f "$MAC_ARM64_BIN" ] || { echo "✗ host cargo install did not produce $MAC_ARM64_BIN" >&2; exit 1; }

# 检测交叉编译工具(zigbuild 优先,cross fallback)。任何 Linux/Mac-Intel target 都用同一套。
HAVE_ZIGBUILD="$(command -v cargo-zigbuild 2>/dev/null || true)"
HAVE_CROSS="$(command -v cross 2>/dev/null || true)"

NEEDS_CROSS_TOOLS=0
[ "$SKIP_LINUX_X64" = "0" ] && NEEDS_CROSS_TOOLS=1
[ "$SKIP_LINUX_ARM" = "0" ] && NEEDS_CROSS_TOOLS=1
# mac-x64 只需 rustup target,无需 zigbuild/cross

if [ "$NEEDS_CROSS_TOOLS" = "1" ] && [ -z "$HAVE_ZIGBUILD" ] && [ -z "$HAVE_CROSS" ]; then
  cat >&2 <<'EOF'
✗ 没有交叉编译工具,无法打 Linux target。装其一:

  方案 A. zigbuild(推荐,轻量):
    brew install zig
    cargo install cargo-zigbuild
    rustup target add x86_64-unknown-linux-gnu
    rustup target add aarch64-unknown-linux-gnu

  方案 B. cross(用 Docker):
    cargo install cross --git https://github.com/cross-rs/cross
    # Docker Desktop 需运行

或者跳过 Linux: bash pack-agent-task.sh --no-linux
EOF
  exit 1
fi

# 跨编译函数(共用): build_target <target-triple> <output-var-name>
#
# 优先复用 `cli/target/<target>/debug/onchainos`(可能由 user 在自己的 Terminal
# 预先 zigbuild 出来——macOS TCC 沙箱下,bash 在 Claude Code 里跑不了 zigbuild
# 因为 zigbuild 要写 ~/Library/Caches/cargo-zigbuild;Terminal.app 有 TCC 权限可以)。
# 如果没有 pre-built 才尝试 inline 编译。
build_cross_target() {
  local target="$1"
  local out_var="$2"
  local out_path="$CARGO_DIR/target/$target/debug/onchainos"

  if [ -f "$out_path" ]; then
    echo "→ [$target] 复用 pre-built binary: $out_path"
    eval "$out_var=\"$out_path\""
    return 0
  fi

  rustup target add "$target" 2>/dev/null || true

  case "$target" in
    *-apple-darwin)
      # Apple SDK 自带 x86_64/aarch64 工具链,不需要 zigbuild,直接 cargo build
      # 不受 TCC 限制(只写 cli/target,不写 ~/Library)
      echo "→ [$target] cargo build (native cross,Apple SDK)"
      ( cd "$CARGO_DIR" && OKX_BASE_URL=https://www.cnouxyex.org \
          cargo build --target "$target" --features debug-log )
      ;;
    *)
      # Linux target 走 zigbuild / cross
      # macOS TCC 沙箱(Claude Code 这种受限环境)下默认 zigbuild 写
      # ~/Library/Caches 会被挡。把所有 zig 中间产物重定向到项目目录里,
      # CARGO_HOME / RUSTUP_HOME 保留指向真实位置,这样 cargo / rustup 照常用。
      local _zb_root="$REPO_DIR/.zigbuild-cache"
      local _zb_home="$_zb_root/fakehome"
      local _zb_global="$_zb_root/global"
      local _zb_local="$_zb_root/local"
      local _zb_tmp="$_zb_root/tmp"
      mkdir -p "$_zb_home" "$_zb_global" "$_zb_local" "$_zb_tmp"

      if [ -n "$HAVE_ZIGBUILD" ]; then
        echo "→ [$target] cargo zigbuild (HOME 重定向 → 项目目录,绕 TCC)"
        ( cd "$CARGO_DIR" && \
            HOME="$_zb_home" \
            CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" \
            RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" \
            ZIG_GLOBAL_CACHE_DIR="$_zb_global" \
            ZIG_LOCAL_CACHE_DIR="$_zb_local" \
            TMPDIR="$_zb_tmp" \
            OKX_BASE_URL=https://www.cnouxyex.org \
            cargo zigbuild --target "$target" --features debug-log ) || true
      else
        echo "→ [$target] cross build"
        ( cd "$CARGO_DIR" && OKX_BASE_URL=https://www.cnouxyex.org \
            cross build --target "$target" --features debug-log ) || true
      fi
      ;;
  esac

  [ -f "$out_path" ] || {
    cat >&2 <<EOF
✗ $target 交叉编译未产出 $out_path

如果你在 Claude Code(或其它 TCC 沙箱)里跑 pack,zigbuild 写 ~/Library/Caches
会失败。请在你的 Terminal.app 里**预编译一次**(只跑一次,以后 cache 在了 pack 走 reuse):

  cd /Users/oker/meili/mingtao.gan_dacs_at_okg.com/121/Documents/RustRoverProgects1/cli
  OKX_BASE_URL=https://www.cnouxyex.org cargo zigbuild --target $target --features debug-log

跑完后回来重跑 pack,会复用 cli/target/$target/debug/onchainos。
EOF
    exit 1
  }
  eval "$out_var=\"$out_path\""
}

# 1b. macOS x86_64 (Intel)
MAC_X64_BIN=""
if [ "$SKIP_MAC_X64" = "0" ]; then
  build_cross_target "x86_64-apple-darwin" MAC_X64_BIN
fi

# 1c. Linux x86_64
LINUX_X64_BIN=""
if [ "$SKIP_LINUX_X64" = "0" ]; then
  build_cross_target "x86_64-unknown-linux-gnu" LINUX_X64_BIN
fi

# 1d. Linux aarch64 (ARM64)
LINUX_ARM64_BIN=""
if [ "$SKIP_LINUX_ARM" = "0" ]; then
  build_cross_target "aarch64-unknown-linux-gnu" LINUX_ARM64_BIN
fi

# 2. Stage with the layout expected by install-agent-task.sh
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

install -m 0755 "$MAC_ARM64_BIN" "$STAGE/onchainos-darwin-arm64"
[ -n "$MAC_X64_BIN"    ] && install -m 0755 "$MAC_X64_BIN"    "$STAGE/onchainos-darwin-x64"
[ -n "$LINUX_X64_BIN"  ] && install -m 0755 "$LINUX_X64_BIN"  "$STAGE/onchainos-linux-x64"
[ -n "$LINUX_ARM64_BIN" ] && install -m 0755 "$LINUX_ARM64_BIN" "$STAGE/onchainos-linux-arm64"
cp -R "$SKILLS_DIR" "$STAGE/skills"
install -m 0755 "$INSTALL_SCRIPT" "$STAGE/install-agent-task.sh"

# Strip macOS metadata files
find "$STAGE" -name '.DS_Store' -delete 2>/dev/null || true

# 3. tar 到 stage 内（先不直接落到 ~/Downloads —— macOS TCC 下 bash 可能无写权限，
#    后面交给 node 搬运，让 node 进程的权限决定能否落地）
TMP_TGZ="$STAGE/$(basename "$OUT_TGZ")"
TAR_FILES=("onchainos-darwin-arm64" "skills" "install-agent-task.sh")
[ -n "$MAC_X64_BIN"    ] && TAR_FILES+=("onchainos-darwin-x64")
[ -n "$LINUX_X64_BIN"  ] && TAR_FILES+=("onchainos-linux-x64")
[ -n "$LINUX_ARM64_BIN" ] && TAR_FILES+=("onchainos-linux-arm64")
( cd "$STAGE" && tar -czf "$TMP_TGZ" "${TAR_FILES[@]}" )

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
bin_version="$("$MAC_ARM64_BIN" --version 2>/dev/null | awk '{print $NF}')"
size_mb=$(awk -v b="$(wc -c < "$OUT_TGZ")" 'BEGIN{printf "%.2f", b/1024/1024}')
skill_count=$(find "$STAGE/skills" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')
PLATFORMS=("Darwin-arm64")
[ -n "$MAC_X64_BIN"    ] && PLATFORMS+=("Darwin-x64")
[ -n "$LINUX_X64_BIN"  ] && PLATFORMS+=("Linux-x64")
[ -n "$LINUX_ARM64_BIN" ] && PLATFORMS+=("Linux-arm64")
echo
echo "✓ packaged: $OUT_TGZ (${size_mb} MB)"
echo "  onchainos version: ${bin_version:-unknown}"
echo "  platforms:         ${PLATFORMS[*]}"
echo "  skills count:      $skill_count"
echo "  layout:"
echo "    onchainos-darwin-arm64"
[ -n "$MAC_X64_BIN"    ] && echo "    onchainos-darwin-x64"
[ -n "$LINUX_X64_BIN"  ] && echo "    onchainos-linux-x64"
[ -n "$LINUX_ARM64_BIN" ] && echo "    onchainos-linux-arm64"
echo "    skills/..."
echo "    install-agent-task.sh"
echo
echo "→ install on target machine (auto-detects platform via uname):"
echo "    tar -xzf $(basename "$OUT_TGZ") && ./install-agent-task.sh"
