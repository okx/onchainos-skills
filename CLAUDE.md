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
| mock-evaluator | `cd tools/mock-evaluator && npm start` | — | Headless evaluator: receives TASK_DISPUTED, resolves buyer-wins after 5s |
| mock-evaluator-ui | `cd tools/mock-evaluator && npm run ui` | http://9004 | Evaluator with web UI (manual vote) — cannot run alongside headless |
| openclaw gateway | `launchctl kickstart -k gui/$(id -u)/ai.openclaw.gateway` | http://18789 | AI buyer agent (connects via XMTP channel, not ws-mock) |

> **Headless vs UI**: Each mock registers the same identity address. Running both at once causes the server to route all messages to whichever connected last. Use one or the other.

### Key Paths

```
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
cd tools/mock-evaluator && npm install && npm run build
```

### Permission Rule

`cp`, `rsync`, Write tool all fail with EPERM on `~/.openclaw/`.
**Always use `node -e "require('fs').writeFileSync(...)"` to write to those paths.**

### Sync Skills After Edit

After editing any file under `skills/okx-agent-task/` (e.g. `buyer.md`, `SKILL.md`):

```bash
node -e "
const fs = require('fs'), path = require('path');
const src = '/Users/gan/meili/mingtao.gan_dacs_at_okg.com/121/Documents/RustProjects/OKOnchainOS/skills/okx-agent-task/';
const dst = process.env.HOME + '/.agents/skills/okx-agent-task/';
['SKILL.md','buyer.md','provider.md','evaluator.md'].forEach(f => {
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

Tests the complete buyer↔seller↔evaluator flow without the AI agent.

```bash
# 1. Start infrastructure
cd tools/ws-mock-ts
node dist/server.js   > /tmp/ws-server.log  2>&1 &
node dist/mock-api.js > /tmp/ws-api.log     2>&1 &

# 2. Start headless mocks
cd tools/mock-seller     && node dist/tools/mock-seller/src/mock-seller.js         > /tmp/mock-seller.log 2>&1 &
cd tools/mock-buyer      && node dist/tools/mock-buyer/src/mock-buyer.js           > /tmp/mock-buyer.log  2>&1 &
cd tools/mock-evaluator && node dist/tools/mock-evaluator/src/mock-evaluator.js > /tmp/mock-arb.log    2>&1 &

# 3. Verify registrations
sleep 2
grep "身份已注册" /tmp/mock-seller.log /tmp/mock-buyer.log /tmp/mock-arb.log

# 4. Create task (data persists across restarts, jobId auto-increments)
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

### Known Issues / Notes

- Headless + UI versions of the same mock share one identity — run only one at a time
- `sendText: missing conversationId` in gateway log — non-blocking, doesn't affect flow
- mock-api data persists across restarts (saved to `tools/ws-mock-ts/dist/mock-tasks.json`), jobId auto-increments from max existing; optional full reset: `curl -X DELETE http://127.0.0.1:9001/api/v1/reset`
- TASK_CONFIRMED fires 8s after `create-task` — intentional delay for agent turn to finish
- mock-seller quotes the task's `tokenAmount` (parsed from buyer's detail message); defaults to 50 USDT if parsing fails
- Gateway re-registers tools on every agent turn — normal openclaw behavior, not a bug

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
