---
name: okx-how-to-play
description: "Use this skill when the user asks open-ended questions about Onchain OS itself or how to start using it. Triggers include: 'what is onchainos', 'what is onchain os', 'what's onchain os', 'what can it do', 'what can I do here', 'what does this do', 'how do I use this', 'how do I play', 'how to use onchainos', 'how to play onchainos', 'how does this work', 'how do I start', 'getting started', 'how do I get started', 'tutorial', 'onboarding', 'first time', 'I just installed', 'I just installed it now what', 'now what', 'what do I do now', 'where do I start', 'who are you', 'what are you', 'introduce yourself', 'introduction', 'introduce onchainos', 'tell me about onchainos', 'I'm new', 'I'm new here'. This is the entry router that checks login / user-type / balance state and funnels the user into the right DApp workflow (Polymarket / smart-money signals / new-token screening / daily brief / custom). Do NOT use when the user already specifies a concrete intent — e.g. wallet balance, swap X for Y, transfer, login only, token search, price check — route those to the matching skill (okx-agentic-wallet, okx-dex-swap, okx-wallet-portfolio, okx-dex-market, okx-dex-token, etc.) instead."
license: MIT
metadata:
  author: okx
  version: "2.6.0"
  homepage: "https://web3.okx.com"
---

# Onchain OS — How to Play (Entry Router)

The first-time / "I don't know what to do" entry point. Routes the user from a blank prompt into a concrete DApp workflow in ≤ 3 turns.

## Instruction Priority

Tagged blocks indicate rule severity (higher wins on conflict):

1. **`<NEVER>`** — Absolute prohibition.
2. **`<MUST>`** — Mandatory step. Skipping breaks the flow.
3. **`<SHOULD>`** — Best practice.

## Pre-flight Checks

<MUST>
> Read `../okx-agentic-wallet/_shared/preflight.md`. If that file does not exist, read `_shared/preflight.md` instead.
</MUST>

## Trigger Criteria

<MUST>
Only trigger this skill when the user message is **open-ended / guidance-seeking**. Positive examples:

- "how do I use this / what can I do / what is this / getting started"
- "I just installed it, now what?"
- "tutorial / onboarding / first time / where do I start"

Negative examples (use the matching skill instead, **not** this one):

- "check my balance" → `okx-agentic-wallet` / `okx-wallet-portfolio`
- "swap 0.1 ETH for USDC" → `okx-dex-swap`
- "what's the price of BTC" → `okx-dex-market`
- "login" alone → `okx-agentic-wallet`
- "search for PEPE token" → `okx-dex-token`
</MUST>

## Free-Zone Source of Truth — "What is / How to use" Questions

<MUST>
When the user's opener is asking **what Onchain OS is** or **how to use it** (e.g. "what is onchain os", "what does it do", "how do I use this", "how do I play", "introduce yourself", "what can it do"), the agent's free-zone answer **must be grounded in fresh content fetched at runtime** from:

```
https://web3.okx.com/onchainos/dev-docs/home/what-is-onchainos
```

Use the agent's `WebFetch` tool (or environment-equivalent) to retrieve this URL **before composing the free-zone answer**. Summarize / paraphrase the fetched content in the user's language; do NOT rely on training-data knowledge of what Onchain OS is, since the product evolves.

If the fetch fails (network error, 4xx/5xx), fall back to a one-sentence neutral acknowledgement (e.g. "Onchain OS is OKX's on-chain agentic OS") and continue the flow — do not block on the fetch.

This rule applies **only** to the "what is / how to use" opener. For other openers (e.g. "I just installed it, now what"), skip the fetch — go straight to **Status Check**.
</MUST>

## Authoring Pattern — Free Zone vs Fixed Zone

Most user-facing copy in this flow is split into two parts:

- **Free zone** — the agent answers the user's actual question or acknowledgement first, in 1–5 sentences, language-matched and contextually woven. No fixed copy. The user shouldn't feel like they hit a script.
- **Fixed zone** — the verbatim copy block (login options, welcome menu, etc.) follows immediately, with a natural segue from the free zone.

