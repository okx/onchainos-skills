# OKX.AI Trading Hackathon Registration — Flow & CLI Reference (`hackathon register`)

> Scope: the step-by-step registration flow and CLI/MCP reference for `hackathon register`. Global rules (wrong-skill guard, pre-flight, output rules) live in `../SKILL.md` — read that first.

## Flow

Wallet login is required. If not logged in, route via `../SKILL.md` → Pre-flight, then resume at the step that failed.

### Step 1 — Pick the Trading ASP agent

1. List the user's agents: `onchainos agent get-my-agents --page-size 20` — **no** `--role` filter (the per-row `roleLabel` gives the total-vs-ASP split from one call), and there is no MCP tool for this call, so always use the CLI text command. `--page-size 20` avoids the backend's default page size of 5 silently truncating the list; if the user has more than 20 agents, paginate with `--page` rather than stopping at page 1 and guessing.
2. Split the rows client-side by role: a row is ASP-eligible **only if `roleLabel` is exactly `"ASP"`**. Any other value (`"User"`, `"Evaluator"`, anything else) or a missing/absent `roleLabel` → **not eligible**. Never default a row into the ASP bucket. Then present the summary line, followed by the ASP-only numbered list — `0` first for creating a new ASP, then one line per existing ASP with its **name and agent id** (shown here only, to disambiguate ASPs sharing a name — see `../SKILL.md` Output Rules):

```
You have <N> agents in total, <M> of which are Trading ASPs (the other <N-M> are Evaluator / User identities and cannot register).

Which Trading ASP would you like to register for the OKX.AI Trading Hackathon?

0. Create a new ASP
1. <name> (ID: <agent_id>)
2. <name> (ID: <agent_id>)
...

Reply with a number.
```

Translate to the user's language; keep the numbered structure. Do **not** add a guessed-eligibility hint next to any name (e.g. "this one looks like it qualifies") — the three preconditions are backend-checked and not inferable from a name.

3. If the user picks `0`, hand off to ASP creation/registration (`okx-ai` skill) instead of continuing this flow.
4. Otherwise resolve the reply to the selected `agent_id`, and from here on identify the ASP **by name only** (`../SKILL.md` Output Rules).
   - If the user's original request already named an ASP (or gave an account type / UID) upfront, still run this list and match it against the name to get a real `agent_id` — never fabricate or guess an id. If the name matches more than one ASP, ask which one. Do not skip straight to the confirmation on a one-shot request; still show the list explicitly.
5. Before submitting, confirm the three ASP preconditions with the user (the backend is authoritative and rejects on failure — this pre-confirmation only avoids surprising the user with a rejection). Keep the ASP's name in the surrounding sentence, not inside the checklist:

```
Before I submit "<name>", please confirm it:

  ✓ is a trading-type ASP
  ✓ offers a subscription service
  ✓ offers a 3-day free trial

1. Confirm and submit

Reply 1 to proceed.
```

`**SHOULD**:` proceed only after the user replies `1`.

### Step 2 — Choose the competition account

Ask which account type to register, and include the funding reminder:

```
Which account should compete?

1. web3 — your current wallet's X Layer address
2. cefi — an OKX UID (you will provide the UID)

Either way, fund the account with >300U-equivalent assets before trading begins.

Reply with a number.
```

- Reply `1` (web3) → `--account-type web3`. `--address` auto-resolves to the current wallet's X Layer address; do not ask for it.
- Reply `2` (cefi) → `--account-type cefi --uid <uid>`. Ask the user for their OKX UID. The X Layer `--address` is still submitted (auto-resolved), plus the `uid`.
- The >300U funding requirement is a **reminder only** — the flow does NOT check the balance and does NOT gate on it.

### Step 3 — Submit

Call `hackathon_register` (MCP) or `onchainos hackathon register …` (CLI). See the reference below for flags.

### Step 4 — Report the result

**On success** — output the fixed template (translate to the user's language; keep the chain name verbatim):

```
Registered "{agentName}" for the OKX.AI Trading Hackathon on X Layer with your {accountType} account. Good luck! Remember to fund the account with >300U-equivalent assets before trading begins.
```

Identify the hackathon and agent by name, never by an internal id (`../SKILL.md` Output Rules). The wallet address is public and MAY be shown.

**On failure** — two different shapes, do not conflate them:
- **Backend business rejection**: `{ok:false, error:<msg>}` with a descriptive English `msg` from the registration endpoint (single generic non-zero `code`, no per-condition code). `**MUST**:` surface that `msg` verbatim — it is the authoritative reason (e.g. the ASP is not trading-type, lacks a subscription, or lacks a 3-day trial). Do not paraphrase, translate the machine `msg`, or invent your own reason.
- **Transport/HTTP-layer failure**: connection error, timeout, or an HTTP status (404/5xx) instead of a normal API response body. This is NOT a precondition rejection — do not tell the user their ASP failed the trading-type/subscription/trial checks. Say the hackathon registration service is currently unavailable and suggest retrying shortly.

## CLI / MCP reference — `hackathon register`

**Requires wallet login.**

```
onchainos hackathon register --agent-id <id> --account-type <web3|cefi> [--address <addr>] [--uid <uid>]
```

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--agent-id` | Yes | — | Trading ASP agent id (from `agent get-my-agents`). |
| `--account-type` | Yes | — | `web3` or `cefi` (clap-validated; other values rejected). |
| `--address` | No | wallet X Layer addr | EVM wallet address. Auto-resolved from the current wallet's X Layer address when omitted (both account types). |
| `--uid` | Conditional | — | OKX UID. **Required when `--account-type cefi`** (CLI bails otherwise). Omitted for `web3`. |

The activity id and chain index (X Layer, `196`) are **fixed internally** — no flag or param sets, overrides, or returns either. MCP tool `hackathon_register` mirrors the flags above (same `address` auto-resolve; no `activity_id` / `chain_index` params).

Success returns `{ "registered": true, "agentId", "accountType", "chainIndex", "address" }`, plus `"uid"` for `cefi`.

**CLI-side errors** (backend rejections and transport failures are handled in Step 4):
- `--uid is required for CeFi account registration` → collect the UID and retry.
- invalid `--address` for the chain → failed `validate_address_for_chain`; fix and retry.
- `not logged in` → run `onchainos wallet login`, then retry.
