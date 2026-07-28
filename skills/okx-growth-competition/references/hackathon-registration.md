# OKX.AI Trading Hackathon Registration (`competition register`)

> Scope: register the user's **Trading ASP agent** for the OKX.AI trading hackathon (交易黑客松). Global rules in `../SKILL.md`. This is a DIFFERENT flow from `competition join` — see the disambiguation below before doing anything.

## `join` vs `register` — do not conflate

Both live under `competition`, but they register **different subjects** against **different endpoints**:

| Signal in the request | Flow | Command | What is registered |
|---|---|---|---|
| "hackathon" / "黑客松" / "OKX.AI 交易黑客松" / "报名黑客松" / "register my ASP / agent" | **this file** | `competition register` | the user's **Trading ASP agent** for the OKX.AI hackathon |
| "join / register for <competition name>" / a normal trading contest or cup | `participation.md` (Step 3) | `competition join` | the active **wallet account** for a standard trading competition |

**NEVER** run `competition join` for a hackathon request, or `competition register` for a plain "join this competition" request — they hit different backends and the wrong one fails or mis-registers.

## Flow

Wallet login is required. If not logged in, route via `../SKILL.md` → Pre-flight (Cross-skill routing), then resume at the step that failed.

### Step 1 — Pick the Trading ASP agent

1. List the user's agents: MCP `agent_get_my_agents` (CLI: `onchainos agent get-my-agents`).
2. Present the agents by **name** and ask the user which one to register. **NEVER** show the numeric agent id in a user-facing message (internal id — see `../SKILL.md` Output Rules); keep it in the data layer to pass as `--agent-id`.
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

Call `competition_register` (MCP: `activity_id` defaults to "5", `chain_index` to "196", `address` auto-resolves; CLI: `onchainos competition register …`). See the CLI reference below for flags.

### Step 4 — Report the result

**On success** — output the fixed template (translate to the user's language; keep the chain name verbatim):

```
Registered "{agentName}" for the OKX.AI Trading Hackathon on X Layer with your {accountType} account. Good luck! Remember to fund the account with ~300U-equivalent assets before trading begins.
```

Identify the hackathon and agent by name, never by numeric id (`../SKILL.md` Output Rules). The wallet address is public and MAY be shown.

**On failure** — the backend returns a single generic non-zero `code` + a descriptive English `msg` (there is no per-condition code). `**MUST**:` surface that backend `msg` to the user verbatim — it is the authoritative reason (e.g. the ASP is not trading-type, lacks a subscription, or lacks a 3-day trial). Do not paraphrase, translate the machine `msg`, or invent your own reason.

## CLI / MCP reference — `competition register`

Register the user's Trading ASP for the hackathon. **Requires wallet login.**

```
onchainos competition register --agent-id <id> --account-type <web3|cefi> [--activity-id <id>] [--address <addr>] [--chain-index <id>] [--uid <uid>]
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

MCP tool `competition_register` mirrors these (`activity_id` / `chain_index` / `address` optional with the same defaults). Unlike `competition_join` / `competition_claim`, it takes `activity_id` directly, not `activity_name`.

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

## Security

- **Non-spending**: registration receives value, so there is no confirm-to-spend (`CliConfirming`) gate — same as `competition claim`.
- The JWT `access_token` is injected by the client layer from the keychain; it is **NEVER** logged, printed, or placed in a flag — leaking it would let an attacker act as the user.
- No new secrets are created or stored; the flow reads the existing wallet session.
