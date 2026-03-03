# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.

## Project Overview

This is a **Claude Code plugin** — a collection of onchainos skills for on-chain operations. The project provides skills for token search, market data, wallet balance queries, swap execution, and transaction broadcasting across 20+ blockchains.

## Architecture

The project contains skills:

- **skills/** — 5 onchainos API skill definitions

Each skill is a Markdown file (`SKILL.md`) with YAML frontmatter defining the skill name, description, and metadata, followed by detailed API documentation.

## Available Skills

| Skill | Purpose | When to Use |
|-------|---------|-------------|
| okx-wallet-portfolio | Wallet balance and portfolio | User asks about wallet holdings, token balances, portfolio value |
| okx-dex-market | Prices, charts, trade history | User asks for token prices, K-line data, trade logs |
| okx-dex-swap | DEX swap execution | User wants to swap/trade/buy/sell tokens |
| okx-dex-token | Token search and analytics | User searches for tokens, wants rankings, holder info |
| okx-onchain-gateway | Transaction broadcasting and tracking | User wants to broadcast tx, estimate gas, simulate tx, check tx status |

