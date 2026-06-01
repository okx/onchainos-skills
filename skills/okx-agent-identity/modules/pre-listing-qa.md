# Pre-Listing Quality Assurance

This file defines the quality gate the AI runs **before invoking `agent activate`** for any `provider`-role agent. It operationalises the five display-field standards from the OKX marketplace listing specification.

## When to Run

Automatically trigger this checklist when:

- The user intends to activate an agent, **AND**
- The target agent's `role` is `provider` (determined from the most recent `agent get` response already in context)

If the role is `requester` or `evaluator`, skip this file — it does not apply (those roles have no service fields).

## How to Run

1. Use the `agent get --agent-ids <N>` data already fetched during the activate pre-check (do **NOT** make an extra CLI call just for QA).
2. Extract: top-level `name`, `description`, `picture`, and all `services[]` entries (each with `name`, `servicedescription`, `servicetype`, `fee`, `endpoint`).
3. Run **every** check in the tables below against each service.
4. **All checks pass** → proceed to `agent activate`. No report needed.
5. **Any check fails** → render the §QA Report (with two explicit options) and stop. Wait for the user to choose. **Exception: L1 (no avatar) is always blocking** — if `picture` is absent, do NOT offer option 2 (list anyway); only offer option 1 (fix first).

---

## Universal Prohibitions (apply to all fields)

| # | Rule | Failing pattern | Fix |
|---|------|----------------|-----|
| U1 | No test / environment markers | Field contains any of the following patterns (case-insensitive): **parentheses / bracket forms** `(pre)` `(test)` `(dev)` `(beta)` `(alpha)` `(staging)` `(uat)` `(sandbox)` `[pre]` `[test]` `[dev]` `[beta]` `{pre}` `{test}`; **delimiter-suffix forms** `-pre` `-test` `-dev` `-beta` `-staging` `_pre` `_test` `_dev` `_beta` `_staging` `.pre` `.test`; **space-suffix forms** ` pre` ` test` ` dev` ` beta` ` staging` appearing at the **end** of the field value (trailing space marker). Matching is **case-insensitive** (`(PRE)`, `_Test`, `-DEV` all fail). | Remove the marker entirely — do not replace with another tag |
| U2 | No internal addresses | Any `0x…` wallet / owner / tx hash in name, description, or service fields | Remove the address |
| U3 | No negative capability statements | Contains `目前不支持` / `暂不支持` / `currently not supported` / `does not support` | Rewrite positively or remove |
| U4 | Free service must be explicit | A2MCP `fee` is empty/blank when the service is free | Set to `0 USDT` |

---

## Field 1 — Agent Name (`name`)

| # | Rule | Failing pattern | Fix |
|---|------|----------------|-----|
| N1 | Length in range | CN: < 2 or > 12 characters; EN: < 3 or > 25 characters | Shorten or expand |
| N2 | No agent ID embedded | Contains `#123`, `_1083`, or any numeric agent ID | Remove the ID |
| N3 | No ordinal suffixes | Ends with bare digit, `_2`, `_v2`, `(2)`, `3号` | Remove the ordinal |
| N4 | No personal names or account labels | Contains personal name, email prefix, or wallet account label (e.g. `Account2`, `Jim的`, `xicheng`) | Remove the personal reference |
| N5 | Brand name — not a sentence | Reads as a full verb + object sentence rather than a product brand | Rewrite as a short brand name |
| N6 | Bilingual separator | Bilingual name must use `中文名 · EnglishName` format (middle dot `·` + spaces) | Fix the separator |
| N7 | No test / environment markers in name | Name contains any U1 marker — e.g. `健身教练(pre)` / `WeatherBot-test` / `MyAgent_dev` / `SentryX(beta)` / `链镜 staging`. This is the **#1 reported rejection reason for names** and must be checked explicitly even though U1 also covers it globally. Caution: `Predict` is NOT a violation (`pre` is embedded in a genuine word); only flag when the marker is delimited (parentheses / bracket / hyphen / underscore / trailing space). | Remove the marker; pick a clean brand name |

**Good:** `Predict-Raven` / `灵镜 · ChainMirror` / `SentryX` / `WakeMeUp` / `PMAlpha`

**Bad:** `Robert的健身教练(pre)` / `天气小助手_test` / `MyAgent-dev` / `SentryX(beta)` / `Account2买家`

---

## Field 2 — Service Type (`servicetype`)

| # | Rule | Failing pattern | Fix |
|---|------|----------------|-----|
| T1 | Enum values only | Value is not exactly `A2A` or `A2MCP` (case-sensitive) | Correct to `A2A` or `A2MCP` |
| T2 | A2MCP requires endpoint | `servicetype=A2MCP` but `endpoint` is empty or absent | Provide a valid public HTTPS endpoint |
| T3 | A2A does not use endpoint | `servicetype=A2A` but `endpoint` is non-empty | Remove the endpoint value |

**Reminder:** A2A = agent-to-agent natural-language interaction, no endpoint needed. A2MCP = API-interface service, endpoint is mandatory.

---

## Field 3 — Service Name (`name` inside each service object)

| # | Rule | Failing pattern | Fix |
|---|------|----------------|-----|
| S1 | Length 5–30 characters | < 5 or > 30 characters | Adjust |
| S2 | Noun phrase — not a sentence | Contains a full sentence with subject + verb | Rewrite as a short noun phrase |
| S3 | Not a duplicate of agent name | Service `name` is identical to the agent-level `name` | Write a distinct service name |
| S4 | No price in service name | Contains price info (`0.1 USDT`, `free`, `免费`) | Move pricing to the fee field |
| S5 | No technical implementation details | Mentions internal framework, API key, infra provider | Remove or abstract |
| S6 | No test / environment markers in service name | Service name contains any U1 marker — e.g. `天气查询(pre)` / `分析接口_test` / `推荐服务-beta`. Apply the same delimiter-awareness as N7: `protest` is NOT a violation; only flag when the marker is clearly delimited. | Remove the marker; rewrite as a clean noun phrase |

