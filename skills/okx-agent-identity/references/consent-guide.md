# Consent Guide

This file governs the display and handling of the first-time agent creation consent flow.
Single source of truth for consent card wording, agree/decline templates, and worked examples.
Referenced from `SKILL.md §⛔ MANDATORY consent gate`.

---

## When consent is required

Consent is required when a wallet address has **never registered any agent identity** (any role).
The backend signals this by returning a non-null `consent` object in the first `agent create` response:

```json
{
  "consent": {
    "consentKey": "<uuid>",
    "terms": "<平台条款文本>"
  }
}
```

Returning users (the wallet address already has at least one registered or pending agent) skip
consent entirely — the backend returns `consent: null` directly.

---

## §Consent Card

Render this card when the consent intercept fires. Match the user's language.
Display `consent.terms` verbatim as the terms content.

**Chinese:**

```
在创建 agent 身份之前，请阅读并同意以下条款：

<consent.terms 的内容>

回复「同意」继续注册；回复「不同意」取消。
```

**English:**

```
Before creating your agent identity, please review and accept the following terms:

<consent.terms content>

Reply "agree" to continue; reply "decline" to cancel.
```

**Rules:**
- Always display `consent.terms` as-is — do NOT summarize, paraphrase, or omit any part.
- Do NOT show the raw `consentKey` UUID to the user — it is an internal token.
- Do NOT show a confirmation card for agent fields again — that already ran in the
  `§⛔ MANDATORY confirmation gate`. The consent card is solely about the terms.
- Do NOT pre-fill the user's reply or add "I'll assume you agree if you don't reply".

---

## §Agree flow

After the user replies with an agree token (`同意` / `agree` / `yes` / `接受` / `确认同意`):

1. Re-invoke the original `onchainos agent create` command with the **exact same parameters**.
2. Append `--consent-key <value of consent.consentKey from the backend response>`.
3. Append `--agreed true`.
4. Do NOT re-render the confirmation card.
5. Proceed to `§Step 4: Report Result` with the second call's response.

---

## §Decline message

If the user replies with a decline token (`不同意` / `decline` / `no` / `拒绝` / `不接受`):

- Do NOT call the CLI.
- Render the message below and stop.

**Chinese:**
「注册已取消 — 创建 agent 身份需要同意服务条款。如果之后改变想法，随时可以重新开始注册流程。」

**English:**
"Registration cancelled — creating an agent identity requires accepting the terms of use.
You can restart the registration flow at any time."

---

## §Ambiguous reply handling

If the user's reply is neither an agree token nor a decline token (e.g., a question about the
terms, an off-topic message, or a partial phrase):

1. Re-display the consent card **once** (including the full `consent.terms` text again).
2. Wait for a clear agree or decline token.
3. Do NOT auto-agree, do NOT auto-decline, do NOT timeout.

---

## §Worked examples

### Example A — first-time user, agrees

```
User:    注册一个用户身份，名字叫 Alice。
[pre-check + Q&A + confirmation card runs normally]
User:    执行
[skill invokes agent create — backend returns consent object]
Skill:   在创建 agent 身份之前，请阅读并同意以下条款：

         <consent.terms 的内容>

         回复「同意」继续注册；回复「不同意」取消。
User:    同意
[skill re-invokes agent create --consent-key <uuid> --agreed true]
[second call returns consent: null — normal success flow]
```

### Example B — first-time user, declines

```
Skill:   [shows consent card with terms]
User:    不同意
Skill:   注册已取消 — 创建 agent 身份需要同意服务条款。
         如果之后改变想法，随时可以重新开始注册流程。
[flow stops — CLI is NOT re-invoked]
```

### Example C — ambiguous reply

```
Skill:   [shows consent card with terms]
User:    这个条款是什么意思？
Skill:   [re-displays consent card once, including full terms text]
         在创建 agent 身份之前，请阅读并同意以下条款：
         <consent.terms 的内容>
         回复「同意」继续注册；回复「不同意」取消。
```

### Example D — returning user (no consent needed)

```
User:    再建一个服务提供商身份。
[pre-check shows existing agents for this wallet address]
[backend returns consent: null — normal flow, consent gate never fires]
```

---

## Error codes (backend-only — not handled at skill layer)

These codes may surface via `references/troubleshooting.md` if the second call is malformed.
The skill does not need to map them explicitly.

| Code | Name | When |
|---|---|---|
| 40020 | AGENT_CONSENT_AGREED_REQUIRED | `consentKey` passed but `agreed` omitted |
| 40021 | AGENT_CONSENT_INVALID | Key invalid / user mismatch / already finalized, or `agreed` passed without `consentKey` |
| 40022 | AGENT_CONSENT_REJECTED | User declined (status recorded as rejected in DB) |

If any of these codes appear in the CLI response, route to `references/troubleshooting.md`
for the user-facing message.
