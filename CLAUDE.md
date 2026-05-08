# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

A **Claude Code plugin** — onchainos skills for on-chain operations (token search, market data, wallet balance, swap execution, DeFi, transaction broadcasting) across 20+ blockchains. The `onchainos` CLI doubles as a native MCP server.

## Build / Test / Lint

All Rust commands run from the `cli/` directory.

```bash
# Build
cd cli && cargo build

# Lint (CI uses -D warnings — fix all warnings before pushing)
cd cli && cargo clippy -- -D warnings

# Format check
cd cli && cargo fmt --check

# Run all tests (needs OKX API keys in env or .env)
cd cli && cargo test

# Run a single test
cd cli && cargo test token_search_by_symbol

# Run tests for one module
cd cli && cargo test --test cli_token

# Debug build with debug logging
cd cli && cargo build --features debug-log

# Dependency vulnerability audit
cd cli && cargo audit
```

Integration tests live in `cli/tests/` and use `assert_cmd`. They hit real OKX APIs, so `OKX_API_KEY`, `OKX_SECRET_KEY`, and `OKX_PASSPHRASE` must be set. Test helpers are in `cli/tests/common/mod.rs` — provides `onchainos()` command builder, `run_with_retry()` for rate-limit resilience, and `assert_ok_and_extract_data()` to validate the JSON envelope.

## CI / Release

**CI** (`ci.yml`): runs on push/PR across ubuntu, macos, windows — checks `cargo fmt`, `cargo clippy -D warnings`, `cargo test`, and `cargo audit`.

**Release** (`release.yml`): manual `workflow_dispatch` with version input. Builds 9 targets (Linux musl/gnu, Windows MSVC, macOS signed). Version in `Cargo.toml` must match the input. Beta releases use `X.Y.Z-beta.N` format (marked as pre-release on GitHub).

## Architecture

### Directory Layout

- **skills/** — 18 skill definitions (each is a `SKILL.md` with YAML frontmatter + CLI command reference). Some skills have sub-docs (e.g. `okx-agent-task/` has `buyer.md`, `provider.md`, `evaluator.md` role-specific protocols and `_shared/` for state machine, message types, negotiate protocol).
- **workflows/** — Multi-step workflow docs (`INDEX.md` routes intents → workflow files, `TEMPLATE.md` for authoring new ones).
- **cli/** — Rust CLI binary (`onchainos`), built with `clap`; source in `cli/src/`.
- **cli/src/mcp/mod.rs** — MCP server implementation (~2200 lines, rmcp v1.1.1, exposes all commands as MCP tools over stdio).
- **tools/** — Dev/test mock servers (TypeScript): `ws-mock-ts` (WebSocket mock API), `xmtp-mock-buyer`, `xmtp-mock-seller`, `mock-evaluator` for agent-task flow testing.
- **.claude-plugin/** / **.cursor-plugin/** — Plugin manifests for Claude Code and Cursor.

### CLI Architecture

**Entry point**: `cli/src/main.rs` — parses `clap` CLI args, dispatches to command modules, records every invocation to `~/.onchainos/audit.jsonl`.

**Output protocol**: all commands emit JSON `{ "ok": true, "data": ... }` on success or `{ "ok": false, "error": "..." }` on failure. Exit code 2 means "confirming" (needs user confirmation for next step). Use `output::success()` / `output::error()` / `output::confirming()`.

**Auth cascade** (`client.rs`): `ApiClient` tries JWT (from keyring) → AK env vars (`OKX_API_KEY` / `OKX_SECRET_KEY` / `OKX_PASSPHRASE`) → Anonymous. `client_async()` adds JWT auto-refresh.

**Command context** (`commands/mod.rs`): `Context` struct holds `AppConfig`, base_url/chain overrides, and creates `ApiClient`. Chain names resolve to OKX chainIndex via `chains.rs` (e.g. "ethereum" → "1", "solana" → "501").

**Config**: `~/.onchainos/config.json` (managed by `config.rs`). Credentials loaded from env vars or `~/.onchainos/.env` via dotenvy.

**MCP mode**: `onchainos mcp` starts a JSON-RPC 2.0 server over stdio (configure via `.mcp.json.example`).

### Key Command Modules

Commands map 1:1 to top-level CLI subcommands: `market`, `signal`, `memepump`, `token`, `swap`, `cross-chain`, `gateway`, `portfolio`, `wallet`, `security`, `payment`, `defi`, `ws`, `workflow`, `agent` (identity + task + chat). Each module has an `execute()` async fn dispatched from `main.rs`.

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

## Inbound Message Routing (highest priority — before ALL skill matching)

**STOP — check this table FIRST before matching any skill.** When the inbound message matches any shape below, route directly — do NOT fall through to the Available Skills table.

| Envelope shape | Action |
|---|---|
| `msgType: "a2a-agent-chat"` + non-empty `jobId` | **Must** load `skills/okx-agent-task/SKILL.md` → check `sender.role` → load `buyer.md` (if role=2) or `provider.md` (if role=1). This is a task lifecycle message, not a chat. |
| `{agentId, message: {source: "system", event, jobId, ...}}` | **Must** load `skills/okx-agent-task/SKILL.md` → call `next-action` with the event. This is a chain event notification. |
| Structured service envelope with `Agent <id>` + `ServiceTitle` + `ServiceType` + `Endpoint` + `"Please use onchainos to send a request to this endpoint"` | **Must** load `skills/okx-agent-task/buyer.md` → Scene 3.4（x402 指定卖家，走任务系统）. Even though the message mentions x402/Endpoint/paid, this is a **task-system** flow, NOT the standalone `okx-x402-payment` skill. |
| Structured service envelope with `Agent <id>` + `ServiceTitle` + `ServiceType` + `Price` + `"Please initiate a direct conversation with this provider"` | **Must** load `skills/okx-agent-task/buyer.md` → Scene 3.3（A2A 指定卖家，走任务系统）. |
| Contains `"Please send a request to this endpoint."` but does **NOT** contain `"use onchainos"` and does **NOT** have `Agent`/`ServiceTitle`/`ServiceType` fields | Route to `okx-x402-payment` skill. |

These envelope-based routes take **absolute priority** over the Available Skills table. Do NOT handle these envelopes with only XMTP tools — the task protocol (buyer.md / provider.md / evaluator.md) defines the required behavior.

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
| okx-agent-task | Agent task marketplace: publish, accept, deliver, dispute, AI-evaluate jobs | User wants to publish a task / accept a job / deliver work / confirm or reject completion / open a dispute |

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

The release profile uses `lto = true`, `codegen-units = 1`, `strip = true`, `panic = "abort"` — release builds are slow but fully optimized.
