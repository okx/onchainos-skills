---
name: okx-hackathon
description: "Register a Trading ASP agent for the OKX.AI trading hackathon (交易黑客松 / OKX.AI 交易黑客松). Use when the user wants to sign up / 报名 their Trading ASP agent for the hackathon, asks about hackathon registration requirements, or mentions '黑客松' / 'hackathon' together with an agent/ASP. Do NOT use for joining a normal trading competition or cup (that is `okx-growth-competition`) — see the disambiguation table below."
license: MIT
metadata:
  author: okx
  version: "1.0.0"
  homepage: "https://web3.okx.com"
---

# OKX.AI Trading Hackathon Registration

Register the user's **Trading ASP agent** for the OKX.AI trading hackathon. Wraps the standalone `onchainos hackathon` CLI command group / `hackathon_register` MCP tool.

## `hackathon register` vs `competition join` — do not conflate

They register **different subjects** against **different endpoints**:

| Signal in the request | Skill | Command | What is registered |
|---|---|---|---|
| "hackathon" / "黑客松" / "OKX.AI 交易黑客松" / "报名黑客松" / "register my ASP / agent" | **this skill** | `hackathon register` | the user's **Trading ASP agent** for the OKX.AI hackathon |
| "join / register for `<competition name>`" / a normal trading contest or cup | `okx-growth-competition` | `competition join` | the active **wallet account** for a standard trading competition |

**NEVER** run `competition join` for a hackathon request, or `hackathon register` for a plain "join this competition" request — they hit different backends and the wrong one fails or mis-registers.

## Pre-flight

> Read `../okx-agentic-wallet/_shared/preflight.md`. If missing, read `_shared/preflight.md`.

Wallet login is required. If not logged in, walk the user through the `okx-agentic-wallet` login flow (run `onchainos wallet login`), then resume at the step that failed.

## Flow

### Step 1 — Pick the Trading ASP agent

1. List the user's agents: MCP `agent_get_my_agents` (CLI: `onchainos agent get-my-agents`).
2. Present the agents by **name** and ask the user which one to register. **NEVER** show the numeric agent id in a user-facing message (internal id — keep it in the data layer to pass as `--agent-id`).
3. Before submitting, confirm the three ASP preconditions with the user (the backend is authoritative and rejects on failure — the skill only pre-confirms so the user is not surprised by a rejection):

```
To register "{agentName}" for the OKX.AI Trading Hackathon, please confirm it is:
1. a trading-type ASP,
2. offering a subscription service, and
3. offering a 3-day free trial.
Reply "confirm" to proceed.
```

`**SHOULD**:` proceed only after the user confirms (a mis-configured ASP is rejected server-side).

### Step 2 — Choose the competition account

Ask which account type to register, and include the funding reminder:

```
Which account should compete?
- web3 — your current wallet's X Layer address
- cefi — an OKX UID (you will provide the UID)
Either way, fund the account with ~300U-equivalent assets before trading begins.
```

- `web3` → `--account-type web3`. `--address` auto-resolves to the current wallet's X Layer address; do not ask for it.
- `cefi` → `--account-type cefi --uid <uid>`. Ask the user for their OKX UID. The wallet's X Layer `--address` is still submitted (auto-resolved), plus the `uid`.
- The ~300U funding requirement is a **reminder only** — the flow does NOT check the balance and does NOT gate on it.

### Step 3 — Submit

Call `hackathon_register` (MCP: `activity_id` defaults to "5", `chain_index` to "196", `address` auto-resolves; CLI: `onchainos hackathon register …`). See the CLI reference below for flags.

### Step 4 — Report the result

**On success** — output the fixed template (translate to the user's language; keep the chain name verbatim):

```
Registered "{agentName}" for the OKX.AI Trading Hackathon on X Layer with your {accountType} account. Good luck! Remember to fund the account with ~300U-equivalent assets before trading begins.
```

Identify the hackathon and agent by name, never by numeric id. The wallet address is public and MAY be shown.

**On failure** — the backend returns a single generic non-zero `code` + a descriptive English `msg` (there is no per-condition code). `**MUST**:` surface that backend `msg` to the user verbatim — it is the authoritative reason (e.g. the ASP is not trading-type, lacks a subscription, or lacks a 3-day trial). Do not paraphrase, translate the machine `msg`, or invent your own reason.

## CLI / MCP reference — `hackathon register`

Register the user's Trading ASP for the hackathon. **Requires wallet login.**

```
onchainos hackathon register --agent-id <id> --account-type <web3|cefi> [--activity-id <id>] [--address <addr>] [--chain-index <id>] [--uid <uid>]
```

**API**: `POST /priapi/v5/wallet/agentic/activity/registration`
**Extra header**: `OK-ACCESS-PROJECT: 4d156bf0c61130f2692d097ecb68dbe4`

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--agent-id` | Yes | — | Trading ASP agent id (from `agent get-my-agents`). |
| `--account-type` | Yes | — | `web3` or `cefi` (clap-validated; other values rejected). |
| `--activity-id` | No | `5` | Hackathon activity id. Default `5` is the current OKX.AI hackathon; override only for a future activity. |
| `--address` | No | wallet X Layer addr | EVM wallet address. Auto-resolved from the current wallet's X Layer address when omitted (both `web3` and `cefi`). |
| `--chain-index` | No | `196` | Chain id string. Always `196` (X Layer) for this hackathon. |
| `--uid` | Conditional | — | OKX UID. **Required when `--account-type cefi`** (CLI bails otherwise). Omitted for `web3`. |

MCP tool `hackathon_register` mirrors these (`activity_id` / `chain_index` / `address` optional with the same defaults).

**Request body** (built automatically): `{ "activityId", "agentId", "chainIndex", "address" }`; `"uid"` is added only for `cefi`.

**Output** (CLI wraps the bare `{code:"0", data:[]}` success into a confirmation object):
```json
{ "registered": true, "activityId": "5", "agentId": "...", "accountType": "web3", "chainIndex": "196", "address": "0x..." }
```
For `cefi`, the confirmation additionally echoes `"uid"`.

**Errors:**
- `--uid is required for CeFi account registration` → CLI validation; collect the UID and retry.
- invalid `--address` for the chain → address failed `validate_address_for_chain`; fix and retry.
- `not logged in` → run `onchainos wallet login`, then retry.
- backend non-zero `code` (ASP not trading-type / no subscription / no 3-day trial, or other) → surface the backend `msg` verbatim (see Step 4).

## Output Rules

**Never include any internal id (agent id, activity id) in a message produced for the user — under ANY circumstance, in ANY format.** Identify the agent and hackathon EXCLUSIVELY by name.

## Security

- **Non-spending**: registration receives value, so there is no confirm-to-spend (`CliConfirming`) gate.
- The JWT `access_token` is injected by the client layer from the keychain; it is **NEVER** logged, printed, or placed in a flag — leaking it would let an attacker act as the user.
- No new secrets are created or stored; the flow reads the existing wallet session.
