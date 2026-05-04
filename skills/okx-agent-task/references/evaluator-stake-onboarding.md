# Evaluator Stake Onboarding 流程

> 仅当 evaluator.md 1.5 触发条件命中时打开此文档。包含完整 4-step 质押流程。

**触发：** 其他 skill（身份 / ERC-8004 注册流程）在用户注册完 evaluator 身份后，把上下文交接到本 skill。**身份 skill 不携带金额**——金额由本场景决定。

## 识别条件

两条路径都要能触发：

- **同轮链式**：身份 skill 的输出就在当前 turn 的先前内容里，agent 直接继续进本场景（用户会一次性看到身份注册成功 + 质押确认提示）
- **跨轮触发**：身份 skill 输出后 turn 已结束，用户下一轮回一句话（通常是短确认）才进入

### A. 身份 skill handoff 信号（出现在当前或上一轮的 agent 输出里，任一项命中即可）

- `Evaluator 身份已注册` / `Evaluator 身份 #<id> 已注册`
- `要被系统分派仲裁案子`
- `follow evaluator.md`
- `/skills/okx-agent-task/evaluator.md`（路径字符串出现在上一轮输出里）
- `已注册为 evaluator` / `evaluator 身份注册完成` / `请继续质押流程`
- English: `stake to become evaluator`, `evaluator onboarding stake`

> ⚠️ 身份 skill **不会提供质押数量**。金额完全由本 skill 在 Step 1 实时拉 `staking-config` + `my-stake` 计算。不要把任何具体数字当作路由关键词——即使上一轮出现了数字也不作为匹配条件。

### B. 用户意图信号（当前 turn 用户输入，跨轮路径用）

- `我要质押` / `质押成为仲裁者` / `帮我质押` / `去质押`
- English: `let's stake` / `stake now` / `proceed with staking`
- **短确认仅在 A 之后才算**：`好` / `继续` / `ok` / `go` / `嗯` / `yes` / `好的` / `确认`——只有上一轮明确有 A 信号时才激活。**没有前置 A 信号的短确认不激活本场景。**

> 同轮链式路径下，B 不必要——当前 turn 先前输出里有 A 信号就足以激活，直接跑 Step 1 → Step 2，不要等用户输入。

### C. 反误触保护

不激活的情况：
- 和 evaluator 身份/质押无关的 staking 提及（DeFi staking、其他链的 validator staking、代币质押产品）
- 用户只是*询问*质押相关信息而非要执行（"质押多少钱？"/"质押有什么风险？" → 直接回答问题，不跳进本场景）
- 当前会话里 Step 4 已经跑过一次——不要重复激活

## 动作（严格顺序）

> ⚠️ **核心概念区分**：本场景里有三个金额，**必须分清，不可混用**：
> - **钱包余额** (`wallet balance`)：EOA 上可花费的 OKB，用 `onchainos wallet balance` 查
> - **已质押** (`activeStake`)：已经从余额转入 `VoterStaking` 合约锁仓的 OKB（已扣历史罚没），用 `onchainos agent evaluator my-stake` 查
> - **本次质押 N**：本次要从余额追加锁仓的 OKB
>
> 累计门槛规则的判断是 `activeStake + N >= minCumulativeStakeOkb`，**绝不能用钱包余额代替 `activeStake`**。

### Step 1 — 拉取门槛 + 已质押状态（链上权威值，不读 13 的硬编码默认）

并发执行两条只读 CLI：

```bash
onchainos agent evaluator staking-config   # 取 minCumulativeStakeOkb
onchainos agent evaluator my-stake         # 取 activeStake (OKB) / registered / activeDisputes
```

从 `my-stake` 输出抓 `activeStake: <X> OKB` 行的 `<X>`（就是 OKB 字符串，无需自己换 wei）。从 `staking-config` 抓 `minCumulativeStakeOkb`。

**早退分支（执行前必须先判）**：

| my-stake 输出 | 处理 |
|---|---|
| `registered=false` (`agentId=0`) | 还不是 evaluator，**回到身份 skill 完成注册**，不进 Step 2 |
| `activeStake >= minCumulativeStakeOkb` | 已经满足门槛，告诉用户「你已质押 `<X>` OKB，超过门槛 `<min>`，仲裁者候选状态正常，无需再次质押」，结束本场景 |
| `activeDisputes > 0` 且 `activeStake >= min` | 同上，无需重质押；若用户坚持加质押，引导他用 evaluator.md 12 的 `increase-stake` |

> **累计门槛规则语义**：合约按累计校验 `activeStake + N >= minCumulativeStakeOkb`——首次质押 `activeStake=0` 时 `N >= min`；被 slash 后 `activeStake < min` 时 `N >= min - activeStake` 才能补齐。

### Step 2 — 向用户展示现状、奖罚机制 + 要求用户给出质押数量（⚠️ 强制步骤，不允许跳过）

> ⚠️ **agent 不替用户决定质押金额**。agent 只展示现状（已质押多少、门槛多少、差额多少），由用户自己说要质押的数量。

