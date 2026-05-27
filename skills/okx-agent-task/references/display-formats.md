# Display Formats

> Standardized output templates for the task module. Use these verbatim — do not improvise column counts or formats.

**Table convention (matches `okx-agent-identity`):** every table in every output is a **Markdown pipe table** — header row of `|` cells + a separator row of `|---|`. Do not wrap tables in code blocks; do not use Unicode box-drawing characters (`┌ ├ │ └ ─`). They render as a single top line in most clients and look broken.

**Truncation rule:** table cell values **≤ 200 characters**. Anything over 200 characters is truncated with `…`. Long-text fields (description, acceptance criteria, deliverable content, etc.) are rendered in full as prose outside the table (see the prose region of each template).

**Language matching.** Field labels must match the user's language. Each section below shows Chinese-variant and English-variant; render one variant, not both.

**Short jobId rule.** `jobId` is a hex hash. In tables and notifications, render it in short form: `0x` + first 4 chars + `…` + last 4 chars (e.g. `0xbb31…ba4f`). Also show the internal sequence number `#N` (if any). Only render the full hash when the user explicitly asks for it.

---

## 1. Task list — `onchainos agent tasks`

| jobId | 标题 | 预算 | 状态 |
|---|---|---|---|
| 0xbb31…ba4f (#478) | 查询江苏天气 | 0.1 USDT | 🟢 Created |
| 0xa1c2…d3e4 (#475) | 翻译白皮书 | 100 USDT | 🔵 Accepted |

> 共 N 个任务。查看详情请说 "详情 #478"。

Rules:

- Four columns, exactly.
- `jobId`: short-form + internal id.
- `标题` / `Title`: truncate to 20 chars with `…`.
- `预算` / `Budget`: `{tokenAmount} {paymentTokenSymbol}`.
- `状态` / `Status`: emoji prefix + status string. Emoji mapping: 🟢 Created, 🔵 Accepted, 📦 Submitted, ❌ Rejected, ⚖️ Disputed, ✅ Complete, 🔒 Closed, ⏰ Expired.

---

## 2. Task detail card — `onchainos agent status` / context display

Chinese variant:

| 字段 | 值 |
|---|---|
| 任务 ID | 0xbb31…ba4f (#478) |
| 标题 | 查询江苏天气 |
| 预算 | 0.1 USDT |
| 最高预算 | 0.15 USDT |
| 支付方式 | 担保支付 (Escrow) |
| 可见性 | 私有 (Private) |
| 任务过期时间 | 24 小时 |
| 预期工作时长 | 24 小时 |
| 当前状态 | 🟢 Created — 等待接单 |
| 买家 | Agent #802 |
| 卖家 | 尚未匹配 |

**描述**：
请查询江苏省当前天气情况，包括温度、湿度、天气状况等信息，并以清晰易懂的格式返回结果。

**验收标准**：
返回温度、湿度、风力、天气状况四项数据，中文输出。

English variant:

| Field | Value |
|---|---|
| Task ID | 0xbb31…ba4f (#478) |
| Title | Query Jiangsu weather |
| Budget | 0.1 USDT |
| Max Budget | 0.15 USDT |
| Payment | Escrow |
| Visibility | Private |
| Acceptance Window | 24h |
| Delivery Window | 24h |
| Status | 🟢 Created — awaiting provider |
| Buyer | Agent #802 |
| Provider | Not matched |

**Description**:
Query the current weather of Jiangsu province...

**Quality Standards**:
Return temperature, humidity, wind, weather condition in Chinese.

Rules:

- Two-column table for all fields. When the description is ≤ 200 chars, put it in the table; when > 200 chars, write `见下方` / `See below` in that row and render the full text as prose under the table.
- `任务 ID` / `Task ID`: short-form hash + internal id.
- `支付方式` / `Payment`: render as user-language label — `担保支付 (Escrow)` / `Escrow` / `x402`.
- `可见性` / `Visibility`: `公开 (Public)` / `私有 (Private)` / `Public` / `Private`.
- `状态` / `Status`: emoji + status string + one-line description.
- `买家` / `卖家` / `Buyer` / `Provider`: `Agent #<id>`, or `尚未匹配` / `Not matched`.
- If a field is absent, omit the row.

---

## 3. Task creation confirmation card — create-task (before CLI call)

Chinese variant:

| 字段 | 值 |
|---|---|
| 标题 | 查询江苏天气 |
| 摘要 | 请查询江苏省当前天气情况，包括温度、湿度等信息。 |
| 描述 | 请查询江苏省当前天气情况，包括温度、湿度、天气状况等信息，并以清晰易懂的格式返回结果。 |
| 支付代币 | USDT |
| 预算 | 0.1 |
| 最高预算 | 0.15 |
| 任务过期时间 | 24h |
| 预期工作时长 | 24h |

> 确认无误？确认后我立即上链创建任务。

English variant:

| Field | Value |
|---|---|
| Title | Query Jiangsu weather |
| Summary | Query current weather of Jiangsu province including temperature and humidity. |
| Description | Query the current weather of Jiangsu province... |
| Currency | USDT |
| Budget | 0.1 |
| Max Budget | 0.15 |
| Acceptance Window | 24h |
| Delivery Window | 24h |

> Confirm? I will submit the task on-chain immediately after confirmation.

Rules:

- The summary always goes inside the table.
- When the description is ≤ 200 chars, put it in the table; when > 200 chars, write `见下方` / `See below` in that row and render the full text as prose under the table.
- Do not display the acceptance criteria field.
- Chinese/English field labels match user language.
- If attachments are present, add a row: `附件` / `Attachments` with the file count and names (e.g. `2 files: spec.pdf, mockup.png`).
- Footer must be a blockquote asking for confirmation.

---

## 4. x402 pricing confirmation card

| 字段 | 值 |
|---|---|
| 卖家 | Agent #806 |
| 服务 | Weather Query |
| Endpoint | `https://api.example.com/weather` |
| 费用 | 0.1 USDT |

Rules:

- Two-column table. All values are short — no prose section needed.
- Wrap Endpoint URL in backticks.

---

## 5. Deliverable verification card — job_submitted (push to buyer)

### 5a. Text deliverable

**交付物（文本）**：
卖家已提交交付物。

| 字段 | 值 |
|---|---|
| 任务 | <title> (0xbb31…ba4f) |
| 卖家 | Agent #806 |

**交付内容**：
江苏省当前天气：温度 28°C，湿度 65%，东南风 3 级，多云。

**验收标准**：
返回温度、湿度、风力、天气状况四项数据，中文输出。

> 请验收：回复「通过」确认完成，或回复「拒绝：<原因>」拒绝交付。

### 5b. File deliverable

**交付物（文件）**：
卖家已提交交付物，文件已下载到本地。

| 字段 | 值 |
|---|---|
| 任务 | <title> (0xbb31…ba4f) |
| 卖家 | Agent #806 |
| 文件路径 | /path/to/deliverable.pdf |

**卖家说明**：
已按要求完成翻译，见附件。

**验收标准**：
返回温度、湿度、风力、天气状况四项数据，中文输出。

> 请验收：回复「通过」确认完成，或回复「拒绝：<原因>」拒绝交付。

### 5c. URL deliverable

**交付物（网址）**：

| 字段 | 值 |
|---|---|
| 任务 | <title> (0xbb31…ba4f) |
| 卖家 | Agent #806 |
| 交付地址 | `https://result.example.com/abc` |

**卖家说明**：
查询结果已生成，请访问链接查看。

**验收标准**：
返回温度、湿度、风力、天气状况四项数据，中文输出。

> 请验收：回复「通过」确认完成，或回复「拒绝：<原因>」拒绝交付。

Rules:

- Table for short metadata (task ref, provider, file path / URL).
- **交付内容** / **卖家说明** / **验收标准** (Chinese variant) — **Deliverable Content** / **Provider Notes** / **Quality Standards** (English variant): always in prose — full text, no truncation. Buyer needs complete content for verification.
- Wrap URLs in backticks.
- Footer blockquote with acceptance prompt.

---

## 6. Status notifications — xmtp_dispatch_user (informational, no user action needed)

Format: single-line with task prefix and emoji status.

```
[<emoji> <status_label>] <title>（<short_jobId>）<one-line summary>
```

Examples:

- `[🟢 任务上链] 查询江苏天气（0xbb31…ba4f）任务已上链成功，正在自动查询推荐卖家...`
- `[🔵 接单成功] 查询江苏天气（0xbb31…ba4f）卖家 Agent #806 已接单，开始执行。`
- `[✅ 任务完成] 查询江苏天气（0xbb31…ba4f）验收通过，款项已释放。`
- `[💰 退款到账] 查询江苏天气（0xbb31…ba4f）退款已到账。`
- `[⚖️ 仲裁结果] 查询江苏天气（0xbb31…ba4f）仲裁裁决：买家胜诉，款项已退回。`
- `[⚠️ CLI 报错] 查询江苏天气（0xbb31…ba4f）<error summary>，请检查后重试。`

Rules:

- One line, no table. Emoji + status label in square brackets.
- Include task title + short jobId for context.
- Keep summary ≤ 1 sentence.
- Never expose CLI command names, backend field names, or stderr in notifications.

---

## 7. Decision prompts — xmtp_prompt_user (requires user action)

Format: task prefix + context + numbered options.

```
[Task <short_id> you as <role>] <context description>

<choose-prompt>:
A. <option_1>
B. <option_2>
C. <option_3> (if applicable)
```

- `<role>` resolves to `buyer` / `seller` (English) or `买家` / `卖家` (Chinese).
- `<choose-prompt>` resolves to `Please choose:` (English) or `请选择：` (Chinese).

Examples:

**Buyer — dispute/refund decision:**
```
[Task 0xbb31…ba4f you as seller] 任务被买家拒绝。

请选择：
A. 发起仲裁 — 回复「发起仲裁，理由是<理由>」
B. 同意退款 — 回复「同意退款」

⚠️ 24 小时内必须决策，超时自动退款给买家。
```

**Buyer — review deadline warning:**
```
[Task 0xbb31…ba4f you as buyer] 验收截止时间即将到期。超时后卖家可自动领取资金。

请选择：
A. 通过验收 — 回复「通过」
B. 拒绝交付物 — 回复「拒绝：<原因>」
```

Rules:

- Task prefix in square brackets: `[Task <short_id> you as <role>]` (English) or `[Task <short_id> 你作为<role>]` (Chinese).
- Context in plain text, 1-2 sentences.
- Options labeled with `A.` / `B.` / `C.`, each on its own line with action instruction.
- Deadline warnings with `⚠️` emoji.

---

## 8. Error card — task CLI errors

> ❌ **操作失败：<one-line summary>**
> 原因：<user-friendly explanation>
> 下一步：<recovery action>
>
> `raw: <exact CLI error message>`

Rules:

- Same format as `okx-agent-identity` §7.
- First line: `❌` + bold summary.
- `原因` (Chinese) / `Reason` (English): user-friendly translation.
- `下一步` (Chinese) / `Next step` (English): concrete recovery action.
- Last line: raw CLI message in inline code — never translated.
- Never auto-retry.
