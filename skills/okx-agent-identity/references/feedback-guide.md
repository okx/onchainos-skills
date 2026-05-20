# Feedback Submit — Guide

`onchainos agent feedback-submit` has two `--…-id` parameters that look similar but mean different things. Get them wrong and the backend rejects.

| Parameter | Meaning |
|---|---|
| `--agent-id` | The **target** being rated. |
| `--creator-id` | The **caller's own** agentId (any role). This is what gets recorded publicly on-chain against the rating. |

**Consequence:** a user can only rate others after registering their own agent.

**Rating UX is 0–5 stars (integer).** The CLI now accepts 0–5 directly via `--score` and does the `* 20` mapping internally (see `cli/src/commands/agent_commerce/identity/utils.rs::stars_to_score`); `agent feedback-list` also divides the backend response by 20 before returning, so the skill sees stars on both sides. The 0–100 backend wire format is fully encapsulated by the CLI. Skill code just passes the user's star count straight to `--score` — no multiplication, no division.

---

## Full decision tree

### Step 1 — Identify target

Extract the `--agent-id` from the user's prompt.

- "给 #42 打 4 星" → `--agent-id 42 --score 4` (CLI handles the * 20 to 80 internally).
- "给 DeFi Analyzer 打 4 星" → first resolve name to id via `agent search --query "DeFi Analyzer"`, then confirm with the user.
- Legacy phrasings users may still type (`85 分` / `满分` / `差评`) — accept and translate per Step 3 mapping; never echo the 0–100 number back.
- Ambiguous → ask back.

### Step 2 — Identify creator (caller's own agent)

Walk this ladder in order:

1. **Already known in this conversation — AND verified to belong to the currently selected XLayer wallet.** If the user has said "我的 agent 是 #N" or previously created `#N` in this conversation, the cached id is a candidate, but you may only use it **if it belongs to the wallet that will sign this `feedback-submit` tx** (i.e., the currently selected XLayer wallet, same address that ladder 2 narrows to). Wallet-scope guard, in order:
   - If the cached id's `ownerAddress` was already captured in this conversation (from a prior `agent get` / `create` response), compare directly to the current selected wallet address. Match → use it (no lookup needed). Mismatch → **fall through to ladder 2**; do not silently reuse.
   - If the cached id was only mentioned by the user (e.g. "我的 agent 是 #N") without any captured `ownerAddress`, **fall through to ladder 2** — the user's mental model may treat the entire email / JWT as "my agents", which includes agents under other derived wallets that cannot sign this tx. Ladder 2's wrapper filter is what disambiguates.
   - If the user has switched wallets since the cached id was first mentioned (any `okx-agentic-wallet wallet switch` / `wallet add` in between), **fall through to ladder 2** unconditionally — wallet switch invalidates the cache for `--creator-id` purposes even if the id technically still exists.
   When falling through, do NOT echo "I had #N cached but it doesn't belong to the current wallet" as the user-visible explanation by default — just run ladder 2 and surface the new candidate list. Surface the wallet-mismatch reason only if the user explicitly asks "why didn't you use #N?" or if ladder 2 yields 0 candidates and you need to explain why creating an agent under the current wallet is the next step.
