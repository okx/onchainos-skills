# OKX.AI Trading Hackathon Registration — Flow & CLI Reference (`hackathon register`)

> Scope: the step-by-step registration flow and CLI/MCP reference for `hackathon register`. Global rules (wrong-skill guard, pre-flight, output rules) live in `../SKILL.md` — read that first.

## Flow

Wallet login is required. If not logged in, route via `../SKILL.md` → Pre-flight, then resume at the step that failed.

### Step 1 — Pick the Trading ASP agent

1. List the user's agents (CLI only — there is no MCP tool for this call). Project the response down to the three fields this flow uses; the full rows carry `card[]` / `cells[]` render arrays this flow never reads, and they are ~35× larger:

```
onchainos agent get-my-agents --page-size 20 | jq -c '{total: .data.total, asps: [.data.list[] | select(.roleLabel == "ASP") | {agentId, name}]}'
```

   - **No** `--role` filter: `.data.total` must stay the all-roles count for the summary line in §2, and the `select` above already applies the exact ASP rule.
   - `--page-size 20` avoids the default page size of 5 silently truncating the list; if the user has more than 20 agents, paginate with `--page` rather than stopping at page 1 and guessing.
   - If `jq` is unavailable, or the projection comes back empty while the raw call clearly has agents, drop the pipe and apply §2 to the raw JSON.
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

**SHOULD**: proceed only after the user replies `1` — the confirmation exists so a backend rejection does not come as a surprise.

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

**On failure** — two different outcomes. **MUST**: branch on the `errorCode` field, never on the wording of `error` — the wording comes from the backend and changes without notice, while the code is the contract:

| `errorCode` | Meaning | What to say |
|---|---|---|
| `hackathon_registration_rejected` | The backend evaluated the ASP and refused it. `error` carries the reason (e.g. not trading-type, no subscription, no 3-day trial). | **Translate `error` into the user's language** and show it as the reason — it is authoritative. Translate faithfully: keep the same condition and the same required action, and do **not** soften it, generalise it, add a cause the backend did not give, or swap in one of the checklist items. If a term has no clean translation, keep it in the original alongside the translation. |
| `hackathon_service_unavailable` | The request never reached the registration logic — connection error, timeout, 5xx, or an HTML error page. | Say the hackathon registration service is currently unavailable and suggest retrying shortly. **Never** tell the user their ASP failed the trading-type / subscription / trial checks — nothing was evaluated. |

Anything else (`invalid_input`, or a plain `{ok:false, error}` with no `errorCode`) is a CLI-side validation failure — see the error list at the end of this file.

If a `hackathon_registration_rejected` message says the activity is missing, closed, or ended, the hackathon this CLI build is pinned to is over — tell the user that and suggest upgrading `onchainos`. Do not present it as a problem with their ASP.

## CLI / MCP reference — `hackathon register`

**Requires wallet login.**

```
onchainos hackathon register --agent-id <id> --account-type <web3|cefi> [--address <addr>] [--uid <uid>]
```

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--agent-id` | Yes | — | Trading ASP agent id (from `agent get-my-agents`). |
| `--account-type` | Yes | — | Exactly `web3` or `cefi`, lowercase. Any other value — including a differently cased `CeFi` — is rejected on both the CLI and the MCP tool. |
| `--address` | No | wallet X Layer addr | EVM wallet address. Auto-resolved from the current wallet's X Layer address when omitted (both account types). |
| `--uid` | Conditional | — | OKX UID. **Required when `--account-type cefi`**, and **rejected when `--account-type web3`** — the request body carries no account-type field, so uid presence is the only signal the backend gets. |

The activity id and chain index (X Layer, `196`) are **fixed internally** — no flag or param sets, overrides, or returns either. MCP tool `hackathon_register` mirrors the flags above (same `address` auto-resolve; no `activity_id` / `chain_index` params) and runs the **same validation**: both surfaces share one validator, so neither accepts anything the other rejects.

Success returns `{ "registered": true, "agentId", "accountType", "chainIndex", "address" }`. The OKX UID is **never** echoed back — it is submitted, redacted in the audit log, and not returned. When you print the executed command, mask it (`--uid <hidden>`); never paste the raw UID into the conversation.

**CLI-side errors** (backend rejections and transport failures are handled in Step 4):
- `--uid is required for CeFi account registration` → collect the UID and retry.
- `--uid is only valid with --account-type cefi` → the user picked `web3` but a UID was passed; confirm which account they meant, then retry with one or the other.
- `invalid account type …` → `--account-type` was not exactly `web3` or `cefi`; re-send it lowercase.
- invalid `--address` for the chain → failed `validate_address_for_chain`; fix and retry.
- `--agent-id is required` / `contains control characters` / `is too long` → the id was blank or mangled; re-run Step 1 and take the id from the list rather than retyping it.
- `not logged in` → run `onchainos wallet login`, then retry.

A blank or whitespace-only `--uid` counts as **not supplied** — it does not satisfy `cefi`, and it does not trip the `web3` rule. If the user sends an empty reply when asked for their UID, ask again; do not submit.
