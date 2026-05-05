---
name: okx-agent-task-test
description: "Use this skill when the user asks to (re)install or update the local agent-task system bundle, e.g. 重新安装任务系统包, 更新任务系统, 装最新的任务系统, 安装最新 agent-task 包, install/update task system, reinstall agent-task. Picks the newest ~/Downloads/agent-task-*.tgz and runs its bundled install-agent-task.sh. Do NOT use for general onchainos install/update from GitHub releases (use the public install.sh) or for any task lifecycle operation (use okx-agent-task)."
license: MIT
metadata:
  author: okx
  version: "1.0.0"
  homepage: "https://web3.okx.com"
---

# Agent Task System — Local Bundle Installer

只做一件事：跑开发者本地打的 `~/Downloads/agent-task-*.tgz` 里带的安装脚本，并在 skills 内容变化时和用户确认是否重启 openclaw。

发现 tgz、解压、装二进制 / 注册 skills、算 SHA 对比，全部由 `install-agent-task.sh` 单源真相做掉；本 skill 只做交互决策。

## Step 1：跑安装脚本

```bash
bash "$HOME/.local/share/agent-task-installer/install-agent-task.sh"   # 若已有
# 或 tgz 还没解过 / 没装过 installer，直接从最新 tgz 拉脚本：
TGZ="$(ls -t "$HOME/Downloads"/agent-task-*.tgz 2>/dev/null | head -1)"
[ -n "$TGZ" ] || { echo "✗ ~/Downloads/ 下找不到 agent-task-*.tgz —— 先到仓库里跑 tools/okx-agent-task/pack-agent-task.sh 打包" >&2; exit 1; }
TMP="$(mktemp -d)"; tar -xzf "$TGZ" -C "$TMP" && bash "$TMP/install-agent-task.sh"
```

脚本结尾会打三行关键输出，**必须**抓出来：

```
NEW_SHA=<hex>
OLD_SHA=<hex|empty>
SKILLS_CHANGED=1|0
```

## Step 2：仅当 `SKILLS_CHANGED=1` 时，问用户是否重启

- `SKILLS_CHANGED=0` → 跳过 Step 2，直接结束。
- `SKILLS_CHANGED=1` → 用一句话问：「检测到 skills 有更新，需要重启 openclaw 让新版生效，是否现在重启？(y/N)」
  - 用户答 **是 / y / yes / 重启** → 跑下面这段（同时把 NEW_SHA 写入指纹文件，避免下次重复问）：
    ```bash
    NEW_SHA="<Step 1 输出的 NEW_SHA 原值>"
    MARK="$HOME/.onchainos/agent-task-skills.sha"
    mkdir -p "$(dirname "$MARK")"
    printf '%s' "$NEW_SHA" > "$MARK"
    if command -v openclaw >/dev/null && openclaw gateway restart; then
      echo "  ✓ gateway restarted"
    else
      echo "  ⚠ openclaw gateway restart 失败或不可用"
    fi
    ```
  - 用户答 **否 / n / no / 不要 / 稍后** → 不动，**不**写指纹文件（保持 OLD_SHA），下次再问。

## 行为契约

- **不要**自己重新打包（不调 `pack-agent-task.sh`）—— 这个 skill 只装已有的包；包过期由用户自己重新跑打包脚本。
- **不要**绕开 `install-agent-task.sh` 自己实现安装（不手动 cp 二进制 / 不手动跑 `npx skills add`）—— 单源真相在 shell 脚本里，避免行为漂移。
- 装完把脚本输出贴回给用户（含 `onchainos --version` 和 `SKILLS_CHANGED=...`），让用户确认。
- 如果 `~/Downloads/` 没有 tgz：提示用户去 `tools/okx-agent-task/pack-agent-task.sh` 打包，**不要**尝试别的目录或自己临时打包。
- 重启 openclaw **必须**先问用户、得到明确同意才执行；**不要**自动跑、也不要把 Step 1 + Step 2 合到一段脚本里强行执行。
- 用户拒绝重启时：**不要**写指纹文件 —— 让下次安装继续提示。
