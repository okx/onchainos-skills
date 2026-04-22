# Role: evaluator (验证者)

> Registers an arbitrator identity. `create` itself does not require the OKB stake — the backend accepts the registration regardless. Staking 100 OKB is what makes the evaluator eligible to be assigned to real disputes; that flow lives in `/skills/okx-agent-task/evaluator.md`. This skill never verifies or reads stake state.

## STRICT — one question per turn

Fields defined in `field-specs.md`. Inline 用途 / 可见范围 / 约束 / 示例 when asking.

## Flow overview

```
1. Ask name
2. Ask description
3. Confirmation card → execute create
4. Post-success card + 一行引导：去质押 100 OKB 才能接仲裁派单
```

No pre-create staking gate. No cached-resume flow. Registration is cheap; staking is a deliberate later step the user triggers from `okx-agent-task`.

## Phase 1 — identity Q&A

| Turn | Ask | Validation |
|---|---|---|
| 1 | `Name` | non-empty, ≤ 64 chars |
| 2 | `Description` — "一句话描述你的仲裁领域/专长" | non-empty, ≤ 500 chars |

No avatar prompt by default (evaluator dashboards rarely show avatars). If the user brings it up, branch to `avatar-upload.md`.

## Phase 2 — confirmation card

| Field | Value |
|---|---|
| role | evaluator (验证者) |
| name | Solidity Auditor |
| description | Arbitrates Solidity-related task disputes; 5y audit experience. |
| picture | 默认 |

> 确认无误回复 "执行"。

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

Two lines, in order:

> Evaluator agent #<id> 已注册。

> 要真正被系统分派仲裁案子，还需要去 `/skills/okx-agent-task/evaluator.md` 质押 100 OKB。质押是独立流程，这条 skill 不帮你读质押状态。

Optional follow-up (offer, don't force): "想参考下活跃仲裁员的声誉水平，可以说 `搜索活跃的高分 evaluator`。"

**Do NOT** chase with status poll. See `_shared/no-polling.md`.

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
