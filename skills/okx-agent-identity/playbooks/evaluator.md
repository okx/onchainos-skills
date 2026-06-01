# Role: evaluator (仲裁者 / Evaluator Agent)

> Registers an Evaluator-Agent identity. `create` itself does not require the stake — the backend accepts the registration regardless. A separate stake is what makes the Evaluator Agent eligible to be assigned to real disputes; that flow (and the current amount) lives in `/skills/okx-agent-task/references/evaluator-staking.md` — the onboarding-specific entry point (§2) is what the post-create same-turn handoff loads, but the file as a whole also owns top-up / unstake / claim / query for later sessions. On successful create, this skill **hands off to that skill in the same turn** (see §Post-success) — do not stop between create success and stake confirmation. This skill never verifies or reads stake state, and never hardcodes the amount.

## STRICT — one question per turn

See `playbooks/README.md §STRICT` for the full rule. One field per message; `core/field-specs.md` four segments inline.

## Flow overview

```
1. Ask name
2. Confirmation card → execute create
3. Post-success card (two visible lines) + same-turn handoff to `okx-agent-task/references/evaluator-staking.md §2`
```

No pre-create staking gate. No cached-resume flow. Registration is cheap; the post-success card hands off to `okx-agent-task` in the same turn, and the user confirms the stake in their next turn.

## Phase 1 — identity Q&A

### Phase preview (render BEFORE Q1)

Once role is `evaluator` and pre-check passed (evaluator is unique per address — if found, hand off to `update` per `playbooks/README.md §Pre-check`), render a short declarative preview, then start Q1.

Chinese:
```
好，开始注册新仲裁者身份。接下来会收集以下基本信息：
  1. 名称
（仲裁者默认不问头像；想设头像直接说。）
```

English:
```
Got it — starting a new Evaluator Agent registration. We'll collect:
  1. Name
(No profile photo prompt by default; just say so if you want to set one.)
```

Preview is declarative; the first question follows after a blank line.

### Q&A

> ⛔ Fields from user's literal reply only — never pre-fill from userEmail, wallet name, or session metadata. Anti-pattern to avoid: "Jim 的仲裁者" / "yuhui 的 Evaluator Agent". Full rules: SKILL.md Red line 6.

The `Q1 / Q2` column labels below are **maintainer-internal indexes**. The prompt strings in the Chinese / English columns are the **literal text** rendered to the user — they carry **no `Q1：` / `Q1:` prefix** (see `SKILL.md §UX Output Red Lines Red line 3` and `core/ux-lexicon.md`). Each prompt inlines the four-segment field spec from `core/field-specs.md` in the user's language only. Skip any Q whose field was already captured via `core/choice-prompts.md §One-Shot Capture`.

| Q | Chinese prompt | English prompt | Validation |
|---|---|---|---|
| Q1 | `你要注册的仲裁者身份叫什么名字？` + 4 segments | `What's the name of this Evaluator Agent?` + 4 segments | non-empty, CN ≤ 30 文字 / EN ≤ 64 chars |

> **Description — do NOT prompt, do NOT show in confirmation card when absent.** For Evaluator Agent, skip the description question entirely. If the user volunteers a description in the same message as the name (one-shot capture), accept and record it AND include a `描述` row in the confirmation card for that run; if not volunteered, omit the `描述` row from the confirmation card entirely (do NOT render "未填" or "(not set)") and send `ProfileDescription: ""` on-chain silently. Do NOT ask "描述（可选）" / "description (optional)" in any turn.

No profile-photo prompt by default (Evaluator-Agent dashboards rarely show photos). If the user brings it up, branch to `modules/avatar-upload.md`.

## Phase 2 — confirmation card

> ⛔ Mandatory before invoking the CLI. See the mandatory confirmation gate in SKILL.md for the canonical rule + rationalizations (`auto-execute` / plan-mode exit / one-shot capture / urgency / "intent obvious") that do **NOT** bypass it.

Render in the user's language. Pick ONE variant.

Chinese variant (user did NOT volunteer a description — omit 描述 row):

| 字段 | 值 |
|---|---|
| 角色 | 仲裁者 |
| 名字 | Solidity Auditor |
| 头像 | 默认 |
| 预计费用 | **0 USDT**（注册/修改/上下架均无手续费，由 OKX 承担） |

> 确认无误回复 "执行"。

Chinese variant (user volunteered a description via one-shot capture — include 描述 row):

| 字段 | 值 |
|---|---|
| 角色 | 仲裁者 |
| 名字 | Solidity Auditor |
| 描述 | 仲裁 Solidity 相关任务的争议；5 年审计经验。 |
| 头像 | 默认 |
| 预计费用 | **0 USDT**（注册/修改/上下架均无手续费，由 OKX 承担） |

