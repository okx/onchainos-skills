# onchainos Skills — Agent Instructions

This is an **onchainos skill collection** providing 8 skills for on-chain operations: token search, market data, wallet balance, swap execution, transaction broadcasting, leaderboard rankings, address tracker activity, and token cluster analysis across 20+ blockchains.

## Available Skills

| Skill | Purpose | When to Use |
|-------|---------|-------------|
| okx-wallet-portfolio | Wallet balance and portfolio value | User asks about wallet holdings, token balances, portfolio value, remaining funds; user wants to check if a wallet has enough balance before a swap; user asks "how much ETH do I have", "what tokens are in my wallet", "show me my portfolio on Solana", "is my address funded" |
| okx-dex-market | Prices, K-line charts, index prices, signals, meme pump, wallet PnL | User asks for token prices, candlestick data, index prices, smart money/whale/KOL signals, meme token scanning, or wallet PnL analysis; user asks "what's the current price of USDT", "show me the 1h candle chart for ETH", "show smart money signals", "scan new meme launches on Solana", "what's my wallet PnL", "show my DEX transaction history" |
| okx-dex-swap | DEX swap execution | User wants to swap, trade, buy, or sell tokens on-chain; user wants to get a swap quote before executing; user asks "swap 10 USDC for ETH on Base", "buy some SOL with USDT", "what's the best rate to trade ARB for WETH", "execute the trade for me" |
| okx-dex-token | Token search, metadata, rankings, liquidity, hot tokens, advanced info, holders, top traders, trade history | User searches for tokens by name/symbol/address, wants trending rankings, liquidity pools, holder distribution, top trader analysis, or filtered trade history; user asks "find the contract address for PEPE", "show me trending tokens on Base", "who holds the most of this token", "show top liquidity pools", "show hot tokens on Solana", "is this token a honeypot", "show KOL trades for this token" |
| okx-onchain-gateway | Gas estimation, tx simulation, broadcasting | User wants to broadcast a signed tx, estimate gas fees, simulate a transaction before sending, or track a tx by hash; user asks "how much gas will this cost", "simulate this tx before I send it", "broadcast my signed transaction", "is my tx confirmed", "what's the status of this hash" |
| okx-dex-leaderboard | Smart money leaderboard / 牛人榜, top trader rankings | User asks for top traders ranked by PnL, win rate, transaction count, volume, or ROI; user asks "show me the 牛人榜", "who are the top smart money traders on Solana", "show me top traders by win rate this week", "leaderboard for Ethereum" |
| okx-dex-tracker | Address tracker trading activity (KOL / smart money / custom group) | User wants to see what KOL, smart money, or a custom group of addresses is buying or selling; user asks "what are KOLs buying", "show me smart money trades", "what's my tracked group trading", "show KOL buy activity on Solana" |

## Architecture

- **skills/** — 8 onchainos CLI skill definitions (each is a `SKILL.md` with YAML frontmatter + CLI command reference)
- **cli/** — Rust CLI binary (`onchainos`), built with `clap`; source in `cli/src/`, config in `cli/Cargo.toml`
- **cli/src/mcp/mod.rs** — MCP server implementation (rmcp v1.1.1)
- **.mcp.json.example** — MCP server configuration template
- **.github/workflows/** — CI/CD pipeline (`release.yml`: tag-triggered build for 9 platforms → GitHub Release)
- **install.sh** — One-line installer for macOS / Linux (`curl | sh`)

## Skill Discovery

Each skill in `skills/` contains a `SKILL.md` with:

- YAML frontmatter (name, description, metadata)
- Full CLI command reference with parameters and response schemas
- Usage examples (bash)
- Cross-skill workflow documentation
- Edge cases and error handling
