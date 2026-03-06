# onchainos Skills — Agent Instructions

This is an **onchainos skill collection** providing 5 skills for on-chain operations: token search, market data, wallet balance, swap execution, and transaction broadcasting across 20+ blockchains.

## Available Skills

| Skill | Purpose | When to Use |
|-------|---------|-------------|
| okx-wallet-portfolio | Wallet balance and portfolio value | User asks about wallet holdings, token balances, portfolio value, remaining funds; user wants to check if a wallet has enough balance before a swap; user asks "how much ETH do I have", "what tokens are in my wallet", "show me my portfolio on Solana", "is my address funded" |
| okx-dex-market | Prices, K-line charts, trade history | User asks for token prices, candlestick data, trade logs, index prices; user wants to analyze price trends, check recent trades, or get OHLCV data; user asks "what's the current price of USDT", "show me the 1h candle chart for ETH", "what was the last trade on this pair", "is the market bullish" |
| okx-dex-swap | DEX swap execution | User wants to swap, trade, buy, or sell tokens on-chain; user wants to get a swap quote before executing; user asks "swap 10 USDC for ETH on Base", "buy some SOL with USDT", "what's the best rate to trade ARB for WETH", "execute the trade for me" |
| okx-dex-token | Token search, metadata, rankings | User searches for tokens by name/symbol/address, wants trending rankings, holder distribution, market cap, or token security info; user asks "find the contract address for PEPE", "show me trending tokens on Base", "who holds the most of this token", "is this token a honeypot", "what's the total supply" |
| okx-onchain-gateway | Gas estimation, tx simulation, broadcasting | User wants to broadcast a signed tx, estimate gas fees, simulate a transaction before sending, or track a tx by hash; user asks "how much gas will this cost", "simulate this tx before I send it", "broadcast my signed transaction", "is my tx confirmed", "what's the status of this hash" |

## Architecture

- **skills/** — 5 onchainos CLI skill definitions (each is a `SKILL.md` with YAML frontmatter + CLI command reference)
- **cli/** — Rust CLI binary (`onchainos`), built with `clap`; source in `cli/src/`, config in `cli/Cargo.toml`
- **Formula/** — Homebrew formula template (`onchainos.rb`), auto-updated on release by `scripts/update-formula.sh`
- **scripts/** — Release automation scripts (e.g. `update-formula.sh` rewrites Formula SHA256 from GitHub Release checksums)
- **.github/workflows/** — CI/CD pipeline (`release.yml`: tag-triggered build for 9 platforms → GitHub Release → Homebrew update)
- **install.sh** — One-line installer for macOS / Linux (`curl | sh`)

## Skill Discovery

Each skill in `skills/` contains a `SKILL.md` with:

- YAML frontmatter (name, description, metadata)
- Full CLI command reference with parameters and response schemas
- Usage examples (bash)
- Cross-skill workflow documentation
- Edge cases and error handling
