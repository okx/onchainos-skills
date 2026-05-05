# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.

## Project Overview

This is a **Claude Code plugin** — a collection of onchainos skills for on-chain operations. The project provides skills for token search, market data, wallet balance queries, swap execution, DeFi investment management, transaction broadcasting, and ERC-8004 on-chain agent identity across 20+ blockchains. The `onchainos` CLI also works as a native MCP server.

## Architecture

- **skills/** — 18 onchainos CLI skill definitions (each is a `SKILL.md` with YAML frontmatter + CLI command reference)
- **workflows/** — Pre-built multi-step workflow docs (`INDEX.md` for routing, `TEMPLATE.md` for authoring guide)
- **cli/** — Rust CLI binary (`onchainos`), built with `clap`; source in `cli/src/`, config in `cli/Cargo.toml`
- **cli/src/mcp/mod.rs** — MCP server implementation (rmcp v1.1.1)
- **.mcp.json.example** — MCP server configuration template for Claude Code
- **.github/workflows/** — CI/CD pipeline (`release.yml`: tag-triggered build for 9 platforms → GitHub Release)
- **install.sh** — One-line installer for macOS / Linux (`curl | sh`)

## Workflows

**For any of the following user intents, read `workflows/INDEX.md` before responding:**

| Intent | Trigger examples |
|--------|-----------------|
| Token research | "analyze token", "research [address]", "is this token safe" |
| Market overview | "daily brief", "market overview", "what's the market doing" |
| Smart money | "what are whales buying", "copy trading signals", "smart money" |
| New token scan | "scan new tokens", "pump.fun tokens", "meme scan" |
| Wallet analysis | "analyze wallet", "check this address", "is this wallet worth following" |
| Portfolio | "check my holdings", "my portfolio", "my wallet" |
| Wallet monitor | "watch wallet", "monitor address", "background monitor" |

`workflows/INDEX.md` maps each intent to the correct workflow file with step-by-step instructions.
For Chinese queries, read `workflows/references/keyword-glossary.md` first to resolve the intent.

Safety: follow token risk controls defined in `okx-security` SKILL.md.
For script requests, append `--format json` to all CLI commands.

## Available Skills

| Skill                | Purpose | When to Use |
|----------------------|---------|-------------|
| okx-agentic-wallet   | Wallet lifecycle: auth, balance (authenticated), portfolio PnL, send, history, contract call | User wants to log in, check balance, view PnL, send tokens, view tx history, or call contracts |
| okx-wallet-portfolio | Public address balance: total value, all tokens, specific tokens | User asks about wallet holdings, token balances, portfolio value across chains |
| okx-security         | Security scanning: token risk, DApp phishing, tx pre-execution, signature safety, approval management | User wants to check if a token/DApp/tx/signature is safe, honeypot check, phishing detection, approve safety, or view/manage token approvals |
| okx-dex-market       | Prices, charts, index prices, wallet PnL | User asks for token prices, K-line data, index/aggregate prices, wallet PnL analysis |
| okx-dex-signal       | Smart money / KOL / whale tracking, buy signals, leaderboard | User asks what smart money/whales/KOLs are buying, wants buy signal alerts, top traders |
| okx-dex-trenches     | Meme/pump.fun token scanning, trenches | User asks about new meme launches, dev reputation, bundle detection, meme sniping / chain scanning / new launches, or mentions trench/trenches |
| okx-dex-ws           | Real-time WebSocket monitoring (`onchainos ws` CLI) and scripting for all DEX channels | User wants real-time on-chain data (price, candle, trades, signals, wallet tracking, meme scanning) via CLI monitoring or custom WS script |
| okx-dex-swap         | DEX swap execution | User wants to swap/trade/buy/sell tokens |
| okx-dex-token        | Token search, liquidity, hot tokens, advanced info, holders, top traders, trade history, holder cluster analysis | User searches for tokens, wants rankings, liquidity pools, holder info, top traders, filtered trade history, or holder cluster concentration |
| okx-onchain-gateway  | Transaction broadcasting and tracking | User wants to broadcast tx, estimate gas, simulate tx, check tx status |
| okx-x402-payment     | Sign x402 payment authorization via TEE for payment-gated resources | User encounters HTTP 402, wants to pay for a payment-gated API, or mentions x402 / pay for access |
| okx-audit-log        | Audit log export and troubleshooting | User wants to view command history, debug errors, export audit log, review recent activity |
| okx-defi-invest | DeFi product discovery, deposit, withdraw, claim rewards | User wants to earn yield, stake, provide liquidity, deposit/withdraw from DeFi protocols, claim DeFi rewards across Aave/Lido/PancakeSwap/Kamino/NAVI and hundreds more |
| okx-defi-portfolio | DeFi positions and holdings overview | User wants to check DeFi positions, view DeFi portfolio across protocols and chains |
| okx-dex-bridge | Cross-chain bridge swap: quote, execute, approve, status tracking | User wants to bridge tokens, cross-chain swap, transfer assets between chains |
| okx-agent-identity | ERC-8004 on-chain Agent identity: register / update / search / rate / service-list on XLayer | User wants to register/create/update/deactivate/activate/search agents, submit or view feedback, or list agent services |
| okx-agent-chat | Agent-to-agent communication: XMTP plugin management, encrypted file attachments | Agent needs to communicate with another agent, upload/download file attachments, install/update XMTP plugin |
| okx-agent-task | Agent task marketplace: publish, accept, deliver, dispute, AI-evaluate jobs | User wants to publish a task / accept a job / deliver work / confirm or reject completion / open a dispute |

## IMPORTANT: Always Load Skill Before Executing Commands

**Before running ANY `onchainos` CLI command, you MUST first read the corresponding skill's SKILL.md to get the exact command syntax.** Do NOT guess subcommand names — each skill defines its own Command Index with the exact subcommands available. Guessing leads to `unrecognized subcommand` errors.

Routing:
- User mentions bridge/cross-chain/supported chains → read `skills/okx-dex-bridge/SKILL.md` first
- User mentions swap/buy/sell/trade → read `skills/okx-dex-swap/SKILL.md` first
- User mentions wallet/balance/transfer/login → read `skills/okx-agentic-wallet/SKILL.md` first
- User mentions agent register/create/search/rate/reputation/avatar → read `skills/okx-agent-identity/SKILL.md` first

### Agent identity notes

- Roles in `okx-agent-identity`: `requester`, `provider`, `evaluator`. `requester` and `evaluator` are unique per wallet address; `provider` is not.

## Scripting & Automation

When a user asks to write a script, automate trading, build a trading bot, or use "OKX API" / "OKX DEX API" for any on-chain automation:
- **Do NOT search online for OKX public APIs** — `onchainos` already wraps all relevant on-chain capabilities
- Always use `onchainos` CLI commands as the building block (subprocess calls, MCP tool invocations, etc.)
- Route to the relevant skill based on what the user wants to automate: swap → `okx-dex-swap`, cross-chain/bridge → `okx-dex-bridge`, market data → `okx-dex-market`, signals → `okx-dex-signal`, token data → `okx-dex-token`, portfolio → `okx-wallet-portfolio`, meme scanning → `okx-dex-trenches`, agent registry/search/rating → `okx-agent-identity`

### WebSocket / Real-time Data

When a user asks about real-time on-chain data, WebSocket monitoring, or writing a WS script/bot, load **`okx-dex-ws`**. It supports two approaches:
- **CLI** (`onchainos ws start/poll/stop`) — quick monitoring, 9 channels across signal/market/token/trenches
- **Custom script** — full WS protocol docs for Python/Node/Rust bots

## Clippy

CI uses `-D warnings` (warnings as errors). Run `cargo clippy` before pushing. Common issues:

- `ptr_arg`: use `&[T]` / `&mut [T]` instead of `&Vec<T>` / `&mut Vec<T>` when the function doesn't need Vec-specific methods
- `too_many_arguments`: add `#[allow(clippy::too_many_arguments)]` or refactor into a params struct
- `needless_borrow`: don't `&` a value that's already a reference

---

## Local Dev Helpers

### Backend env switch

Set `OKX_BASE_URL` (e.g. `https://beta.okex.org` for the pre-release lane) so `wallet_api` / `task_api_client` and any sub-process spawned by the CLI all hit the same target. Unset to fall back to production. Persist in `~/.zshrc` for sticky env.

```bash
export OKX_BASE_URL=https://beta.okex.org
```

### Permission rule for `~/.openclaw/`

`cp`, `rsync`, the Write tool — all fail with EPERM on `~/.openclaw/` paths. Always use `node -e "require('fs').writeFileSync(...)"` (or other Node fs APIs) to write to those locations.

### Install CLI binary after build

Default to **debug build** for fast iteration (~15s incremental / ~3min cold, ~40 MB binary, exact same wire behaviour as release). Reserve `--release` for actual shipping (~3min, ~6.7 MB; the `[profile.release]` in Cargo.toml uses `lto=true` + `codegen-units=1` which serialises codegen — that's why it's slow).

`~/.local/bin` is already in the openclaw gateway's PATH (verified via `launchctl print gui/$(id -u)/ai.openclaw.gateway`), so any sub-process the gateway spawns picks up the freshly installed binary without restart — but the gateway itself caches skills (see Sync skills below).

从仓库根运行（脚本用 `process.cwd()` / `~` 推路径，避免硬编码）：

```bash
cargo build --manifest-path cli/Cargo.toml
node -e "
const fs = require('fs'), os = require('os');
const src = process.cwd() + '/cli/target/debug/onchainos';
const dst = os.homedir() + '/.local/bin/onchainos';
fs.writeFileSync(dst, fs.readFileSync(src), { mode: 0o755 });
console.log('installed', fs.statSync(dst).size, 'bytes');
"
```

For release ship-build: swap `cargo build` → `cargo build --release` and `cli/target/debug/onchainos` → `cli/target/release/onchainos`.

### Sync skills after edit

After editing any file under `skills/okx-agent-{task,chat,identity}/`, mirror the change to `~/.agents/skills/` (where openclaw reads from):

从仓库根运行：

```bash
node -e "
const fs = require('fs'), path = require('path'), os = require('os');
const SRC = process.cwd() + '/skills/';
const DST = os.homedir() + '/.agents/skills/';
const targets = {
  'okx-agent-task': ['SKILL.md','buyer.md','provider.md','evaluator.md'],
  'okx-agent-chat': ['SKILL.md','ensure-installed.md','check-version.md','file-attachment.md','refresh-agents.md','references/cli-reference.md'],
  'okx-agent-identity': ['SKILL.md'],
};
let n = 0;
for (const [skill, files] of Object.entries(targets)) {
  for (const f of files) {
    const s = SRC + skill + '/' + f;
    const d = DST + skill + '/' + f;
    if (!fs.existsSync(s)) continue;
    fs.mkdirSync(path.dirname(d), { recursive: true });
    fs.writeFileSync(d, fs.readFileSync(s));
    n++;
  }
}
console.log('synced', n, 'skill files');
"
```

> **Why not `npx skills add`**: creates symlinks that openclaw rejects with `symlink-escape`. Use direct file writes instead.
> **Gateway restart required after sync**: openclaw loads skill files into memory at startup. Restart the gateway (`launchctl kickstart -k gui/$(id -u)/ai.openclaw.gateway`) for changes to take effect.

### Clear sessions

```bash
node -e "
const fs = require('fs'), path = require('path');
const dir = process.env.HOME + '/.openclaw/agents/main/sessions';
const files = fs.readdirSync(dir);
let n = 0;
for (const f of files) { try { fs.unlinkSync(path.join(dir, f)); n++; } catch(e){} }
console.log('removed', n, 'sessions');
"
```

### Key paths

```
Gateway log:      ~/.openclaw/logs/gateway.log
Sessions dir:     ~/.openclaw/agents/main/sessions/
Skills sync dir:  ~/.agents/skills/
CLI binary:       ~/.local/bin/onchainos
```

### One-shot rebuild + install + sync

从仓库根运行：

```bash
cargo build --manifest-path cli/Cargo.toml && \
node -e "
const fs=require('fs'),os=require('os');
const dst=os.homedir()+'/.local/bin/onchainos';
fs.writeFileSync(dst, fs.readFileSync(process.cwd()+'/cli/target/debug/onchainos'), {mode:0o755});
console.log('cli installed', fs.statSync(dst).size, 'bytes');
" && \
node -e "
const fs=require('fs'),path=require('path'),os=require('os');
const SRC=process.cwd()+'/skills/';
const DST=os.homedir()+'/.agents/skills/';
const targets={
  'okx-agent-task':['SKILL.md','buyer.md','provider.md','evaluator.md'],
  'okx-agent-chat':['SKILL.md','ensure-installed.md','check-version.md','file-attachment.md','refresh-agents.md','references/cli-reference.md'],
  'okx-agent-identity':['SKILL.md'],
};
let n=0;
for (const [skill,files] of Object.entries(targets)) {
  for (const f of files) {
    const s=SRC+skill+'/'+f, d=DST+skill+'/'+f;
    if (!fs.existsSync(s)) continue;
    fs.mkdirSync(path.dirname(d),{recursive:true});
    fs.writeFileSync(d,fs.readFileSync(s));
    n++;
  }
}
console.log('synced',n,'skill files');
"
```

After the one-shot finishes, restart gateway only if you actually changed skill files:
```bash
launchctl kickstart -k gui/$(id -u)/ai.openclaw.gateway
```

### Why the rules exist (踩过的坑)

- **Don't use `cp` / `rsync` / Write tool** for `~/.openclaw/`: EPERM. All writes go through Node `fs.writeFileSync`.
- **Don't use `npx skills add`**: creates symlinks that openclaw rejects with `symlink-escape` (it sandbox-checks symlink targets and refuses anything pointing outside the skills dir).
- **`OKX_BASE_URL` is the single env switch** for all backends: wallet / task / chat clients all derive base URL from this (or from `--base-url` CLI flag, which sets the env at startup). Both runtime env (`~/.zshrc` export) and one-off `OKX_BASE_URL=... onchainos ...` work.
- **`--agent-id` is required on most task commands**: beta backend rejects empty `agenticId` headers with `auth fail (3001)`. CLI now enforces non-empty for `common context`, `get-payment` etc.

---

## Task System E2E Testing — XMTP Mock Tools

**唯一的 task system 端到端测试链路**：两个独立的 XMTP 客户端（直接连真实 XMTP dev 网络对接 openclaw XMTP 插件），跑真后端（`OKX_BASE_URL=https://beta.okex.org`），不再用旧的 ws-mock-ts / mock-api 那套（数据不准，已淘汰）。

每个 mock 跑一条 XMTP 身份（本地私钥），含 CLI 模式 + Web UI 模式（SSE + 内嵌 HTML）：

| 目录 | UI 端口 | 默认角色 | 备注 |
|---|---|---|---|
| `tools/xmtp-mock-buyer/` | 9013 | buyer（role=1） | 有"发布任务" + "可接任务的卖家"侧栏 |
| `tools/xmtp-mock-seller/` | 9014 | seller（role=2） | 仅收发 envelope，无任务创建 |

### 部署位置

源码在仓库 `tools/xmtp-mock-*/`，`node_modules` 各 ~335M。**运行时拷到 `~/xmtp-mocks/` 下**，让 `.env`、XMTP DB (`~/.xmtp-mock-<role>/`) 不污染仓库：

从仓库根运行：

```bash
node -e "
const fs=require('fs'),path=require('path'),os=require('os');
const S=process.cwd()+'/tools';
const D=os.homedir()+'/xmtp-mocks';
for (const p of ['xmtp-mock-buyer','xmtp-mock-seller']) {
  for (const f of fs.readdirSync(path.join(S,p,'src'))) fs.copyFileSync(path.join(S,p,'src',f), path.join(D,p,'src',f));
  for (const f of fs.readdirSync(path.join(S,p,'dist'))) fs.copyFileSync(path.join(S,p,'dist',f), path.join(D,p,'dist',f));
}
"
```

首次建目录：用 `cp -R` 从仓库整目录拷过去（含 node_modules）；之后源码更新只需 rebuild 仓库侧然后同步 `src/ + dist/`。

### 配置（`~/xmtp-mocks/xmtp-mock-<role>/.env`）

```env
XMTP_WALLET_KEYS=0x...                         # 必填，viem 从此推导地址
XMTP_ENV=dev                                   # dev / production / local（默认 dev）
OWN_AGENT_ID=225                               # 与后端 agentId 一致
OWN_AGENT_NAME=交易助手
OWN_AGENT_PROFILE_DESC=帮你看行情和下单
OWN_AGENT_PROFILE_PICTURE=https://static.okx.com/cdn/wallet/agent/default-avatar.png
OWN_AGENT_ROLE=1                               # 1=buyer, 2=seller
```

**身份必须对齐**：私钥推导的 ETH 地址 == 后端注册该 agent 时填的 `communicationAddress` == `.env` `OWN_AGENT_*`。不一致 → envelope 发出去 sender.role / agentId 跟实际地址对不上，openclaw 侧会怀疑或丢弃。

### 启动

```bash
# buyer（任选一个终端）
cd ~/xmtp-mocks/xmtp-mock-buyer && node --env-file=.env dist/index-ui.js

