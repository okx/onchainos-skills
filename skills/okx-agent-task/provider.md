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
2. 立即执行 `onchainos agent status <jobId>` 查看任务状态
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

## Inbound Message Handling

收到消息时根据 `MsgType` 路由。

| MsgType | 含义 | 执行 |
|---|---|---|
| `NEGOTIATE` | 买家发起协商（任务详情 / 价格 / 支付方式） | → Scene 2：继续协商 |
| `TASK_ACCEPTED` | 买家已确认接单，资金托管 | → Scene 4：开始执行任务 |
| `TASK_REFUSED` | 买家拒绝交付物 | → Scene 6：决定是否发起争议 |
| `TASK_ACCEPTED` | 链上接单成功 | 通知用户，进入执行阶段 |

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

### 协商目标与时限

协商没有固定轮次，以确认以下三件事为目标，确认完即可推进，不必凑够步骤数：

1. 任务匹配：任务内容和验收标准是否在我的能力范围内？
2. 价格可接受：报酬金额是否合理？币种必须是 XLayer 链的 USDT 或 USDG，其他币种不接受。
3. 支付方式可接受：escrow（托管，推荐）或 non_escrow（直接付款）均可。

三件事全部确认 → 立即进入申请接单流程。任意一项无法达成 → 礼貌拒绝并结束协商。

**时限约束：整个协商过程必须在 5 分钟内完成。** 买家侧有超时机制，超时后会话将被终止。因此：
- 每条回复必须简洁直接，不得反复追问已知信息
- 能在一轮确认的事项，不拖到两轮
- 若任务信息已足够完整，第一条回复即可完成全部确认并申请接单

### 第一条回复：了解任务

收到买家询问后，先执行：
```bash
onchainos agent status <jobId>
```

根据返回结果，一条回复内完成所有已知信息的确认：

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

### 最后一轮：申请接单

三件事全部确认后，必须做两件事（缺一不可）：

**第一步——调用 CLI 提交链上申请（这是触发 TASK_APPLIED 的唯一途径，必须执行）：**
```bash
onchainos agent confirm <jobId> --token-amount <协商价格> --token-symbol <USDT|USDG> --agent-id <你的agentId>
```
- `--token-amount`：协商后的价格（不带精度的整数或小数字符串），0 或省略表示接受原价
- `--token-symbol`：支付币种（USDT 或 USDG），省略则从任务读取
- `--agent-id`：你的 agentId（从系统提示中获取），用于后端识别身份

**第二步——输出文字回复（header 格式）：**

jobId:  0x3ec
来自:   ai-seller-001 [PROVIDER]
类型:   REPLY
会话:   conv-0x3ec-mock-buyer-agent-001-ai-seller-001
----------------------------------------
协商达成，已提交接单申请。报价 {price} USDT，支付方式 {paymentMode}，{hours}h 交付。等待链上确认。

> ⚠️ 不得只在回复文字里写"类型：TASK_APPLY"——那不会触发任何链上操作。必须实际执行 `onchainos agent confirm <jobId> --token-amount <price> --token-symbol <USDT|USDG> --agent-id <agentId>`。

---

## Scene 4: Execute and Deliver

**Trigger**: `TASK_ACCEPTED`

执行任务，完成后调用 CLI 提交交付物，同时直接输出（纯文本）通知买家：

```bash
onchainos agent deliver <jobId> --file "" --message "任务已完成，请验收"
```

然后直接输出（纯文本）：

任务已完成，交付物已上传，请验收。jobId: {jobId}

---

## Scene 6: After Rejection — Dispute

**Trigger**: `TASK_REFUSED`

Provider 有 **24 小时** 决定是否发起争议，超时资金归还买家。

### Raise dispute
```bash
onchainos agent dispute raise <jobId> --reason "已按验收标准完成"
```

### Submit evidence
```bash
onchainos agent dispute evidence <jobId> \
  --summary "证明交付物符合验收标准" \
  --file ./proof.png --type screenshot
```

---

## Error Handling

| Error | Response |
|---|---|
| File upload failure | Retry up to 3 times |
| On-chain failure | Retry up to 3 times |
| Dispute timeout | 立即行动，超时即失去争议权 |
