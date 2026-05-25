# Pending Decisions v2 — Interaction Design

CLI 入口：`onchainos agent pending-decisions-v2 <subcommand>`
代码位置：`cli/src/commands/agent_commerce/task/common/pending_v2.rs`

---

## 1. 设计目标

把"sub session 想问用户、用户回复后又要把回复带回给 sub"这一段交互从模型自己拼接（`xmtp_prompt_user` + `xmtp_dispatch_session`）抽出来，由 CLI 接管。要解决的问题：

- **单 Active 不变量**：同一时刻只有一张决策卡呈现给用户，避免多卡叠加用户不知道答的是哪张
- **多 sub 并发安全**：多个 sub 同时来 `request`，flock 保证不会两条都被写成 Active
- **回复路由**：用户回复后，CLI 知道把回复 relay 给哪个 sub（不需要 LLM 转抄 sessionKey）
- **生命周期管理**：FIFO 排队、TTL 驱逐、断线恢复、防止重复推送
- **状态机收敛**：用 playbook 文本明确告诉 LLM 下一步做什么，避免模型自由发挥导致的回复丢失 / 重复派发

---

## 2. 核心概念

### 2.1 唯一标识 `sub_key`
完整的 XMTP sessionKey：
```
agent:main:okx-a2a:group:okx-xmtp:my=<myAgentAddr>&to=<peerAgentAddr>&job=<jobId>&gid=<groupId>
```
- **必须**通过 `session_status` 工具获取，**禁止**自己拼接前缀（`review-` / `decision-` / jobId 单独 / label 等）
- CLI 入口已校验：`agent:` 开头 + 含 `&job=<job_id>` + 含 `&gid=`，不符直接 bail 报错
- 同 `sub_key` 重复 `request` = 原地覆盖（`created_at` 保留以维持 FIFO，`updated_at` 刷新）

### 2.2 状态枚举 `Status`
| 值 | 含义 |
|---|------|
| `Active` | 当前已推送给用户、等待用户回复的那唯一一条 |
| `Queued` | 排队中、等 Active 被消费后可能 promote |

### 2.3 角色 `role`
`buyer` / `provider` / `evaluator`——和 task role 一致；CLI 不区分 role 做特殊处理，只在 list 输出里展示。

### 2.4 单 Active 不变量
**队列里任何时刻最多只有一条 `Status::Active`**。所有 handler 入口都跑 `ensure_invariant_and_evict`：

- 发现 ≥2 Active → 保留 `created_at` 最老的，其他降级到 Queued
- TTL 驱逐过期条目（默认 7 天，可用 `ONCHAINOS_PENDING_DECISIONS_TTL_DAYS` env 覆盖）
- 如果驱逐删掉了 Active，自动把最老 Queued promote 上来（仅在 `evicted > 0` 时触发）

---

## 3. 文件存储

存在 `$ONCHAINOS_HOME/task/` 下（缺省 `~/.onchainos/task/`）：

| 文件 | 用途 |
|---|------|
| `pending-decisions-new.json` | 队列正本（PendingEntry 数组） |
| `pending-decisions-new.lock` | flock 互斥锁文件（5s 超时） |
| `last-display.json` | 最近一次显示给用户的列表快照——`pick --index N` 用它把数字映射回 sub_key |

写入用 `tempfile + persist` 原子重命名，避免半写状态。

---

## 4. CLI 命令

### 4.1 `request` — sub 入队
```bash
onchainos agent pending-decisions-v2 request \
  --sub-key <full XMTP sessionKey> \
  --job-id <jobId> \
  --role <buyer|provider|evaluator> \
  --agent-id <agentId> \
  --user-content "<完整决策卡文本，用户最终看到的>" \
  --list-label "<列表里的一行短标签，例 [Decision 0xab12] Approve/Reject>" \
  [--llm-content "<自定义 llmContent override，可选>"]
```

行为：
- 取 flock → 读队列 → 校验 sub_key 格式
- 若同 sub_key 已存在 → 覆盖（保留 `created_at`）
- 若是新 sub_key 且队列里**没**有 Active → 自己写为 Active
- 若是新 sub_key 且队列里**有** Active → 自己写为 Queued
- 写回文件 → 返回对应 playbook

