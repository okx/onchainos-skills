# Buyer 模块 Content / Prompt / Print 审计

## 1. xmtp_dispatch_user 内容审计

用户通知模板，定义在 `content.rs`，由 playbook 指令传给 `xmtp_dispatch_user`。

### 1.1 正常模板

| # | 函数 | content.rs 行号 | 有 L10N 指令 | 状态 |
|---|---|---|---|---|
| 1 | `job_created_non_designated_user_notify` | L44 | ✅ match_provider.rs Action 2 | OK |
| 2 | `job_created_designated_user_notify` | L50 | ✅ L10N_DISPATCH_SHORT | OK |
| 3 | `job_accepted_escrow_user_notify` | L83 | ✅ L10N_DISPATCH_SHORT | OK |
| 4 | `job_rejected_user_notify` | L121 | ✅ L10N_DISPATCH_SHORT | OK |
| 5 | `job_completed_escrow_user_notify` | L131 | ✅ L10N_DISPATCH_SHORT | OK |
| 6 | `job_completed_x402_user_notify` | L141 | ✅ L10N_DISPATCH_SHORT | OK |
| 7 | `dispute_won/lost_user_notify` | L163-182 | ✅ L10N_DISPATCH_SHORT | OK |
| 8 | `rating_submitted_user_notify` | L187 | ✅ L10N_DISPATCH_SHORT | OK |
| 9 | `job_refunded/auto_refunded_user_notify` | L198-207 | ✅ L10N_DISPATCH_SHORT | OK |
| 10 | `job_closed_user_notify` | L220 | ✅ L10N_DISPATCH_SHORT | OK |
| 11 | `visibility_public/private_user_notify` | L228-234 | ✅ L10N_DISPATCH_SHORT | OK |
| 12 | `x402_paying_user_notify` | L244 | ✅ L10N_DISPATCH_SHORT | OK |
| 13 | `review_expired_user_notify` | L310 | ✅ L10N_DISPATCH_SHORT | OK |
| 14 | `job_auto_completed_user_notify` | L320 | ✅ L10N_DISPATCH_SHORT | OK |
| 15 | `reward_claimed_user_notify` | L330 | ✅ L10N_DISPATCH_SHORT | OK |
| 16 | `submit_expired/reject_expired_user_notify` | L279-292 | ✅ L10N_DISPATCH_SHORT | OK |
| 17 | `complete_failed_user_notify` | L423 | ✅ L10N_DISPATCH_SHORT | OK |

### 1.2 有问题的模板

#### P1: `job_expired_user_notify` (content.rs:212) — 缺少 `[Label]` 前缀

```
"Job `{job_id}` has expired (no ASP accepted before the acceptance window expired, ..."
```

所有其他通知都有 `[Job Created]` / `[Rejection Confirmed]` / `[Job Completed]` 等前缀，唯独这条没有。

**建议**: 改为 `[Job Expired] Job '{job_id}' has expired...`

#### P2: `payment_mode_escrow_user_notify` (content.rs:241) — 缺少 `[Label]` 前缀

```
"{title} (`{job_id}`) — payment mode updated successfully; ASP <providerName> ..."
```

**建议**: 改为 `[Payment Mode Set] {title} ('{job_id}') — payment mode updated...`

#### P3: `wakeup_resume_user_notify` (content.rs:338) — 内部概念泄露

```
"Job `{job_id}` is back online. Please continue your decision in the user session."
```

"user session" 是内部架构术语，用户看不懂。

**建议**: 改为 `"Job '{job_id}' is back online. Please continue when ready."`

#### P4: `job_accepted_x402_replay_fail_user_notify` (content.rs:108) — 原始 HTTP 错误暴露

```
"HTTP status: <replayStatus>\nError: <replayBody>"
```

`<replayBody>` 可能包含原始 HTTP 错误体（stack trace、内部错误码等）。

**建议**: 模板改为 `"Error: <one-sentence summary of replayBody>"`，在 playbook 端指导 sub AI 提取关键信息而非原样传递。