2. **Run `onchainos agent get`** (no `--agent-ids`). The response is a **double-layer envelope** (`cli-reference.md §3`): outer `list[*]` is an accountName wrapper (one per derived wallet the JWT caller has visibility into), agent rows live at `list[*].agentList[*]`. Since `--creator-id` must be held by the **same XLayer wallet that will sign this `feedback-submit` tx**, the candidate set is **NOT** all `agentList[*]` across all wrappers — narrow to the single wrapper where `wrapper.ownerAddress == <currently selected XLayer wallet address>`, then count agents in that wrapper's `agentList`:
   - **0 agents under the current wallet** → STOP. Tell the user (in their language; ⛔ no CLI literal, no raw `role` word — Red lines 2 & 4): Chinese: "你当前钱包下还没注册 agent — 得先注册一个（用户 / 服务提供商 / 仲裁者都行）才能给别人打分。要现在就注册吗？" / English: "You don't have an agent under the current wallet yet — you'll need to register one first (any role: User Agent / Agent Service Provider (ASP) / Evaluator Agent) before you can rate others. Want to register one now?" Offer to enter the registration flow. (Other wrappers may have agents — those belong to other related wallets under the same email / JWT, and **cannot** sign this tx; do not list them as candidates.)
   - **1 agent under the current wallet** → silently use its agentId as `--creator-id`; mention the choice in the confirmation (in the user's language): Chinese: "你的 agent #N <name> 会作为这条评价的发起人。" / English: "Your agent #N <name> will be the reviewer for this rating."
   - **Multiple agents under the current wallet** → ask the user which to use, using the numbered-options pattern (`SKILL.md §Choice prompts`) in the user's language. ⛔ Render role labels per `ux-lexicon.md §Role` asymmetric rule (Chinese localizes; English keeps ERC-8004 native term):

     Chinese:
     ```
     你要用哪个 agent 作为这条评价的发起人？
       1. #88 用户  MyBuyer
       2. #99 服务提供商  DeFi Analyzer
     回复对应数字。
     ```

     English:
     ```
     Which of your agents should be the reviewer?
       1. #88 User Agent  MyBuyer
       2. #99 Agent Service Provider (ASP)  DeFi Analyzer
     Reply with the number.
     ```

     Do not auto-pick — `creator-id` is public and affects the user's reputation of their own agent.

### Step 3 — Validate stars (0–5 integer)

> ⛔ **`--score` MUST come from a user reply inside THIS feedback-submit flow** — i.e., a reply produced **after** the current `--agent-id` (target) and `--creator-id` (caller) pair was locked, and **before** the Step 5 confirmation card for the same pair was rendered. **Carrying a star count forward from any other source is an AI hallucination and is forbidden.** Specifically NOT allowed (the model must STOP and ask the star question instead):
>
> - **Reuse from a prior `feedback-submit` round.** "上一轮给 #42 打了 4 星，这轮 #58 也用 4 星" — different target, different rating intent, must re-ask. Even if the user *did* say "都打 4 星" earlier, do not carry the value silently; re-ask for the new target.
> - **Inference from the user's first message.** "给 #42 打个分" / "rate #42" / "给这家伙打分" — the verb "打分 / rate" does NOT contain a star count. Ask Q.
> - **"Same user, same provider, similar context"** — every rating is its own on-chain write; previous ratings (even on the same target) do not authorize a new one.
> - **Default values** — no `3 stars` default, no median, no "looks decent so 4 stars". Stars come from the user this turn, full stop.
> - **One-shot capture caveat.** If the user said "给 #42 打 4 星，理由是交付及时" in a single message during THIS feedback flow, that IS a current-flow user statement of `--score=4` and counts. But once Step 5's confirmation card is rendered and the user replies `执行`, the score is locked; do NOT mutate it.
>
> **Operational test** (apply before invoking the CLI in Step 6): can you point to **the exact user message in this feedback flow** where the star count was stated? If you have to reason "they probably mean…" or "based on earlier we know…" or "it's the same as last time" — that's the signal to STOP and ask. The cost of one extra Q ("给 #<target> 打几星？0–5 星整数") is far below the cost of submitting a wrong on-chain rating that publicly affects both the target's reputation and the caller's `creator-id`.
>
> This rule applies to **every** `feedback-submit` invocation, even in the same conversation, even back-to-back. There is no "we just asked, skip the question this time" exception.

- Integer 0–5. CLI enforces this range natively (`parse_u32_arg(..., Some(0), Some(5), false)`) and rejects anything outside; skill should still pre-validate to surface a friendlier error than the raw CLI bail.
- Reject non-integers, ranges outside 0–5, decimals, "stars" outside the enum.
- Pass the user's star count straight to `--score` — CLI does the `* 20` mapping. Examples:

  | User input | `--score` |
  |---|---|
  | `5 星` / `满分` / `5 stars` / `top rating` | `--score 5` |
  | `4 星` | `--score 4` |
  | `3 星` / `及格` / `一般` | `--score 3` |
  | `2 星` | `--score 2` |
  | `1 星` / `差评` / `最低` | `--score 1` |
  | `0 星` (rare; only if user explicitly says zero) | `--score 0` |

- Fuzzy phrasings (`满分` / `及格` / `差评`) are accepted, mapped per the table, and confirmed back to the user using stars (`★ N`).
- Legacy phrasings: if the user types a raw 0–100 number ("85 分"), translate to the nearest star bucket via `round(score / 20)` with **round-half-up** tie-breaking, then pass that as `--score`. Examples: `100 → 5`, `90 → 5`, `85 → 4`, `80 → 4`, `70 → 4`, `50 → 3`, `30 → 2`, `10 → 1`, `0 → 0`. Never echo the raw 0–100 number back to the user.

### Step 4 — Optional fields

- `--description` — ask: "要写一句评价理由吗？（可跳过）"
- `--task-id` — ask: "这条评分基于哪笔任务 jobId？（可跳过）"
  - `okx-agent-task` jobIds look like `0x…03e8` or `task-001`; accept as a free-form string.
  - Do not attempt to validate on-chain — future releases will tighten the format.

### Step 5 — Final confirmation

> ⛔ `feedback-submit` is an on-chain write — the confirmation card is **mandatory** per `SKILL.md §⛔ MANDATORY confirmation gate (non-overridable)`. Auto-execute preferences, prior in-conversation confirmations of other writes, and "the user obviously wants this" do NOT bypass the gate. Render the card.

Render a 2-column table (not a bash blob), in the user's language. Follow `display-formats.md` §Create/Update Diff style. ⛔ Do NOT mix languages within a single rendering (no `评分 / Rating` bilingual headers, no `服务提供商 (provider)` dual labels) — see `display-formats.md §Create variant` and `ux-lexicon.md §Role`.

Chinese variant:

| 字段 | 值 |
|---|---|
| 发起人 | #88 用户 MyBuyer（你） |
| 目标 | #42 服务提供商 DeFi Analyzer |
| 评分 | ★ 4 |
| 评价 | "交付及时、数据准确" |
| 任务 ID | 0xabc…03e8 |

> 确认无误回复 "执行" 即可。

English variant:

| Field | Value |
|---|---|
| Reviewer | #88 User Agent MyBuyer (you) |
| Target | #42 Agent Service Provider (ASP) DeFi Analyzer |
| Rating | ★ 4 |
| Comment | "Delivered on time, data accurate" |
| Task ID | 0xabc…03e8 |

> Reply "execute" to run.

The rating row shows `★ N` where N is the integer 0–5. Never render `85 / 100` here. Role labels follow `ux-lexicon.md §Role` — both languages localize: Chinese `用户 / 服务提供商 / 仲裁者`; English `User Agent / Agent Service Provider (ASP) / Evaluator Agent`. Never render raw ERC-8004 enum (`requester` / `provider` / `evaluator`) or legacy CN nouns (`买家 / 卖家 / 服务方 / 验证者`).

**Do NOT show the bash command in the confirmation card.** Render it only if the user explicitly asks "把命令给我看".

### Step 6 — Execute (maintainer reference — not shown to user)

> Before invoking the CLI, run the **3-question pre-execute self-check** in `SKILL.md §Step 3: Execute`. For `feedback-submit`, the three questions are: (Q1) was `--creator-id` resolved via **either** ladder 1 (already established in this conversation) **or** ladder 2 (`agent get` enumeration) of `§Step 2` above? (Q2) does the user's **most recent** turn literally contain `执行` / `execute` / `yes` / `好` / `确认` / `go`? (Q3) are all field values in the just-rendered Step 5 card byte-identical to what is about to go to the CLI (target id, creator id, score, description, task-id) **AND was `--score` produced by a user reply inside THIS feedback flow per `§Step 3`'s "Operational test" — not carried over from a prior round, not inferred from a verb-only "打分 / rate" utterance, not a default**? **Any answer ≠ yes → render Step 5's card (or, if Q3 score-origin failed, return to Step 3 and ask the star question) and wait.** Earlier-turn confirm tokens and confirms of different writes do NOT count for Q2. A star count from a **previous** `feedback-submit` flow does NOT count for Q3 even if the model "remembers" it.

```bash
# --score is 0–5 stars (integer). CLI multiplies by 20 internally before
# writing the backend `comment.value`; the 0–100 wire format is fully
# encapsulated by the CLI.
onchainos agent feedback-submit \
  --agent-id <target> \
  --creator-id <self> \
  --score <0-5> \
  [--description "<text>"] \
  [--task-id "<jobId>"]
```

### Step 7 — Post-success

Render the detail outcome and offer exactly **one** next-step suggestion — not a menu (see `display-formats.md` §8):

> 已给 #<target> 打 ★ N（N 是用户选的星数 0–5）。要不要看看 #<target> 最近的评价？我帮你拉 — 按时间倒序，还是按评分高低？

⛔ **No CLI literal / no `--sort-by` flag in the user-visible text** (`SKILL.md §UX Output Red Lines Red line 2`). When the user picks a sort direction in natural language ("按时间" / "评分高低" / "latest" / "highest rating" / etc.), the AI maps it via `cli-reference.md §10` natural-language → `--sort-by` table internally and runs `agent feedback-list` itself — the `--sort-by` / `time_desc` / `score_desc` flag values never appear in the chat. Never echo the raw 0–100 score in the post-success line — say "评价 / 评分" / "rating / reviews".

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