返回的三种 playbook：
- `playbook_push` — 新 Active，告诉 sub 调 `xmtp_prompt_user`
- `playbook_wait` — Queued、不需要重新唤起，告诉 sub end turn
- `playbook_wait_with_reprompt` — Queued 但需要重新唤起 Active 卡，告诉 sub 用 `xmtp_prompt_user` 重新渲染 Active 那张（因为中间聊天可能把 Active 卡刷下去了）

### 4.2 `resolve` — master 处理用户回复
```bash
onchainos agent pending-decisions-v2 resolve --user-reply "<用户原文 verbatim>"
```

行为：
- 取 flock → 读队列
- 找 Active 条目；找不到时分两种情况：
  - **队列里还有 Queued**（selection mode）→ 返回 `playbook_stale_relist`，让 master 重新渲染列表 + 提示用户"先挑数字"
  - **队列真空**→ 返回 `playbook_error_no_active`，告诉 master"这就是普通聊天"
- 找到 Active：从队列移除，拼接 `relay_content`：
  - 默认：`[USER_DECISION_RELAY] decision: <verbatim>`
  - 若 `user-reply` 以 `[intent:` 开头：`[USER_DECISION_RELAY][intent:CODE] user said: ...`
- 按剩余 Queued 数返回 3 种 playbook（见 §5）

**注意**：`resolve` **不**接受 sub_key 参数——直接从队列里取 Active。这是设计选择：避免 LLM 转抄长 sub_key 出错 / 派错 sub。

### 4.3 `pick` — master 在 selection mode 下挑选
```bash
onchainos agent pending-decisions-v2 pick --index <N>
```

行为：
- 取 flock → 读队列 + 读 last-display.json 快照
- 若队列**已有** Active → 错误："There is already an active decision; resolve it first"
- 校验 index 在快照范围内
- 校验 sub_key 在队列里还存在（stale 检查）
- 校验 entry.updated_at ≤ snapshot.displayed_at（stale 检查）
- 任何 stale → 返回 `playbook_stale_relist` 让用户重新选
- 通过 → 把第 N 条 promote 到 Active → 返回 `playbook_render`（带 🛑🛑🛑 防御警告）

### 4.4 `list` — 只读检查
```bash
onchainos agent pending-decisions-v2 list [--format markdown|json]
```

行为：
- 取 flock → 读队列 + 跑 evict（顺便清理过期）
- 重新 build 快照写回 `last-display.json`
- 输出当前队列（JSON 含 `evicted_since_last_call` 计数，sub 端可用来检查"上次 push 之后我是不是被默默驱逐了"）

---

## 5. 状态机 + Playbook 类型

### 5.1 状态
| 状态 | 描述 |
|------|------|
| **empty** | 队列为空 |
| **1A**（normal） | 1 Active + 0 Queued |
| **1A+NQ**（lineup） | 1 Active + N Queued 等候 |
| **0A+NQ**（**selection mode**） | 0 Active + N≥2 Queued — 仅在 resolve 时剩 ≥2 queued 才会出现，等用户挑数字 |

### 5.2 状态转换
```
empty
  ─[request 第一条]→  1A

1A
  ─[同 sub_key request 覆盖]→ 1A
  ─[新 sub_key request]→ 1A+NQ (N=1)
  ─[resolve]→ empty

1A+NQ (N=1)
  ─[同 sub_key request 覆盖]→ 1A+NQ
  ─[新 sub_key request]→ 1A+NQ (N=2)
  ─[resolve]→ 1A  (auto-promote 最老 Queued)

1A+NQ (N≥2)
  ─[resolve]→ 0A+NQ  (★ selection mode)

0A+NQ (selection mode)
  ─[pick --index N]→ 1A+(N-1)Q
  ─[resolve（误调）]→ stale_relist 防御，状态不变
  ─[新 sub_key request]→ 1A+NQ  (★ 新来的会被写为 Active，因为队列里没 Active)
  ─[同 Queued sub_key 覆盖]→ 0A+NQ
```

### 5.3 Playbook 一览
全部 playbook 都是纯文本指令字符串，CLI `print!` 到 stdout，由 caller LLM 读取并执行。

