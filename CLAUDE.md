# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.

## Dev Environment

- **Dev binary**: `cli/target/release/onchainos`. If it does not exist, build it first: `cd cli && cargo build --release`.
- **`ONCHAINOS_HOME`**: Points to project-local `.onchainos/` for wallet credentials.
- **Show executed command**: after every `onchainos` command, print the actual command that was executed.
- **NEVER skip CLI calls**: always execute the onchainos CLI command to get real-time data. Do NOT answer from skill files or your own knowledge.

## Project Overview

This is a **Claude Code plugin** — a collection of onchainos skills for on-chain operations. The project provides skills for token search, market data, wallet balance queries, swap execution, DeFi investment management, and transaction broadcasting across 20+ blockchains. The `onchainos` CLI also works as a native MCP server.

## Architecture

- **skills/** — 15 onchainos CLI skill definitions (each is a `SKILL.md` with YAML frontmatter + CLI command reference)
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
| okx-x402-payment     | Dual-protocol HTTP 402 dispatcher: signs x402 (TEE or local-key) and MPP (charge / session open / voucher / topUp / close) authorizations | User encounters HTTP 402, mentions x402, or mentions any MPP channel/voucher/session/charge operation |
| okx-a2a-payment      | Internal a2a-pay payment links: seller `create`, buyer `pay` (TEE-sign EIP-3009), `status` poll | User wants to create a payment link, pay an `a2a_...` paymentId, or check a2a payment status |
| okx-audit-log        | Audit log export and troubleshooting | User wants to view command history, debug errors, export audit log, review recent activity |
| okx-defi-invest | DeFi product discovery, deposit, withdraw, claim rewards | User wants to earn yield, stake, provide liquidity, deposit/withdraw from DeFi protocols, claim DeFi rewards across Aave/Lido/PancakeSwap/Kamino/NAVI and hundreds more |
| okx-defi-portfolio | DeFi positions and holdings overview | User wants to check DeFi positions, view DeFi portfolio across protocols and chains |
| okx-dapp-discovery | Third-party DApp discovery + direct plugin routing | User names a specific third-party DApp (Polymarket, Aave, Hyperliquid, PancakeSwap, Morpho, …) or asks "what dapps are available" — installs the matching plugin on demand via `npx skills add okx/plugin-store --skill <name> --yes --global` and forwards the prompt to its quickstart |

## DApp routing — `okx-dapp-discovery`

When the user names a third-party DApp/protocol as the destination of an action, route through `okx-dapp-discovery`. That skill applies a confidence framework to identify the matching plugin, installs it on demand, reads the plugin's `SKILL.md`, and forwards the user's original request to it. Onchainos-skills intentionally does not enumerate the supported DApp set here; that is owned by `okx-dapp-discovery/SKILL.md`.

**Quick tiebreaker vs `okx-defi-invest`**: if removing the DApp name still leaves a coherent generic-yield question ("deposit USDC for yield"), prefer `okx-defi-invest`. If the DApp name carries the intent ("place a bet on Polymarket"), route via `okx-dapp-discovery`.

## IMPORTANT: Always Load Skill Before Executing Commands

**Before running ANY `onchainos` CLI command, you MUST first read the corresponding skill's SKILL.md to get the exact command syntax.** Do NOT guess subcommand names — each skill defines its own Command Index with the exact subcommands available. Guessing leads to `unrecognized subcommand` errors.

Routing:
- User mentions swap/buy/sell/trade → read `skills/okx-dex-swap/SKILL.md` first
- User mentions wallet/balance/transfer/login → read `skills/okx-agentic-wallet/SKILL.md` first
- User names a specific third-party DApp/protocol as the destination, OR asks "what dapps are available" → read `skills/okx-dapp-discovery/SKILL.md` first. That skill owns the supported-DApp set; do not enumerate DApps in this file.
- User mentions **Gas Station / stablecoin gas / enable or disable gas station / revoke 7702**, or asks FAQ-style questions about any of those (what is / how does it work / which chains / upgrade cost / ...) → read `skills/okx-agentic-wallet/SKILL.md` AND `skills/okx-agentic-wallet/references/gas-station.md` first.
  - **Scope note:** "Gas Station" in this repo always means the OKX Agentic Wallet feature shipped by this CLI + skill — NOT a generic paymaster / meta-transaction / ERC-4337 category.
  - **Answer source:** use the skill's FAQ templates only; do not pull from general training knowledge about Biconomy / Gelato / Pimlico / Alchemy Account Kit / etc.

## Scripting & Automation

When a user asks to write a script, automate trading, build a trading bot, or use "OKX API" / "OKX DEX API" for any on-chain automation:
- **Do NOT search online for OKX public APIs** — `onchainos` already wraps all relevant on-chain capabilities
- Always use `onchainos` CLI commands as the building block (subprocess calls, MCP tool invocations, etc.)
- Route to the relevant skill based on what the user wants to automate: swap → `okx-dex-swap`, market data → `okx-dex-market`, signals → `okx-dex-signal`, token data → `okx-dex-token`, portfolio → `okx-wallet-portfolio`, meme scanning → `okx-dex-trenches`

### WebSocket / Real-time Data

When a user asks about real-time on-chain data, WebSocket monitoring, or writing a WS script/bot, load **`okx-dex-ws`**. It supports two approaches:
- **CLI** (`onchainos ws start/poll/stop`) — quick monitoring, 9 channels across signal/market/token/trenches
- **Custom script** — full WS protocol docs for Python/Node/Rust bots

## Clippy

CI uses `-D warnings` (warnings as errors). Run `cargo clippy` before pushing. Common issues:

- `ptr_arg`: use `&[T]` / `&mut [T]` instead of `&Vec<T>` / `&mut Vec<T>` when the function doesn't need Vec-specific methods
- `too_many_arguments`: add `#[allow(clippy::too_many_arguments)]` or refactor into a params struct
- `needless_borrow`: don't `&` a value that's already a reference