> 确认无误回复 "执行"。

English variant (user did NOT volunteer a description — omit Description row):

| Field | Value |
|---|---|
| Role | Evaluator Agent |
| Name | Solidity Auditor |
| Profile photo | default |
| Estimated cost | **0 USDT** (creating/editing/activating/deactivating costs no transaction fees — OKX covers them) |

> Reply "execute" to run it.

English variant (user volunteered a description via one-shot capture — include Description row):

| Field | Value |
|---|---|
| Role | Evaluator Agent |
| Name | Solidity Auditor |
| Description | Arbitrates Solidity-related task disputes; 5 years of audit experience. |
| Profile photo | default |
| Estimated cost | **0 USDT** (creating/editing/activating/deactivating costs no transaction fees — OKX covers them) |

> Reply "execute" to run it.

Do **NOT** add a `stake` row here — create does not consume the stake and this skill has no way to verify it. Mentioning stake in the confirmation card implies a gate that does not exist.

**Do NOT** show the bash command. **Do NOT** mention OKB or stake tx hashes inside the confirmation card.

## Execute (maintainer reference — not shown to user)

```bash
onchainos agent create \
  --role evaluator \
  --name "<name>" \
  [--description "<description>"] \
  [--picture "<url>"]
```

## ⛔ Post-success — MANDATORY template (do NOT paraphrase)

> ⛔ **After the visible lines, this turn is NOT over — and comm-init applies to evaluator too.** → proceed to SKILL.md §Operation Flow Step 5. The evaluator row routes **first** to `/skills/okx-agent-task/references/evaluator-staking.md §2`; the staking flow's terminal handoff feeds into `§Step 6` (comm-init). **Fallback**: if the staking flow ends without that handoff for any reason (user declines stake, staking error, etc.), the evaluator row in Step 5 requires invoking `§Step 6` from this skill before stopping the turn. Full rules live in Step 5 + Step 6 — not duplicated here.

Render **two visible lines** using the template below — **verbatim except for the `#<id>` substitution rule**. Then follow the §Agent directive block (internal — not rendered to the user). Paraphrasing, hardcoding the stake amount, mentioning the downstream skill path, adding follow-up questions, or summarizing the CLI's other JSON output are all violations of `SKILL.md §⛔ MANDATORY post-execute gate`.

### Visible lines (template)

Pick the variant matching the user's language. Render exactly **two lines** in this order; both are declarative, no question mark, no pre-announcement of which skill path is loaded next:

- Chinese:
  > 仲裁者身份 #<id> 注册完成。
  > 要被系统分派仲裁案子还需要完成质押。
- English:
  > Evaluator Agent identity #<id> registered.
  > A separate stake is still required before you can be assigned disputes.

**`#<id>` substitution rule** (per `core/display-formats.md` top, `#<id>` placeholder rule, **evaluator-specific constraints**):

- Evaluator is **unique per address** (product invariant — see `playbooks/README.md §Pre-check`). If we got here successfully, the pre-check `agent get` lookup by definition returned **zero evaluators** under this address — otherwise the pre-check gate would have stopped the flow and redirected to `update`. **Therefore the pre-check list contains NO same-role agent for this create**; any agent ids in that list belong to *other* roles (requester, provider) and MUST NOT be used as `#<id>` here.
- The legitimate sources of `#<id>` for this post-success line are, in priority order:
  1. **CLI response (direct):** the `create` call's response directly contains the new agent id.
  2. **Post-create envelope diff:** follow the two-step algorithm in `core/cli-create.md §1` "Finding the newly-minted agentId". For evaluator: pre-check returned 0 evaluators by construction, so the lone newly-appeared evaluator-role row in the current wallet's wrapper IS the new id. ❌ Do NOT write the filter as `agentList[*].ownerAddress == ...` — agent rows have no `ownerAddress` field.
  3. (Future) a follow-up `agent get` in a later turn — irrelevant for this immediate response.
- If **both** source 1 and source 2 miss — i.e. CLI returned `txHash` only **AND** the post-create `agentList` segment is absent (WS + HTTP both failed, per `core/cli-create.md §1`) **OR** the diff yielded no new candidate under the current wallet — → **omit the `#<id> ` substring entirely** — do NOT render `#`, `#<id>`, `# ?`, do NOT invent a number, and do NOT borrow an id from the pre-check list. The second line is unaffected. Fallback first lines:
  - Chinese: `仲裁者身份注册完成。`
  - English: `Evaluator Agent identity registered.`

