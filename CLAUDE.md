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

## Task System E2E Testing

All mock components are TypeScript (Node.js). No Rust build needed.

### Component Map

| Component | Start Command | Port | Role |
|---|---|---|---|
| ws-mock server | `cd tools/ws-mock-ts && node dist/server.js` | ws://9000 | XMTP simulator, WS message router |
| mock-api | `cd tools/ws-mock-ts && node dist/mock-api.js` | http://9001 | Task REST backend + dashboard, sends WS system notifications |
| mock-seller | `cd tools/mock-seller && npm start` | — | Headless provider: auto-negotiates price from task budget, auto-applies, auto-delivers |
| mock-seller-ui | `cd tools/mock-seller && npm run ui` | http://9002 | Provider with web UI (manual control) — cannot run alongside headless |
| mock-buyer | `cd tools/mock-buyer && npm start` | — | Headless buyer: waits for TASK_CONFIRMED, auto-negotiates, auto-accepts, auto-completes |
| mock-buyer-ui | `cd tools/mock-buyer && npm run ui` | http://9003 | Buyer with web UI — cannot run alongside headless |
| mock-arbitrator | `cd tools/mock-arbitrator && npm start` | — | Headless evaluator: receives TASK_DISPUTED, resolves buyer-wins after 5s |
| mock-arbitrator-ui | `cd tools/mock-arbitrator && npm run ui` | http://9004 | Evaluator with web UI (manual vote) — cannot run alongside headless |
| openclaw gateway | `launchctl kickstart -k gui/$(id -u)/ai.openclaw.gateway` | http://18789 | AI buyer agent, loads ws-channel plugin |
| ws-channel plugin | `~/openclaw-plugins/ws-channel/src/index.ts` | — | Routes WS messages to openclaw agent sessions |

> **Headless vs UI**: Each mock registers the same identity address. Running both at once causes the server to route all messages to whichever connected last. Use one or the other.

### Key Paths

```
Source plugin:    plugins/ws-channel/src/*.ts          ← edit here
Deployed plugin:  ~/openclaw-plugins/ws-channel/src/   ← gateway loads this
Gateway log:      ~/.openclaw/logs/gateway.log
Sessions dir:     ~/.openclaw/agents/main/sessions/
WS server src:    tools/ws-mock-ts/src/server.ts
mock-api src:     tools/ws-mock-ts/src/mock-api.ts
CLI binary:       ~/.local/bin/onchainos
```

### First-time Setup (build all TS packages)

```bash
cd tools/ws-mock-ts  && npm install && npm run build
cd tools/mock-seller && npm install && npm run build
cd tools/mock-buyer  && npm install && npm run build
cd tools/mock-arbitrator && npm install && npm run build
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
node -e "
const fs = require('fs'), path = require('path');
const src = '/Users/gan/meili/mingtao.gan_dacs_at_okg.com/121/Documents/RustProjects/OKOnchainOS/skills/okx-agent-task/';
const dst = process.env.HOME + '/.agents/skills/okx-agent-task/';
['SKILL.md','client.md','provider.md','evaluator.md'].forEach(f => {
  fs.writeFileSync(dst+f, fs.readFileSync(src+f));
  console.log('synced', f);
});
"
```

> **Why not `npx skills add`**: creates symlinks that openclaw rejects with `symlink-escape`. Use direct file writes instead.
> **Gateway restart required after skill sync**: gateway loads skill files into memory at startup. After syncing skills, run `npm run reset:gw` (in `tools/ws-mock-ts`) to restart gateway and clear sessions.

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

### Full E2E Test: mock-only (no openclaw)

Tests the complete buyer↔seller↔arbitrator flow without the AI agent.

```bash
# 1. Start infrastructure
cd tools/ws-mock-ts
node dist/server.js   > /tmp/ws-server.log  2>&1 &
node dist/mock-api.js > /tmp/ws-api.log     2>&1 &

# 2. Start headless mocks
cd tools/mock-seller     && node dist/tools/mock-seller/src/mock-seller.js         > /tmp/mock-seller.log 2>&1 &
cd tools/mock-buyer      && node dist/tools/mock-buyer/src/mock-buyer.js           > /tmp/mock-buyer.log  2>&1 &
cd tools/mock-arbitrator && node dist/tools/mock-arbitrator/src/mock-arbitrator.js > /tmp/mock-arb.log    2>&1 &

# 3. Verify registrations
sleep 2
grep "身份已注册" /tmp/mock-seller.log /tmp/mock-buyer.log /tmp/mock-arb.log

# 4. Reset DB and create task
curl -s -X DELETE http://127.0.0.1:9001/api/v1/reset
curl -s -X POST http://127.0.0.1:9001/api/v1/task/create \
  -H "Content-Type: application/json" \
  -d '{"title":"测试任务","description":"...","descriptionSummary":"...","tokenAddress":"0xUSDT","tokenAmount":"50","paymentType":0,"openType":1,"chainId":1,"minCreditScore":0,"buyerAgentId":"mock-buyer-agent-001","buyerAgentAddress":"0xBuyer000000000000000000000000000000001","expireConfig":{"openExpireSec":86400,"acceptedExpireSec":86400}}'

# 5. Watch: TASK_CONFIRMED fires after 8s, then auto-negotiation begins
tail -f /tmp/mock-buyer.log
```

