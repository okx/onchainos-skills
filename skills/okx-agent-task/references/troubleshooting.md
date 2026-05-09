# Troubleshooting

> **何时打开本文档**：CLI 报错 / agent 看到非预期返回时按错误码 / 错误信息查表。

> **重试策略**先看 [`_shared/exception-escalation.md`](../_shared/exception-escalation.md)：业务错误 0 重试 → 推 user session；唯二自动重试一次的例外是网络瞬断 + JWT 过期；累计 3 次失败立即停。

---

## 0. 后端统一错误码（class-level）

后端 task / dispute / evaluator API 全部走这套 5 大类错误码方案。**`code` 只是类别**，**具体错误**靠返回的 `msg` 字段区分（同一个 `1001` 可能是参数缺字段、参数取值非法、参数格式错等不同 msg）。

| code | 类别 | 含义 | 重试? |
|---|---|---|---|
| **0** | 成功 | API 正常返回 | — |
| **1001** | 参数校验失败 | 必填字段缺失 / 类型不对 / 取值越界 / 业务规则前置校验失败 | ❌ 不重试，推 user session |
| **2001** | 风控敏感词 | 文本内容触发风控 | ❌ 不重试，推 user session 让用户改文本 |
| **3001** | 权限问题 | JWT 失效 / 未登录 / agenticId header 空 / 钱包 session 过期 | ⚠️ **JWT 过期**（msg 含 `JWT verification failed` / `JWT expired` / `unauthorized`）允许刷新登录态后**自动重试一次**；其他 3001（agenticId header 空 / 钱包未登录）推 user session |
| **4001** | 服务内部错 | 后端 panic / DB 错 / 外部依赖挂 | ❌ 不重试，推 user session |
| **5001** | 重试码 | 后端明确指示客户端可重试（具体哪些场景待后端确认） | ❌ 不重试，推 user session（即便后端建议可重试，agent 侧统一让用户拍板） |

**agent 处理铁律**（叠加 SKILL.md Layer 1.5 + [`_shared/exception-escalation.md`](../_shared/exception-escalation.md)）：

- 看到 `code != 0` → **第一次失败立即推 user session**，**不要 retry 同命令**
- 唯一通用例外是 JWT 过期（3001 + 特定 msg）→ 刷新 + 自动重试一次；仍失败再推用户
- 网络 timeout / connection error 不属于例外——按业务错处理推用户，**不在 sub 里盲重**
- **Role-specific 例外**：`vote-commit` / `vote-reveal` / `arbitration-claim` 因错过窗口直接罚 0.3% stake，allow sub 内部最多重试 3 次（详见 `references/evaluator-decision-rubric.md` 第 6 节）。其他 evaluator 命令仍走 0 retry 规则。Buyer / provider 没有此类例外。

> ⚠️ `2004` / `4000` 等子码：本文档下面表里部分错误信息提到 `2004 / 4000`——那是 **`msg` 内嵌的业务子码**（比如 staking 模块自己的 sub-error），不是 class-level code。class-level code 一律来自上表 5 个值。

---

## 1. 认证 / 身份错误

| 错误码 / 信息 | 触发原因 | 处理 |
|---|---|---|
| `code=3001` + `msg` 含 `auth fail` / `unauthorized` / `agenticId` | beta 后端拒空 `agenticId` header；几乎所有 task API 命令缺 `--agent-id` 都会撞这个 | 检查 envelope 顶层 `agentId`，**原样**透传给 `--agent-id`（CLI 已加必填校验，应该在前面就 bail） |
| `code=3001` + `msg` 含 `JWT verification failed` / `JWT expired` | JWT 过期 | 唯一允许自动重试一次的认证错（重试前需先刷新登录态）；仍失败 → 推 user session 让用户重登 `okx-agentic-wallet` |
| `msg` 含 `agentId 无效` / `session 丢失`（业务子码 4000） | 钱包 session 过期 / agentId 不属于当前钱包 | 推 user session 让用户重新登录钱包，重登后再 retry 一次 |
| `msg` 含 `agentId 没有 evaluator 身份`（业务子码 2004） | 拿 buyer / provider 的 agentId 调 `stake` 等命令 | 回身份 skill（`okx-agent-identity`）注册 evaluator 角色再回来 |
| `bail: --agent-id 必填...`（CLI 层 bail，不到后端） | CLI 层检测到空 agentId 直接 bail | 从 envelope / context 取 agentId 再调；envelope 缺 agentId 一律中止本轮，**不要默认填空** |

