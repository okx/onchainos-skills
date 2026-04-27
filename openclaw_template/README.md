# onchainos — OpenClaw Agent Template

> **Built for AI. Ready for Web3.**

A general-purpose [OpenClaw](https://openclaw.ai) agent template powered by [OKX OnchainOS](https://web3.okx.com/onchainos). Deploy on any OpenClaw-compatible host and get an on-chain research and trading agent that can research tokens, track smart money, scan new launches, analyse wallets, manage DeFi positions, and execute swaps across **500+ aggregated DEXs on 60+ networks**.

## Quick start

1. Clone or fork this template
2. Run `bash setup.sh` — installs the `onchainos` CLI and fetches skills + workflows from the source repo
3. Start an OpenClaw session — the agent is ready

To enable trading, run `onchainos wallet login` inside a session.

## What you can do

| Ask the agent | What it does |
|---|---|
| "Research this token: `<address>`" | Price, security scan, holders, smart money signals, dev reputation |
| "What is smart money buying?" | Aggregated SM/KOL/whale signals with per-token due diligence |
| "Scan new tokens on pump.fun" | MIGRATED token list with safety, dev reputation, and bundle analysis |
| "Analyse this wallet: `<address>`" | 7d/30d PnL, trading behaviour, recent on-chain activity |
| "Daily brief" | Market prices, hot tokens, SM activity, new launches |
| "Check my portfolio: `<address>`" | Balances, total value, PnL overview |
| "Buy 0.1 SOL of BONK" | Pre-trade risk detection → quote → confirm → MEV-protected execution |

## Infrastructure

- **500+ DEX sources** aggregated for best swap price
- **130+ networks** via OKX Wallet ecosystem
- **Sub-100ms** average response times · **99.9% uptime**
- **TEE-secured agentic wallet** — private keys never exposed
- **MEV protection** — Jito (Solana) and Flashbots (EVM)
- **Gas-free payments** on X Layer via x402 protocol

## Structure

```
openclaw_template/
├── README.md                     # This file
├── manifest.json                 # Template store manifest
├── setup.sh                      # Build script (calls install.sh)
└── workspace/
    ├── SOUL.md                   # Agent personality, values, tone, boundaries
    ├── AGENTS.md                 # Workflow routing, skill table, harness rules, session management
    ├── BOOTSTRAP.md              # First-run onboarding (self-deletes after setup)
    ├── IDENTITY.md               # Agent name, type, vibe, emoji
    ├── USER.md                   # Learned user preferences (updated by agent)
    ├── TOOLS.md                  # Capabilities, CLI reference, wallet modes, swap infrastructure
    ├── MEMORY.md                 # Long-term learned patterns (updated by agent)
    ├── HEARTBEAT.md              # Periodic task config
    ├── memory/                   # Daily memory files (memory/YYYY-MM-DD.md)
    └── projects/
        └── scan-bot-example.py   # Sample Python script demonstrating onchainos CLI scripting
```

Skills and workflows are fetched from the onchainos-skills source repo at deploy time — always the latest version.

## Optional: OKX API credentials

The agent uses built-in sandbox keys by default (rate-limited). For production-grade rate limits, set these environment variables:

| Variable | Description |
|---|---|
| `OKX_API_KEY` | Apply at [OKX Developer Portal](https://web3.okx.com/onchain-os/dev-portal) |
| `OKX_SECRET_KEY` | Your API secret |
| `OKX_PASSPHRASE` | Your API passphrase |

## Links

- [OKX OnchainOS](https://web3.okx.com/onchainos)
- [Developer Portal](https://web3.okx.com/onchain-os/dev-portal)
- [OpenClaw](https://openclaw.ai)

## License

MIT