#### P5: `x402_replay_fail_user_notify` (content.rs:411) — 同 P4

```
"Error: <replayBody>"
```

同样的原始 HTTP 错误暴露风险。

---

## 2. xmtp_prompt_user / pending-decisions-v2 request userContent 审计

决策卡片模板，作为 `--user-content` 传给 `pending-decisions-v2 request`，最终成为 `xmtp_prompt_user` 的 `userContent`。

### 2.1 正常模板

| # | 场景 | 文件:行号 | 有 L10N 指令 | 状态 |
|---|---|---|---|---|
| 1 | recommend 卡片 | match_provider.rs:67 | ✅ Action 4 翻译 | OK |
| 2 | not_provider A/B/C | designated.rs:297 | ✅ 🌐 Localize | OK |
| 3 | provider_offline A/B/C | designated.rs:309 | ✅ 🌐 Localize | OK |
| 4 | endpoint_not_found A/B/C | designated.rs:280 | ✅ 🌐 Localize | OK |
| 5 | x402_invalid A/B/C | designated.rs:195 | ✅ L10N_PROMPT | OK |
| 6 | negotiate_over_budget A/B/C | events.rs:119 | ✅ L10N_PROMPT | OK |
| 7 | job_submitted 审核卡 | core.rs:336 | ✅ L10N_PROMPT_BOLD | OK |
| 8 | review_deadline_warn | terminal.rs:130 | ✅ request_command_block | OK |

### 2.2 有问题的模板

#### P6: `price_mismatch` (designated.rs:228) — `(from CLI response)` 泄露 + 缺少前缀

```
Job `{job_id}` — the specified ASP (agentId={dp_id}) actually charges
<amountHuman> <tokenSymbol> (from CLI response), which differs from the
registered fee <feeAmount> <feeTokenSymbol>. Accept this price?
```

问题：
1. `(from CLI response)` 是内部实现注释，不应出现在用户可见内容中
2. 缺少 `[Job <short_id> — you are the User Agent]` 统一前缀

**建议**: 改为 `[Job {short_id} — you are the User Agent] The designated ASP (agentId={dp_id}) charges <amountHuman> <tokenSymbol>, which differs from the registered fee <feeAmount> <feeTokenSymbol>. Accept this price?`

#### P7: `over_budget` (designated.rs:241) — `(from CLI response)` 泄露

```
[Job {short_id} — you are the User Agent] The x402 fee from the designated ASP
(agentId={dp_id}) is <amountHuman> <tokenSymbol> (from CLI response), which exceeds
your max budget...
```

**建议**: 删除 `(from CLI response)`。

#### P8: `provider_conversation` ASP 列表 (match_provider.rs:199) — 字段名不友好

```
<N>. agentId: <agentId> | name: <name> | credit: <creditScore> | completed jobs: <completedTaskCount>
```

`agentId:` 是 camelCase 技术字段名，`credit:` 是缩写。

**建议**: 改为 `Agent ID: ... | Credit score: ... | Completed jobs: ...`

#### P9: `no_more_sellers_user_notify` (content.rs:345) — 使用完整 job_id

```
[Job `{job_id}` — you are the User Agent] All pending ASPs have been contacted...
```

其他所有决策卡片使用 `short_id`（6-8字符），唯独这条用完整 `job_id`（60+字符），前缀过长。

**建议**: 改为接收 `short_id` 参数：`[Job {short_id} — you are the User Agent]`

#### P10: `escalation_cli_failed_notify` (content.rs:502) — status 枚举暴露

```
- Current status: <status>
```

`<status>` 占位符可能被 sub AI 填入内部枚举值（`submitted` / `accepted` / `disputed`），违反 flow.rs Rule 7（不允许技术术语出现在用户可见内容中）。

**建议**: 改为 `- Current status: <describe in plain language>`

---

## 3. llmContent 审计

`pending_v2.rs` 中生成，作为 `xmtp_prompt_user` 的 `llmContent`，指导 user session AI 如何处理用户回复。

