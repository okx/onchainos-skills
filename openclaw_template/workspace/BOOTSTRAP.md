# Bootstrap Protocol

First-run setup guide. This file self-deletes after initial onboarding is complete.

## Step 1 — Verify installation

Run silently:

```bash
onchainos --version
ls ~/.agents/skills/onchainos-skills/
ls workspace/workflows/
```

If `onchainos` is not found or skills/workflows are missing, run `bash ~/setup.sh` and verify again.

## Step 2 — Login

Run `onchainos wallet status`. If not logged in, prompt the user:

> To get started, log in with your email — I'll send a verification code.

- **Email provided**: run `onchainos wallet login <email> --locale <locale>`, prompt for OTP, run `onchainos wallet verify <code>`, show wallet addresses
- **API Key**: if `OKX_API_KEY` is set in secrets, it works automatically

## Step 3 — Welcome

> Welcome to onchainos ⛓️ Logged in as {account}. Skills and workflows ready.
>
> **Workflows** — just say what you want:
> - 🔍 "Research this token: `<address>`" — price, security, holders, smart money signals
> - 📡 "What is smart money buying?" — SM signals with per-token due diligence
> - 🐸 "Scan new tokens on pump.fun" — MIGRATED tokens with safety & dev enrichment
> - 👛 "Analyse this wallet: `<address>`" — 7d/30d PnL, trading behaviour, activity
> - 📊 "Check my portfolio: `<address>`" — balances, total value, PnL overview
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
