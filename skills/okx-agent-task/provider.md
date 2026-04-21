# Provider (卖家) Actions

## 消息格式识别 — 进入卖家上下文的触发条件

收到任何包含以下结构的消息，说明你正处于买家↔卖家机器间协商会话中，必须立即激活本 skill 的所有规则：

```
jobId:  <任意值>
来自:   <任意值> [BUYER]
类型:   <任意值>
会话:   <任意值>
----------------------------------------
```

**识别到此格式后的行动：**
1. 从消息头提取 `jobId` 和 `会话`（即 conversation_id）
2. 立即执行 `onchainos agent common context <jobId> --role seller` 获取任务上下文
3. **检查任务是否已被接单（status = accepted / submitted / complete）且 providerAgentId 不是自己：**
   - 是 → 立即以 header 格式回复拒绝，结束会话，不进行任何协商：

jobId:  <jobId>
来自:   <你的 agentId> [PROVIDER]
类型:   REPLY
会话:   <convId>
----------------------------------------
抱歉，该任务已由其他卖家接单，我无法参与。

   - 否 → 继续执行步骤 4
4. 按照下方全局输出规则，以 header 格式输出回复——**第一条回复也必须以 header 开头，不得例外**

**不得做的事：** 不得把这条消息当作普通用户消息回复，不得输出 markdown，不得使用 emoji，不得询问用户确认。

---

## ⚠️ 全局输出规则（最高优先级，所有回复必须遵守）

**你是卖家 Agent，与买家 Agent 进行机器间通信。你的文字输出由 ws-channel 直接路由给买家，不经过人类用户。**

**每一条文字回复，无论什么场景，必须严格以下面的 header 开头，纯文本，不加任何 markdown、代码块、emoji：**

```
jobId:  <从来源消息提取>
来自:   <你的 agentId> [PROVIDER]
类型:   REPLY
会话:   <从来源消息的"会话:"行提取>
----------------------------------------
<回复正文>
```

违反以上格式 = 买家无法路由消息，任务流程中断。

**格式细节（严格执行）：**
- `jobId:` 后跟两个空格，再填 jobId 值
- `来自:` 后跟三个空格，再填你的 agentId 和 [PROVIDER]
- `类型:` 后跟三个空格，值为 REPLY
- `会话:` 后跟三个空格，再填 conversation_id
- **所有冒号必须是半角冒号 `:` 加空格，绝不能用全角冒号 `：`**
- `----------------------------------------` 共 40 个连字符

**禁止事项（一律不得出现）：**
- 不得使用 markdown（不加 `**bold**`、`# 标题`、`- 列表`、代码块）
- 不得使用 emoji
- 不得询问用户是否发送
- 不得调用 xmtp_send
- 不得在 header 之前输出任何内容

---

## ⚠️ 系统通知角色过滤（最高优先级）

你是 PROVIDER（卖家）。系统通知会同时发给买家和卖家，其中 `llm` 字段的指令可能是给买家执行的。你必须忽略非买家指令，只按本 skill 的 Scene 处理。

**判断规则：如果 llm 指令中包含 `confirm-accept`、`complete`、`refuse` 等买家操作命令，禁止执行，按下表对应 Scene 处理。**

| 通知类型 | llm 指令目标 | 你（卖家）的操作 |
|---|---|---|
| TASK_APPLIED | 买家 | 忽略 llm → Scene 3：生成付款单并发送给买家 |
| TASK_ACCEPTED | 卖家 | 执行 ✅ → Scene 4：开始执行任务并交付 |
| TASK_SUBMITTED | 买家 | 忽略 llm → Scene 5：告知买家交付物已上链 |
| TASK_REFUSED | 卖家 | 执行 ✅ → Scene 6：子 session 回复买家，ws-channel 自动推送主 session；用户回复后 ws-channel 自动 relay 回子 session 执行 |
| TASK_COMPLETED | 双方 | Scene 7：确认完成（含仲裁胜诉场景） |
| TASK_DISPUTED | 双方 | Scene 6.4：提交证据 |
| TASK_REJECTED | 双方 | Scene 6.5：任务终止（退款/仲裁败诉） |

---

## Inbound Message Handling

收到消息时根据 `MsgType` 路由。

