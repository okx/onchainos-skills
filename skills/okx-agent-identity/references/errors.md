# Errors — non-success CLI response → friendly card

Loaded when: a CLI call returns an error / non-success (on demand only; never on the happy path).

## Card format

```
❌ **<summary>**
Reason: <friendly translation>
Next step: <user action / what you'll do>
`<raw CLI line — verbatim, never translated>`
```

Translate, don't parrot — the friendly line is for the user; the raw line sits in inline code for debug. **Redaction overrides verbatim:** if the raw CLI line would contain an `onchainos ...` command literal, a skill name (`okx-*`), or an internal label, strip/redact that token before showing it — SKILL §UX Red Lines 1 wins over "verbatim". **Never auto-retry** a business error (retry once only on 5xx / network, per SKILL §Gates No-poll). Never chase a failure with `agent get` — the error is authoritative. Each row resumes at a concrete step. An unlisted string → put it in the raw line and ask how to proceed.

## CLI `bail!` rows (usually never reach the user — you collect params upfront)

| Raw line | Friendly + next |
|---|---|
| `session expired` | "Session expired." → hand to wallet login (business language, no skill name — SKILL §Routing), re-run original. |
| `no XLayer address found` | "No XLayer address in the current account." → hand to wallet add / switch. |
| `missing required parameter: <flag>` | "`<flag>` can't be empty." → re-ask it. `--agent-id` → ask which agent (`agent get` if needed); `--file` → ask path. |
| `unexpected argument '<v>' found` (positional) | User typed e.g. `update 42`. Re-ask in plain language; you supply the flag yourself, never echo it. |
| `missing required field in --service: name`/`: servicedescription` | "Service <name/description> can't be empty." → re-ask that field. |
| `... for A2MCP: fee`/`: endpoint` | API service needs a fee (USDT ≤6 dp) / a public https endpoint → re-ask. Gloss type once (SKILL §Invariants Lexicon); never echo `A2MCP`. |
| `invalid servicetype` | "Type must be API service or agent-to-agent." → re-render numbered type prompt. Never echo `A2MCP`/`A2A`. |
| `invalid value for --role` | "Role must be User Agent / ASP / Evaluator Agent." → re-render numbered role prompt. Never echo the enum. |
| `provider agents require at least one service` | "An ASP needs at least one service." → return to service Q&A. No raw `provider`. |
| `failed to read file` | "Can't read that file." → recheck path; terminal → offer generate / skip. |
| `upload response missing url` | "Upload succeeded but no URL came back." → retry once; if persists, surface and ask. |

(`xmtp-sign ... missing signature` is not user-facing — `xmtp-sign` is never exposed.)

## Backend keyword rows (match keyword, not equality)

| Keyword | Friendly + next |
|---|---|
| `approved agent whitelist` / `10016` | "Your account isn't in the agent beta whitelist yet. Apply here: `<URL>`. We'll email you when approved." Extract the FIRST url from `msg` **verbatim** (keep `/zh-hans/` etc.); none → drop the URL sentence, say "Contact OKX support for the application portal." **No auto-retry**; no further create / update. |
| `agent not found` / 404 | "Agent not found." → verify id with `agent get`. |
| `already active` / `already inactive` | "Agent is already active / inactive." No-op; show detail card. |
| `pending settlements` / `cannot deactivate` | "There's an unsettled task on this agent — close it first. Want me to take you there?" → on yes, hand to task flow internally (no skill name). |
| consent `40020` / `40021` / `40022` | "Consent <incomplete / invalid / was declined> — registration failed. Please restart the registration flow." Raw line shown. **Hard stop**, no auto-retry, no in-flow re-agree (40022 = restart from scratch). No Step 5/6. |
| `81602` / `blocked` | "This agent has been blocked by the platform and can't be operated right now." **Stop.** Don't suggest re-activate / update. No Step 5/6. |
| Region `50125` / `80001` | "Service is not available in your region." **Never** echo the code, suggest a VPN / region workaround, or auto-retry. [eval 24, backstop to SKILL §Gates Post-execute + register §Execute] |
| `stake` / `staking` / `insufficient` on create evaluator | Not a normal path — create doesn't consume the stake. "Backend returned a staking error; registration doesn't require staking." Surface raw line; point to the task-side staking flow; don't cache drafts. |
| `HTTP 500` | "Backend temporarily unavailable." Retry once; if persists, surface and move on. |

### activate / submit-approval outcomes

- `success:false` + `approvalStatus 2` → "Your agent is under review — usually ready within 24h; once approved it appears on the marketplace." **Stop.** Don't call `submit-approval`; no Step 5/6.
- `success:false` + `approvalStatus 5` → treat **identically to `1`**: run `agent validate-listing`, then `agent submit-approval` (carrying `--preferred-language`, per manage.md §activate). Never render a rejection, never echo `rejectReason`, never stop on the 5.
- `submit-approval success:true` → "Done — submitted for listing review. Usually ready within 24h; once approved it appears on the marketplace." **Stop.** No Step 5/6.
- `submit-approval success:false` (non-blacklist) → "Failed to submit for listing review." + raw line + "You can try again later." **Stop.**

## Skill-side guards (caught before the CLI — no bail!)

- **No-op update** — nothing changed → "No changes to submit." Don't call `agent update`; re-enter update Q&A.
- **Empty search query** — catch before sending; ask for the query.
- **A2A fee format** — non-empty fee not a number ≤6 dp (`5 USDT`, `approx 10`) → "agent-to-agent fee is optional — leave it empty or give a USDT number with up to 6 decimal places." Re-ask.
- **Endpoint >512 chars** → "The endpoint URL must be at most 512 chars; this one is longer. Use a shorter URL." Re-ask (mention the limit only here).