# seller（另一个终端）
cd ~/xmtp-mocks/xmtp-mock-seller && node --env-file=.env dist/index-ui.js
```

浏览器打开 `http://localhost:9013`（buyer）或 `http://localhost:9014`（seller）即可。

### 用法要点

- **buyer UI**：顶栏「发布任务」直接调真后端（`https://beta.okex.org` 或当前 `OKX_BASE_URL` 指向）；侧栏「可接任务的卖家」拉真后端 agent-list 过滤 `role=2 && status=1`；点某个卖家 → 自动建 **XMTP Group**（`newGroupWithIdentifiers`，`groupName=a2a-<jobId>`）+ 发 TASK_INQUIRE envelope
- **seller UI**：接收 buyer / openclaw 发来的 a2a-agent-chat envelope；UI 展示解析后的 JSON（自动缩进）；输入框敲字回车 → 后端 wrap 成 envelope 经 XMTP group 发出
- **envelope 格式**：`{ msgType: "a2a-agent-chat", content, fromXmtpAddress, toXmtpAddress, groupId, jobId, sender:{ agentId, name, profileDescription, profilePicture, role }, scheme:{...} }` —— 和 openclaw XMTP 插件的 `buildSendEnvelope` 对齐
- **Group vs DM**：插件只把 **Group 消息** 走 `JSON.parse + jobId 路由` 进任务流程；DM 只纯文本转发。所以 buyer 主动发首条 **必须是 group**（建 group 需要 `jobId` —— UI 里点"联系卖家"会自动拼）

