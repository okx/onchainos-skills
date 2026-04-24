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

Confirm to user:
- onchainos version
- Number of skills available (expect 14)
- Number of workflows available (expect 8)

## Step 2 — Check wallet status

Run `onchainos wallet status`.

### If NOT logged in

> Welcome to onchainos ⛓️
>
> Everything is set up and ready to go.
>
> **Workflows** — multi-step operations, just say what you want:
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
> - 👛 Wallet: login, balance, send, tx history
> - 🔗 Gas estimation, tx simulation, broadcasting
> - 🌾 DeFi: discover, deposit, withdraw, claim rewards
> - 📈 DeFi portfolio across protocols
> - 💳 x402 gas-free payment authorization
> - 📋 Audit log & command history
>
> To enable **trading**, log in with your OKX account:
> - **Email login**: tell me your email and I'll send a code
> - **API Key**: set `OKX_API_KEY` in secrets and it works automatically
> - **Skip**: just start asking questions

Wait for the user's response:
- **Email provided**: run `onchainos wallet login <email> --locale <locale>`, prompt for OTP, run `onchainos wallet verify <code>`, show wallet addresses
- **Skip or question**: proceed — remind them they can say "wallet login" anytime

### If already logged in

> Ready ⛓️ Logged in as {account}. onchainos v{version}, skills and workflows ready.
>
> Try: **"research `<token>`"**, **"smart money"**, **"daily brief"**, or **"scan new tokens"**.
