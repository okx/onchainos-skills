# onchainos — Pinata Agent Template

> **Built for AI. Ready for Web3.**

The official [OKX OnchainOS](https://web3.okx.com/onchainos) agent for Pinata — a complete on-chain workstation powered by onchainos skills and pre-built workflows. Research tokens, track smart money, scan new pump.fun launches, analyse wallets, and execute swaps across **500+ aggregated DEXs on 60+ networks** with sub-100ms response times.

## What you can do

| Ask the agent | What it does |
|---|---|
| "Research this token: `<address>`" | Price, security scan, holders, smart money signals, dev reputation, launchpad data |
| "What is smart money buying?" | Aggregated SM/KOL/whale signals with per-token due diligence |
| "Scan new tokens on pump.fun" | MIGRATED token list with safety, dev reputation, and bundle analysis |
| "Analyse this wallet: `<address>`" | 7d/30d PnL, trading behaviour, recent on-chain activity |
| "Daily brief" | Market prices, hot tokens, SM activity, new launches, portfolio alerts |
| "Check my portfolio: `<address>`" | Balances, total value, 30d PnL overview |
| "Buy 0.1 SOL of BONK" | Pre-trade risk detection → quote → confirm → MEV-protected execution |
| "Watch this wallet: `<address>`" | Real-time alerts when it trades |

## Infrastructure

- **500+ DEX sources** aggregated for best swap price
- **130+ networks** via OKX Wallet ecosystem
- **Sub-100ms** average response times · **99.9% uptime**
- **TEE-secured agentic wallet** — private keys never exposed
- **MEV protection** — Jito (Solana) and Flashbots (EVM)
- **Gas-free payments** on X Layer via x402 protocol

## Supported chains

Solana, Ethereum, Base, BSC, Arbitrum, Polygon, XLayer, Sui, TON, and 60+ others.

## Setup

### Deploy on Pinata

1. Select this template in the Pinata Agent Template Store
2. Click **Deploy** — `setup.sh` installs the `onchainos` CLI and links all skills automatically
3. Start chatting — all read-only research works immediately with no login

### Optional: OKX API credentials

The agent uses built-in sandbox keys by default (rate-limited). For production-grade rate limits, set these in Pinata's secret manager:

| Secret | Description |
|---|---|
| `OKX_API_KEY` | Apply at [OKX Developer Portal](https://web3.okx.com/onchain-os/dev-portal) |
| `OKX_SECRET_KEY` | Your API secret |
| `OKX_PASSPHRASE` | Your API passphrase |

### Optional: Enable trading (TEE-secured)

To execute swaps with the agentic wallet:

```
onchainos wallet login
```

Follow the prompts (email + OTP). Once logged in, say "Buy 0.1 SOL of BONK" — the agent runs a security check, shows you the quote, and asks for confirmation before executing. Private keys are secured in TEE and never exposed.

## What's included

```
├── manifest.json                 # Pinata template manifest
├── setup.sh                      # Installs onchainos CLI + fetches skills & workflows from the source repo
├── README.md
├── SECURITY.md
└── workspace/
    ├── SOUL.md                   # Agent personality, values, tone, boundaries
    ├── AGENTS.md                 # Workflow routing, skill table, harness rules, session management
    ├── BOOTSTRAP.md              # First-run onboarding (self-deletes after setup)
    ├── IDENTITY.md               # Agent name, type, vibe, emoji
    ├── USER.md                   # Learned user preferences (updated by agent over time)
    ├── TOOLS.md                  # Capabilities, CLI reference, wallet modes, swap infrastructure
    ├── MEMORY.md                 # Long-term learned patterns (updated by agent over time)
    ├── HEARTBEAT.md              # Periodic task config
    ├── memory/                   # Daily memory files (memory/YYYY-MM-DD.md)
    └── projects/
        └── scan-bot-example.py   # Sample Python script demonstrating onchainos CLI scripting
```

Skills and workflows are fetched from the onchainos-skills source repo at deploy time by `setup.sh` — always the latest version.

## Skills

| Skill | Purpose |
|---|---|
| `okx-dex-token` | Token search, price, holders, cluster analysis, top traders |
| `okx-dex-market` | Prices, K-line charts, index prices, wallet PnL |
| `okx-dex-signal` | Smart money / KOL / whale signal tracking and leaderboard |
| `okx-dex-trenches` | Meme/pump.fun scanning, dev reputation, bundle detection |
| `okx-dex-swap` | Swap execution across 500+ DEX sources with MEV protection |
| `okx-dex-ws` | Real-time WebSocket monitoring for prices, trades, and signals |
| `okx-security` | Token risk scan, DApp phishing detection, tx pre-execution analysis |
| `okx-wallet-portfolio` | Public address balances and portfolio value across chains |
| `okx-agentic-wallet` | TEE-secured wallet: login, balance, send tokens, tx history |
| `okx-onchain-gateway` | Gas estimation, tx simulation, broadcasting, order tracking |
| `okx-defi-invest` | DeFi discovery, deposit, withdraw, claim across Aave/Lido/Kamino/NAVI/+ |
| `okx-defi-portfolio` | DeFi positions overview across protocols and chains |
| `okx-x402-payment` | Gas-free x402 payment-gated API authorization on X Layer |
| `okx-audit-log` | Command history and audit log export |

## Links

- [OKX OnchainOS](https://web3.okx.com/onchainos)
- [Developer Portal](https://web3.okx.com/onchain-os/dev-portal)

## License

MIT