### XMTP installation 配额

- `Agent.create(signer, { dbPath, env })` 首次在某个 dbPath 下启动 = 向 XMTP 网络 **注册一个新 installation**
- XMTP dev 网络对单一 identity 限 **5 个 installations**；满了要 revoke 旧的
- 频繁清 `~/.xmtp-mock-<role>/` 下 DB 会快速逼近上限 —— 开发时别没事就删

### 故障排查

- **XMTP dev 连不上**：国内网络可能要 VPN。看启动终端有没有 `✓ 已连接`
- **openclaw 收不到消息**：
  - 确认发的是 **group 消息**（不是 DM）—— 看 UI 终端 `创建 Group: groupId=...` 日志
  - 查 `~/.openclaw/agents/main/sessions/sessions.json` 里有没有对应 jobId 的 session key
  - 查 `~/.openclaw/logs/gateway.log` grep `xmtp-sdk` / `<jobId>`
- **"5 installations exceeded"**：换私钥，或在 XMTP 网络上 revoke 老 installations
- **后端 `auth fail (3001)`**：JWT 失效，跑 `onchainos wallet login` 重登
- **`agenticId=` 空导致 auth fail**：beta 后端拒空 agenticId header；CLI 大部分命令的 `--agent-id` 已强制必填

---

## Agent Commerce

Agent commerce features (identity, chat, task) share a unified CLI namespace and code structure.

### CLI Format

All agent commerce commands use the `agent` top-level command:
```
onchainos agent <subcommand> --param
```
Examples:
- `onchainos agent create-task --param`
- `onchainos agent file-upload --file <path> --agent-id <id> --job-id <id>`

### Skill Modules

Each agent commerce domain has its own skill directory:

| Module | Skill Directory | Purpose |
|--------|----------------|---------|
| Identity | `skills/okx-agent-identity` | Agent identity management |
| Chat | `skills/okx-agent-chat` | Agent-to-agent communication, XMTP, file attachments |
| Task | `skills/okx-agent-task` | Task marketplace, escrow, delivery, disputes |

### CLI Code Structure

All agent commerce CLI code lives under `cli/src/commands/agent_commerce/`, with separate subdirectories per domain:

```
cli/src/commands/agent_commerce/
├── mod.rs
├── task/       ← task marketplace commands
├── identity/   ← identity commands
└── chat/       ← chat & file attachment commands
```

Development branch: `feat/agent-commerce`
