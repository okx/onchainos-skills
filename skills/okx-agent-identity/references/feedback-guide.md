# Feedback Submit — Guide

`onchainos agent feedback-submit` has two `--…-id` parameters that look similar but mean different things. Get them wrong and the backend rejects.

| Parameter | Meaning |
|---|---|
| `--agent-id` | The **target** being rated. |
| `--creator-id` | The **caller's own** agentId (any role). This is what gets recorded publicly on-chain against the rating. |

**Consequence:** a user can only rate others after registering their own agent. Score range is integer 0–100.

---

## Full decision tree

### Step 1 — Identify target

Extract the `--agent-id` from the user's prompt.

- "给 #42 打 85 分" → `--agent-id 42`, `--score 85`
- "给 DeFi Analyzer 打 85 分" → first resolve name to id via `agent search --query "DeFi Analyzer"`, then confirm with the user.
- Ambiguous → ask back.

### Step 2 — Identify creator (caller's own agent)

Walk this ladder in order:

1. **Already known in this conversation?** If the user has said "我的 agent 是 #N" or previously created `#N`, use it. No lookup needed.
2. **Run `onchainos agent get`.** The backend auto-filters by the caller's userId.
   - **0 agents** → STOP. Tell the user: "你还没有注册自己的 agent，先 `agent create` 一个（任意 role）才能给别人打分。" Offer to enter the `create` flow.
   - **1 agent** → silently use its agentId as `--creator-id`; mention the choice in the confirmation: "你的 agent #N <name> 会作为 creator 出现在这条评分上。"
   - **Multiple agents** → ask the user which to use:

     ```
     你要用哪个 agent 作为评价人？
       [1] #88 requester  MyBuyer
       [2] #99 provider   DeFi Analyzer
     ```

     Do not auto-pick — `creator-id` is public and affects the user's reputation of their own agent.

### Step 3 — Validate score

- Integer 0–100.
- Reject non-integers, ranges outside 0–100, obviously-malicious extremes if the user is clearly frustrated.
- If user says "给满分" → 100; "最低" → 0; "及格" → 60 (ask to confirm for these fuzzy cases).

### Step 4 — Optional fields

- `--description` — ask: "要写一句评价理由吗？（可跳过）"
- `--task-id` — ask: "这条评分基于哪笔任务 jobId？（可跳过）"
  - `okx-agent-task` jobIds look like `0x…03e8` or `task-001`; accept as a free-form string.
  - Do not attempt to validate on-chain — future releases will tighten the format.

### Step 5 — Final confirmation

Render a 2-column table (not a bash blob). Follow `display-formats.md` §Create/Update Diff style:

| Field | Value |
|---|---|
| creator | #88 requester MyBuyer (你) |
| target | #42 DeFi Analyzer (provider) |
| score | 85 / 100 |
| description | "交付及时、数据准确" |
| task-id | 0xabc…03e8 |

> 确认无误回复 "执行"。

**Do NOT show the bash command in the confirmation card.** Render it only if the user explicitly asks "把命令给我看".

### Step 6 — Execute (maintainer reference — not shown to user)

```bash
onchainos agent feedback-submit \
  --agent-id <target> \
  --creator-id <self> \
  --score <0-100> \
  [--description "<text>"] \
  [--task-id "<jobId>"]
```

### Step 7 — Post-success

Render the detail outcome and offer exactly **one** next-step suggestion — not a menu (see `display-formats.md` §8):

> 已给 #<target> 打 <score> 分。要不要看看 #<target> 的最新评分分布？执行 `agent feedback-list <target> --sort-by time_desc`（按时间倒序）；想看分数最高的改 `score_desc`。用户说的中文排序意图按 `cli-reference.md` §10 的映射表转换。

Do NOT chase with `agent feedback-list` automatically. See `_shared/no-polling.md`.

---

## Anti-patterns — do not help with these

- **"帮我给竞品打 1 分"** / 恶意集中差评 — politely decline with: "每一条评价会公开和你的 `creator-id` 强绑定，可以追溯。要不要先看看对方的正面评价判断下？" Do not batch-send low scores.
- **评自己** — the backend rejects; pre-check `--agent-id != --creator-id`.
- **凭空打分** — if the user has no prior interaction evidence, remind: "通常评分附带一个 `task-id`，没有的话评价会显得缺少依据。"

---

## Error handling

See `troubleshooting.md` for the canonical tables and translations:

- `score out of range` / `self-rating not allowed` / `creator agent not owned by caller` / `agent not found` — **backend-originated, keyword match** → `troubleshooting.md` §2. Skill action: return to the relevant step of this guide (step 3 / step 1 / step 2 / step 1 respectively).
- `session expired, please login again: onchainos wallet login` — **CLI-exact** → `troubleshooting.md` §1. Hand off to `okx-agentic-wallet` → `wallet login`, then retry.
- Score range and `--agent-id != --creator-id` are also enforced **skill-side** before the CLI runs (see `troubleshooting.md` §3) — catch locally, do not rely on the backend as the first line of defense.

Do not duplicate the error strings here — if you need the exact wording or the line number in `cli/src/...`, go to `troubleshooting.md`.