**Good:** `Polymarket Daily Signal` / `On-chain Signature Analysis` / `Crypto Price Alert`

**Bad:** Same as agent name (duplication) / too vague (e.g. `General Query`) / too long + tech exposure / `Market Push(test)`

---

## Field 4 — Default Price (`fee`) — A2MCP required; A2A optional

| # | Rule | Failing pattern | Fix |
|---|------|----------------|-----|
| P1 | Format: `{number} {currency}` — both segments required | Missing either segment | Complete both |
| P2 | Currency must be `USDT` or `USDG` | Any other currency symbol | Change to `USDT` or `USDG` |
| P3 | No negotiation language | Contains `可协商` / `TBD` / `negotiable` / `flexible` | Set a concrete price |
| P4 | No parenthetical notes | Contains `(支持 USDG 结算)` or any parenthetical after the price | Remove the parenthetical |
| P5 | A2A fee format | A2A fee is optional; if provided, must still follow all format rules above | Leave empty or apply format |

**Good:** `0.1 USDT` / `0.5 USDG` / `0 USDT`

**Bad:** `0.2 USDT 起按复杂度协商` / `USDG` (missing number) / `0.05 USDT（支持 USDG 结算）`

---

## Field 5 — Service Description (`servicedescription`)

| # | Rule | Failing pattern | Fix |
|---|------|----------------|-----|
| D1 | Three-part structure required | Missing any of: ① summary / ② capabilities / ③ example prompts | Add the missing part |
| D2 | Total ≤ 400 characters | Total character count > 400 | Trim |
| D3 | Part 1 summary ≤ 50 characters | First paragraph > 50 characters | Shorten: "what it is + who it's for" in one sentence |
| D4 | Part 2 capabilities ≤ 150 characters | Second paragraph > 150 characters | Reduce to 3–5 key capability points separated by `;` |
| D5 | Part 3: 1–3 example prompts, each ≤ 80 characters | No prompts / > 3 prompts / any prompt > 80 characters | Adjust count and length |
| D6 | No external links or GitHub URLs | Contains `github.com` or any URL | Remove |
| D7 | No wallet/contract addresses | Contains `0x…` | Remove |
| D8 | No tech-stack exposure | Mentions internal framework names, model names, infra details | Abstract or remove |
| D9 | No negative statements | Contains `目前不支持` / `暂不支持` / `currently not supported` | Remove or rephrase |
| D10 | No legal disclaimers | Contains liability statements or legal caveats | Remove |

**Good structure:**
```
① [≤50 chars] Prediction market research agent for Polymarket, delivering daily actionable betting signals.
② [≤150 chars] Scans active markets; combines market data, settlement rules, order book liquidity, and web search; outputs direction, AI probability, evidence chain, position sizing, and key risks.
③ [1–3 prompts, each ≤80 chars] 1. Recommend 3 Polymarket opportunities worth betting on now  2. Scan active markets for top 3 mispriced opportunities
```

---

## Logo — Required (missing avatar blocks activation)

Avatar upload is **mandatory** — the platform no longer provides a default. Check the `picture` field from `agent get`.

| # | Rule | Failing pattern | Fix |
|---|------|----------------|-----|
| L1 | Avatar must be uploaded | `picture` field is empty, null, or absent | Ask the user to upload an avatar via `agent upload` before listing |
| L2 | 1:1 aspect ratio | Non-square image | Re-upload a square image |
| L3 | < 1 MB | File too large | Compress and re-upload via `modules/avatar-upload.md` |

L1 is a **blocking** check (❌) — do not proceed to `agent activate` without an avatar. L2 and L3 are ⚠️ warnings (cannot always be verified post-upload; surface at upload time).

---

## §QA Report Format

When any check fails, render the report below in the user's language, then ask whether to fix first or proceed anyway.

```
QA check found some issues before listing:

**Agent #<id> — <name>**

Service [N] "<service name>":
  ⚠️ <Field> — <issue> → <suggestion>

(repeat for each service with failures)

---
How would you like to proceed?
  1. Fix and list (Recommended)
  2. List anyway (⚠️ non-compliant information may cause listing failure)
```

**Rules for the report:**

- Use ⚠️ (warning), not ❌ (error) — this is advisory, not a hard block.
- Group failures by service (service index + name); list all failing checks.
- Use fix instructions from the tables above, translated to the user's language.
- ⛔ Do NOT show raw JSON, field key names (`servicedescription`, `servicetype`), or CLI flag names — use the user-facing labels from `core/ux-lexicon.md`.
- ⛔ Do NOT auto-correct values — the user must supply corrected content (Red line 6 in `SKILL.md`).
- **On option 1 (fix first)**: route through `§Update` flow (`agent update` → re-run QA → `agent activate`).
- **On option 2 (list anyway)**: invoke `agent activate` immediately without re-prompting.

---

## Pass Message (all checks green)

No separate message needed — silently proceed to `agent activate`. The post-activate line from `§Suggest Next Steps` is the only user-visible output.

If you want to surface the clean result (optional, e.g. when the user explicitly asked for a QA check without intending to activate right away):

- "QA passed — all fields meet listing requirements. Say the word and I'll activate it."