**Happy path timeline** (~30s total):
```
+0s   task created
+8s   TASK_CONFIRMED → mock-buyer starts negotiation
+10s  TASK_INQUIRE → seller asks for details
+13s  buyer sends task details → seller quotes budget price (escrow)
+16s  buyer accepts → seller confirms payment mode
+18s  buyer confirms → seller sends TASK_APPLY + calls apply API
+20s  TASK_APPLIED → TASK_ACCEPTED (chain notifications)
+25s  seller sends TASK_DELIVER + calls submit API → TASK_SUBMITTED
+26s  buyer calls complete API → status = complete ✅
```

### Full E2E Test: real openclaw agent (AI buyer)

Uses the openclaw AI agent as buyer instead of mock-buyer.

```bash
# 1. Start infrastructure (same as above, but NO mock-buyer)
cd tools/ws-mock-ts
node dist/server.js   > /tmp/ws-server.log 2>&1 &
node dist/mock-api.js > /tmp/ws-api.log    2>&1 &
cd tools/mock-seller && node dist/tools/mock-seller/src/mock-seller.js > /tmp/mock-seller.log 2>&1 &
sleep 2 && grep "身份已注册" /tmp/mock-seller.log

# 2. Reset DB
curl -s -X DELETE http://127.0.0.1:9001/api/v1/reset

# 3. Clear sessions and restart gateway
node -e "const fs=require('fs'),p=require('path'),d=process.env.HOME+'/.openclaw/agents/main/sessions';fs.readdirSync(d).forEach(f=>{try{fs.unlinkSync(p.join(d,f))}catch(e){}});console.log('cleared');"
launchctl kickstart -k gui/$(id -u)/ai.openclaw.gateway
until grep -q "ws-channel.*已注册" ~/.openclaw/logs/gateway.log 2>/dev/null; do sleep 1; done
echo "gateway ready"

# 4. Send task creation message (natural language)
openclaw agent --agent main -m "帮我发布一个任务：开发一个 Python 脚本，实时监控以太坊主网上金额大于 10 万 USDT 的转账并输出到终端。质量标准：代码有注释，支持以太坊主网，可直接运行。预算 50 USDT，卖家接受期限 48 小时，交付期限 24 小时。"

# 5. Watch gateway log
tail -f ~/.openclaw/logs/gateway.log | grep --line-buffered -E "TASK_|conv:|dispatch|CLI echo"
```

**Expected gateway log sequence**:
```
[ws-channel] TASK_CONFIRMED jobId=0x... → 触发 main session agent turn
[ws-mock] CLI echo: activating conv conv-{jobId}-buyer-123-mock-seller-agent-001 type=TASK_INQUIRE
[ws-channel] conv:conv-{jobId}-... from:0xSeller... type:TASK_REPLY      ← seller asks for details
[ws-channel] dispatch 完成 (replies=1 mode=sub)                           ← agent sends task details
[ws-channel] conv:conv-{jobId}-... from:0xSeller... type:TASK_REPLY      ← seller quotes price
[ws-channel] dispatch 完成 (replies=1 mode=sub)                           ← agent accepts/negotiates
[ws-channel] conv:conv-{jobId}-... from:0xSeller... type:TASK_APPLY
[ws-channel] conv:conv-{jobId}-... from:0xMockChain... type:TASK_APPLIED  mode:main
[ws-channel] conv:conv-{jobId}-... from:0xMockChain... type:TASK_ACCEPTED mode:main
[ws-channel] TASK_ACCEPTED jobId=... → 向 main session 推送接单通知
[ws-channel] conv:conv-{jobId}-... from:0xSeller... type:TASK_DELIVER
[ws-channel] conv:conv-{jobId}-... from:0xMockChain... type:TASK_SUBMITTED
```

**Key invariant**: all messages must be on the same `conv-{jobId}-buyer-123-mock-seller-agent-001`. Two different conv_ids = regression.

### Known Issues / Notes

- Headless + UI versions of the same mock share one identity — run only one at a time
- `sendText: missing conversationId` in gateway log — non-blocking, doesn't affect flow
- mock-api data persists across restarts (saved to `tools/ws-mock-ts/dist/mock-tasks.json`); reset: `curl -X DELETE http://127.0.0.1:9001/api/v1/reset`
- TASK_CONFIRMED fires 8s after `create-task` — intentional delay for agent turn to finish
- mock-seller quotes the task's `tokenAmount` (parsed from buyer's detail message); defaults to 50 USDT if parsing fails
- Gateway re-registers tools on every agent turn — normal openclaw behavior, not a bug
