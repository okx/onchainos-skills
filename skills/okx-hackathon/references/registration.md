# OKX.AI Trading Hackathon Registration — Flow & CLI Reference (`hackathon register`)

> Scope: the step-by-step registration flow and full CLI/MCP reference for `hackathon register`. Global rules (disambiguation vs `competition join`, pre-flight, output rules, security) live in `../SKILL.md` — read that first.

## Flow

Wallet login is required. If not logged in, route via `../SKILL.md` → Pre-flight, then resume at the step that failed.

### Step 1 — Pick the Trading ASP agent

1. List the user's agents: MCP `agent_get_my_agents` (CLI: `onchainos agent get-my-agents --role asp --page-size 20`). `--role asp` filters to Trading ASP agents only; `--page-size 20` avoids the backend's default page size of 5 silently truncating the list (paginate further with `--page` if the user has more than 20 ASPs — do not stop at page 1 and guess).
2. Present the choices as a numbered list, `0` first for creating a new ASP, then one line per existing ASP showing its **name and agent id** (the id is shown here only, to disambiguate ASPs that share a name — see `../SKILL.md` Output Rules):

```
Which Trading ASP would you like to register for the OKX.AI Trading Hackathon?

0. Create a new ASP
1. <name> (ID: <agent_id>)
2. <name> (ID: <agent_id>)
...

Reply with a number.
```

3. If the user picks `0`, hand off to ASP creation/registration (`okx-ai` skill) instead of continuing this flow.
4. Otherwise resolve the reply to the selected `agent_id` and drop the id from every later message — from here on identify the ASP **by name only** (`../SKILL.md` Output Rules).
   - If the user's original request already named an ASP (or gave an account type / UID) upfront, still run this list and match it against the name to get a real `agent_id` — never fabricate or guess an id. If the name matches more than one ASP, ask which one (now easy: show the disambiguating options). Do not skip straight to Step 5's confirmation on a one-shot request; still show it explicitly.
5. Before submitting, confirm the three ASP preconditions with the user (the backend is authoritative and rejects on failure — the skill only pre-confirms so the user is not surprised by a rejection). Keep the ASP's name in the surrounding sentence, not inside the checklist:

```
Before I submit "<name>", please confirm it:

  ✓ is a trading-type ASP
  ✓ offers a subscription service
  ✓ offers a 3-day free trial

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

Call `hackathon_register` (MCP: `address` auto-resolves when omitted; `activity_id`/`chain_index` are fixed internally, not caller-supplied; CLI: `onchainos hackathon register …`). See the CLI reference below for flags.

### Step 4 — Report the result

**On success** — output the fixed template (translate to the user's language; keep the chain name verbatim):

```
Registered "{agentName}" for the OKX.AI Trading Hackathon on X Layer with your {accountType} account. Good luck! Remember to fund the account with ~300U-equivalent assets before trading begins.
```

Identify the hackathon and agent by name, never by numeric id (`../SKILL.md` Output Rules). The wallet address is public and MAY be shown.

**On failure** — two different shapes, do not conflate them:
- **Backend business rejection**: `{ok:false, error:<msg>}` with a descriptive English `msg` from the registration endpoint itself (no per-condition code, single generic non-zero `code`). `**MUST**:` surface that `msg` to the user verbatim — it is the authoritative reason (e.g. the ASP is not trading-type, lacks a subscription, or lacks a 3-day trial). Do not paraphrase, translate the machine `msg`, or invent your own reason.
- **Transport/HTTP-layer failure**: connection error, timeout, or an HTTP status (404/5xx) instead of a normal API response body. This is NOT a precondition rejection — do not tell the user their ASP failed the trading-type/subscription/trial checks. Tell the user the hackathon registration service is currently unavailable and suggest retrying shortly; do not guess a business reason.

## CLI / MCP reference — `hackathon register`

Register the user's Trading ASP for the hackathon. **Requires wallet login.**

```
onchainos hackathon register --agent-id <id> --account-type <web3|cefi> [--address <addr>] [--uid <uid>]
```

**API**: `POST /priapi/v5/wallet/agentic/activity/registration`
**Extra header**: `OK-ACCESS-PROJECT: 4d156bf0c61130f2692d097ecb68dbe4`

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--agent-id` | Yes | — | Trading ASP agent id (from `agent get-my-agents`). |
| `--account-type` | Yes | — | `web3` or `cefi` (clap-validated; other values rejected). |
| `--address` | No | wallet X Layer addr | EVM wallet address. Auto-resolved from the current wallet's X Layer address when omitted (both `web3` and `cefi`). |
| `--uid` | Conditional | — | OKX UID. **Required when `--account-type cefi`** (CLI bails otherwise). Omitted for `web3`. |

The activity id (`5`) and chain index (`196`, X Layer) are **fixed internally** — there is no flag/param to set or override either; they are not exposed to the CLI user or the MCP-calling AI.

MCP tool `hackathon_register` mirrors these (`address` optional, same auto-resolve behavior; no `activity_id`/`chain_index` params).

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
- connection error / non-2xx HTTP status (e.g. `API error (code=404): Not Found`, 5xx, timeout) → transport-layer failure, NOT a precondition rejection; see Step 4's "Transport/HTTP-layer failure" handling — tell the user the service is unavailable, don't blame the ASP's configuration.
