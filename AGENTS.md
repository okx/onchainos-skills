# onchainos — Agent Instructions

This is an **onchainos skill + workflow collection** providing 18 skills and pre-built workflows for on-chain operations across 20+ blockchains.

## Workflows (Primary Routing)

**For any of the following user intents, read `workflows/INDEX.md` before responding:**

| Intent | Trigger examples |
|--------|-----------------|
| Token research | "analyse token", "research [address]", "is this token safe" |
| Market overview | "daily brief", "market overview", "what's the market doing" |
| Smart money | "what are whales buying", "copy trading signals", "smart money" |
| New token scan | "scan new tokens", "pump.fun tokens", "meme scan" |
| Wallet analysis | "analyse wallet", "check this address", "is this wallet worth following" |
| Portfolio | "check my holdings", "my portfolio", "my wallet" |
| Wallet monitor | "watch wallet", "monitor address", "background monitor" |

`workflows/INDEX.md` maps each intent to the correct workflow file.
For Chinese queries, read `workflows/references/keyword-glossary.md` first.

Safety: follow token risk controls defined in `okx-security` SKILL.md.
For script requests, append `--format json` to all CLI commands.

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
| okx-dex-bridge | Cross-chain bridge swap: quote, execute, approve, status tracking | User wants to bridge tokens, cross-chain swap, transfer assets between chains |
| okx-agent-identity | ERC-8004 on-chain Agent identity: register / update / search / rate / service-list on XLayer | User wants to register/create/update/deactivate/activate/search agents, submit or view feedback, or list agent services |
| okx-agent-chat | Agent-to-agent communication: XMTP plugin management, encrypted file attachments | Agent needs to communicate with another agent, upload/download file attachments, install/update XMTP plugin |
| okx-agent-task | Agent task marketplace: publish, accept, deliver, dispute, AI-evaluate jobs | User wants to publish a task / accept a job / deliver work / confirm or reject completion / open a dispute |

## Architecture

- **skills/** — 18 onchainos CLI skill definitions (each is a `SKILL.md` with YAML frontmatter + CLI command reference)
- **workflows/** — Pre-built workflow docs (`INDEX.md` for routing, `TEMPLATE.md` for authoring guide)
- **cli/** — Rust CLI binary (`onchainos`), built with `clap`; source in `cli/src/`, config in `cli/Cargo.toml`
- **cli/src/mcp/mod.rs** — MCP server implementation (rmcp v1.1.1)

## CLI Composite Commands

| Command | What it does |
|---------|-------------|
| `onchainos token report --address <addr>` | Token info + price + advanced-info + security scan in one parallel call |
| `onchainos workflow token-research --address <addr>` | Full token research: core data + holders + cluster + signals + optional launchpad |
| `onchainos workflow smart-money` | Smart money signals: signal list + per-token due diligence |
| `onchainos workflow new-tokens` | New token screening: MIGRATED token scan + safety enrichment |
| `onchainos workflow wallet-analysis --address <addr>` | Wallet analysis: performance + behaviour + recent activity |
| `onchainos workflow portfolio --address <addr>` | Portfolio check: balances + total value + PnL overview |