| Playbook | 由谁调用产生 | 受众 | 内容 |
|---|---|---|---|
| `playbook_push` | sub 调 request、新 Active | sub | "用 EXACT args 调 `xmtp_prompt_user`、然后 end turn" |
| `playbook_wait` | sub 调 request、自己被 Queued、不需要 reprompt | sub | "已排队 (position N)，端 turn" |
| `playbook_wait_with_reprompt` | sub 调 request、自己被 Queued、需要重新唤起 Active 卡 | sub | "用 `xmtp_prompt_user` 把 ACTIVE 那张卡重新推一次，顶上加 '新决策已排队' 提示" |
| `playbook_relay_only` | master 调 resolve、剩 0 Queued | master | "调 `xmtp_dispatch_session(sub_key, relay_content)` 一次、然后 end turn" |
| `playbook_relay_and_render` | master 调 resolve、剩 1 Queued | master | "Step 1: dispatch_session relay；Step 2: 用 `xmtp_prompt_user` auto-render 唯一剩下的那张" |
| `playbook_relay_and_list` | master 调 resolve、剩 ≥2 Queued | master | "Step 1: dispatch_session relay；Step 2: 在 assistant 回复里渲染列表给用户挑数字" |
| `playbook_render` | master 调 pick | master | "调 `xmtp_prompt_user` 渲染选中那张、END THE TURN（带 🛑🛑🛑 '数字不是决策内容' 防御）" |
| `playbook_stale_relist` | pick 命中 stale / resolve 在 selection mode 误调 | master | "原列表已失效，渲染新列表 + 提示用户重新选" |
| `playbook_error_no_active` | resolve 在真正空队列时调 | master | "队列空、这是普通聊天、end turn" |
| `playbook_error(msg)` | 其他错误（pick 时已有 active 等） | master | "Cannot proceed: {msg}, end turn" |

---

## 6. 角色职责

### 6.1 Sub session
- 业务流（`next-action` 脚本）走到需要用户决策时，调 `pending-decisions-v2 request`
- 收到 CLI 返回的 playbook → 严格按 playbook 执行（`xmtp_prompt_user` 或 end turn）
- 收到 `[USER_DECISION_RELAY] decision: <verbatim>` 时（用户回复经 master 中转回来），按业务规则路由到 `next-action --jobStatus <pseudo_event>`
- **禁止**自己手拼 llmContent / 直接调 `xmtp_dispatch_session` / 跳过 request 直接调 `xmtp_prompt_user`

### 6.2 Master / user-session
- 收到 user_message 后，判断当前是否有 pending 决策——主要通过观察上下文里有没有 `[USER_DECISION_REQUEST]` 的 llmContent 来感知
- 若有：调 `resolve --user-reply "<user verbatim 原文，不翻译不解释不总结>"`，再按返回的 playbook 执行
- 在 selection mode：用户输入数字时调 `pick --index N`、defer 关键词时 end turn、其他文本时**重新渲染列表**（不要调 resolve）
- **禁止**：
  - 自己转抄 sub_key 当 sessionKey 参数
  - 同 turn 内调 resolve 两次（dispatch playbook 已强调）
  - pick 之后同 turn 内立刻 resolve（playbook_render 的 🛑🛑🛑 警告）
  - 把 turn 开头的用户 reply 当成"对刚渲染卡片的回复"重复利用（Case A/B 框架）

---

## 7. 防御机制

### 7.1 flock 串行化（顶层防御）
所有 handler 入口 `acquire_lock()`，5s 超时。两个并发 `request` 进程会被序列化、不会都看到"队列空"然后都把自己写为 Active。

### 7.2 sub_key 格式校验
`request` 入口拒收非 `agent:` 开头 / 不含 `&job=<id>` / 不含 `&gid=` 的 sub_key，错误信息明确指引"先调 session_status"。

### 7.3 Multi-active heal
`ensure_invariant_and_evict` 每次都跑——即便 flock 失效 / 文件被外部改坏 / 旧版二进制留下双 Active，下次任何 CLI 调用都会把最老的保留为 Active、其他降为 Queued。

### 7.4 TTL 驱逐 + 自动 promote
- 默认 7 天 TTL，超期条目静默驱逐
- 若驱逐删掉了 Active，自动把最老 Queued promote 到 Active（避免"无 Active 但队列非空"的死状态）

### 7.5 "同 turn 不要重复 resolve / dispatch" 三层防御
| 层 | 位置 | 防御对象 |
|---|------|---------|
| L1 外层 playbook | `playbook_render` 顶部 🛑🛑🛑 | pick 之后 master 立刻把"1" / "2"当成决策内容调 resolve |
| L2 外层 playbook | `playbook_relay_and_render` Step 2 之前 🛑 | resolve 之后 master 又对刚 promote 的那条重复调 resolve |
| L3 内层 llmContent | `resolve_llm_content` 的 Case A/B 框架 | 主 session 看到 llmContent 时，按"最近一条消息是 tool_result 还是 fresh user_message"判断该 end turn 还是 resolve |

