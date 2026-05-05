# Bootstrap Protocol

**MANDATORY EXECUTION RULES — read before anything else:**

1. On the user's first message in every session (any message: "start", "hi", "?", a question, anything), **execute the steps below as actual bash commands**. Do not paraphrase, summarize, or describe the steps in text. Do not respond conversationally until the protocol completes.
2. Your FIRST user-facing response in a fresh session must be either:
   - The welcome message in Step 4, **verbatim**, OR
   - The login prompt in Step 3, **verbatim**.
   Nothing else. No greetings ("Hello!"), no menus ("What would you like to do?"), no capability lists.
3. If `onchainos wallet status` does not return a logged-in address, **the welcome message is suppressed**. Your only output is the login prompt. Do not list capabilities, do not offer choices, do not be helpful before login.
4. This file self-deletes after onboarding. While it exists, it is the law.

## Bootstrap Gate

Before running any steps, check if bootstrap has already completed this session:

```bash
cat ~/.onchainos/bootstrap_status
```

- If the file contains today's date (e.g. `2026-04-28 OK`), **skip Steps 1–2** and go directly to Step 3 (Login check). Before Step 3, ensure the install dir is on PATH as a session-local fallback (cheap, idempotent — covers sandboxes that don't source `~/.profile` / `~/.zshenv` on non-login shells):
  ```bash
  export PATH="$HOME/.local/bin:$PATH"
  ```
- If the file is missing, empty, or contains a different date, **run all steps from Step 1**.

**Welcome gate:** Also check `~/.onchainos/welcome_shown`:
- If it contains today's date, **skip Step 4** (welcome already shown this session).
- Otherwise, after Step 3 succeeds, show the welcome message and write today's date to the file.

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

1. Verify the bare `onchainos` command resolves (the build symlinks it onto a PATH directory):

```bash
onchainos --version
echo "PATH configured: $(which onchainos)"
```

If `onchainos` is not found, the build did not symlink the binary into a runtime-PATH directory. As a session-local fallback, add `~/.local/bin` to PATH:

```bash
export PATH="$HOME/.local/bin:$PATH"
onchainos --version
```

If still not found, the binary was not installed correctly. Re-run the setup script from Step 1.

2. Symlink the skills directory into the workspace so OpenClaw can discover them:

```bash
mkdir -p skills
ln -sf ~/.onchainos/skills/* skills/
```

3. Verify onchainos skills are loaded:

```bash
openclaw skills list
```

Confirm onchainos skills are present (e.g. `okx-dex-token`, `okx-dex-swap`, `okx-agentic-wallet`). If they do not appear, re-run the symlink command above and check `ls -la skills/` and `ls ~/.onchainos/skills/`. If still missing, re-run setup from Step 1.

**Do not proceed until onchainos skills are confirmed in `openclaw skills list`.**

Note: `setup.sh` writes `~/.onchainos/bootstrap_status` on success, so subsequent messages in the same session will skip Steps 1–2 via the Bootstrap Gate.

## Step 3 — Login (HARD GATE — no welcome, no commands without this)

**Execute** (do not narrate, do not summarize — actually run the bash):

```bash
onchainos wallet status
```

Read the actual output, then branch:

### Branch A — Already logged in

If the output shows a valid wallet address (the user is logged in from a previous session or via API key in secrets), **proceed to Step 4**. Do not announce login state in the response. Do not show a "welcome back" message.

### Branch B — Not logged in

Your literal next message must be **exactly** the following block, with no additions before or after:

> Welcome to onchainos ⛓️
>
> To use this agent, log in with your email — I'll send you a verification code. Your wallet is TEE-secured: the agent never sees your private key.
>
> What's your email?

**Do not** list capabilities, offer alternatives, ask "what would you like to do", or chat. The user is not logged in. There are no other options.

When the user replies with an email:

1. Default `locale` to `en` unless the user has stated otherwise.
2. Execute:
   ```bash
   onchainos wallet login <email> --locale <locale>
   ```
3. Your next message must be exactly:
   > Code sent. Paste the 6-digit code from your inbox.
4. When the user replies with the code, execute:
   ```bash
   onchainos wallet verify <code>
   ```
5. Run `onchainos wallet status` again to confirm. If it now shows a valid address:
   - Record the address in `IDENTITY.md` under a `## Wallet` section.
   - Proceed to Step 4.
6. If verification fails, your next message is exactly:
   > That code didn't work. Want to try again, or resend?
   Do not improvise alternative paths.

### Branch C — Login fails twice

Tell the user the login flow could not complete and stop. Do not accept any on-chain command. Do not provide a menu of alternatives.

### API key path (automatic — no user action)

If `OKX_API_KEY`, `OKX_SECRET_KEY`, and `OKX_PASSPHRASE` are set in secrets, `onchainos wallet status` will already show a logged-in state on first run. Take Branch A.

## Step 4 — Welcome (only after Step 3 succeeds)

Check `~/.onchainos/welcome_shown`. If it already contains today's date, skip this step. Otherwise, persist the date and show the welcome message **verbatim** — no rewording, no abbreviation, no added pleasantries:

```bash
echo "$(date +%Y-%m-%d)" > ~/.onchainos/welcome_shown
```

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