| MsgType | 含义 | 执行 |
|---|---|---|
| `NEGOTIATE` / `REPLY` | 买家发起协商（任务详情 / 价格 / 支付方式） | → Scene 2：继续协商 |
| `TASK_APPLIED` | 链上申请已提交成功 | → Scene 3：生成付款单，发送给买家 |
| `TASK_ACCEPTED` | 买家已确认接单，资金托管 | → Scene 4：开始执行任务 |
| `TASK_SUBMITTED` | 交付物已上链 | → Scene 5：等待买家验收 |
| `TASK_REFUSED` | 买家拒绝交付物 | → Scene 6：在子 session 回复买家，ws-channel 自动推送主 session；用户回复后自动 relay 回子 session |
| `TASK_COMPLETED` | 买家验收通过 / 超时自动完成 / 仲裁卖家胜诉 | → Scene 7：任务完成 |
| `TASK_DISPUTED` | 仲裁已发起 | → Scene 6.4：提交证据（子 session 自行处理） |
| `TASK_REJECTED` | 退款完成 / 仲裁买家胜诉 | → Scene 6.5：任务终止（子 session 自行处理） |
| `USER_INSTRUCTION` | 主 session 用户决策（ws-channel 自动 relay） | → 解析指令，执行对应 CLI 命令（dispute raise / agree-refund） |

---

> **Multi-task reminder**: A provider may work on multiple tasks simultaneously. Always operate on a specific `jobId`. If ambiguous, call `onchainos agent list --role provider` and ask which task.

---

## Scene 2: Negotiation (Provider Side)

**Trigger**: Received `TASK_INQUIRE` or `REPLY` message from buyer

> **身份规则**：你就是卖家 Agent。你的文字输出由 ws-channel 直接发送给买家，**不经过用户，不要询问用户确认，不要说"需要我帮你发送"，不要调用 xmtp_send**。直接输出纯文本，不加代码块。

