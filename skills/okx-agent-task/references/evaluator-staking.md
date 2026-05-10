# Evaluator Staking & 经济模型

> evaluator 角色 staking + 经济模型单一权威。覆盖：
> - **质押生命周期**：首次 onboarding（`okx-agent-identity` handoff 后唯一入口）+ 补质押 / 解质押 / 领取 / 取消 / 查询
> - **奖励规则**：投中多数方分仲裁押金 + 少数方罚没的质押
> - **罚没规则**：投中少数方 / Commit / Reveal 超时 + 罚没后冷却期
> - **解质押冷却期**
>
> ⚠️ 所有数字参数（bps / 天数 / 小时数）一律由 `onchainos agent staking-config` 动态拉取，本文件**不写常量**；具体字段名（如 `arbitrationFeeBps`）出现在 §2 文案模板的占位符里，那里同时给出取值步骤。

## §1 场景路由

> 收到入站后匹配下面的场景路由表，命中即跳到对应节按节内说明执行；都不命中则不属于本文件。
>
> 上下文里出现的任何数字**不得用于路由匹配**——路由只看意图信号。

| 场景 | 入站信号 | 进 |
|---|---|---|
| 首次质押 Onboarding | **identity handoff**（上一轮 / 当前 turn 先前内容含：`Evaluator 身份已注册` / `Evaluator 身份 #<id> 已注册` / `要被系统分派仲裁案子`，三条对应 `okx-agent-identity/references/role-evaluator.md` Post-success 的实际输出）；**或 用户意图**（`我要质押` / `质押成为仲裁者` / `帮我质押` / `去质押` / `let's stake` / `stake now` / `proceed with staking`）；**或 短确认**（`好` / `继续` / `ok` / `go` / `嗯` / `yes` / `好的` / `确认`，**仅当上一轮存在 handoff 信号时**才算） | §2 |
| 追加质押 | `追加 / 补充 / 增加质押 <N>` / 被罚后要"补齐" | §3 |
| 申请解质押 | `我要解质押 <N>` / `取回质押` / `赎回质押` | §4 |
| 领取解质押（冷却期满） | `领取解质押` / `取走我的 OKB` | §5 |
| 撤回解质押（冷却期内） | `取消解质押` / `撤回解质押申请` | §6 |
| 查询质押态 | `我现在质押多少` / `查我的质押` / `还能解多少` | §7 |

---

## §2 首次质押 Onboarding

### Step 1 — 并发拉门槛 + 已质押态

```bash
onchainos agent staking-config
onchainos agent my-stake
```

若 `activeStake >= minCumulativeStakeOkb`（已达门槛）：
- 告知用户：「你已质押 `<activeStake>` OKB，超过门槛 `<minCumulativeStakeOkb>`，仲裁者候选状态正常。要追加质押提升选中权重吗？」
- 想追加 → 引导进 §3 走 `increase-stake`
- 不追加 → 结束本场景

### Step 2 — 展示现状 + 等用户给数字（⚠️ 不可省略）

> **硬性规则**：
> 1. agent **不替**用户决定金额：不从上下文推断、不用公式算默认、不"帮用户补齐"。
> 2. 未收到用户**显式数字**前，**绝不执行** Step 3 的 CLI——`stake` 是上链操作，解质押需冷却期才能取回，静默发起 = 严重违反用户授权。
> 3. 即使经由同轮链式 handoff 进入本节，**Step 2 的展示 + 等用户回复数字仍不可省略**。
> 4. 金额**只能**来自**用户当轮显式输入的数字**，其他任何来源一律禁用。

文案模板（**占位符全部替换为 Step 1 拉到的真实值**）：