### 7.6 Selection mode 误调 resolve 的兜底
之前 bug：selection mode 下 master 误调 resolve → CLI 返回 `playbook_error_no_active` 文案"normal chat message"→ master end turn → 用户回复被吞掉 → 队列里 N 个 sub 等不到 relay → 任务卡死。

现在：handle_resolve 先判断"队列里是否还有 Queued"，**若有**返回 `playbook_stale_relist`（重渲染列表），**仅当真正空队列**才返回 `playbook_error_no_active`。

### 7.7 Stale snapshot 检查（pick）
- index 越界 → stale_relist
- snapshot 里的 sub_key 已不在队列 → stale_relist
- 条目 `updated_at > snapshot.displayed_at`（pick 期间被 sub 覆盖了内容）→ stale_relist

### 7.8 LLM relay shape 兼容
两种 relay shape 共存：
- 默认：`[USER_DECISION_RELAY] decision: <verbatim>`
- intent-tag 场景（如 JobRefused 谈判等）：`[USER_DECISION_RELAY][intent:CODE] user said: <verbatim>`，由 sub 在 request 时通过 `--llm-content` 自定义指示 master 用这种 shape

CLI 自动检测：用户回复以 `[intent:` 开头 → 直接拼接、不加 `decision: ` 前缀。

---

## 8. 典型流程示例

### 8.1 单条决策（最常见）
```
sub-A: request → playbook_push → sub 调 xmtp_prompt_user → 用户看到卡
用户: "approve"
master: 看到上下文有 [USER_DECISION_REQUEST] → resolve --user-reply "approve"
       → playbook_relay_only → dispatch_session "[USER_DECISION_RELAY] decision: approve" → sub-A
sub-A: 收到 relay → next-action --jobStatus approve_review → 执行业务流
```

### 8.2 并发多条决策、用户按顺序答
```
sub-A: request → 队列 [A(Active)] → playbook_push → 渲染 A
sub-B: request → 队列 [A(Active), B(Queued)] → playbook_wait_with_reprompt → 再唤起 A 卡（带"新决策已排队"提示）
用户: "answer-A"
master: resolve --user-reply "answer-A"
       → 剩 1 queued → playbook_relay_and_render
       → Step 1: dispatch_session relay 给 A；Step 2: xmtp_prompt_user 渲染 B
用户: "answer-B"
master: resolve --user-reply "answer-B" → playbook_relay_only → 派给 B
```

### 8.3 并发 ≥3 条、进入 selection mode
```
sub-A request → [A(Active)]
sub-B request → [A(Active), B(Queued)]
sub-C request → [A(Active), B(Queued), C(Queued)]
用户: "answer-A"
master: resolve → 剩 2 queued → playbook_relay_and_list
       → Step 1: dispatch_session 派给 A
       → Step 2: 渲染列表"1. [B] / 2. [C], 选数字"
用户: "1"
master: pick --index 1 → playbook_render → xmtp_prompt_user 渲染 B
       → 🛑🛑🛑 防御：本 turn 不要把"1"当成对 B 的回复调 resolve
用户（FUTURE turn）: "answer-B"
master: resolve → 剩 1 queued → playbook_relay_and_render → 派给 B + 渲染 C
用户: "answer-C"
master: resolve → playbook_relay_only → 派给 C → 队列空
```

### 8.4 Selection mode 下用户答非数字（兜底）
```
（接 8.3 selection mode 状态）
用户: "我同意 B 那条"  ← 非数字、非 defer
master: ❌ 不应该调 resolve（playbook_relay_and_list 已明确说"Else → DO NOT call resolve"）
       应该在 assistant 回复里说：
       "我看到您的消息但还有 2 条决策待回复，请先回复数字 1-2 挑一条，我再把您的答复转给对应的服务。
        1. [B]
        2. [C]"

（万一 master 还是调了 resolve）
master: resolve --user-reply "我同意 B 那条"
       → handle_resolve 检测 0 active + N queued
       → 返回 playbook_stale_relist
       → master 渲染"选择已失效，请挑数字 1-2"给用户
       ★ 用户回复不会丢，队列状态不变
```

