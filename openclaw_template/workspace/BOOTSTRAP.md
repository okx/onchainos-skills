# Bootstrap Protocol

**On the user's first message in every session, run ALL steps below before responding.** This file self-deletes after initial onboarding is complete.

## Gates

Run these two checks before anything else:

### Gate 1 — Bootstrap status

```bash
cat ~/.onchainos/bootstrap_status 2>/dev/null || echo "MISSING"
```

- Contains today's date (e.g. `2026-04-29 OK`) → **skip Steps 1–2**, but always run this PATH export before continuing:
  ```bash
  export PATH="$HOME/.local/bin:$PATH"
  ```
  Then go to Step 3.
- Missing or different date → **run all steps from Step 1**.

### Gate 2 — Welcome status

```bash
cat ~/.onchainos/welcome_shown 2>/dev/null || echo "MISSING"
```

- Contains today's date → **skip Step 4**.
- Missing or different date → **you MUST run Step 4 and write the flag**. Do not skip it.

## Step 1 — Verify installation

Check that the CLI binary, workflows, and skills exist:

```bash
ls ~/.local/bin/onchainos
ls ~/.onchainos/workflows/
ls ~/.onchainos/skills/
```

If **any** of these are missing, run the setup script to install everything:

```bash
curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/openclaw_template/setup.sh | sh
```

Then verify all three paths again before proceeding.

## Step 2 — Configure PATH and skills

**This step is mandatory. Do not skip it.**

1. Add the onchainos binary to PATH so it can be invoked by name:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

2. Verify PATH is set correctly:

```bash
onchainos --version
echo "PATH configured: $(which onchainos)"
```

If `onchainos` is still not found, the binary was not installed correctly. Re-run the setup script from Step 1.

3. Symlink the skills directory into the workspace so OpenClaw can discover them:

```bash
mkdir -p skills
ln -sf ~/.onchainos/skills/* skills/
```

4. Verify onchainos skills are loaded:

```bash
openclaw skills list
```

Check the output and confirm that onchainos skills are present (e.g. `okx-dex-token`, `okx-dex-swap`, `okx-agentic-wallet`, etc.). If onchainos skills do not appear:
- Verify symlinks exist: `ls -la skills/`
- Verify source is populated: `ls ~/.onchainos/skills/`
- Re-run the symlink command from sub-step 3 above
- If still missing, re-run setup from Step 1

**Do not proceed until onchainos skills are confirmed in `openclaw skills list`.**

Note: `setup.sh` writes `~/.onchainos/bootstrap_status` on success, so subsequent messages in the same session will skip Steps 1–2 via the Bootstrap Gate.

## Step 3 — Welcome

**This step is controlled by Gate 2.** If Gate 2 said MISSING or stale date, you MUST:
1. Output the **exact** welcome message below — do not paraphrase, summarise, or improvise your own version
2. Write the flag: `echo "$(date +%Y-%m-%d)" > ~/.onchainos/welcome_shown`

**Copy this message verbatim:**

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

## Step 4 — Login

Run `onchainos wallet status`. If not logged in, prompt the user:

> To get started, log in with your email — I'll send a verification code.

- **Email provided**: run `onchainos wallet login <email> --locale <locale>`, prompt for OTP, run `onchainos wallet verify <code>`, show wallet addresses
- **API Key**: if `OKX_API_KEY` is set in secrets, it works automatically
