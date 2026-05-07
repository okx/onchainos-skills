# Evaluator Staking

> evaluator 角色全部 staking 相关流程的单一权威文档。覆盖：首次质押 onboarding（identity handoff 后唯一入口）+ 后续 staking 生命周期（补质押 / 解质押 / 领取 / 取消）+ 共享业务规则 + 错误码。
>
> 路由：identity skill `agent create --role evaluator` 成功后 same-turn handoff 进 §2（onboarding）；用户主动发起补质押 / 解质押 / 领取 / 取消 → 进 §3（lifecycle）。仲裁判决方法论不在本文，见 `evaluator-decision-rubric.md`。

---

## §1 共享概念（先读）

### §1.1 三个金额必须分清

| 概念 | 含义 | 怎么查 |
|---|---|---|
| **钱包余额** (`wallet balance`) | EOA 上可花费的 OKB | `onchainos wallet balance` |
| **已质押** (`activeStake`) | 已锁仓到 `VoterStaking` 合约的 OKB（已扣历史罚没） | `onchainos agent my-stake` |
| **本次质押 N** | 本次要从余额追加锁仓的 OKB | 用户在 §2 Step 2 显式给出 |

⚠️ 累计门槛规则的判断永远是 `activeStake + N >= minCumulativeStakeOkb`，**绝不能用钱包余额代替 `activeStake`**。

### §1.2 累计门槛规则

合约按**累计**校验：`activeStake + 本次 >= minCumulativeStakeOkb`（从 `staking-config` 拉）。

- `stake` 用于首次（`activeStake = 0` → 一次到位 ≥ `min`）
- `increase-stake` 用于补齐（`activeStake < min` → 本次 ≥ `min - activeStake`）

### §1.3 部分赎回保留规则

部分赎回后剩余质押必须 ≥ `partialUnstakeMinRetainOkb`（从 `staking-config` 拉），否则只允许全额赎回。

向用户确认金额前，先 `my-stake` + `staking-config`：
- 若 `activeStake - 本次 < partialUnstakeMinRetainOkb 且 > 0` → 提醒「部分赎回后余额将低于最低保留 `<retain>` OKB，建议改为全额赎回」
- 本地不阻塞，合约侧兜底

### §1.4 冷却期 + 活跃仲裁约束

- **解质押冷却期**：`request-unstake` 后进入 `unstakeCooldownSeconds` 冷却（从 `staking-config` 拉，通常 7 天），到期才能 `claim-unstake`
- **冷却期内**可 `cancel-unstake` 撤回；过期则链上 unstake 已 claimable，撤不回
- **冷却期由合约记录，不可缩短**
- **活跃仲裁期间**（`activeDisputes > 0`）合约会 revert `request-unstake`；调用前先 `my-stake` 看 `activeDisputes`，> 0 先提醒用户等裁决完成
- **罚没冷却**：`slashed` 后进入 `slashedCooldownSeconds` 冷却（`cooldownEndsAt`），期间不被选为陪审

### §1.5 数值实时拉，不写死

阈值（`minCumulativeStakeOkb` / `partialUnstakeMinRetainOkb` / `unstakeCooldownSeconds` / `slashMinorityBps` / `slashTimeoutBps` / `slashedCooldownSeconds` / `arbitrationFeeBps`）→ 运行时 `agent staking-config`。

账户态（`activeStake` / `pendingUnstake` / `validStake` / `activeDisputes` / `unstakeAvailableAt` / `cooldownEndsAt`）→ 运行时 `agent my-stake`。

文档里的占位符 `<min>` / `<retain>` / `<X>` / `<feeBps>` 等仅作概念示意，**不得当真实默认值给用户**。

---

## §2 首次质押 Onboarding（identity handoff 后唯一入口）

> 仅当 §2.A 触发识别命中时打开本节（identity skill `agent create --role evaluator` 后 same-turn handoff，或用户跨轮显式说"我要质押"）。

**触发：** 身份 skill（ERC-8004 注册流程）在用户注册完 evaluator 身份后把上下文交接到本 skill。**身份 skill 不携带金额**——金额由本节决定。

### §2.A 触发识别（两条路径任一命中即可）

- **同轮链式**：身份 skill 的输出就在当前 turn 的先前内容里，agent 直接继续进本场景（用户一次性看到身份注册成功 + 质押确认）
- **跨轮触发**：身份 skill 输出后 turn 已结束，用户下一轮回一句话（通常是短确认）才进入

#### A. 身份 skill handoff 信号（出现在当前或上一轮的 agent 输出里）