### 8.5 同 sub_key 重复 request（用户答了无关内容，sub 想重新追问）
```
sub-A: request "请选 A 或 B" → 渲染卡
用户: "我不想答"
master: resolve --user-reply "我不想答" → 派给 sub-A
sub-A: 收到 relay → 解析发现既不是 A 也不是 B → request 再次（同 sub_key + 新 user_content "您刚才回复'我不想答'我没理解，请回复 A 或 B"）
CLI: 找到同 sub_key、status=Active 不变、created_at 保留、updated_at 刷新、user_content 覆盖
   → 返回 playbook_push → sub 再次 xmtp_prompt_user 渲染新内容
```

### 8.6 Sub 重启后断线恢复
```
sub-A 已 request 过、被推给用户、然后 sub-A session 重启
sub-A 重连后收到 wakeup_notify 系统事件
sub-A 业务流（next-action wakeup_notify）playbook 包含：
  1. 调 `pending-decisions-v2 list --format json` 看队列
  2. 如果 entries[] 里已有 job_id 匹配自己的 sub_key → 已经推送过、跳过重复 push，仅发"任务已恢复"通知
  3. 否则按正常流程继续
```

---

## 9. 边缘场景 + 错误恢复

### 9.1 用户在 selection mode 列表显示后，新 sub-D 又来 request
- handle_request 见队列里 0 active → sub-D 写为 Active
- 用户视角："列表显示着 1.[A] / 2.[B]" → 突然 sub-D 的 push 卡冒出来
- 这是预期行为（防止新决策饿死），但 UX 可能略乱
- 用户答 D 之后回到 [A(Q), B(Q)]、又是 selection mode

### 9.2 同一 turn 内 sub 多次 request 同 sub_key
- 没问题：CLI 覆盖同条目，FIFO `created_at` 保留
- 仅 `updated_at` 和 `user_content` 刷新

### 9.3 队列文件被外部手动删除
- 下一次 `request` 调用：read_queue 见文件不存在 → 返回空 Queue → 把当前 request 写为新 Active
- 但**已经在等 relay 的 sub 永远收不到答复**——这是手动操作的代价
- 建议：业务流跑期间不要手动改文件

### 9.4 队列文件被手动改坏（JSON 语法错误）
- read_queue 用 `serde_json::from_str(...).unwrap_or_default()` 兜底 → 直接当空队列处理
- 同上，已等待中的 sub 永远收不到 relay

### 9.5 master 多次调 resolve（非同 turn）
- 第一次正常消费 Active
- 第二次：若还有 queued → playbook_stale_relist 重渲染；若空 → playbook_error_no_active

### 9.6 ONCHAINOS_HOME 在 sub / master 进程不一致
- sub 写到 path-A、master 读 path-B → master 看到的队列是空的 → resolve 失败
- 排查办法：两个进程都 `echo $ONCHAINOS_HOME` 对比；或 audit log 里检查 task_dir() 实际路径

---

## 10. 关键不变量速查

| 不变量 | 由谁保证 |
|---|------|
| 队列里最多 1 条 Active | `ensure_invariant_and_evict` + flock |
| sub_key 是合法 XMTP sessionKey | request 入口 `validate_sub_key` |
| 同 sub_key 不会出现两次 | request 时按 sub_key 查 prev_idx 并覆盖 |
| FIFO 顺序（按 `created_at`） | `ensure_invariant_and_evict` sort + 同 sub_key 覆盖时保留旧 `created_at` |
| resolve 不需要 LLM 转抄 sub_key | handle_resolve 自动从队列取 active |
| 用户回复不会被吞 | selection mode 误调 resolve 返回 stale_relist 而非 error_no_active |
| Pick 数字不会被误当成决策内容 | playbook_render 的 🛑🛑🛑 警告 + Case A/B 框架 |
| 不会两个并发 request 都写为 Active | flock 串行化 |

---

## 11. 用户交互流程图

### 11.1 单条决策（最常见路径）