### 3.1 `resolve_llm_content_cli` (L1432-1453)

CLI 模式（`OKX_A2A_IS_CLI=1`），Claude Code sub session 使用。

- 指导 user session 先调 `okx-a2a user check` claim todo
- 然后调 `resolve-with-sessionkey` 并预填所有路由参数
- 包含 defer 关键词列表（中英双语）
- **注意**: 引用了 `okx-task-watch SKILL.md §kind == decision_request`，user session 可能尝试加载 skill 文件而非直接执行命令。低风险，因为完整命令模板已内联。

**状态**: OK

### 3.2 `resolve_llm_content_prompt_user` (L1460-1491)

非 CLI 模式（OpenClaw / Hermes），MCP `xmtp_prompt_user` 使用。

- 包含多决策卡片消歧逻辑（Step 2: 扫描多个 `[USER_DECISION_REQUEST]`）
- 包含 `resolve-prompt` 命令模板
- **注意**: Step 2 的示例文本 `'⚠️ You have multiple decisions pending...'` 是英文，虽然标注了 `(in user's language)` 但示例本身可能被 AI 直接使用。

**状态**: OK，建议示例改为多语言提示。

### 3.3 `resolve_llm_content` (L1397-1413)

旧默认模式（已被 L605 以下的死代码引用）。

**状态**: 死代码，仅被 `playbook_push`（L1416，同为死代码）调用。可安全删除。

---

## 4. println!/print!/eprintln! 审计

### 4.1 recommend.rs — Sub AI 消费的 stdout

| 行号 | 内容 | 消费者 | 状态 |
|---|---|---|---|
| L102 | `"All providers on this page have failed negotiation; auto-advancing..."` | Sub AI | OK — 流程状态 |
| L107 | `"The recommended provider list is empty; no matching providers."` | Sub AI | OK |
| L151-156 | `"Recommended ASPs (page N, N available). Card file: <path>"` | Sub AI | OK — playbook 依赖 `Card file:` 解析路径 |
| L298-313 | 每个 provider 的路由提示 `→ onchainos agent confirm-accept ...` | Sub AI | **问题**: 这些直接 CLI 命令提示仅在 `emit.enabled=false` 路径输出（即非 `--emit-decision` 路径），是给老链路 `job_created_with_designated_provider` 里 Step 1 用的。但 designated 路径已不走 recommend，这些输出目前只被 `--next-page` 和手动 `recommend` 调用消费。Sub AI 看到这些命令可能跳过 `next-action` 直接执行。**建议**: 验证是否仍需要，若不需要可删除路由提示部分。 |

### 4.2 create.rs — User session 消费的 stdout

| 行号 | 内容 | 消费者 | 状态 |
|---|---|---|---|
| L252 | `"✓ Calldata generated (jobId: {job_id})"` | User session AI | OK |
| L284-313 | `"✓ Task publish in progress..."` + txHash + `[Watch]` block | User session AI | OK — playbook 依赖 |
| L313 | `"🛑 Do NOT call set-payment-mode."` | User session AI | OK — 安全防护 |

### 4.3 accept.rs — Sub AI 消费的 stdout

| 行号 | 内容 | 消费者 | 状态 |
|---|---|---|---|
| L204-233 | `"✓ Payment mode is already/set to..."` | Sub AI | OK — 分支判断依赖 |
| L356 | `"✓ providerConfirmStatus: provider has applied..."` | Sub AI | OK |
| L388 | `"✓ escrow payment signing complete."` | Sub AI | OK |
| L443-444 | `"✓ Provider accepted (escrow); funds are now in escrow."` | Sub AI | OK |
| L494-496 | `"✓ x402 acceptance complete; task status → accepted."` | Sub AI | OK |

### 4.4 pending_v2.rs — playbook 输出（设计如此）

所有 `print!`/`println!` 都是 playbook 生成器的核心输出，AI 读取并执行。

**状态**: OK — 是设计意图，非多余输出。

