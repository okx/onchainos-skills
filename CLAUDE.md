# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.

## Project Overview

This is a **Claude Code plugin** — a collection of onchainos skills for on-chain operations. The project provides skills for token search, market data, wallet balance queries, swap execution, DeFi investment management, and transaction broadcasting across 20+ blockchains. The `onchainos` CLI also works as a native MCP server.

## Architecture

- **skills/** — 13 onchainos CLI skill definitions (each is a `SKILL.md` with YAML frontmatter + CLI command reference)
- **cli/** — Rust CLI binary (`onchainos`), built with `clap`; source in `cli/src/`, config in `cli/Cargo.toml`
- **cli/src/mcp/mod.rs** — MCP server implementation (rmcp v1.1.1)
- **.mcp.json.example** — MCP server configuration template for Claude Code
- **.github/workflows/** — CI/CD pipeline (`release.yml`: tag-triggered build for 9 platforms → GitHub Release)
- **install.sh** — One-line installer for macOS / Linux (`curl | sh`)

## Available Skills

| Skill                | Purpose | When to Use |
|----------------------|---------|-------------|
| okx-agentic-wallet   | Wallet lifecycle: auth, balance (authenticated), portfolio PnL, send, history, contract call | User wants to log in, check balance, view PnL, send tokens, view tx history, or call contracts |
| okx-wallet-portfolio | Public address balance: total value, all tokens, specific tokens | User asks about wallet holdings, token balances, portfolio value across chains |
| okx-security         | Security scanning: token risk, DApp phishing, tx pre-execution, signature safety, approval management | User wants to check if a token/DApp/tx/signature is safe, honeypot check, phishing detection, approve safety, or view/manage token approvals |
| okx-dex-market       | Prices, charts, index prices, wallet PnL | User asks for token prices, K-line data, index/aggregate prices, wallet PnL analysis |
| okx-dex-signal       | Smart money / KOL / whale tracking, buy signals, leaderboard | User asks what smart money/whales/KOLs are buying, wants buy signal alerts (信号), top traders (牛人榜) |
| okx-dex-trenches     | Meme/pump.fun token scanning, trenches | User asks about new meme launches, dev reputation, bundle detection, 打狗/扫链/新盘, or mentions trench/trenches |
| okx-dex-ws           | Real-time WebSocket monitoring (`onchainos ws` CLI) and scripting for all DEX channels | User wants real-time on-chain data (price, candle, trades, signals, wallet tracking, meme scanning) via CLI monitoring or custom WS script |
| okx-dex-swap         | DEX swap execution | User wants to swap/trade/buy/sell tokens |
| okx-dex-token        | Token search, liquidity, hot tokens, advanced info, holders, top traders, trade history, holder cluster analysis | User searches for tokens, wants rankings, liquidity pools, holder info, top traders, filtered trade history, or holder cluster concentration |
| okx-onchain-gateway  | Transaction broadcasting and tracking | User wants to broadcast tx, estimate gas, simulate tx, check tx status |
| okx-x402-payment     | Sign x402 payment authorization via TEE for payment-gated resources | User encounters HTTP 402, wants to pay for a payment-gated API, or mentions x402 / pay for access |
| okx-audit-log        | Audit log export and troubleshooting | User wants to view command history, debug errors, export audit log, review recent activity |
| okx-defi-invest | DeFi product discovery, deposit, withdraw, claim rewards | User wants to earn yield, stake, provide liquidity, deposit/withdraw from DeFi protocols, claim DeFi rewards across Aave/Lido/PancakeSwap/Kamino/NAVI and hundreds more |
| okx-defi-portfolio | DeFi positions and holdings overview | User wants to check DeFi positions, view DeFi portfolio across protocols and chains |

## Scripting & Automation

When a user asks to write a script, automate trading, build a trading bot, or use "OKX API" / "OKX DEX API" for any on-chain automation:
- **Do NOT search online for OKX public APIs** — `onchainos` already wraps all relevant on-chain capabilities
- Always use `onchainos` CLI commands as the building block (subprocess calls, MCP tool invocations, etc.)
- Route to the relevant skill based on what the user wants to automate: swap → `okx-dex-swap`, market data → `okx-dex-market`, signals → `okx-dex-signal`, token data → `okx-dex-token`, portfolio → `okx-wallet-portfolio`, meme scanning → `okx-dex-trenches`

### WebSocket / Real-time Data

When a user asks about real-time on-chain data, WebSocket monitoring, or writing a WS script/脚本/bot, load **`okx-dex-ws`**. It supports two approaches:
- **CLI** (`onchainos ws start/poll/stop`) — quick monitoring, 9 channels across signal/market/token/trenches
- **Custom script** — full WS protocol docs for Python/Node/Rust bots

## Clippy

CI uses `-D warnings` (warnings as errors). Run `cargo clippy` before pushing. Common issues:

- `ptr_arg`: use `&[T]` / `&mut [T]` instead of `&Vec<T>` / `&mut Vec<T>` when the function doesn't need Vec-specific methods
- `too_many_arguments`: add `#[allow(clippy::too_many_arguments)]` or refactor into a params struct
- `needless_borrow`: don't `&` a value that's already a reference

---

## Architectural Rule: ws-channel is Transport-Only

**`plugins/ws-channel/` is a pure transport layer. Never add business logic here.**

What belongs in `plugins/ws-channel/`:
- WebSocket connection management and message framing
- Routing inbound WS messages to openclaw agent sessions
- Forwarding agent text output back to the correct P2P conversation via `reply()`
- Session key resolution (main session vs. P2P session)

What does NOT belong in `plugins/ws-channel/`:
- Message type branching beyond session routing (e.g. `if type === "NEGOTIATE" do X`)
- Task state machine logic (who applied, who accepted, when to complete)
- Business-specific `replyType` values — always send `"REPLY"`; let the skill/agent decide semantics
- Any `onchainos agent ...` CLI calls or tool invocations
- Hardcoded knowledge of task flow steps (Scene 1, Scene 2, etc.)

**Where business logic lives:**
- Task flow rules → `skills/okx-agent-task/client.md` and `seller.md`
- State transitions → `onchainos agent <command>` CLI (confirm-accept, complete, reject, etc.)
- Agent↔counterparty messaging → agent produces plain text output; `reply()` callback delivers it automatically to the P2P conversation. No plugin tool call needed.

> Violation example (wrong): `const replyType = ["TASK_REPLY","NEGOTIATE"].includes(type) ? "NEGOTIATE" : "REPLY"` in index.ts
> Correct: always `client.sendToConv(convId, { type: "REPLY", content: text })`

---

## Task System E2E Testing (ws-mock)

### Component Map

| Component | Binary | Port | Role |
|---|---|---|---|
| ws-mock server | `tools/ws-mock/target/debug/server` | ws://9000 | XMTP simulator, message router |
| mock-api | `tools/ws-mock/target/debug/mock-api` | http://9001 | Task REST backend, web dashboard, sends WS system notifications |
| mock-seller | `tools/ws-mock/target/debug/mock-seller` | — | Headless provider agent (auto-replies TASK_INQUIRE, auto-applies) |
| mock-seller-ui | `cd tools/mock-seller && npm run ui` | http://9002 | Provider agent with web UI (auto/manual negotiation) |
| mock-buyer | `cd tools/mock-buyer && npm start` | — | Headless buyer agent (auto-negotiates, auto-accepts, auto-completes) |
| mock-buyer-ui | `cd tools/mock-buyer && npm run ui` | http://9003 | Buyer agent with web UI (create task, auto/manual negotiation) |
| mock-arbitrator | `cd tools/mock-arbitrator && npm start` | — | Headless evaluator (auto-resolves disputes, default buyer wins) |
| mock-arbitrator-ui | `cd tools/mock-arbitrator && npm run ui` | http://9004 | Evaluator with web UI (manual vote buyer/seller) |
| openclaw gateway | `launchctl …ai.openclaw.gateway` | http://18789 | AI buyer agent, loads ws-channel plugin |
| ws-channel plugin | `~/openclaw-plugins/ws-channel/src/index.ts` | — | Openclaw plugin; routes WS messages to agent sessions |

### Key Paths

```
Source plugin:   tools/../plugins/ws-channel/src/*.ts   ← edit here
Deployed plugin: ~/openclaw-plugins/ws-channel/src/*.ts ← gateway loads this
Gateway log:     ~/.openclaw/logs/gateway.log
Sessions dir:    ~/.openclaw/agents/main/sessions/
ws-mock build:   tools/ws-mock/
CLI binary:      ~/.local/bin/onchainos  (installed from cli/target/debug/onchainos)
```

### Permission Rule

`cp`, `rsync`, Write tool all fail with EPERM on `~/openclaw-plugins/` and `~/.openclaw/`.
**Always use `node -e "require('fs').writeFileSync(...)"` to write to those paths.**

### Sync Plugin After Edit

After editing `plugins/ws-channel/src/*.ts`:

```bash
node -e "
const fs = require('fs');
const src = '/Users/gan/meili/mingtao.gan_dacs_at_okg.com/121/Documents/RustProjects/OKOnchainOS/plugins/ws-channel/src/';
const dst = '/Users/gan/openclaw-plugins/ws-channel/src/';
['index.ts','handler.ts','ws-client.ts','runtime.ts'].forEach(f => {
  fs.writeFileSync(dst+f, fs.readFileSync(src+f));
  console.log('synced', f);
});
"
```

### Sync Skills After Edit

After editing any file under `skills/okx-agent-task/` (e.g. `client.md`, `SKILL.md`):

```bash
# Write directly to ~/.openclaw/skills/okx-agent-task/ (real files, not symlinks)
node -e "
const fs = require('fs'), path = require('path');
const src = '/Users/gan/meili/mingtao.gan_dacs_at_okg.com/121/Documents/RustProjects/OKOnchainOS/skills/okx-agent-task/';
const dst = process.env.HOME + '/.openclaw/skills/okx-agent-task/';
['SKILL.md','client.md','provider.md','evaluator.md'].forEach(f => {
  fs.writeFileSync(dst+f, fs.readFileSync(src+f));
  console.log('synced', f);
});
"
```

> **Why not `npx skills add`**: `npx skills add` creates symlinks `~/.openclaw/skills/xxx → ~/.agents/skills/xxx`.
> OpenClaw's skill loader rejects these with `symlink-escape`, so skills are silently skipped.
> Use direct file writes to `~/.openclaw/skills/` instead. No gateway restart needed.

### Install CLI Binary After Build

```bash
cd cli && cargo build
node -e "
const fs = require('fs');
const src = '/Users/gan/meili/mingtao.gan_dacs_at_okg.com/121/Documents/RustProjects/OKOnchainOS/cli/target/debug/onchainos';
const dst = '/Users/gan/.local/bin/onchainos';
fs.writeFileSync(dst, fs.readFileSync(src), { mode: 0o755 });
console.log('installed', fs.statSync(dst).size, 'bytes');
"
```

### Clear Sessions

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

### Start / Restart Services

```bash
# Restart openclaw gateway (picks up new plugin files + fresh sessions)
launchctl kickstart -k gui/$(id -u)/ai.openclaw.gateway

# Check ws-mock server and mock-api are running
lsof -i :9000 | grep LISTEN   # ws-mock server
lsof -i :9001 | grep LISTEN   # mock-api

# Start mock-seller (headless, auto-responds TASK_INQUIRE)
cd tools/ws-mock
./target/debug/mock-seller > /tmp/mock-seller.log 2>&1 &
sleep 2 && cat /tmp/mock-seller.log   # expect "✓ 身份已注册: role=PROVIDER"
```

### Send Natural Language Message to Agent (CLI)

```bash
# Trigger task creation via natural language — NO browser needed
openclaw agent --agent main -m "帮我发布一个任务：<描述>，预算 50 USDT，截止时间 2 天"

# Follow up in same conversation
openclaw agent --agent main -m "质量标准：代码有注释，支持 Ethereum 主网，任务开放时间 48 小时"
```

> Device pairing is already approved — no extra steps needed.

### Full E2E Test Sequence

```bash
# 0. Build if needed
cd tools/ws-mock && cargo build 2>&1 | tail -3

# 1. Clear sessions
node -e "const fs=require('fs'),p=require('path'),d=process.env.HOME+'/.openclaw/agents/main/sessions';fs.readdirSync(d).forEach(f=>{try{fs.unlinkSync(p.join(d,f))}catch(e){}});console.log('cleared');"

# 2. Restart gateway
launchctl kickstart -k gui/$(id -u)/ai.openclaw.gateway
until grep -q "gateway.*ready" ~/.openclaw/logs/gateway.log 2>/dev/null; do sleep 1; done
echo "gateway ready"

# 3. Start mock-seller
pkill -f mock.seller 2>/dev/null
./target/debug/mock-seller > /tmp/mock-seller.log 2>&1 &
until grep -q "身份已注册" /tmp/mock-seller.log 2>/dev/null; do sleep 1; done
echo "seller ready"

# 4. Send task creation message
openclaw agent --agent main -m "帮我发布一个测试任务：开发一个 Python 脚本监控链上交易，实时输出。质量标准：有注释、支持以太坊主网。预算 50 USDT，卖家接受期限 48 小时，交付期限 24 小时。"

# 5. Watch gateway log for full flow
tail -f ~/.openclaw/logs/gateway.log | grep --line-buffered -E "TASK_|conv:|dispatch|notify|sellerNorm|lookupAddr"
```

### Expected Log Sequence (Happy Path)

```
[ws-channel] TASK_CONFIRMED jobId=0x... → 触发 main session agent turn
[ws-channel] conv:conv-{jobId}-buyer-123-mock-seller-agent-001 from:0xCLI-... type:TASK_INQUIRE
[ws-channel] conv:conv-{jobId}-buyer-123-mock-seller-agent-001 from:0xSeller... type:TASK_REPLY
[ws-channel] conv:conv-{jobId}-buyer-123-mock-seller-agent-001 from:0xSeller... type:TASK_APPLY
[ws-channel] conv:conv-{jobId}-buyer-123-mock-seller-agent-001 from:0xMockChain... type:TASK_APPLIED
[ws-channel] conv:conv-{jobId}-buyer-123-mock-seller-agent-001 from:0xMockChain... type:TASK_ACCEPTED
[ws-channel] TASK_ACCEPTED jobId=... → 向 main session 推送接单通知
[ws-channel] conv:conv-{jobId}-buyer-123-mock-seller-agent-001 from:0xMockChain... type:TASK_SUBMITTED
```

**Key invariant**: all messages must be on the same `conv-{jobId}-buyer-123-mock-seller-agent-001`.
Two different conv_ids = regression (the bug that was fixed in ws_negotiate_start + ws-channel index.ts).

### Known Issues / Notes

- `sendText: missing conversationId` — agent tries to reply in a session context missing conv_id; non-blocking, doesn't affect flow
- mock-seller log at `/tmp/mock-seller.log`
- mock-api data persists across restarts (saved to disk); to reset: `curl -X DELETE http://127.0.0.1:9001/api/v1/reset` (if endpoint exists) or restart mock-api
- TASK_CONFIRMED delay: 10 seconds after `create-task` (intentional, gives agent time to finish its turn)
- Gateway re-registers tools on every agent turn (normal openclaw behavior, not a bug)