```
┌─────┐         ┌─────┐         ┌─────────┐         ┌────────┐         ┌──────┐
│Sub-A│         │ CLI │         │Queue.json│         │ Master │         │ User │
└──┬──┘         └──┬──┘         └────┬─────┘         └───┬────┘         └──┬───┘
   │   request    │                  │                    │                 │
   │─────────────▶│   acquire flock  │                    │                 │
   │              │─────────────────▶│                    │                 │
   │              │   write [A(Act)] │                    │                 │
   │              │─────────────────▶│                    │                 │
   │  playbook_   │                  │                    │                 │
   │    push      │                  │                    │                 │
   │◀─────────────│                  │                    │                 │
   │                                                                         │
   │  xmtp_prompt_user(llmContent, userContent)                              │
   │────────────────────────────────────────────────────▶│ 渲染决策卡          │
   │                                                     │────────────────▶│
   │  'sent' (sub end turn)                              │                  │
   │                                                     │                  │
   │                                                     │  user reply       │
   │                                                     │◀─────────────────│
   │                                                     │                  │
   │                                  resolve --user-reply                  │
   │              ◀─────────────────────────────────────│                  │
   │              │ remove A from queue                  │                  │
   │              │─────────────────▶│                   │                  │
   │              │ playbook_relay_only                  │                  │
   │              ├─────────────────────────────────────▶│                  │
   │                                                     │                  │
   │   xmtp_dispatch_session(A.sub_key, [USER_DECISION_RELAY] decision: ..) │
   │◀────────────────────────────────────────────────────│                  │
   │ next-action --jobStatus <pseudo_event>                                 │
   ▼ 执行业务流
```

### 11.2 并发多条（≥3 条进入 selection mode）

```
Sub-A   Sub-B   Sub-C    CLI/Queue        Master         User
  │      │       │           │              │             │
  │ request                  │              │             │
  │─────────────────────────▶│ [A(Act)]    │             │
  │  playbook_push           │              │             │
  │◀─────────────────────────│              │             │
  │ xmtp_prompt_user ─────────────────────────────────▶ 看到 A
  │ end turn                                              │
  │                                                       │
  │      │ request           │              │             │
  │      │──────────────────▶│ [A(Act),B(Q)]              │
  │      │ playbook_wait_with_reprompt                    │
  │      │◀──────────────────│              │             │
  │      │ xmtp_prompt_user 重新唤起 A 卡 + "新决策已排队"提示
  │      │ end turn                                       │
  │      │                                                │
  │      │      │ request    │              │             │
  │      │      │───────────▶│ [A(Act),B(Q),C(Q)]         │
  │      │      │ playbook_wait_with_reprompt             │
  │      │      │◀───────────│                            │
  │      │      │ xmtp_prompt_user 重新唤起 A 卡           │
  │      │      │ end turn                                │
  │                                                       │
  │                                  user 回复 "answer-A" │
  │                                          │◀──────────│
  │                                  resolve --user-reply │
  │             ◀────────────────────────────│            │
  │             │ remove A, 剩 2 queued                  │
  │             │ ★ 进入 selection mode                    │
  │             │ playbook_relay_and_list                 │
  │             ├────────────────────────────▶│           │
  │                                          │            │
  │ xmtp_dispatch_session(A.sub_key, decision: answer-A) │
  │◀─────────────────────────────────────────│            │
  │ 执行 A 业务流                                          │
  │                                          │            │
  │                                          │ assistant 文本渲染列表:
  │                                          │   "✓ 上一条已处理。
  │                                          │    1. [B]
  │                                          │    2. [C]
  │                                          │    回复数字 1-2 挑选"
  │                                          │──────────▶│
  │                                                       │
  │                                          user 回复 "1"
  │                                          │◀──────────│
  │                                  pick --index 1       │
  │             ◀────────────────────────────│            │
  │             │ promote B to Active                     │
  │             │ playbook_render (带 🛑🛑🛑 防御警告)        │
  │             ├────────────────────────────▶│           │
  │                                          │            │
  │ xmtp_prompt_user(B) 渲染 B 卡 ──────────────────────▶ 看到 B
  │                                          │ 🛑 end turn │
  │                                          │ ❌ 不要同 turn 把 "1" 当 B 的回复 resolve
  │                                                       │
  │                                          user 回复 "answer-B"  (NEW turn)
  │                                          │◀──────────│
  │                                  resolve --user-reply │
  │             ◀────────────────────────────│            │
  │             │ remove B, 剩 1 queued (C)               │
  │             │ playbook_relay_and_render               │
  │             ├────────────────────────────▶│           │
  │                                                       │
  │ Step 1: dispatch_session(B.sub_key, decision: answer-B)
  │ Step 2: xmtp_prompt_user(C) auto-render C ─────────▶ 看到 C
  │                                          │ end turn   │
  │                                          │            │
  │                                          user 回复 "answer-C"
  │                                  resolve → relay_only → 派给 C
  │                                          → 队列空    │
```

