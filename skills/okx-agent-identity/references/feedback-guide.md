# Feedback Submit — Guide

`onchainos agent feedback-submit` has two `--…-id` parameters that look similar but mean different things. Get them wrong and the backend rejects.

| Parameter | Meaning |
|---|---|
| `--agent-id` | The **target** being rated. |
| `--creator-id` | The **caller's own** agentId (any role). This is what gets recorded publicly on-chain against the rating. |

**Consequence:** a user can only rate others after registering their own agent.

**Rating UX is 0–5 stars (integer).** The CLI / backend wire format remains 0–100 integer; the skill translates between the two. Mapping: `0★→0`, `1★→20`, `2★→40`, `3★→60`, `4★→80`, `5★→100`. Never expose the raw 0–100 score to the user — all user-facing prompts, confirmation cards, post-success lines, error messages, and detail / list / feedback / search renderings use stars only. The 0–100 number appears only in the maintainer bash block (which is hidden from end users) and in CLI / backend logs.

---

## Full decision tree

### Step 1 — Identify target

Extract the `--agent-id` from the user's prompt.

- "给 #42 打 4 星" → `--agent-id 42`, internally maps to `--score 80`
- "给 DeFi Analyzer 打 4 星" → first resolve name to id via `agent search --query "DeFi Analyzer"`, then confirm with the user.
- Legacy phrasings users may still type (`85 分` / `满分` / `差评`) — accept and translate per Step 3 mapping; never echo the 0–100 number back.
- Ambiguous → ask back.

### Step 2 — Identify creator (caller's own agent)

Walk this ladder in order:

1. **Already known in this conversation?** If the user has said "我的 agent 是 #N" or previously created `#N`, use it. No lookup needed.
2. **Run `onchainos agent get`** (no `--agent-ids`). In default list mode the backend returns the caller's **own** agents — pick one of these as `--creator-id`. (`agent get --agent-ids X` is the open-lookup mode for any agent's record and is irrelevant here.)
   - **0 agents** → STOP. Tell the user: "你还没有注册自己的 agent，先 `agent create` 一个（任意 role）才能给别人打分。" Offer to enter the `create` flow.
   - **1 agent** → silently use its agentId as `--creator-id`; mention the choice in the confirmation: "你的 agent #N <name> 会作为 creator 出现在这条评分上。"
   - **Multiple agents** → ask the user which to use, using the numbered-options pattern (`SKILL.md §Choice prompts`) in the user's language:

     Chinese:
     ```
     你要用哪个 agent 作为评价人？
       1. #88 requester  MyBuyer
       2. #99 provider   DeFi Analyzer
     回复对应数字。
     ```

     English:
     ```
     Which of your agents should be the reviewer?
       1. #88 requester  MyBuyer
       2. #99 provider   DeFi Analyzer
     Reply with the number.
     ```

     Do not auto-pick — `creator-id` is public and affects the user's reputation of their own agent.

### Step 3 — Validate stars (0–5 integer)

- Integer 0–5. Skill validates before sending; CLI is unaware (it only accepts 0–100).
- Reject non-integers, ranges outside 0–5, decimals, "stars" outside the enum.
- Mapping table (skill applies before invoking the CLI):

  | User input | Stars | `--score` sent to CLI |
  |---|---|---|
  | `5 星` / `满分` / `5 stars` / `top rating` | 5 | 100 |
  | `4 星` | 4 | 80 |
  | `3 星` / `及格` / `一般` | 3 | 60 |
  | `2 星` | 2 | 40 |
  | `1 星` / `差评` / `最低` | 1 | 20 |
  | `0 星` (rare; only if user explicitly says zero) | 0 | 0 |

- Fuzzy phrasings (`满分` / `及格` / `差评`) are accepted, mapped per the table, and confirmed back to the user using stars (`★ N`), never `<score> / 100`.
- If the user types a raw 0–100 number ("85 分"), translate **silently** into the star bucket via `round(score / 20)` with **round-half-up** tie-breaking — the same rule the display layer uses for per-review rendering, so a single backend score always projects to the same star count regardless of which side of the flow you're on. Examples: `100 → 5`, `90 → 5` (round(4.5)=5), `85 → 4`, `80 → 4`, `75 → 4` (round(3.75)=4), `70 → 4` (round(3.5)=4, **half-up**), `65 → 3` (round(3.25)=3), `50 → 3` (round(2.5)=3, **half-up**), `30 → 2`, `10 → 1` (round(0.5)=1), `0 → 0`. Echo back the star count for confirmation, never the raw number. Canonical rule lives in `SKILL.md §Amount Display Rules` reputation bullet — keep this guide in sync with that source.

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
| 评分 / Rating | ★ 4 |
| description | "交付及时、数据准确" |
| task-id | 0xabc…03e8 |

> 确认无误回复 "执行" 即可。

The rating row shows `★ N` where N is the integer 0–5. Never render `85 / 100` here. Localize the row label per `SKILL.md §Language matching` — `评分` for Chinese users, `Rating` for English users.

**Do NOT show the bash command in the confirmation card.** Render it only if the user explicitly asks "把命令给我看".

### Step 6 — Execute (maintainer reference — not shown to user)

```bash
# Skill maps user's 0–5 stars to 0/20/40/60/80/100 before invocation.
# Maintainers running this CLI directly still pass the raw 0–100 integer.
onchainos agent feedback-submit \
  --agent-id <target> \
  --creator-id <self> \
  --score <0-100> \
  [--description "<text>"] \
  [--task-id "<jobId>"]
```

### Step 7 — Post-success

Render the detail outcome and offer exactly **one** next-step suggestion — not a menu (see `display-formats.md` §8):

> 已给 #<target> 打 ★ N（N 是用户选的星数 0–5）。要不要看看 #<target> 的最新评价？执行 `agent feedback-list --agent-id <target> --sort-by time_desc`（按时间倒序）；想看评分最高的改 `score_desc`。用户说的中文排序意图按 `cli-reference.md` §10 的映射表转换。Never echo the raw 0–100 score in the post-success line.

Do NOT chase with `agent feedback-list` automatically. See `_shared/no-polling.md`.

---

## Anti-patterns — do not help with these

- **"帮我给竞品打 1 星"** / 恶意集中差评 — politely decline with: "每一条评价会公开和你的 `creator-id` 强绑定，可以追溯。要不要先看看对方的好评判断下？" Do not batch-send low ratings.
- **评自己** — the backend rejects; pre-check `--agent-id != --creator-id`.
- **凭空打分** — if the user has no prior interaction evidence, remind: "通常评分附带一个 `task-id`，没有的话评价会显得缺少依据。"

---

## Error handling

See `troubleshooting.md` for the canonical tables and translations:

- `score out of range` / `self-rating not allowed` / `creator agent not owned by caller` / `agent not found` — **backend-originated, keyword match** → `troubleshooting.md` §2. Skill action: return to the relevant step of this guide (step 3 / step 1 / step 2 / step 1 respectively). Translate `score out of range` to user with stars wording — never echo the 0–100 bound.
- `session expired, please login again: onchainos wallet login` — **CLI-exact** → `troubleshooting.md` §1. Hand off to `okx-agentic-wallet` → `wallet login`, then retry.
- Star range (0–5 integer) and `--agent-id != --creator-id` are also enforced **skill-side** before the CLI runs (see `troubleshooting.md` §3) — catch locally, do not rely on the backend as the first line of defense.

Do not duplicate the error strings here — if you need the exact wording or the line number in `cli/src/...`, go to `troubleshooting.md`.
