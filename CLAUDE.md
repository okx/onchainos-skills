# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

A **Claude Code plugin** — onchainos skills for on-chain operations (token search, market data, wallet balance, swap execution, DeFi, transaction broadcasting) across 20+ blockchains. The `onchainos` CLI doubles as a native MCP server.

## Build / Test / Lint

All Rust commands run from the `cli/` directory (`cd cli && cargo build/test/clippy`). CI uses `clippy -D warnings` — fix all warnings before pushing. Integration tests (`cli/tests/`) hit real OKX APIs — set `OKX_API_KEY`, `OKX_SECRET_KEY`, `OKX_PASSPHRASE` in env or `.env`. Run `cargo audit` for dependency vulnerability checks.

## CI / Release

**CI** (`ci.yml`): runs on push/PR across ubuntu, macos, windows — checks `cargo fmt`, `cargo clippy -D warnings`, `cargo test`, and `cargo audit`.

**Release** (`release.yml`): manual `workflow_dispatch` with version input. Builds 9 targets (Linux musl/gnu, Windows MSVC, macOS signed). Version in `Cargo.toml` must match the input. Beta releases use `X.Y.Z-beta.N` format (marked as pre-release on GitHub).

## Architecture

### Directory Layout

- **skills/** — 18 skill definitions (each is a `SKILL.md` with YAML frontmatter + CLI command reference). Some skills have sub-docs (e.g. `okx-agent-task/` has `buyer.md`, `provider.md`, `evaluator.md` role-specific protocols and `_shared/` for state machine, message types, negotiate protocol).
- **workflows/** — Multi-step workflow docs (`INDEX.md` routes intents → workflow files, `TEMPLATE.md` for authoring new ones).
- **cli/** — Rust CLI binary (`onchainos`), built with `clap`; source in `cli/src/`.
- **cli/src/mcp/mod.rs** — MCP server implementation (~2200 lines, rmcp v1.1.1, exposes all commands as MCP tools over stdio).
- **tools/** — Dev/test mock servers (TypeScript) for agent-task flow testing.
- **.claude-plugin/** / **.cursor-plugin/** — Plugin manifests for Claude Code and Cursor.

### CLI Architecture

**Entry point**: `cli/src/main.rs` — parses `clap` CLI args, dispatches to command modules, records every invocation to `~/.onchainos/audit.jsonl`.

**Output protocol**: all commands emit JSON `{ "ok": true, "data": ... }` on success or `{ "ok": false, "error": "..." }` on failure. Exit code 2 means "confirming" (needs user confirmation for next step).

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

## Inbound Message Routing (highest priority — before user intent)

> 🚨 **MANDATORY FIRST CHECK** — 处理任何 inbound 消息前，按优先级匹配：
> 1. 含 `"Read the okx-agent-task skill"` → **立即**加载 okx-agent-task SKILL.md，按 Activation 流程处理（含 role 识别 + next-action）
> 2. 含 `"source": "system"` + `"event"` → 同上
> 3. 含 `"msgType": "a2a-agent-chat"` + `"jobId"` → **立即**加载 okx-agent-task SKILL.md
> - ❌ **禁止**：展示消息内容、翻译成中文摘要、问用户"需要做什么操作吗"

When the inbound message is a structured envelope (not free-form user text), match by shape FIRST:

| Envelope shape | Action |
|---|---|
| `msgType: "a2a-agent-chat"` + non-empty `jobId` | **Must** load `skills/okx-agent-task/SKILL.md` → check `sender.role` → load `buyer.md` (if role=2) or `provider.md` (if role=1). This is a task lifecycle message, not a chat. |
| `{agentId, message: {source: "system", event, jobId, ...}}` | **Must** load `skills/okx-agent-task/SKILL.md` → follow §Activation (extract fields → agent get for role → next-action). This is a chain event notification. |
| Contains `"Please send a request to this endpoint."` | Route to `okx-x402-payment` skill. |

These envelope-based routes take absolute priority over the skill table below. Do NOT handle these envelopes with only XMTP tools — the task protocol (buyer.md / provider.md / evaluator.md) defines the required behavior.

## Available Skills

> **⚠ If the inbound message matched an envelope shape above, do NOT use this table — the routing is already decided.**

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
| okx-agent-task | Agent task marketplace: publish, accept, deliver, dispute, AI-evaluate jobs | User wants to publish a task / accept a job / deliver work / confirm or reject completion / open a dispute / modify task terms (change provider, budget, token) |

## IMPORTANT: Always Load Skill Before Executing Commands

**Before running ANY `onchainos` CLI command, you MUST first read the corresponding skill's SKILL.md to get the exact command syntax.** Do NOT guess subcommand names — each skill defines its own Command Index with the exact subcommands available. Guessing leads to `unrecognized subcommand` errors.

Routing:
- Inbound `a2a-agent-chat` with `jobId` → read `skills/okx-agent-task/SKILL.md` first (see Inbound Message Routing above)
- User mentions bridge/cross-chain/supported chains → read `skills/okx-dex-bridge/SKILL.md` first
- User mentions swap/buy/sell/trade → read `skills/okx-dex-swap/SKILL.md` first
- User mentions wallet/balance/transfer/login → read `skills/okx-agentic-wallet/SKILL.md` first

## Scripting & Automation

When a user asks to write a script, automate trading, build a trading bot, or use "OKX API" / "OKX DEX API" for any on-chain automation:
- **Do NOT search online for OKX public APIs** — `onchainos` already wraps all relevant on-chain capabilities
- Always use `onchainos` CLI commands as the building block (subprocess calls, MCP tool invocations, etc.)
- Route to the relevant skill based on what the user wants to automate: swap → `okx-dex-swap`, cross-chain/bridge → `okx-dex-bridge`, market data → `okx-dex-market`, signals → `okx-dex-signal`, token data → `okx-dex-token`, portfolio → `okx-wallet-portfolio`, meme scanning → `okx-dex-trenches`

### WebSocket / Real-time Data

When a user asks about real-time on-chain data, WebSocket monitoring, or writing a WS script/bot, load **`okx-dex-ws`**. It supports two approaches:
- **CLI** (`onchainos ws start/poll/stop`) — quick monitoring, 9 channels across signal/market/token/trenches
- **Custom script** — full WS protocol docs for Python/Node/Rust bots

## Clippy

CI uses `-D warnings` (warnings as errors). Run `cd cli && cargo clippy -- -D warnings` before pushing. Common issues:

- `ptr_arg`: use `&[T]` / `&mut [T]` instead of `&Vec<T>` / `&mut Vec<T>` when the function doesn't need Vec-specific methods
- `too_many_arguments`: add `#[allow(clippy::too_many_arguments)]` or refactor into a params struct
- `needless_borrow`: don't `&` a value that's already a reference