> 当前你的链上质押：**`<activeStake>` OKB**
> 平台累计门槛：**`<minCumulativeStakeOkb>` OKB**
> 还需至少质押：**`<minCumulativeStakeOkb - activeStake>` OKB**
>
> **收益：**
> - 投中多数方 → 按质押比例分仲裁押金（任务金额的 **`<arbitrationFeeBps>`**）+ 少数方被罚没的质押
>
> **风险（罚没）：**
> - 投中少数方 → 罚没 **`<slashMinorityBps>`** 的质押
> - Commit / Reveal 超时 → 罚没 **`<slashTimeoutBps>`** 的质押，踢出本轮 + **`<slashedCooldownHours>` 小时**冷却期不被选中
> - ⚠️ 无弃权选项：被选中必须投票，拖到超时即按超时处理
>
> **解质押规则：**
> - 随时可申请解质押（活跃仲裁期间除外）；申请后进入 **`<unstakeCooldownDays>` 天冷却期**，到期后告诉我"领取解质押"即可提走
> - 冷却期内告诉我"取消解质押"可撤回申请
>
> 请告诉我你要质押多少 OKB（至少 **`<minCumulativeStakeOkb - activeStake>`**，多于门槛可提升选中权重）：
> - 回复**具体数字** → 用该金额质押
> - 回复 **"取消"** / **"cancel"** → 放弃质押

### Step 3 — 收到用户回复后决定 N 并执行

用户回复**纯数字** → 用该数字作 `N` 跑 CLI（CLI 内部强制阈值 / 路由 / 异常文案）。其他回复按下表处理：

| 用户回复 | 处理 |
|---|---|
| 取消 / cancel / 不 | 「已取消质押。需要时再来。」→ 结束 |
| 确认 / yes / ok（无数字） | 「请告诉我具体要质押多少 OKB」→ 回 Step 2 |

```bash
onchainos agent stake --amount <N>
```

### Step 4 — 成功后 handoff

CLI exit code = 0 且 stdout 含 `stake submitted` 时，**same-turn handoff** 到 `/skills/okx-agent-chat/after-agent-list-changed.md` 的 Execution Flow（agent list 状态变了，要同步 OpenClaw）。

---

## §3 Increase-stake / 追加质押

**触发**：用户主动追加（被罚后补齐 / 自愿加大选中权重）。

### Step 1 — 确认金额

向用户复述：「将追加 **`<N>` OKB**，确认？」用户**明确确认**后才进 Step 2。`<N>` 必须由用户给出，**不由** agent 推算。

### Step 2 — 执行

```bash
onchainos agent increase-stake --amount <N>
```

---

## §4 Request-unstake / 申请解质押

**触发**：用户主动申请解质押。

### Step 1 — 拉 cooldown 天数（用于确认文案）

```bash
onchainos agent staking-config   # 取 unstakeCooldownDays
```

### Step 2 — 确认金额

「将申请解质押 **`<N>` OKB**。申请后进入 **`<unstakeCooldownDays>` 天冷却期**：期间可撤回申请；到期后再领取。**冷却期不可提前结束**。确认？」

### Step 3 — 执行

```bash
onchainos agent request-unstake --amount <N>
```

---

## §5 Claim-unstake / 领取（冷却期满）

无金额参数，用户**明确命令**后直接执行。CLI 内部判断冷却期是否到期。

```bash
onchainos agent claim-unstake
```

---

## §6 Cancel-unstake / 撤回（冷却期内）

无金额参数，用户**明确命令**后直接执行。CLI 内部判断是否仍在冷却期内。

```bash
onchainos agent cancel-unstake
```

---

## §7 My-stake / 只读查询

```bash
onchainos agent my-stake
```

只读查询，无需确认，直接执行后把关键字段摘给用户：

| 字段 | 含义 |
|---|---|
| `activeStake` | 当前已质押 OKB |
| `pendingUnstake` | 冷却期中待解锁 OKB |
| `validStake` | 可加权选取的有效质押 = `activeStake - pendingUnstake` |
| `activeDisputes` | 参与中的仲裁数；`>0` 时禁止解质押 |
| `unstakeAvailableAt` | 解质押冷却期结束 unix 秒；`0` = 无待解锁 |
| `cooldownEndsAt` | 罚没冷却期结束 unix 秒（被罚没后不被选中的窗口）；`0` = 不在该冷却期 |