Do NOT mention the `okx-agent-task/references/evaluator-staking.md §2` path to the user in the visible lines, and do NOT state a stake amount — the same-turn handoff below will take the user directly into that skill's own prompt, which owns both the path and the amount (and reads it from `staking-config`, so any number hardcoded here will go stale).

### ❌ Anti-pattern → ✅ Correct

❌ Agent paraphrased:
> "✅ 仲裁者身份 #88 注册完成！下一步需要质押 100 OKB（这是当前的最低要求）才能开始接案子。我现在带你进入质押流程。"

Why this is a violation of `SKILL.md §⛔ MANDATORY post-execute gate`:

- **Hardcoded the stake amount** (`100 OKB`) — the identity skill never owns the amount; the staking flow reads `staking-config` at request time. Hardcoding leaks stale info — when product bumps the minimum, this response is wrong.
- "下一步需要质押 X OKB 才能开始接案子" replaces the template's lean `要被系统分派仲裁案子还需要完成质押。` with a verbose variant — this is paraphrase.
- "我现在带你进入质押流程" pre-announces the handoff explicitly. The handoff loads the next skill's own prompt; the user does not need a meta-narration about it.
- Adds reassurance phrasing (`注册完成！`) — not in the template.

✅ Correct (with id):
> 仲裁者身份 #88 注册完成。
> 要被系统分派仲裁案子还需要完成质押。

✅ Correct (id unknown, txHash-only return):
> 仲裁者身份注册完成。
> 要被系统分派仲裁案子还需要完成质押。

### Agent directive (internal — do NOT render to the user)

After emitting the two visible lines above, **do not stop the turn**. → proceed to SKILL.md §Operation Flow Step 5 — the evaluator row routes first to `/skills/okx-agent-task/references/evaluator-staking.md` §2 (Step 1 → Step 2); render its staking confirmation card as the next part of the same response. The stake amount is owned by that skill — identity does not pass one. Staking's terminal handoff feeds `§Step 6` (comm-init); the fallback path (staking declined / errored) is documented in Step 5's evaluator row.

Skip carve-out (staking ONLY, not comm-init): if the user has already declined staking earlier in this conversation — see §Good / bad cases row "不想质押". This skips the staking handoff (do NOT load `evaluator-staking.md`), but **Step 5's evaluator fallback still applies** — proceed to `SKILL.md §Operation Flow Step 6` (comm-init) from this skill before stopping the turn. The local agent list still changed when `create` succeeded, so the OpenClaw cache still needs sync regardless of stake status. Comm-init decline is a **separate** axis owned by Step 6.

---

**Do NOT** chase with status poll (that is about querying chain state — different from the Step 5 → staking / Step 6 chain above, which just loads the next skill's prompt). See `_shared/no-polling.md`.

## Error recovery

Translations and classifications live in `troubleshooting.md`. This section only records the **evaluator-specific skill actions**:

- **Session expired** (CLI-exact, `troubleshooting.md` §1 row 1): render the error card → hand off to `okx-agentic-wallet` for `wallet login`, then re-run the confirmation card. No cached-resume needed — if the conversation is still alive the name/description are still in scope.
- **Name / description validation** (there is no CLI bail for these; if the backend rejects, §2 keyword match): re-ask the offending field in Phase 1.

`stake` / `insufficient` keywords are **not expected on create** — create does not consume the stake. If such a rejection ever appears, surface the raw message verbatim and tell the user staking lives in `/skills/okx-agent-task/references/evaluator-staking.md`. Do not infer a staking gate on create.

Do not invent error strings here — add new rows to `troubleshooting.md` §1 or §2 first, then reference them from this list.

## Good / bad cases

| User input | Action |
|---|---|
| "我想当仲裁者" | Start Phase 1 turn 1 (ask name). Do NOT dump the staking requirement up front — it belongs in the post-success message, not as a pre-create gate. |
| "我还没质押，能先注册吗" | 可以。Proceed with create. In the post-success message, remind them that没质押拿不到仲裁派单。 |
| "帮我直接质押再注册" | Correct them: "得先注册再质押。这边先建好仲裁者身份，我接着帮你走质押那一步。" (Skill internally hands off to the staking flow afterwards — do **not** name the skill path in user-visible text, Red line 1.) |
| "不想质押" | Offer: "仲裁者身份可以先注册放着，但没质押不会被派单。要不你考虑改注册用户或服务提供商身份？或者就保留这个仲裁者身份，想好了再质押。" (Chinese localization mandatory — do not leak raw `evaluator` / `requester` / `provider`, Red line 4.) |
| "帮我查下我质押没" | Decline — this skill doesn't read stake state. Hand off to the staking flow. |
