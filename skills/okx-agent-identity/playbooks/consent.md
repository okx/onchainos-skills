# Consent Guide

This file governs the display and handling of the first-time agent-creation consent flow.
Single source of truth for consent card wording, agree/decline templates, and worked examples.
Referenced from `SKILL.md §⛔ MANDATORY consent gate` and Core Flow gate 3.

> **Architecture (2026-06):** consent is the legal module's **standalone two-step
> flow**, decoupled from `agent create`. It runs as its own `agent consent` call
> **before any identity info is collected** (Core Flow gate 3 — right after
> pre-check, before Role Q&A). `agent create` no longer carries `--consent-key`
> / `--agreed` and no longer returns a `consent` field. Never pass consent flags
> to `create`; never look for consent in the create response.

---

## When consent is required

Consent is required when a wallet address has **never registered any agent identity** (any role).
The standalone `agent consent` step (no flags) signals this by returning:

```json
{
  "required": true,
  "consent": {
    "consentKey": "<uuid>",
    "terms": "<platform terms text>"
  }
}
```

Returning users (the wallet address already owns an agent) or a disabled feature flag get:

```json
{ "required": false, "consent": null }
```

→ skip the consent card entirely and proceed straight to Role Q&A (identity info collection).

---

## §Step 1 — fetch terms

At Core Flow gate 3, **before collecting any identity field**, call `agent consent` with no
flags. Branch on the result:

- `required: false` → no terms to show; continue to Role Q&A.
- `required: true` → render the §Consent Card using `consent.terms`, then wait for the user.

---

## §Consent Card

Render this card when `required: true`.
Display `consent.terms` verbatim as the terms content.

```
Before creating your agent identity, please review and accept the following terms:

<consent.terms content>

Reply "agree" to continue; reply "decline" to cancel.
```

**Rules:**
- Display `consent.terms` in the **current conversation language** (match the language the
  user is communicating in). If `consent.terms` is in a different language than the
  conversation, translate it to match before displaying. Translation is permitted for
  readability, but the translated content MUST be complete — do NOT summarize, paraphrase,
  or omit any clause.
- Do NOT show the raw `consentKey` UUID to the user — it is an internal token.
- This card is solely about the terms. No agent-field confirmation card runs here —
  identity info has not been collected yet (it comes after consent).
- Do NOT pre-fill the user's reply or add "I'll assume you agree if you don't reply".

---

## §Agree flow

After the user replies with an agree token (`agree` / `yes` / `accept` / `confirm`):

1. Call `onchainos agent consent --consent-key <value of consent.consentKey> --agreed true`
   to finalize the decision (returns `required: false` / `consent: null`).
2. Then proceed to **Role Q&A** (Core Flow gate 4) — collect name / description / services.
3. Continue normally through the confirmation card (gate 5) and `agent create`.

---

## §Decline message

If the user replies with a decline token (`decline` / `no` / `reject` / `cancel`):

1. Call `onchainos agent consent --consent-key <value of consent.consentKey> --agreed false`
   to record the rejection (finalizes the decision in the backend).
2. Render the message below and stop. Do NOT enter Role Q&A or `agent create`.

"Registration cancelled — creating an agent identity requires accepting the terms of use.
You can restart the registration flow at any time."

---

## §Ambiguous reply handling

If the user's reply is neither an agree token nor a decline token (e.g., a question about the
terms, an off-topic message, or a partial phrase):

1. Re-display the consent card **once** (including the full `consent.terms` text again).
2. Wait for a clear agree or decline token.
3. Do NOT auto-agree, do NOT auto-decline, do NOT timeout, do NOT call the finalize step yet.

---

## §Worked examples

### Example A — first-time user, agrees

```
User:    Register a user identity named Alice.
[Core Flow: ask role → pre-check (agent get)]
[gate 3: skill calls agent consent (no flags) — returns required:true + consent]
Skill:   Before creating your agent identity, please review and accept the following terms:

         <consent.terms content, full text>

         Reply "agree" to continue; reply "decline" to cancel.
User:    agree
[skill calls agent consent --consent-key <uuid> --agreed true — finalized]
[gate 4: Role Q&A collects name/description; gate 5: confirmation card]
User:    Execute
[skill invokes agent create — no consent fields, normal success flow]
```

### Example B — first-time user, declines

```
Skill:   [shows consent card with terms]
User:    decline
[skill calls agent consent --consent-key <uuid> --agreed false — rejection recorded]
Skill:   Registration cancelled — creating an agent identity requires accepting the terms of use.
         You can restart the registration flow at any time.
[flow stops — Role Q&A and agent create are NOT entered]
```

### Example C — ambiguous reply

```
Skill:   [shows consent card with terms]
User:    What do these terms mean?
Skill:   [re-displays consent card once, including full terms text]
         Before creating your agent identity, please review and accept the following terms:
         <consent.terms content, full text>
         Reply "agree" to continue; reply "decline" to cancel.
```

### Example D — returning user (no consent needed)

```
User:    Register another service provider identity.
[pre-check shows existing agents for this wallet address]
[gate 3: skill calls agent consent (no flags) — returns required:false / consent:null]
[consent card never shown; flow continues straight to Role Q&A]
```

---

## Error codes (backend-only — not handled at skill layer)

These codes may surface via `troubleshooting.md` if the finalize call is malformed.
The skill does not need to map them explicitly.

| Code | Name | When |
|---|---|---|
| 81001 | INCORRECT_PARAMETER | `chainIndex` empty / non-numeric |
| 40020 | AGENT_CONSENT_AGREED_REQUIRED | `consentKey` passed but `agreed` omitted |
| 40021 | AGENT_CONSENT_INVALID | Key invalid / user mismatch / already finalized, or `agreed` passed without `consentKey` |
| 40022 | AGENT_CONSENT_REJECTED | User declined (status recorded as rejected in DB) |

If any of these codes appear in the CLI response, route to `troubleshooting.md`
for the user-facing message.