This applies to: **Login Method Choice**, **API Key Login** (Step 1 heads-up), and **Welcome — Zero Balance** (free-zone preamble before the welcome banner). **Terminal Ack — Active User** is free-zone-only — agent answers naturally, no fixed copy.

## Key Concept — `isNew` Availability

<MUST>
The `isNew` field is **only** available in the response body of a *fresh* `wallet login` or `wallet verify` call made in the current session. It is NOT persisted and is NOT re-readable once the user is already authenticated.

This splits the flow into two paths:

- **Path A — user starts logged out.** We run `wallet login` / `wallet verify` this turn, so we get `isNew` from the response and branch on it.
- **Path B — user starts already logged in.** We did not run a fresh login, so `isNew` is unavailable. Fall back to `wallet balance` to pick the branch.

Never assume `isNew` is available when `wallet status` returns `loggedIn: true` at entry.
</MUST>

## Flow Overview

```
User: "how do I use this" / "what can I do" / "getting started"
          │
          ▼
   Status Check  (run `onchainos wallet status`)
          │
   ┌──────┴───────────────┐
   ▼                       ▼
[Path A — Logged Out]  [Path B — Logged In]
Login Method Choice    Balance Branch
   │                       │
   ├──> Email Login        ├──> bal=0 → Welcome (Zero Balance)
   └──> API Key Login      └──> bal>0 → Terminal Ack (Active User)
        │
        ▼
   Branch on isNew  (read from login response)
   │
   ├──> isNew=true             → Welcome (Zero Balance)
   └──> isNew=false → check balance:
                       bal=0   → Welcome (Zero Balance)
                       bal>0   → Terminal Ack (Active User)

   Welcome (Zero Balance)
          │
          ▼
   Quick-Start Menu Handling  (user picks 1–5)
          │
          ▼
   Route to Workflow  (load workflow file or hand off to skill)
```

---

## Status Check

<MUST>
Run `onchainos wallet status` **before** showing any login or welcome text. Use the `loggedIn` field to branch.
</MUST>

```
onchainos wallet status
```

- `loggedIn: false` → **Path A — Logged Out** (start with **Login Method Choice**)
- `loggedIn: true`  → **Path B — Logged In** (start with **Balance Branch**)

---

# Path A — Logged Out at Entry

User wasn't authenticated when the skill triggered. We run a fresh login, then branch on `isNew` from the response.

## Login Method Choice

