# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.

## Project Overview

This is a **Claude Code plugin** — a collection of onchainos skills for on-chain operations. The project provides skills for token search, market data, wallet balance queries, swap execution, DeFi investment management, and transaction broadcasting across 20+ blockchains. The `onchainos` CLI also works as a native MCP server.

## Architecture

- **skills/** — 13 onchainos CLI skill definitions (each is a `SKILL.md` with YAML frontmatter + CLI command reference)
- **workflows/** — 9 multi-step workflow docs (`INDEX.md` for routing, `TEMPLATE.md` for authoring guide, W1–W9 as `*.md`)
- **cli/** — Rust CLI binary (`onchainos`), built with `clap`; source in `cli/src/`, config in `cli/Cargo.toml`
- **cli/src/mcp/mod.rs** — MCP server implementation (rmcp v1.1.1)
- **.mcp.json.example** — MCP server configuration template for Claude Code
- **.github/workflows/** — CI/CD pipeline (`release.yml`: tag-triggered build for 9 platforms → GitHub Release)
- **install.sh** — One-line installer for macOS / Linux (`curl | sh`)

## Workflows

**For any of the following user intents, read `workflows/INDEX.md` before responding — do not call individual skills directly:**

| Intent | Trigger examples |
|--------|-----------------|
| Token research | "analyze token", "research [address]", "is this token safe" |
| Market overview | "daily brief", "market overview", "what's the market doing" |
| Smart money | "what are whales buying", "copy trading signals", "smart money" |
| New token scan | "scan new tokens", "pump.fun tokens", "meme scan" |
| Wallet analysis | "analyze wallet", "check this address", "is this wallet worth following" |
| Buy / sell / swap | "buy X", "sell X", "swap X for Y", "trade X for Y" |
| Portfolio | "check my holdings", "my portfolio", "my wallet" |
| Wallet monitor | "watch wallet", "monitor address", "background monitor" |

`workflows/INDEX.md` maps each intent to the correct workflow file with step-by-step instructions.
For Chinese queries, read `workflows/references/keyword-glossary.md` first to resolve the intent.

Safety: follow token risk controls defined in `okx-security` SKILL.md.
For script requests, append `--format json` to all CLI commands.

## Available Skills

Skills are **building blocks**. Use them directly only for operations not covered by a workflow above.

### Direct-Use Skills

No corresponding workflow — always invoke these directly:

| Skill | Purpose | Use When |
|-------|---------|----------|
| okx-agentic-wallet | Wallet auth, authenticated balance, send tokens, tx history, contract call | User wants to log in, check their own authenticated balance, send tokens, view tx history, or call contracts |
| okx-security | DApp/URL phishing detection, tx pre-execution scan, signature safety, approval management | User asks about DApp/URL safety, wants to scan a specific tx or signature, or manage token approvals |
| okx-defi-invest | DeFi product discovery, deposit, withdraw, claim rewards | User wants to earn yield, stake, deposit/withdraw from DeFi protocols, claim DeFi rewards |
| okx-defi-portfolio | DeFi positions and holdings | User wants to check DeFi positions across protocols |
| okx-dex-ws | Real-time WebSocket monitoring and scripting | User wants to write a WS script or monitor real-time on-chain data via custom bot |
| okx-onchain-gateway | Transaction broadcasting and tracking | User wants to broadcast a tx, estimate gas, or check tx status outside of a swap flow |
| okx-x402-payment | x402 payment authorization | User encounters HTTP 402 or mentions x402 / pay for access |
| okx-audit-log | Audit log export and troubleshooting | User wants to view command history, debug errors, or export audit log |

### Workflow-Covered Skills (Building Blocks)

Invoked by workflows internally — **do not call directly** in response to user requests that match a workflow trigger above:

| Skill | Used By Workflows |
|-------|------------------|
| okx-dex-token | Token Research, Daily Brief, Smart Money Signals, New Token Screening, Portfolio Check |
| okx-dex-swap | Safe Swap |
| okx-dex-market | Daily Brief, Wallet Analysis, Portfolio Check |
| okx-dex-signal | Smart Money Signals, Daily Brief, Wallet Analysis, Wallet Monitor |
| okx-dex-trenches | New Token Screening, Token Research (launchpad), Smart Money Signals, Daily Brief |
| okx-wallet-portfolio | Portfolio Check, Daily Brief, Wallet Analysis |

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