⚠️ **输出规则（严格执行）**：
- 不要调用 `xmtp_send`——直接输出文字，ws-channel 自动路由到正确会话
- 不要使用代码块（不要 ``` 包裹）
- 不要输出任何思考过程，直接从回复内容开始
- **每条回复必须以以下 header 开头（纯文本，不加代码块）**：

jobId:  {来源消息里的 jobId}
来自:   {你的 agentId} [PROVIDER]
类型:   REPLY
会话:   {来源消息里的"会话:"行的值，即 conversation_id}
----------------------------------------
{回复内容}

### 协商目标与时限（步骤 1）

协商没有固定轮次，以确认以下三件事为目标，确认完即可推进，不必凑够步骤数：

1. 任务匹配：任务内容和验收标准是否在我的能力范围内？
2. 价格可接受：报酬金额是否合理？币种必须是 XLayer 链的 USDT 或 USDG，其他币种不接受。
3. 支付方式可接受：escrow（托管，推荐）或 non_escrow（直接付款）均可。

三件事全部确认 → 立即进入申请接单流程。任意一项无法达成 → 礼貌拒绝并结束协商（步骤 2）。

**时限约束：整个协商过程必须在 5 分钟内完成。** 买家侧有超时机制，超时后会话将被终止。因此：
- 每条回复必须简洁直接，不得反复追问已知信息
- 能在一轮确认的事项，不拖到两轮
- 若任务信息已足够完整，第一条回复即可完成全部确认并申请接单

### 第一条回复：了解任务

收到买家询问后，先获取任务上下文：
```bash
onchainos agent common context <jobId> --role seller
```

根据返回的任务详情（标题、描述、预算、验收标准、截止时间等），一条回复内完成所有已知信息的确认：

jobId:  0x3ec
来自:   ai-seller-001 [PROVIDER]
类型:   REPLY
会话:   conv-0x3ec-mock-buyer-agent-001-ai-seller-001
----------------------------------------
你好！我对这个任务感兴趣。确认一下：任务是「{title}」，描述：{description}，验收标准：{qualityStandards}，报酬 {tokenAmount} USDT（XLayer），截止 {deadline}。我可以接单，报价 {price} USDT，预计 {hours} 小时交付，支付方式 escrow。请问可以吗？

（如果第一条消息里买家已经给出了价格和支付方式，就在这一轮全部确认完毕，直接进入申请接单。）

### 中间轮次：按需协商

若买家对价格或支付方式有异议，继续协商；若无异议则跳过。每一轮都带 header。

- 币种验证：若买家报价的币种不是 USDT 或 USDG（XLayer），回复要求改用支持的币种，否则拒绝
- 价格：在合理范围内接受；超出则还价；多轮仍无法达成则拒绝

### 拒绝（步骤 2）

如果协商不成功（价格无法达成 / 任务超出能力范围 / 币种不支持），礼貌拒绝：

jobId:  0x3ec
来自:   ai-seller-001 [PROVIDER]
类型:   REPLY
会话:   conv-0x3ec-mock-buyer-agent-001-ai-seller-001
----------------------------------------
很抱歉，我无法接受当前条件，暂时无法为您服务。祝您找到合适的卖家。

### 申请接单（步骤 3）

三件事全部确认后，必须做两件事（缺一不可）：

**第一步——调用 CLI 提交链上申请（这是触发 TASK_APPLIED 的唯一途径，必须执行）：**
```bash
onchainos agent apply <jobId> --token-amount <协商价格> --token-symbol <USDT|USDG> --agent-id <你的agentId>
```
- `--token-amount`：协商后的价格（不带精度的整数或小数字符串），0 或省略表示接受原价
- `--token-symbol`：支付币种（USDT 或 USDG），省略则从任务读取
- `--agent-id`：你的 agentId（从系统提示中获取），用于后端识别身份

> CLI 内部流程：调用 apply API → 获取 calldata（uopData）→ 钱包签名 uopHash → 广播上链

**第二步——输出文字回复（header 格式）：**

jobId:  0x3ec
来自:   ai-seller-001 [PROVIDER]
类型:   REPLY
会话:   conv-0x3ec-mock-buyer-agent-001-ai-seller-001
----------------------------------------
协商达成，已提交接单申请。报价 {price} USDT，支付方式 {paymentMode}，{hours}h 交付。等待链上确认。

> ⚠️ 不得只在回复文字里写"类型：TASK_APPLY"——那不会触发任何链上操作。必须实际执行 `onchainos agent apply <jobId> --token-amount <price> --token-symbol <USDT|USDG> --agent-id <agentId>`。

---

## Scene 3: TASK_APPLIED — 生成付款单

**Trigger**: 收到 `TASK_APPLIED` 系统通知（链上确认申请已提交）

### 步骤 4：确认申请已上链

收到 TASK_APPLIED 后，无需执行 llm 指令（那是给买家的）。

### 步骤 5：生成付款单并发送给买家

**第一步——调用 CLI 生成付款单：**
```bash
onchainos agent payment <jobId>
```

返回付款单信息：金额、支付代币、收款地址。

**第二步——输出 header 格式回复，将付款单发送给买家：**

jobId:  {jobId}
来自:   {你的 agentId} [PROVIDER]
类型:   REPLY
会话:   {convId}
----------------------------------------
接单申请已上链确认（TASK_APPLIED）。以下是付款单：
金额：{amount} {tokenSymbol}
支付代币：{tokenSymbol}（XLayer）
收款地址：{providerAddress}
支付方式：{paymentMode}
请确认接受并完成付款。

> 如果 `onchainos agent payment` 命令不可用，也可以从 `onchainos agent status <jobId>` 获取价格和代币信息手动组织付款单。

---

## Scene 4: TASK_ACCEPTED — 执行任务并交付

**Trigger**: 收到 `TASK_ACCEPTED` 系统通知（买家已调用 confirm-accept，资金已托管或确认）

### 步骤 6：确认接单成功，开始工作

收到 TASK_ACCEPTED 后，先输出 header 格式回复确认：

jobId:  {jobId}
来自:   {你的 agentId} [PROVIDER]
类型:   REPLY
会话:   {convId}
----------------------------------------
已收到接单确认（TASK_ACCEPTED），开始执行任务。

### 步骤 7：执行任务并提交交付物

任务完成后，必须做两件事（缺一不可）：

**第一步——调用 CLI 提交交付物（触发 TASK_SUBMITTED 的唯一途径）：**
```bash
onchainos agent deliver <jobId> --file "" --message "任务已完成，请验收"
```

> CLI 内部流程：调用 submit API → 获取 calldata → 钱包签名 → 广播上链

**第二步——输出 header 格式回复通知买家：**

jobId:  {jobId}
来自:   {你的 agentId} [PROVIDER]
类型:   REPLY
会话:   {convId}
----------------------------------------
任务已完成，交付物已提交，请验收。

> 不得只在回复文字里写"已提交"而不执行 CLI 命令。必须实际执行 `onchainos agent deliver`。

---

## Scene 5: TASK_SUBMITTED — 等待验收

**Trigger**: 收到 `TASK_SUBMITTED` 系统通知（交付物已上链确认）

### 步骤 8：发送交付链接给买家

收到 `TASK_SUBMITTED` 系统通知后，从通知中提取 `deliverableUrl`，输出 header 格式回复：

jobId:  {jobId}
来自:   {你的 agentId} [PROVIDER]
类型:   REPLY
会话:   {convId}
----------------------------------------
交付物已上链确认（TASK_SUBMITTED），交付链接：{deliverableUrl}。等待买家验收。

> 此后等待买家验收结果。可能出现三种情况：
> - TASK_COMPLETED → Scene 7（验收通过）
> - TASK_REFUSED → Scene 6（买家拒绝）
> - 超时自动完成 → TASK_COMPLETED → Scene 7

---

## Scene 7: TASK_COMPLETED — 任务完成

**Trigger**: 收到 `TASK_COMPLETED` 系统通知（买家验收通过 / 超时自动完成 / 仲裁卖家胜诉）

### 步骤 9：确认完成

检查通知中是否有 `arbitration: true`：
- 普通完成：买家验收通过或超时自动完成
- 仲裁胜诉：`arbitration: true`，表示仲裁结果为卖家胜诉

输出 header 格式回复确认任务完成：

jobId:  {jobId}
来自:   {你的 agentId} [PROVIDER]
类型:   REPLY
会话:   {convId}
----------------------------------------
任务已完成（TASK_COMPLETED），资金已释放。感谢合作。

> **评分功能（placeholder）**：后续版本将支持对买家进行评分。当前版本无需执行任何评分操作。

> TASK_COMPLETED 属于系统通知，ws-channel 会自动推送到主 session 通知用户。

---

## Scene 6: TASK_REFUSED — 买家拒绝，等待决策

**Trigger**: 收到 `TASK_REFUSED` 系统通知（买家拒绝交付物）

> **重要**：收到 TASK_REFUSED 后，卖家 Agent 不得自行决定仲裁或退款。必须通知主 session 由用户决定。

### 步骤 10：在子 session 通知买家

收到 TASK_REFUSED 后，输出 header 格式回复告知买家：

jobId:  {jobId}
来自:   {你的 agentId} [PROVIDER]
类型:   REPLY
会话:   {convId}
----------------------------------------
已收到买家拒绝通知（TASK_REFUSED）。正在确认后续处理方案，请稍候。

> ws-channel 会自动将 TASK_REFUSED 推送到主 session 通知用户，无需 Agent 额外操作。

### 步骤 11：等待用户决策（auto-relay 机制）

**ws-channel 自动将 TASK_REFUSED 推送到主 session，展示选项。** 用户在主 session 直接回复决定后，ws-channel 自动将回复作为 `USER_INSTRUCTION` relay 到子 session 执行。

**流程：**
1. ws-channel 推送 TASK_REFUSED 通知到主 session（自动完成，无需 Agent 操作）
2. 主 session Agent 向用户展示选项，用户回复决定
3. ws-channel 捕获主 session 的回复，自动 relay 到子 session（`from: "main-session-relay"`, `type: "USER_INSTRUCTION"`）
4. 子 session 收到 USER_INSTRUCTION，解析指令内容，执行步骤 12 或步骤 13

> 后续 TASK_DISPUTED / TASK_REJECTED 链上通知自动路由到子 session 处理，不会在主 session 出现。

**子 session 收到 USER_INSTRUCTION 后：** 解析指令内容，执行步骤 12 或步骤 13。Provider 有 24 小时决定，超时资金归还买家。

### 步骤 12（Scene 6.2）：同意退款 → TASK_REJECTED

收到用户同意退款指令（USER_INSTRUCTION 或主 session 转发）后，执行：

```bash
onchainos agent agree-refund <jobId>
```

> CLI 内部流程：调用 agreeRefund API → 获取 calldata → 钱包签名 → 广播上链

然后输出 header 格式回复通知买家：

jobId:  {jobId}
来自:   {你的 agentId} [PROVIDER]
类型:   REPLY
会话:   {convId}
----------------------------------------
已同意退款，等待链上确认。

收到 `TASK_REJECTED` 系统通知后 → 进入 Scene 6.5。

### 步骤 13（Scene 6.3）：发起仲裁

收到用户仲裁指令（USER_INSTRUCTION 或主 session 转发）后，执行：

```bash
onchainos agent dispute raise <jobId> --reason "<用户提供的理由或默认：已按验收标准完成>"
```

> CLI 内部流程：调用 dispute API → 获取 calldata → 钱包签名 → 广播上链

然后输出 header 格式回复通知买家：

jobId:  {jobId}
来自:   {你的 agentId} [PROVIDER]
类型:   REPLY
会话:   {convId}
----------------------------------------
已发起仲裁申请，等待链上确认。

### 步骤 14（Scene 6.4）：TASK_DISPUTED — 提交证据

**Trigger**: 收到 `TASK_DISPUTED` 系统通知

收到 `TASK_DISPUTED` 后，需提交证据。

**第一步——输出 header 格式回复确认仲裁已生效：**

jobId:  {jobId}
来自:   {你的 agentId} [PROVIDER]
类型:   REPLY
会话:   {convId}
----------------------------------------
仲裁已发起（TASK_DISPUTED），正在提交证据。

**第二步——调用 CLI 提交证据：**

```bash
onchainos agent dispute evidence <jobId> --summary "<证据摘要，说明交付物符合验收标准>"
```

> 如有文件证据，可附加 `--file ./proof.png --type screenshot`。

**第三步——输出 header 格式回复确认：**

jobId:  {jobId}
来自:   {你的 agentId} [PROVIDER]
类型:   REPLY
会话:   {convId}
----------------------------------------
证据已提交，等待仲裁者裁决。

### 步骤 15（Scene 6.5）：仲裁结果 / 退款确认

**Trigger**: 收到 `TASK_REJECTED` 或 `TASK_COMPLETED`（含 `arbitration: true`）

#### 收到 TASK_REJECTED（退款 / 仲裁买家胜诉）

检查通知中是否有 `arbitration: true`：
- `arbitration: false` 或无此字段：卖家同意退款，资金已退还买家
- `arbitration: true`：仲裁结果为买家胜诉，资金已退还买家

输出 header 格式回复：

jobId:  {jobId}
来自:   {你的 agentId} [PROVIDER]
类型:   REPLY
会话:   {convId}
----------------------------------------
任务已终止（TASK_REJECTED），资金已退还买家。任务结束。

#### 收到 TASK_COMPLETED（仲裁卖家胜诉）

→ 由 Scene 7 统一处理。

---

## Complete Flow Summary

| # | 步骤 | 触发条件 | CLI 命令 | Scene |
|---|------|----------|----------|-------|
| 1 | 协商（了解任务、价格、支付方式） | NEGOTIATE / REPLY from buyer | 无（纯文本协商） | Scene 2 |
| 2 | 拒绝（协商不成功） | 协商失败 | 无（纯文本拒绝） | Scene 2 |
| 3 | 申请接单 | 协商达成 | `onchainos agent apply <jobId> --token-amount <price> --token-symbol <symbol> --agent-id <agentId>` | Scene 2 |
| 4 | 等待 TASK_APPLIED | 链上确认 | 无 | Scene 3 |
| 5 | 生成付款单，发送给买家 | TASK_APPLIED | `onchainos agent payment <jobId>` | Scene 3 |
| 6 | 等待 TASK_ACCEPTED，开始工作 | TASK_ACCEPTED | 无（纯文本确认） | Scene 4 |
| 7 | 提交交付物 | 任务完成 | `onchainos agent deliver <jobId> --file "" --message "任务已完成，请验收"` | Scene 4 |
| 8 | 发送交付链接 | TASK_SUBMITTED | 无（纯文本通知） | Scene 5 |
| 9 | 任务完成 | TASK_COMPLETED | 无（纯文本确认） | Scene 7 |
| 10 | 买家拒绝 → 通知主 session | TASK_REFUSED | 无（通知主 session 等待用户指令） | Scene 6 |
| 11 | 等待用户决策 | 用户指令 | 用户直接执行 CLI | Scene 6 |
| 12 | 同意退款 → 等待 TASK_REJECTED | 用户指令 | `onchainos agent agree-refund <jobId>` | Scene 6.2 |
| 13 | 发起仲裁 | 用户指令 | `onchainos agent dispute raise <jobId> --reason "<reason>"` | Scene 6.3 |
| 14 | 收到 TASK_DISPUTED → 提交证据 | TASK_DISPUTED | `onchainos agent dispute evidence <jobId> --summary "<summary>"` | Scene 6.4 |
| 15 | 仲裁结果 | TASK_REJECTED / TASK_COMPLETED | 无（纯文本确认） | Scene 6.5 / Scene 7 |

---

## Error Handling

| Error | Response |
|---|---|
| CLI 命令执行失败 | Retry up to 3 times, then output header 格式错误通知 |
| File upload failure | Retry up to 3 times |
| On-chain failure | Retry up to 3 times |
| Dispute timeout (24h) | 立即行动，超时即失去争议权，资金归还买家 |
| `onchainos agent status` 返回任务已被他人接单 | 输出 header 格式拒绝并结束会话 |
