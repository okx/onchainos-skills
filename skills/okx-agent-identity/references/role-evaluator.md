# Role: evaluator (验证者)

> Registers an arbitrator identity. Requires **100 OKB staked** before `create`. This skill does NOT verify the stake — the backend enforces; `/skills/okx-agent-task/evaluator.md` owns the staking flow.

## STRICT — one question per turn

Fields defined in `field-specs.md`. Inline 用途 / 可见范围 / 约束 / 示例 when asking.

## Flow overview

```
1. Ask name
2. Ask description
3. 质押二选一 card
     Branch A: "①先去质押"       -> cache fields, hand off to staking, stop here
     Branch B: "②已质押直接 create" -> confirmation card -> execute
4. (returning from staking) resume directly at confirmation card
   — do NOT re-ask name/description
```

## Phase 1 — identity Q&A

| Turn | Ask | Validation |
|---|---|---|
| 1 | `Name` | non-empty, ≤ 64 chars |
| 2 | `Description` — "一句话描述你的仲裁领域/专长" | non-empty, ≤ 500 chars |

No avatar prompt by default (evaluator dashboards rarely show avatars). If the user brings it up, branch to `avatar-upload.md`.

## Phase 2 — 质押二选一 card

After capturing `name` + `description`, render this card exactly:

```
Evaluator 需要先质押 100 OKB 才能参与仲裁。

你可以：
  ① 先去质押  — 我把你刚填的 name / description 暂存，质押完回来说 "回来注册 evaluator"，我直接续上
  ② 已质押直接 create  — 如果你已经质押过了，我现在就下发

回复 "1" 或 "2"。
```

### Branch ① — 先去质押

- **Cache** the collected fields in conversation state (you'll recall them when the user comes back). Label the cache so it's obvious: `evaluator-draft: { name: "...", description: "..." }`.
- Hand off: "质押流程在 `/skills/okx-agent-task/evaluator.md`。完成后说 `回来注册 evaluator` 我续上。"
- **Do NOT** execute `create`. **Do NOT** poll the staking status — staking is out of this skill's scope.

### Branch ② — 已质押直接 create

- Trust the user's assertion. Go straight to the confirmation card (Phase 3).
- If the backend rejects with `stake not found` or similar, surface the error card and point them at `/skills/okx-agent-task/evaluator.md`. Do NOT retry automatically.

### Resume keyword — "回来注册 evaluator" / "回来 evaluator" / similar

When the user comes back with this phrasing:

1. Look up the cached `evaluator-draft`.
2. If cache exists → skip directly to Phase 3 confirmation. **Do NOT re-ask name/description.**
3. If cache missing (new conversation, context was lost) → say: "上次的 name/description 已经不在上下文了，我们重新问一下。" Restart Phase 1.

## Phase 3 — confirmation

| Field | Value |
|---|---|
| role | evaluator (验证者) |
| name | Solidity Auditor |
| description | Arbitrates Solidity-related task disputes; 5y audit experience. |
| picture | 默认 |
| stake | 100 OKB（用户已确认） |

> 确认无误回复 "执行"。

**Do NOT** show the bash command. **Do NOT** show the raw stake tx hash in this card — stake verification is backend's job.

## Execute (maintainer reference — not shown to user)

```bash
onchainos agent create \
  --role evaluator \
  --name "<name>" \
  --description "<description>" \
  [--picture "<url>"]
```

## Post-success suggestion

> Evaluator agent #<id> 注册完成，等待系统按 workload 分派仲裁案件。

Optional: "想看看活跃仲裁员的声誉水平作为参考，可以说 `搜索活跃的高分 evaluator`。"

**Do NOT** chase with status poll. See `_shared/no-polling.md`.

## Error recovery

| Backend rejection | Skill action |
|---|---|
| stake missing / insufficient | Error card → "后端拒绝了，因为质押没到位。先去 `/skills/okx-agent-task/evaluator.md` 确认质押，再回来说 `回来注册 evaluator`。" |
| session expired | Error card → `okx-agentic-wallet` login handoff. |
| name / description invalid | Error card → re-ask the offending field (Phase 1). |

## Good / bad cases

| User input | Action |
|---|---|
| "我想当仲裁者" | Start Phase 1 turn 1 (ask name). Do NOT immediately dump the staking requirement — collect name/description first, then show the 二选一 card. |
| "我确认已质押" at Phase 2 | Branch ②. Execute confirmation card. |
| "先去质押" → (later) "回来注册 evaluator" | Branch ① → cache → resume at Phase 3 without re-asking. |
| "不想质押" | Suggest `requester` or `provider` instead: "evaluator 没有质押就没法分派案子，要不要改注册 `requester` / `provider`？" |
| User says "帮我查下我质押没" | Decline — this skill doesn't read stake state. Hand off to the staking flow. |