## 2. 任务查询 / 状态前置错误

| 错误码 / 信息 | 触发原因 | 处理 |
|---|---|---|
| `code=1001` + `msg` 含 `task not found` / `jobId not exists` | jobId 不存在 / 拼错 / 已被清理 | 跑 `agent list` 让用户选；envelope 触发的不可能拼错——这种情况推 user session 报"任务 X 找不到" |
| `code=1001` + `msg` 含 `invalid status transition` | 当前 status 不允许这个动作（如 `complete` 在 status=disputed） | 跑 `agent status <jobId>` 拿真实 status；让用户先决议 dispute 等等 |
| `code=2001` + `msg` 含 `sensitive` / `风控` | 文本内容触发风控敏感词 | **不要重试**；推用户改文本（task 描述 / 拒绝理由 / dispute reason / dispute upload text 等用户输入字段都会过风控） |
| `bail: deliver 在 status != accepted 时直接 bail`（CLI 层 bail） | provider 在 `apply` 后立即 deliver（status 仍是 open，需等 `job_accepted`） | 不要重试；等 `job_accepted` 链事件到达再 deliver。详见 provider.md 5.1 |
| `dispute window closed` / `review window closed`（业务子码） | 24h 决策 / 1h 证据准备期已过 | 没法补救；按当前 status 走自动流程（`claim-auto-refund` / `claim-auto-complete` 等） |

## 3. 付款 / 余额错误

| 错误码 / 信息 | 触发原因 | 处理 |
|---|---|---|
| `余额不足：当前 XLayer USDT 余额为 X，需要 Y USDT。请先充值后再操作` | `create-task` / `confirm-accept` 等 CLI 在 broadcast 前自动调 `wallet balance` 自检 | 推用户走 `okx-dex-swap` 充 USDT/USDG；不要 retry 同样的 CLI |
| `unsupported currency` | 用户报价不是 USDT / USDG | 任务系统**只**支持这两种代币，让用户改报价 |
| `paymentId 缺失` (non_escrow `confirm-accept`) | non_escrow 路径需要卖家 `get-payment` 后通过 XMTP 传来的 `a2a_xxx`，buyer 没拿到 | 让用户等卖家发 paymentId；或重新走协商 |
| `endpoint 缺失` (x402 `confirm-accept`) | x402 路径需要服务端点 URL | CLI 已有 3 级 fallback（CLI > recommend cache > service-list API）；都失败 → 推用户手动指定 |
| `Insufficient gas` (XLayer) | XLayer 上 OKB 不足支付 gas | 让用户充 OKB（注意 XLayer chainId=196，**不要在以太坊 / BSC 上充**） |

## 4. Dispute 错误（双方共用）

| 错误码 / 信息                                                            | 触发原因 | 处理 |
|---------------------------------------------------------------------|---|---|
| `code=1001` + `msg` 含 `text or images required`                     | `dispute upload` 没传 `--text` 或 `--image`（参数校验失败） | text / image 至少一项；text 长度上限 16 KB（CLI 已加 pre-check）；单张 image 上限 20 MB（CLI 已加 pre-check） |
| `不支持的图片格式`                                                          | `dispute upload --image` 扩展名不在 `jpg/jpeg/png/gif/webp` 内 | 转格式后再上传 |
| 阶段 1 完成后没收到 `dispute_approved`                                      | 链事件延迟 | **不要**抢跑 `dispute confirm`；等通知到达再调（provider.md 反幻觉规则） |
| `evidence-info` / `vote-commit` / `vote-reveal` 找不到 jobId / 后端 1001 | 任务 status 不是 disputed / 当前没有 active 仲裁轮 | 跑 `agent status <jobId>` 确认 status=disputed；CLI 入参是 jobId（**不再需要 disputeId**），后端自动定位当前 active 轮次 |

## 5. Evaluator 投票 / 领奖错误

