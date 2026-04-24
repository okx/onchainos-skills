# Evaluator Stake (Demo)

> **This is a stub for identity-skill integration testing.** No real CLI, no backend,
> no signing, no tx. Just proves the handoff routes here and exercises the confirmation
> gate. Delete when the real skill is wired to a running backend.

## When to Activate

Load this skill if **any** of the following holds. Both of these paths must work:

- **Same-turn chaining**: identity skill's output lands in the current turn; agent
  continues immediately into this skill (user sees identity success + our Step 2
  prompt as one block, replies directly to Step 2)
- **Next-turn chaining**: identity skill's output ended the turn; on the user's next
  reply (typically a short affirmative), agent routes into this skill

### A. Identity-skill handoff signals (anywhere in the immediately preceding output — same turn or prior turn)

Any of these phrases (partial match, case-insensitive):

- `Evaluator 身份已注册` / `Evaluator 身份 #<id> 已注册`
- `要被系统分派仲裁案子`
- `follow evaluator.md`
- `/skills/okx-agent-task/evaluator.md`（路径字符串出现在上一轮输出里）
- `已注册为 evaluator` / `evaluator 身份注册完成` / `请继续质押流程`
- English: `stake to become evaluator`, `evaluator onboarding stake`

> ⚠️ 身份 skill **不会提供质押数量**。金额完全由本 skill 决定（Step 1 默认值；未来从后端拉）。不要把 "100 OKB" / 任何具体数字当作路由关键词——即使上一轮出现了数字也不作为匹配条件。

### B. User intent signals (current turn — for next-turn path only)

- `我要质押` / `质押成为仲裁者` / `帮我质押` / `去质押`
- English: `let's stake` / `stake now` / `proceed with staking`
- **Short affirmatives immediately after A**: `好` / `继续` / `ok` / `go` / `嗯` /
  `yes` / `好的` / `确认` —— only when the previous turn clearly contained an A-signal.
  Short affirmatives without a preceding A-signal DO NOT activate this skill.

> Note: for the **same-turn chaining path**, B is not required. A-signal alone in the
> current turn's preceding content is enough to activate. Do not wait for user input
> before running Step 1 → Step 2 in same-turn mode.

### C. Anti-false-positive guard

Do **NOT** activate on:
- Casual mentions of staking in unrelated contexts (DeFi staking, validator staking for
  other chains, token staking products)
- User asking *about* evaluator staking without intent to do it ("质押多少钱？" →
  answer the question, do not jump into the flow)
- Repeated activation after Step 4 has already completed in the current conversation

## Flow

### Step 1 — Decide default amount

```
默认质押金额 = 100 OKB
```

> TODO (real impl): fetch from `GET /priapi/v1/aieco/task/staking/config` →
> `{minAmount, recommendedAmount}`. Currently hardcoded to the contract-level minimum
> (100 OKB, Lark wiki §8.2 error code 1001).

### Step 2 — Show amount to user + wait for confirmation (MANDATORY)

Output this text verbatim (plain text, no code fences, no emoji):

```
即将质押 100 OKB 激活你的仲裁者候选资格。
- 质押后 7 天内不可解质押（锁定期）
- 被选入陪审后未按时 commit/reveal 会被罚没 0.5–1% stake

确认质押 100 OKB 吗？
- 回复"确认" / "yes" / "ok" → 开始质押
- 回复其他数字（如"500"）→ 用该金额代替（需 ≥ 100）
- 回复"取消" / "cancel" → 放弃质押
```

**Hard rule**: do NOT proceed past Step 2 until the user gives an explicit reply.
The confirmation gate exists even in demo mode to exercise the full handoff flow.

### Step 3 — Parse user reply → final amount `N`

| 用户回复 | 动作 |
|---|---|
| 确认 / yes / ok / 同意 | `N = 100` → go to Step 4 |
| 纯数字 ≥ 100（如 `500`） | `N` = 用户给的数字 → go to Step 4 |
| 纯数字 < 100 | 回复 "首次质押最低 100 OKB，当前 `<X>` 太少，请重新选择金额。" → 回到 Step 2 |
| 取消 / cancel / 不 | 回复 "已取消质押。需要时再来。" → 结束 |
| 其他文本 | 简要回答；再次输出 Step 2 的确认提示 |

### Step 4 — Output canned success (FAKE — DO NOT run any tool)

⚠️ **Do NOT execute Bash. Do NOT call any MCP tool. Do NOT invoke `onchainos` CLI.**
Just output the text below verbatim, substituting `{N}` with the confirmed amount:

```
stake submitted (agentId=demo-evaluator-agent-001)
  amount:  {N} OKB
  voter:   0xEvaluator00000000000000000000000000001
  txHash:  0xDEMO00000000000000000000000000000000000000000000000000000000001

next: 等待 `staked` 事件（VoterStaking.Staked 上链）确认质押生效；
生效后你将成为活跃仲裁者候选，可被选入陪审。
```

Then add a single follow-up line (normal prose):

> 质押 `{N}` OKB 已提交（demo 模式，实际未上链）。对接测试完成。

### Step 5 — End

Do not loop. Do not re-prompt. The onboarding flow terminates after Step 4.

## Boundaries (Demo Mode)

- This skill does NOT invoke `onchainos` CLI, any HTTP API, or any wallet.
- It exists solely to verify the identity-skill → evaluator-skill handoff routes
  correctly and the confirmation gate fires.
- For real user-facing onboarding, use `okx-agent-task` + `evaluator.md §1.5` instead.

## Handoff Contract (for the identity skill team)

Your skill's final turn should end with text containing one of the trigger phrases
listed under **When to Activate**. Example:

```
已完成 ERC-8004 evaluator 身份注册（agentId=xxx, address=0x...）。
请继续质押流程激活候选资格。
```

On the next agent turn, this demo skill will load and run the flow above. No
parameters are passed between skills — the amount is decided on our side and
confirmed with the user (future: fetched from `/staking/config`).

If you need to pass structured data (e.g. a specific recommended amount), the
proposed mechanism is an HTML comment with JSON:

```
<!-- HANDOFF {"next":"okx-evaluator-stake-demo","recommendedAmount":"100"} -->
```

This is not wired up yet — the demo ignores it. Document the format here if your
skill starts emitting it so we can pick it up.