### 11.3 状态机图

```
                         ┌──────────────┐
                         │   empty 空    │◀──────────┐
                         └──────┬───────┘            │
                                │                    │
                  request 第一条 │                    │ resolve
                                │                    │ (0 queued)
                                ▼                    │
                    ┌───────────────────┐            │
                    │ 1A  (1 Active)    │────────────┤
                    └───────┬───────────┘            │
                            │                        │
                  新 sub_key│                        │
                    request │      ┌────────────┐    │
                            ▼      │            │    │
              ┌─────────────────────┴──┐         │    │
              │ 1A+NQ  (1 Active +    │  resolve│    │
              │        N Queued)      │ (1 queued)   │
              └──────────┬─────────────┘         │    │
                         │                       │    │
              ≥2 queued  │ resolve        ┌──────┘    │
                         │                │           │
                         ▼                ▼           │
              ┌──────────────────────┐                │
              │ 0A+NQ ★ selection    │ pick --index N │
              │       mode           │────────────────┘
              │ (无 Active 等用户挑数字)│
              └────────┬─────────────┘
                       │ 误调 resolve
                       │ (兜底)
                       │  ▼
                       │  返回 stale_relist
                       │  状态不变 ──┐
                       │           │
                       └───────────┘  ↻ 自循环（用户回复不会丢）

▸ 同 sub_key 重复 request（任何状态）：原地覆盖 user_content，状态不变
▸ TTL 驱逐 Active：自动 promote 最老 Queued 上来
▸ Multi-active heal（外部破坏后）：每次任何 CLI 调用都保留最老 Active、其他降级
```

### 11.4 角色协作架构图

```
                ┌─────────────────────────────────────┐
                │       Sub Sessions (执行业务流)        │
                │                                     │
                │  sub-A (job=0xA)   sub-B (job=0xB)  │
                │  sub-C (job=0xC)         ...        │
                └────────┬───────────────────┬────────┘
                         │                   │
                  request│                   │ [USER_DECISION_RELAY]
                  (push  │                   │ (收回复执行业务)
                   决策卡)│                   │
                         ▼                   │
                ┌─────────────────────────────┴────────┐
                │   onchainos agent pending-decisions-v2│
                │   (CLI: pending_v2.rs)                │
                │                                       │
                │   ┌─────────────────────────────┐    │
                │   │  pending-decisions-new.json │    │
                │   │  (单 Active 队列)            │    │
                │   └─────────────────────────────┘    │
                │   ┌─────────────────────────────┐    │
                │   │  last-display.json (snapshot) │   │
                │   └─────────────────────────────┘    │
                │   ┌─────────────────────────────┐    │
                │   │  .lock (flock 互斥)          │    │
                │   └─────────────────────────────┘    │
                └─────────┬───────────────────┬─────────┘
                          │                   │
                  playbook│                   │ playbook
                  (push/  │                   │ (relay/render/list)
                   wait)  │                   │
                          │                   ▼
                          │           ┌──────────────────────┐
                          │           │  Master / User       │
                          │           │  Session            │
                          │           │  (resolve/pick)      │
                          │           └─────────┬────────────┘
                          │                     │
                          │ xmtp_prompt_user    │ xmtp_prompt_user
                          │ (sub 调，渲染卡)     │ (master 调，渲染下一张)
                          │                     │
                          ▼                     ▼
                       ┌────────────────────────────────┐
                       │            User                │
                       │     (聊天窗口里看决策卡 + 回复)    │
                       └────────────────────────────────┘
```

---

## 12. 改 / 扩展时的注意点

- 新增 playbook 类型 → 同步更新本文档 §5.3 表格
- 调整状态机分支（如增加新状态）→ 同步更新 §5.2 转换图 + §10 不变量表
- 调整 `relay_content` shape → 同步更新 sub 业务流（buyer.md / provider.md §"Step X — 接收 [USER_DECISION_RELAY] 后路由"）
- 改 sub_key 校验规则 → 确认所有 sub 调用点都能产出符合新规则的 sub_key
- 改 flock 超时 → 评估 master 调用频率，过短会误失败、过长会延迟报错