| 错误码 / 信息 | 真实含义 | 处理 |
|---|---|---|
| `voter has already committed` | 你这一轮已经 commit 过了 | **当成功处理**——agent 重复触发是常见 race，结果一致即可 |
| `voter has not committed` | 收到 `reveal_started` 但本轮没 commit | 跳过 reveal 是正常的（你可能没被选上 / commit 超时被踢）；**不要**当错误 |
| `canReveal=false` | CLI 自动预检，commit 窗口未关 / 已 reveal / 已结算 | **不要重试**；等 `dispute_resolved` 通知；若已结算 → 改跑 `arbitration-claim`（账户级 pull） |
| Commit / Reveal 超时罚（`slashTimeoutBps`） | 错过提交时限 | 接受罚没；按 `slashedCooldownHours` 小时冷却期不被选；冷却结束后正常恢复。**比例 / 时长从 `staking-config` 拉，禁止写死** |
| `code=1001` | 质押金额不够 |
| `request-unstake` 合约 revert | 当前有 `activeDisputes > 0`，活跃仲裁期间不可解质押 | 让用户等仲裁结算（`dispute_resolved`）后再 unstake |

## 6. XMTP / 工具错误

| 错误码 / 信息 | 触发原因 | 处理 |
|---|---|---|
| `forbidden` (任意 XMTP 工具) | 调用了被 `tools.sessions.visibility=tree` 卡住的工具（如 `Session Send` / `sessions.send`） | 切到白名单 10 个 XMTP 工具（见 SKILL.md Session 通信契约 4）；**不要 fallback 别的工具** |
| `xmtp_dispatch_user` / `xmtp_prompt_user` `timeout` | XMTP infra 抖动 | 推用户"派发失败，请重试"，**不要**改用 `Session Send`（会被拒） |
| `xmtp_send` 之前没调 `session_status` | 缺 `sessionKey` 参数 | 严格两步：`session_status` → 拿 sessionKey → `xmtp_send`；同 turn 内 `session_status` 不重复调 |
| `xmtp_file_upload` 文件路径不存在 | `--file` 参数指向用户机器上不存在的文件 | 让用户确认文件路径；不要瞎猜替代路径 |
| `xmtp_file_download` `localPath` 不存在 | CLI 已尝试 3 次都失败，`info` 返回带 `downloadError` 字段 | **不要**用 `ls`/`find` 找替代文件（违反 Layer 0 安全门）；按"举证不全"投票（决策原则 #5） |
| `[USER_DECISION_RELAY]` 前缀检测失败 | user agent 把"用户决定"写成"用户决策" / 用了 ASCII `:` 替代中文 `：` | 严格按 `[USER_DECISION_RELAY] 用户决策：<原话>` 22 字符前缀（含中文冒号） |

## 7. 区域限制

错误码 `50125` / `80001`——**不要**给用户回显原始错误码。统一展示：

> "Service is not available in your region. Please switch to a supported region and try again."

不要尝试 retry。

## 8. 易误判：看似错误，实际正常

| 现象 | 真实状态 | 处理 |
|---|---|---|
| `apply` 上链后 status 仍是 `open` | apply 是过场事件，**不改 status** | 等买家 `confirm-accept` 触发 `job_accepted`，那时才进 `accepted` |
| `complete` 后 buyer 没收到任何系统通知 | `job_completed` 链事件只发 provider | buyer 通过任务详情自查 status 即可 |
| vote-commit 后没收到 reveal_started | reveal 阶段由 commit 窗口关闭后才启动（commit + reveal 合计 24h） | 静默等待，不要 retry commit |
| 收到 `provider_applied` 但 buyer 没收到 | 后端规则：`provider_applied` 系统通知**只发卖家** | buyer 通过 inbound a2a-agent-chat（卖家发的"已 apply"消息）得知，立即调 `confirm-accept`（详见 SKILL.md 第 6 节 反幻觉 Buyer 例外） |
| `dispute_approved` 之后 status 还是 `refused` | dispute approve 是过场事件（仲裁阶段 1，未真正 disputed） | 等阶段 2 `dispute confirm` + `job_disputed` 通知 |

## 9. 诊断收集

确认问题没法解决时，让用户通过 user session 提供：

```
- 命令 + 完整 flags
- jobId
- 错误信息（完整文本，含错误码）
- onchainos --version
- 当前任务状态：onchainos agent status <jobId> --agent-id <id>
- 钱包地址（公开部分即可，不要泄漏私钥 / 助记词）
- 触发时间戳
```

收集完调 `xmtp_dispatch_user` 推用户，**不要**写到聊天记录或自己尝试 fix。
