# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.

## Project Overview

This is a **Claude Code plugin** — a collection of onchainos skills for on-chain operations. The project provides skills for token search, market data, wallet balance queries, swap execution, and transaction broadcasting across 20+ blockchains.

## Architecture

- **skills/** — 5 onchainos CLI skill definitions (each is a `SKILL.md` with YAML frontmatter + CLI command reference)
- **cli/** — Rust CLI binary (`onchainos`), built with `clap`; source in `cli/src/`, config in `cli/Cargo.toml`
- **.github/workflows/** — CI/CD pipeline (`release.yml`: tag-triggered build for 9 platforms → GitHub Release)
- **install.sh** — One-line installer for macOS / Linux (`curl | sh`)

## Available Skills

| Skill | Purpose | When to Use |
|-------|---------|-------------|
| okx-wallet-portfolio | Wallet balance and portfolio | User asks about wallet holdings, token balances, portfolio value |
| okx-dex-market | Prices, charts, trade history | User asks for token prices, K-line data, trade logs |
| okx-dex-swap | DEX swap execution | User wants to swap/trade/buy/sell tokens |
| okx-dex-token | Token search and analytics | User searches for tokens, wants rankings, holder info |
| okx-onchain-gateway | Transaction broadcasting and tracking | User wants to broadcast tx, estimate gas, simulate tx, check tx status |
