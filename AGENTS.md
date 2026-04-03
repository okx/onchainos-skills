# onchainos Skills — Agent Instructions

This is an **onchainos skill collection** providing 14 skills for on-chain operations across 20+ blockchains.

## Available Skills

| Skill | Purpose | When to Use |
|-------|---------|-------------|
| okx-agentic-wallet | Wallet lifecycle: auth, balance (authenticated), portfolio PnL, send, history, contract call | User wants to log in, check balance, view PnL, send tokens, view tx history, or call contracts |
| okx-wallet-portfolio | Public address balance, token holdings, portfolio value | User asks about wallet holdings, token balances, portfolio value across chains by providing a specific address |
| okx-security | Security scanning: token risk, DApp phishing, tx pre-execution, signature safety, approval management | User wants to check if a token/DApp/tx/signature is safe, honeypot check, phishing detection, approve safety, or view/manage token approvals |
| okx-dex-market | Prices, charts, index prices, wallet PnL | User asks for token prices, K-line data, index/aggregate prices, wallet PnL analysis |
| okx-dex-signal | Smart money / KOL / whale tracking, buy signals, leaderboard | User asks what smart money/whales/KOLs are buying, wants signal alerts (信号), top traders (牛人榜) |
| okx-dex-trenches | Meme/pump.fun token scanning, trenches | User asks about new meme launches, dev reputation, bundle detection, 打狗/扫链/新盘, or mentions trench/trenches |
| okx-dex-ws | WebSocket scripting for all DEX channels | User wants to write a WebSocket script/脚本/bot for real-time on-chain data |
| okx-dex-swap | DEX swap execution | User wants to swap, trade, buy, or sell tokens on-chain; user wants to get a swap quote before executing |
| okx-dex-token | Token search, metadata, rankings, liquidity, hot tokens, advanced info, holders, top traders, trade history, holder cluster analysis | User searches for tokens, wants rankings, liquidity pools, holder info, top traders, filtered trade history, or holder cluster concentration |
| okx-onchain-gateway | Gas estimation, tx simulation, broadcasting | User wants to broadcast a signed tx, estimate gas fees, simulate a transaction before sending, or track a tx by hash |
| okx-x402-payment | Sign x402 payment authorization via TEE for payment-gated resources | User encounters HTTP 402, wants to pay for a payment-gated API, or mentions x402 / pay for access |
| okx-audit-log | Audit log export and troubleshooting | User wants to view command history, debug errors, export audit log, review recent activity |
| okx-defi-invest | DeFi product discovery, deposit, withdraw, claim rewards | User wants to earn yield, stake, provide liquidity, deposit/withdraw from DeFi protocols |
| okx-defi-portfolio | DeFi positions and holdings overview | User wants to check DeFi positions, view DeFi portfolio across protocols and chains |

## Architecture

- **skills/** — 14 onchainos CLI skill definitions (each is a `SKILL.md` with YAML frontmatter + CLI command reference)
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
