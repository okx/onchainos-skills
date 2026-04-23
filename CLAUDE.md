# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.

## Project Overview

This is a **Claude Code plugin** — a collection of onchainos skills for on-chain operations. The project provides skills for token search, market data, wallet balance queries, swap execution, DeFi investment management, and transaction broadcasting across 20+ blockchains. The `onchainos` CLI also works as a native MCP server.

## Architecture

- **skills/** — 14 onchainos CLI skill definitions (each is a `SKILL.md` with YAML frontmatter + CLI command reference)
- **workflows/** — 8 multi-step workflow docs (`INDEX.md` for routing, `TEMPLATE.md` for authoring guide)
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

| Skill | Purpose | When to Use |
|-------|---------|-------------|
| okx-agentic-wallet | Wallet auth, authenticated balance, send tokens, tx history, contract call | User wants to log in, check balance, send tokens, or view tx history |
| okx-wallet-portfolio | Public address balance, token holdings, portfolio value | User asks about wallet holdings or token balances for a specific address |
| okx-security | DApp/URL phishing detection, tx pre-execution scan, signature safety, approval management | User asks about DApp/URL safety, tx scan, signature safety, or token approvals |
| okx-dex-market | Prices, charts, index prices, wallet PnL | User asks for token prices, K-line data, or wallet PnL analysis |
| okx-dex-signal | Smart money / KOL / whale tracking, buy signals, leaderboard | User asks what smart money/whales/KOLs are buying or wants signal alerts |
| okx-dex-trenches | Meme/pump.fun token scanning, dev reputation, bundle detection | User asks about new meme launches, dev reputation, or bundle analysis |
| okx-dex-ws | Real-time WebSocket monitoring and scripting | User wants a WS script or real-time on-chain data stream |
| okx-dex-swap | DEX swap execution | User wants to swap, trade, buy, or sell tokens |
| okx-dex-token | Token search, metadata, rankings, liquidity, holders, top traders, cluster analysis | User searches for tokens, wants rankings, holder info, or cluster analysis |
| okx-onchain-gateway | Transaction broadcasting and tracking | User wants to broadcast a tx, estimate gas, or check tx status |
| okx-x402-payment | x402 payment authorization | User encounters HTTP 402 or mentions x402 / pay for access |
| okx-defi-invest | DeFi product discovery, deposit, withdraw, claim rewards | User wants to earn yield, stake, deposit/withdraw from DeFi protocols, claim DeFi rewards |
| okx-defi-portfolio | DeFi positions and holdings | User wants to check DeFi positions across protocols |
| okx-audit-log | Audit log export and troubleshooting | User wants to view command history, debug errors, or export audit log |

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