> ⚠️ 文案里的所有数字都从 Step 1 拉的实时配置注入，**不要写死**。下面用 `<min>` / `<X>`（已质押） / `<feeBps>` / `<minorityBps>` / `<timeoutBps>` / `<cooldownDays>` / `<slashedHours>` 等占位符表达，展示给用户时替换成 Step 1 拉到的真实值。

用纯文本输出，示例（假设 Step 1 拉到 `min=100`、`activeStake=0`、`feeBps=5%`、`minorityBps=1%`、`timeoutBps=0.3%`、`cooldownDays=7`、`slashedHours=24`）：

> 当前你的链上质押：**0 OKB**
> 平台累计门槛：**100 OKB**（`minCumulativeStakeOkb`，合约权威值）
> 还需至少质押：**100 OKB**（`门槛 - 已质押`）
>
> **收益：**
> - 投中多数方 → 按质押比例分仲裁押金（任务金额的 **5%**）+ 少数方被罚的 stake
>
> **风险（罚没）：**
> - 投中少数方 → 罚 stake 的 **1%**
> - Commit / Reveal 超时 → 罚 stake 的 **0.3%**（`TIMEOUT_PENALTY_RATE`），踢出本轮 + **24 小时**冷却期不被选中
> - ⚠️ V1 无弃权选项：被选中必须投票，拖到超时即按超时处理
>
> **解质押规则：**
> - 随时可申请解质押（活跃仲裁期间除外）；申请后进入 **7 天冷却期**，到期跑 `claim-unstake` 提走
> - 冷却期内可跑 `cancel-unstake` 撤回；冷却期内平台仍有权根据过往行为 slash
>
> 请告诉我你要质押多少 OKB（至少 **100**，多于门槛也可以提升选中权重）：
> - 回复**具体数字**（如 **"100"**、**"500"**）→ 用该金额质押
> - 回复 **"取消"** / **"cancel"** → 放弃质押

**硬性规则**：
1. **agent 绝不自行决定质押金额**——不从上下文推断、不用公式算默认值、不"帮用户补齐"。金额**只能是用户在 Step 2 展示后的当轮回复中显式给出的数字**。
2. 未收到用户显式给出数字前，**绝不执行 Step 3 的 CLI**。`evaluator stake` 是上链操作，解质押需冷却期才能取回——静默发起 = 严重违反用户授权。
3. **同轮链式路径也不能跳过 Step 2**——身份 skill 输出在当前 turn 的先前内容里时，可以直接跑 Step 1，但 Step 2 的展示 + 等用户回复数字是**不可省略的**。
4. 以下来源的金额**全部禁止用作质押数量**：上下文中的转账金额、注册费用、gas 费、身份 skill 传来的数字、事件 payload 的 amount、会话历史中任何金额。

### Step 3 — 用户给出数字后执行质押 CLI

记 `min = minCumulativeStakeOkb`、`X = activeStake`（都来自 Step 1）。根据用户回复决定最终 `N`：

| 用户回复 | `N` |
|---|---|
| 纯数字 N'，使 `X + N' >= min` | 用 `N'` |
| 纯数字 N'，使 `X + N' < min` | 告知「累计门槛 `<min>` OKB，当前已质押 `<X>`，本次至少需 `<min - X>`，请加大数额」，回 Step 2 重新问 |
| 取消 / cancel / 不 | 回「已取消质押。需要时再来。」然后结束场景 |
| 确认 / yes / ok（没给数字） | 回「请告诉我具体要质押多少 OKB」，回 Step 2 重新问 |
| 其他文本 | 视作问题；简要回答后重新问 Step 2 |

执行：

```bash
onchainos agent evaluator stake --amount <N>
```

### Step 4 — 解析返回

| CLI 输出 | 告诉用户 |
|---|---|
| `stake submitted ... txHash: 0x...` | 质押已提交上链（`<N>` OKB，txHash=`<hash>`），等待 `staked` 事件确认后你将成为活跃仲裁者候选。**Same-turn handoff** → `/skills/okx-agent-chat/after-agent-list-changed.md`（Execution Flow）。 |
| 错误 `4000` | agentId 无效或 session 丢失，请重新登录钱包。 |
| 错误 `2004` | agentId 还没有 evaluator 身份，需要先回身份 skill 完成注册。 |
| 错误 `1001` | 累计质押门槛 `<min>` OKB（累计门槛规则），`activeStake + 本次` 仍不够。重跑 `my-stake` 确认 `activeStake`，回 Step 2 请用户改数。 |
| 其他错误 | 原样展示错误码和消息，让用户决定是否重试。 |

## 边界

- **Confirmation gate 不可跳过**：无论上下文多"自然"，质押必须经过 Step 2 的用户确认。不允许"为了流畅"省略。
- **本场景只处理首次质押 handoff**。后续 staking 生命周期（补充质押 / 申请解质押 / 领取 / 取消）见 evaluator.md 12。