- `Evaluator 身份已注册` / `Evaluator 身份 #<id> 已注册`
- `要被系统分派仲裁案子`
- `follow evaluator.md`
- `/skills/okx-agent-task/evaluator.md`（路径字符串出现在上一轮输出里）
- `已注册为 evaluator` / `evaluator 身份注册完成` / `请继续质押流程`
- English: `stake to become evaluator`, `evaluator onboarding stake`

> ⚠️ 身份 skill **不会提供质押数量**。金额完全由本 skill 在 Step 1 实时拉 `staking-config` + `my-stake` 计算。不要把任何具体数字当作路由关键词——即使上一轮出现了数字也不作为匹配条件。

#### B. 用户意图信号（当前 turn 用户输入，跨轮路径用）

- `我要质押` / `质押成为仲裁者` / `帮我质押` / `去质押`
- English: `let's stake` / `stake now` / `proceed with staking`
- **短确认仅在 A 之后才算**：`好` / `继续` / `ok` / `go` / `嗯` / `yes` / `好的` / `确认`——只有上一轮明确有 A 信号时才激活。**没有前置 A 信号的短确认不激活本场景。**

> 同轮链式路径下，B 不必要——当前 turn 先前输出里有 A 信号就足以激活，直接跑 Step 1 → Step 2，不要等用户输入。

#### C. 反误触保护

不激活的情况：
- 和 evaluator 身份/质押无关的 staking 提及（DeFi staking、其他链的 validator staking、代币质押产品）
- 用户只是*询问*质押相关信息而非要执行（"质押多少钱？"/"质押有什么风险？" → 直接回答问题，不跳进本场景）
- 当前会话里 Step 4 已经跑过一次——不要重复激活

### §2 动作（严格顺序）

#### Step 1 — 拉取门槛 + 已质押状态（链上权威值）

并发执行两条只读 CLI：

```bash
onchainos agent staking-config   # 取 minCumulativeStakeOkb
onchainos agent my-stake         # 取 activeStake (OKB) / registered / activeDisputes
```

从 `my-stake` 输出抓 `activeStake: <X> OKB` 行的 `<X>`（OKB 字符串，无需自己换 wei）。从 `staking-config` 抓 `minCumulativeStakeOkb`。

**早退分支（执行前必须先判）**：

| my-stake 输出 | 处理 |
|---|---|
| `activeStake >= minCumulativeStakeOkb` | 已经满足门槛，告诉用户「你已质押 `<X>` OKB，超过门槛 `<min>`，仲裁者候选状态正常，无需再次质押」，结束本场景 |
| `activeDisputes > 0` 且 `activeStake >= min` | 同上，无需重质押；若用户坚持加质押，引导他去 §3 走 `increase-stake` |

#### Step 2 — 向用户展示现状、奖罚机制 + 要求用户给出质押数量（⚠️ 强制步骤，不允许跳过）

> ⚠️ **agent 不替用户决定质押金额**。agent 只展示现状（已质押多少、门槛多少、差额多少），由用户自己说要质押的数量。
>
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
> - ⚠️ 无弃权选项：被选中必须投票，拖到超时即按超时处理
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
2. 未收到用户显式给出数字前，**绝不执行 Step 3 的 CLI**。`stake` 是上链操作，解质押需冷却期才能取回——静默发起 = 严重违反用户授权。
3. **同轮链式路径也不能跳过 Step 2**——身份 skill 输出在当前 turn 的先前内容里时，可以直接跑 Step 1，但 Step 2 的展示 + 等用户回复数字是**不可省略的**。
4. 以下来源的金额**全部禁止用作质押数量**：上下文中的转账金额、注册费用、gas 费、身份 skill 传来的数字、事件 payload 的 amount、会话历史中任何金额。