**Free zone (1–5 sentences, agent's own words, language-matched):** answer whatever the user actually asked / acknowledged in their opener. Briefly explain what Onchain OS is if relevant. Then segue naturally into "let's get you logged in".

**Fixed zone — output verbatim** (translate to the user's language at runtime; the canonical text below is the source of truth):

> 1. 📧 Email (recommended)
> 2. 🔑 API Key

If the user replies `1` or "email" → **Email Login**.
If the user replies `2` or "API Key" → **API Key Login**.

## Email Login

Handled by `okx-agentic-wallet` skill's Authentication section. Steps:

1. Ask for email → `onchainos wallet login <email> --locale <locale>`
2. Ask for OTP code → `onchainos wallet verify <code>`
3. Read `isNew` from the `verify` response → go to **Branch on isNew**.

## API Key Login

Two steps total: (1) one-time heads-up so the user knows what env vars to set and where to get them, (2) run `onchainos wallet login` once they confirm.

### Step 1 — Heads-up (one-shot, fixed zone)

**Free zone (1–5 sentences):** answer whatever the user actually asked / acknowledged in their opener. Then segue naturally into the heads-up.

**Fixed zone — output verbatim** (translate to the user's language at runtime; canonical English source-of-truth below):

> Configure your API Key environment variables before logging in. You'll need three values:
> 1. `OKX_API_KEY` — API Key
> 2. `OKX_SECRET_KEY` — Secret Key
> 3. `OKX_PASSPHRASE` — Passphrase
>
> You can find these at https://web3.okx.com/onchainos/dev-portal.
>
> ⚠️ Do not paste credentials into the chat — follow the dev-portal instructions and set them locally.

Then **stop and wait** for the user to confirm they're ready (e.g. "done / ok / ready").

### Step 2 — Login

Once the user confirms, run:

```
onchainos wallet login
```

Read `isNew` from the response → hand off to **Branch on isNew**. On login failure, surface the error and ask the user to verify their env vars (do NOT re-show the heads-up — they already saw it).

<NEVER>
- Do NOT accept API Key / Secret / Passphrase inline in chat. If the user pastes credentials in chat: do NOT echo, do NOT use the values, ask them to delete the message + rotate the keys + set the env vars locally instead.
- Do NOT walk the user through generating keys, opening URLs, creating `.env` files, editing `.gitignore`, or any other multi-step setup. The heads-up is one-shot — they handle their own local setup.
- Do NOT ask the user to paste the browser URL or any callback back to the CLI. The dev-portal is read-only.
</NEVER>

## Branch on isNew

Reached only on Path A, after a fresh `wallet login` / `wallet verify` call. The response body contains:

```json
{
  "ok": true,
  "data": {
    "accountId": "b4ec13b2-...",
    "accountName": "Account 1",
    "isNew": false
  }
}
```

- `isNew: true` → **Welcome — Zero Balance** (new wallets have $0 by definition)
- `isNew: false` → run `onchainos wallet balance`:
  - total USD value **> 0** → **Terminal Ack — Active User**
  - total USD value **= 0** → **Welcome — Zero Balance**

---

# Path B — Already Logged In at Entry

User was already authenticated when the skill triggered. No fresh login response, so `isNew` is unavailable.

## Balance Branch

<MUST>
`isNew` is NOT available in this path (no fresh login call was made this session). Use `wallet balance` to branch.
</MUST>

Run `onchainos wallet balance` and read the total USD value:

- total USD value **> 0** → **Terminal Ack — Active User**
- total USD value **= 0** → **Welcome — Zero Balance**

---

# Welcome — Zero Balance

Reached by:

- **Path A** with `isNew=true`
- **Path A** with `isNew=false` and balance = 0
- **Path B** with balance = 0

All three states share the same onboarding need (user has no holdings, agent should orient them and offer next moves), so the same welcome applies.

<MUST>
Output the verbatim copy from `references/welcome-zero-balance.md`. Pull `{evm_address}` and `{solana_address}` from `onchainos wallet addresses` (already returned by `wallet balance`). Never fabricate addresses.
</MUST>

The welcome includes a flat quick-start menu. The number of options depends on `polymarket_available` from the geoblock check (see `references/welcome-zero-balance.md` Step 2): **5 items** when allowed, **4 items** (no Polymarket, renumbered 1–4) when blocked / unable to confirm allowed (fail-closed).

## Quick-Start Menu Handling

When the user replies:

- A numbered option → go directly to **Route to Workflow** using the variant that was rendered (Variant A: 1–5; Variant B: 1–4).
- Free-form text instead of a number → answer naturally in free zone, then route via the free-form fallback table in **Route to Workflow**.

## Route to Workflow

Quick-start picks map to workflow files at `~/.onchainos/workflows/<file>.md`. When a workflow file is the target, the agent **loads that file** and follows its instructions. Use the mapping that matches the menu variant rendered for this user.

### Variant A — `polymarket_available = true` (5 picks)

| Pick | Description | Target |
|---|---|---|
| 1 | Polymarket Top 3 picks → analyze → fund → order | Polymarket workflow (TBD — placeholder; fall back to `okx-dex-market` + free-play analysis until the workflow ships) |
| 2 | "Find me the best strategy" | Strategy advisor (TBD — placeholder; free-play with portfolio + risk profile until shipped) |
| 3 | Smart money — what whales are buying | `~/.onchainos/workflows/smart-money-signals.md` |
| 4 | Scan new on-chain tokens | `~/.onchainos/workflows/new-token-screening.md` |
| 5 | Generate market daily brief | `~/.onchainos/workflows/daily-brief.md` |

### Variant B — `polymarket_available = false` (4 picks, no Polymarket)

| Pick | Description | Target |
|---|---|---|
| 1 | "Find me the best strategy" | Strategy advisor (TBD — placeholder; free-play with portfolio + risk profile until shipped) |
| 2 | Smart money — what whales are buying | `~/.onchainos/workflows/smart-money-signals.md` |
| 3 | Scan new on-chain tokens | `~/.onchainos/workflows/new-token-screening.md` |
| 4 | Generate market daily brief | `~/.onchainos/workflows/daily-brief.md` |

**Free-form fallback** — if the user types something other than a numbered pick:

| Intent | Route to |
|---|---|
| meme sniping / pump.fun / new launches | `okx-dex-trenches` |
| follow smart money / KOL / whale | `okx-dex-signal` (or load `smart-money-signals.md`) |
| bridge across chains | `okx-dex-bridge` |
| yield / earn / stake / DeFi | `okx-defi-invest` |
| is this token safe / approvals | `okx-security` |
| swap / buy / sell | `okx-dex-swap` |
| my holdings / portfolio | `okx-wallet-portfolio` |

<SHOULD>
If the user picks multiple options at once, execute them in order and bookmark unused picks ("we'll come back to 4 after this").
</SHOULD>

---

# Terminal Ack — Active User (balance > 0)

Reached when:

- **Path A** finishes with `isNew=false` and balance > 0
- **Path B** has balance > 0

<MUST>
No fixed copy. Agent free-plays a 1–2 sentence acknowledgement that mentions the user is already set up (balance available), then **stops and waits for concrete intent**. Do NOT show the welcome menu. Do NOT volunteer the quick-start options. The user is an active wallet holder — interrupting them with onboarding noise is wrong.
</MUST>

Examples of acceptable free-play (the agent renders in the user's language at runtime):

- "You're already up and running with $1,500 in the wallet — just tell me what you want to do."
- "Your account has $42.37 — already active. What do you want to tackle?"

Then stop.

---

## Acceptance Criteria

1. **Correct trigger** — open-ended guidance queries pull up this skill; concrete-intent queries do NOT (they hit the matching skill).
2. **Branches resolve correctly** — Path A (Logged Out → Login → Branch on isNew × balance) and Path B (Logged In → Balance Branch) both land on the right state.
3. **Free zone present where required** — **Login Method Choice** and **API Key Login** open with 1–5 sentence agent-authored copy before the fixed zone, not a cold script drop.
4. **Quick-start menu loads workflows** — numbered picks correctly resolve to the workflow file at `~/.onchainos/workflows/`, using the variant table that matches the rendered menu (Variant A or B).
5. **Polymarket geoblock honored** — when `https://polymarket.com/api/geoblock` indicates blocked or the check fails, Polymarket is hidden from the menu entirely (Variant B is rendered with picks 1–4); the agent never explains *why* Polymarket is hidden.
6. **Turn budget** — end-to-end ≤ 3 turns for new user, ≤ 2 turns for returning user with balance=0, ≤ 1 turn (Terminal Ack) for active user with balance>0.

## Notes / Non-obvious

- **AK login also returns `isNew`** — the CLI docs mention it only for email flow, but in practice AK silent login carries the same field. **Branch on isNew** handles both identically.
- **Logout is local-only state wipe** (keyring / session.json / wallets.json / cache.json / balance_cache.json), no HTTP; the refresh_token stays valid server-side — high-sensitivity scenarios must trigger a server-side revoke.
- **AK reuses the TEE-bound account_id** — local `wallets.json` is just a cache; after a reinstall, AK login returns the same `accountId`, which is also the semantic source of `isNew=false`.
- **Path B exists because `isNew` is not re-readable** — once a user is already authenticated at session entry, there's no fresh login response to inspect. Balance is the only signal available.
- **API Key setup is the user's responsibility** — the agent only emits a one-shot heads-up listing the 3 env vars + dev-portal URL + the "don't paste in chat" warning, then waits for the user to confirm and runs `onchainos wallet login`. No `.env`/`.gitignore` automation, no multi-phase walkthrough.
- **Polymarket is geoblocked in some jurisdictions (e.g. US)** — before showing the welcome menu, the agent probes `https://polymarket.com/api/geoblock` via Bash `curl` (NOT `WebFetch` — must use the user's local IP) and renders Variant B (no Polymarket) on any non-confirmed-allowed outcome. The decision is silent — never surface region/IP/blocked info to the user.
- **Workflow files are runtime resources** — at install time they live at `~/.onchainos/workflows/`; in this repo's source they're under `workflows/`. **Route to Workflow** references the runtime path because that's where the agent loads them.
