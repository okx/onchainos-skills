# Role: evaluator (验证者)

> Registers an arbitrator identity. `create` itself does not require the stake — the backend accepts the registration regardless. A separate stake is what makes the evaluator eligible to be assigned to real disputes; that flow (and the current amount) lives in `/skills/okx-agent-task/evaluator.md`. On successful create, this skill **hands off to that skill in the same turn** (see §Post-success suggestion) — do not stop between create success and stake confirmation. This skill never verifies or reads stake state, and never hardcodes the amount.

## STRICT — one question per turn

Fields defined in `field-specs.md`. Inline the four segments (`用途 / 可见范围 / 请注意 / 示例` for Chinese; `Purpose / Visibility / Please note / Example` for English) when asking, in the user's language only.

## Flow overview

```
1. Ask name
2. Ask description
3. Confirmation card → execute create
4. Post-success card (two visible lines) + same-turn handoff to `okx-agent-task/evaluator.md`
```

No pre-create staking gate. No cached-resume flow. Registration is cheap; the post-success card hands off to `okx-agent-task` in the same turn, and the user confirms the stake in their next turn.

## Phase 1 — identity Q&A

### Phase preview (render BEFORE Q1)

Once role is `evaluator` and pre-check passed (evaluator is unique per address — if found, hand off to `update` per `role-playbook.md §Pre-check`), render a short declarative preview, then start Q1.

Chinese:
```
好，开始新 evaluator 的 create 流程。接下来会收集以下基本信息：
  1. 名称
  2. 描述
（evaluator 默认不问头像；想设头像直接说。）
```

English:
```
Got it — starting a new evaluator create. We'll collect:
  1. Name
  2. Description
(No avatar prompt by default; just say so if you want to set one.)
```

Preview is declarative; Q1 follows after a blank line.

### Q&A

Questions labelled `Q1：` / `Q1:`. Each Q inlines the four-segment field spec from `field-specs.md` in the user's language only. Skip any Q whose field was already captured via `SKILL.md §One-shot capture`.

| Q | Chinese prompt | English prompt | Validation |
|---|---|---|---|
| Q1 | `Q1：你要注册的 evaluator 叫什么名字？` + 4 segments | `Q1: What's the name of this evaluator?` + 4 segments | non-empty, ≤ 64 chars |
| Q2 | `Q2：用一句话描述你的仲裁领域或专长。` + 4 segments | `Q2: Describe your arbitration domain or expertise in a sentence.` + 4 segments | non-empty, ≤ 500 chars |

No avatar prompt by default (evaluator dashboards rarely show avatars). If the user brings it up, branch to `avatar-upload.md`.

## Phase 2 — confirmation card

Render in the user's language. Pick ONE variant.

Chinese variant:

| 字段 | 值 |
|---|---|
| 角色 | 验证者 (`evaluator`) |
| 名字 | Solidity Auditor |
| 描述 | 仲裁 Solidity 相关任务的争议；5 年审计经验。 |
| 头像 | 默认 |

> 确认无误回复 "执行"。

English variant:

| Field | Value |
|---|---|
| Role | evaluator |
| Name | Solidity Auditor |
| Description | Arbitrates Solidity-related task disputes; 5y audit experience. |
| Picture | default |

> Reply "execute" to run it.

Do **NOT** add a `stake` row here — create does not consume the stake and this skill has no way to verify it. Mentioning stake in the confirmation card implies a gate that does not exist.

**Do NOT** show the bash command. **Do NOT** mention OKB or stake tx hashes inside the confirmation card.

## Execute (maintainer reference — not shown to user)

```bash
onchainos agent create \
  --role evaluator \
  --name "<name>" \
  --description "<description>" \
  [--picture "<url>"]
```

## Post-success suggestion

Render **two visible lines** in the user's language (one of the four variants below), then follow the **agent directive** block (internal — not rendered to the user). Follow the `#<id>` placeholder rule in `display-formats.md` — include the id only if it's actually known.

### Visible lines (four variants)

With id (Chinese):
> Evaluator 身份 #<id> 已注册。
> 要被系统分派仲裁案子还需要完成质押。

Without id (Chinese):
> Evaluator 身份已注册。
> 要被系统分派仲裁案子还需要完成质押。

With id (English):
> Evaluator agent #<id> registered.
> A separate stake is still required before you can be assigned disputes.

Without id (English):
> Evaluator agent registered.
> A separate stake is still required before you can be assigned disputes.

Do NOT mention the `okx-agent-task/evaluator.md` path to the user in the visible lines, and do NOT state a stake amount — the same-turn handoff below will take the user directly into that skill's own prompt, which owns both the path and the amount.

### Agent directive (internal — do NOT render to the user)

After emitting the two visible lines above, **do not stop the turn**. In the same response, load `/skills/okx-agent-task/evaluator.md` §1.5 and continue with `references/evaluator-stake-onboarding.md` Step 1 → Step 2; render its staking confirmation card as the next part of the same response. The stake amount is owned by that skill — the identity skill does not pass one.

**Skip the handoff** (render visible lines only, then stop) if the user has already declined staking in this conversation — see §Good / bad cases row "不想质押".

This is the documented exception to `SKILL.md §Step 4`'s "Stop. Wait for the user." rule; see that section for the carve-out.

---

**Do NOT** chase with status poll (that is about querying chain state — different from the same-turn handoff above, which just loads the next skill's prompt). See `_shared/no-polling.md`.

## Error recovery

Translations and classifications live in `troubleshooting.md`. This section only records the **evaluator-specific skill actions**:

- **Session expired** (CLI-exact, `troubleshooting.md` §1 row 1): render the error card → hand off to `okx-agentic-wallet` for `wallet login`, then re-run the confirmation card. No cached-resume needed — if the conversation is still alive the name/description are still in scope.
- **Name / description validation** (there is no CLI bail for these; if the backend rejects, §2 keyword match): re-ask the offending field in Phase 1.

`stake` / `insufficient` keywords are **not expected on create** — create does not consume the stake. If such a rejection ever appears, surface the raw message verbatim and tell the user staking lives in `/skills/okx-agent-task/evaluator.md`. Do not infer a staking gate on create.

Do not invent error strings here — add new rows to `troubleshooting.md` §1 or §2 first, then reference them from this list.

## Good / bad cases

| User input | Action |
|---|---|
| "我想当仲裁者" | Start Phase 1 turn 1 (ask name). Do NOT dump the staking requirement up front — it belongs in the post-success message, not as a pre-create gate. |
| "我还没质押，能先注册吗" | 可以。Proceed with create. In the post-success message, remind them that没质押拿不到仲裁派单。 |
| "帮我直接质押再注册" | Correct them: 注册在前、质押在后。先完成这里的 create，我再把你引到 `/skills/okx-agent-task/evaluator.md`。 |
| "不想质押" | Offer: evaluator agent 可以先注册着放在那里，但没质押不会被派单，你可以考虑改注册 `requester` / `provider`，或者保留 evaluator 身份等想好了再质押。 |
| "帮我查下我质押没" | Decline — this skill doesn't read stake state. Hand off to the staking flow. |