### 4.5 DEBUG_LOG 保护的 eprintln!

| 文件 | 位置 | 内容 | 状态 |
|---|---|---|---|
| flow.rs | L326-329, L544-550 | 事件路由日志、输出前 200 字符 | OK — `DEBUG_LOG` 保护 |
| recommend.rs | L136 | emit-decision 调试日志 | OK — `DEBUG_LOG` 保护 |
| accept.rs | L360-365 | 签名输入参数（合约地址、金额） | OK — `DEBUG_LOG` 保护，无私钥泄露 |

**注意**: 确认 release 构建中 `DEBUG_LOG = false`。

---

## 5. 问题汇总

### 高优先级

| ID | 问题 | 文件:行号 | 影响 |
|---|---|---|---|
| P1 | `job_expired_user_notify` 缺 `[Label]` 前缀 | content.rs:212 | 用户体验不一致 |
| P2 | `payment_mode_escrow_user_notify` 缺 `[Label]` 前缀 | content.rs:241 | 用户体验不一致 |
| P3 | `wakeup_resume_user_notify` 暴露 "user session" 内部概念 | content.rs:338 | 用户困惑 |
| P6 | `price_mismatch` 模板含 `(from CLI response)` + 缺前缀 | designated.rs:228 | 内部信息泄露 |
| P7 | `over_budget` 模板含 `(from CLI response)` | designated.rs:241 | 内部信息泄露 |
| P10 | `escalation_cli_failed` 的 `<status>` 可能暴露枚举值 | content.rs:502 | 违反 Rule 7 |

### 中优先级

| ID | 问题 | 文件:行号 | 影响 |
|---|---|---|---|
| P4 | `replay_fail` 模板暴露原始 `<replayBody>` | content.rs:112, 415 | 技术信息泄露给用户 |
| P5 | 同 P4 | content.rs:415 | 同上 |
| P8 | ASP 列表字段名不友好 (`agentId:` / `credit:`) | match_provider.rs:199 | 用户体验 |
| P9 | `no_more_sellers` 用完整 job_id 而非 short_id | content.rs:345 | 前缀过长 |

### 低优先级

| ID | 问题 | 文件:行号 | 影响 |
|---|---|---|---|
| — | recommend.rs 路由提示可能误导 Sub AI 跳过 next-action | recommend.rs:298-313 | AI 误操作风险（低，因路径不常触发） |
| — | `resolve_llm_content` + `playbook_push` + `playbook_wait` 死代码 | pending_v2.rs | 代码维护负担 |
| — | `resolve_llm_content_prompt_user` 示例文本是英文 | pending_v2.rs:1482 | 多语言场景下 AI 可能直接用英文示例 |

---

## 6. SKILL.md L237 渲染规则与翻译链路分析

### 当前翻译链路

```
Sub AI 翻译（L10N_DISPATCH_SHORT / L10N_PROMPT / Action 4）
  → xmtp_dispatch_user / xmtp_prompt_user（已翻译内容）
  → User session → SKILL.md L237 "translate to user's language"（内容已是目标语言，空操作）
```

### 翻译职责归属

| 内容类型 | 翻译者 | 依据 |
|---|---|---|
| `xmtp_dispatch_user` content | Sub AI（发送前翻译） | flow.rs L32 `L10N_DISPATCH_SHORT` |
| `pending-decisions-v2 request` userContent | Sub AI（构造前翻译） | flow.rs L35/L38 `L10N_PROMPT` |
| recommend 卡片 | Sub AI（Action 4 读文件+翻译） | match_provider.rs Action 4 指令 |

### 结论

- **所有用户可见内容的翻译都由 Sub AI 在发送前完成**
- SKILL.md L237 的 "translate" 是兜底安全网，实际上内容到达 user session 时已是用户语言
- 去掉 L237 的 "translate" 对当前流程无实际影响，但会失去兜底能力
- 推荐卡片的 Action 4 翻译是唯一可优化的点（其他模板都是短文本，翻译开销可忽略）
