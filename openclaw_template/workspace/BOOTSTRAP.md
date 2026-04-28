# Bootstrap Protocol

First-run setup guide. This file self-deletes after initial onboarding is complete.

## Step 1 — Verify CLI and workflows

Run:

```bash
onchainos --version
ls ~/.onchainos/workflows/
```

If `onchainos` is not found or workflows are missing, run `bash ~/setup.sh` and verify again.

## Step 2 — Install skills

1. Follow the instructions in `https://github.com/okx/onchainos-skills/blob/main/.openclaw/INSTALL.md` to install onchainos skills
2. Determine the current workspace directory (check `openclaw` config or working directory)
3. Copy skills from `~/.openclaw/onchainos-skills/skills/` into the workspace's `skills/` directory
4. Run `openclaw skills list` to confirm they are loaded

## Step 3 — Login

Run `onchainos wallet status`. If not logged in, prompt the user:

> To get started, log in with your email — I'll send a verification code.

- **Email provided**: run `onchainos wallet login <email> --locale <locale>`, prompt for OTP, run `onchainos wallet verify <code>`, show wallet addresses
- **API Key**: if `OKX_API_KEY` is set in secrets, it works automatically

## Step 4 — Welcome

> Welcome to onchainos ⛓️
>
> **Workflows** — just say what you want:
> - 🔍 "Research this token: `<address>`" — price, security, holders, smart money signals
> - 📡 "What is smart money buying?" — SM signals with per-token due diligence
> - 🐸 "Scan new tokens on pump.fun" — MIGRATED tokens with safety & dev enrichment
> - 👛 "Analyse this wallet: `<address>`" — 7d/30d PnL, trading behaviour, activity
> - 📊 "Check my portfolio" — balances, total value, PnL overview
> - 📰 "Give me a daily brief" — market prices + hot tokens + smart money + new launches
> - 👁 "Watch this wallet: `<address>`" — alert me when it trades
>
> **Skills** — ask me directly about anything:
> - 🪙 Token search, price, holders, top traders, cluster analysis
> - 📈 Prices, K-line charts, wallet PnL
> - 🦈 Smart money / KOL / whale signals & leaderboard
> - 🐸 Meme/pump.fun scanning, dev reputation, bundle detection
> - 🔄 DEX swap execution across 500+ liquidity sources
> - ⚡ Real-time WebSocket monitoring
> - 🛡️ Token risk, DApp phishing, tx pre-execution scan
> - 💼 Public wallet balance & token holdings
> - 👛 Wallet: balance, send, tx history
> - 🔗 Gas estimation, tx simulation, broadcasting
> - 🌾 DeFi: discover, deposit, withdraw, claim rewards
> - 📈 DeFi portfolio across protocols
> - 💳 x402 gas-free payment authorization
> - 📋 Audit log & command history