#### Step 3 — 用户给出数字后执行质押 CLI

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
onchainos agent stake --amount <N>
```

#### Step 4 — 解析返回

| CLI 输出 | 告诉用户 |
|---|---|
| `stake submitted ... txHash: 0x...` | 质押已提交上链（`<N>` OKB，txHash=`<hash>`），等待 `staked` 事件确认后你将成为活跃仲裁者候选。**Same-turn handoff** → `/skills/okx-agent-chat/after-agent-list-changed.md`（Execution Flow）。 |
| 错误码 | 见 §4 错误码速查 |

### §2.边界

- **Confirmation gate 不可跳过**：无论上下文多"自然"，质押必须经过 Step 2 的用户确认。不允许"为了流畅"省略。
- **本节只处理首次质押 handoff**。后续 staking 生命周期（补充质押 / 申请解质押 / 领取 / 取消）见 §3。

---

## §3 后续 Staking 操作（用户主动发起）

> 用户主动发起 staking 操作（补质押 / 解质押 / 领取 / 取消）时进本节。§2 只负责首次质押 handoff；本节其余操作不自动触发。

### §3.1 触发词 → CLI 映射

| 用户意图信号 | CLI |
|---|---|
| `我要追加 / 补充 / 增加质押 <N> OKB` / 被罚后要"补齐" | `onchainos agent increase-stake --amount <N>` |
| `我要解质押 <N> OKB` / `取回质押` / `赎回质押` | `onchainos agent request-unstake --amount <N>` |
| 冷却期结束后：`领取解质押` / `取走我的 OKB` | `onchainos agent claim-unstake` |
| 冷却期内改主意：`取消解质押` / `撤回解质押申请` | `onchainos agent cancel-unstake` |
| `我现在质押多少` / `查我的质押` / `还能解多少` | `onchainos agent my-stake`（只读，无确认门禁） |

### §3.2 确认门禁

- `increase-stake` / `request-unstake` 都是上链操作，执行前**必须让用户确认金额**
- `claim-unstake` / `cancel-unstake` 无金额参数，可在用户明确命令后直接执行
- **所有金额判断都先调一次 `my-stake`** 拿到 `activeStake` / `pendingUnstake` / `validStake` / `activeDisputes` 实时值，**绝不依赖会话里之前缓存的数字**——质押状态可能在外部交易后变化

### §3.3 业务约束（详见 §1）

- `request-unstake`：活跃仲裁期间合约会 revert，先看 `activeDisputes`（§1.4）；部分赎回保留规则见 §1.3
- `stake` / `increase-stake`：累计门槛规则见 §1.2
- `cancel-unstake`：只在冷却期内有效（§1.4）

### §3.4 事件回调处理

上面四个 CLI 执行完后都会收到对应 tx 回执事件（`staked` / `unstake_requested` / `unstake_claimed` / `unstake_cancelled`，被动还有 `stake_stopped` / `cooldown_entered` / `slashed`）。

**所有事件统一按 `evaluator.md §1` 调 `next-action`**，CLI 输出会要求你在 sub 侧调用 `xmtp_dispatch_user` 把人话通知推给用户（**禁止 sessions_send / 直接输出给用户**；evaluator 不用 `xmtp_prompt_user`）。

⚠️ **后端推送的 `envelope.message` 只有 `event` / `jobId` / `timestamp` / `source` / `description` 五个字段**（`jobId` 固定为 `system_voter_staking`，不是真任务）——**不带 `amount` / `availableAt` / `txHash` / `status` / `errorCode`**。需要播报数值或时间，必须先调 `my-stake --agent-id <你的 agentId>` 拉权威值再 dispatch。

| 事件 | next-action 输出会让你做什么 |
|---|---|
| `staked` | 跑 `my-stake` 拿 `activeStake` → dispatch `[质押 ✅] 当前 activeStake=<X> OKB` |
| `unstake_requested` | 跑 `my-stake` 拿 `pendingUnstake` / `unstakeAvailableAt` → dispatch `[解质押 ⏳] 待解 <pending> OKB；<availableAt> 可领取` |
| `unstake_claimed` | dispatch `[解质押 ✅] 已领取，OKB 已入钱包` |
| `unstake_cancelled` | dispatch `[解质押 ✅] 已取消：待解 OKB 回到质押状态` |
| `stake_stopped` | dispatch `[质押 🚪] 已退出 voter 池，不再被选为陪审` |
| `cooldown_entered` | 跑 `my-stake` 拿 `cooldownEndsAt` → dispatch `[冷却 ⏸️] <cooldownEndsAt> 前不会被选为陪审` |
| `slashed` | 跑 `my-stake` 拿 post-slash `activeStake` → dispatch `[Stake 罚没 ⚠️] jobId=<jobId>，stake 已被扣罚；剩余 activeStake=<X> OKB` |

---

## §4 错误码速查

| 错误码 | 含义 | 处理 |
|---|---|---|
| `4000` | agentId 无效或 session 丢失 | 让用户重新登录钱包 |
| `2004` | agentId 还没有 evaluator 身份 | 让用户先回身份 skill 完成注册 |
| `1001` | 累计质押门槛不够（`activeStake + 本次 < minCumulativeStakeOkb`） | 重跑 `my-stake` 确认 `activeStake`，回 §2 Step 2 / §3 让用户改数 |
| 其他 | 未知错误 | 原样展示错误码和消息，让用户决定是否重试 |