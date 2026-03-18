# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.

## Project Overview

This is a **Claude Code plugin** — a collection of onchainos skills for on-chain operations. The project provides skills for token search, market data, wallet balance queries, swap execution, and transaction broadcasting across 20+ blockchains. The `onchainos` CLI also works as a native MCP server.

## Architecture

- **skills/** — 10 onchainos CLI skill definitions (each is a `SKILL.md` with YAML frontmatter + CLI command reference)
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
| okx-dex-market       | Prices, charts, wallet PnL | User asks for token prices, K-line data, or wallet PnL analysis (win rate, DEX history, realized/unrealized PnL) |
| okx-dex-signal       | Smart money / whale / KOL signals | User asks what smart money/whales/KOLs are buying, signal alerts, 大户信号 |
| okx-dex-trenches     | Meme/pump.fun token scanning | User asks about new meme launches, dev reputation, bundle detection, 打狗/扫链/新盘, or mentions trench/trenches |
| okx-dex-swap         | DEX swap execution | User wants to swap/trade/buy/sell tokens |
| okx-dex-token        | Token search, liquidity, hot tokens, advanced info, holders, top traders, trade history | User searches for tokens, wants rankings, liquidity pools, holder info, top traders, or filtered trade history |
| okx-onchain-gateway  | Transaction broadcasting and tracking | User wants to broadcast tx, estimate gas, simulate tx, check tx status |
| okx-audit-log        | Audit log export and troubleshooting | User wants to view command history, debug errors, export audit log, review recent activity |

## Clippy

CI uses `-D warnings` (warnings as errors). Run `cargo clippy` before pushing. Common issues:

- `ptr_arg`: use `&[T]` / `&mut [T]` instead of `&Vec<T>` / `&mut Vec<T>` when the function doesn't need Vec-specific methods
- `too_many_arguments`: add `#[allow(clippy::too_many_arguments)]` or refactor into a params struct
- `needless_borrow`: don't `&` a value that's already a reference
