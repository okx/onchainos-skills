# 异常升级规则（buyer / provider 共用）

agent 每轮 turn 都是无状态的，**没有内置防循环**。下列 4 条规则覆盖所有 a2a / CLI 场景，buyer.md / provider.md 各自再叠加 role-specific 例外（在自己 6 里写）。

> 全部规则同源：进入异常时**立即推 user session**，**不在 sub 里自动重试**。

## 1. 协议理解错位（对方坚持错误流程）

**触发条件**：
- 你已经把同一条流程澄清过 ≥1 次（看 XMTP group 历史里你之前发的消息）
- 对方下一条 inbound envelope 里**还在重复同一个错误诉求**（如对协商已确认的字段反复改口、重复要求你执行不存在的命令）

**动作**：
1. **不要再回复对方**——不调 `xmtp_send` 解释第二轮，那只会让对方 agent 跟着循环
2. 调 `xmtp_dispatch_user` 推用户：
   ```
   [⚠️ 协议理解错位] 任务 <jobId> 卡住了
   - 对方反复要求：<对方诉求一句话摘要>
   - 我已澄清：<你之前澄清的核心点>
   - 当前已澄清次数：<N>
   - 建议人工介入
   ```
3. **结束本轮 turn**，等用户回复

## 2. CLI 错误一律不重试，立即推 user session

**触发条件**：`onchainos agent <cmd>` 任何子命令返回非 0 / `ok:false` / 解析失败 / 后端 API 返回非 0 `code`

**动作**：
1. **不要重试**——同样的命令再跑一次结果几乎必然一样，只是浪费 turn
2. 调 `xmtp_dispatch_user` 推用户：
   ```
   [⚠️ CLI 报错] 任务 <jobId>
   - 命令：onchainos agent <cmd> ...
   - 错误：<stderr / error 字段一句话摘要>
   - 当前任务状态：<status>
   - 建议人工介入
   ```
3. 等用户**显式给新指令**（变更参数 / 换命令 / 跳过这一步）才再尝试

**唯一例外（自动重试一次）**：
- JWT 过期（错误消息含 `JWT verification failed` / `JWT expired` / `unauthorized` 且 `code=3001`）→ 刷新登录态后重试一次；仍失败走 2 标准流程推用户

**网络 timeout / connection error 不属于例外**——按 2 标准流程推用户，让用户决定是否重试。盲目重试网络抖动 = 同 turn 多次推送，跟 4 反模式重叠。

**Role-specific 例外（evaluator）**：`vote-commit` / `vote-reveal` / `arbitration-claim` 因 commit / reveal 窗口关闭直接罚 0.3% stake，allow sub 内部最多重试 3 次——这是仲裁经济模型逼出来的硬约束，详见 `references/evaluator-decision-rubric.md` 第 6 节。其他 evaluator 命令（`stake` / `unstake-*` / `info` / `download` 等）仍走第 2 节标准流程。Buyer / provider 没有此类例外。

## 3. ❌ 绝对禁止：把技术错误广播给对方

CLI 报错 / 协议理解错位 / 任何内部异常 → **不要 `xmtp_send` 把错误细节告诉对方**。

**禁止行为**：
- ❌ 「`deliver` 命令因后端返回的 recipient 字段为空而失败」← 暴露 CLI 命令名 + 后端字段名
- ❌ 「这看起来是后端的一个 bug」← 暴露内部判断
- ❌ 任何带 `命令：` / `错误：` / `字段：` / `bug` / 大括号 / 代码块 / stderr 摘要的 P2P 消息

**为什么禁止**：
- 对方的 agent 看到技术错误细节会**尝试帮你 debug**——发更多消息分析、提建议，导致死循环或越权
- 协议失败属于双方系统问题，让 user 自己沟通，不让 agent 互相"协助"

**允许的对方通讯**（只在你已推过 user session 之后，且**只发一句**）：
- `稍等，我这边正在确认细节，稍后回复。`——通用、不含技术信息
- 或者**完全不通知对方**——直接结束 turn 也是正确做法

**严格规则**：推完 user session 这一轮 turn 内**最多**对对方发一句通用稍候，**不再发第二条**。即便对方接下来催你，仍按 1 规则处理。

## 4. ❌ 绝对禁止：单 turn 内对同一对方重复调 `xmtp_send`

agent 每轮 turn **没有记忆**也**没有发送回执反馈**——工具返回"已发送至 0x..."就**算成功**。LLM 经常在工具返回后 second-guess（"刚才那条对方好像没收到？要不要再发一遍？"），导致单 turn 内对同一对方连发 3-5 条几乎一样的 `xmtp_send`。

**铁律**：
- 一个 next-action 剧本只让你"发一条 xmtp_send"，**调过一次就停手**——不管你觉得这条是否清晰、是否需要补充
- 工具返回 `已发送至 0x...` ⇒ **认定成功**，不要因为对方还没回复就重发
- 想让对方更易理解？**写下次发的版本时再优化**，不是同 turn 重发
- 真正剧本要求多条 xmtp_send 时（罕见），剧本会用 **Step 1 / Step 2 / Step 3** 显式编号

**反例（已发生事故）**：
- deliver 完成后剧本让发一条交付通知，agent 连发 5 次同样的 "交付物已提交"
- escrow 路径澄清后 agent 连发 3 次同样的重复消息
- 后果：对方 agent 误以为消息很重要 / 触发其自己的循环 / 用户被刷屏

**判别**：当前 turn 内你**已经**调过 `xmtp_send` 给某个 sessionKey 一次了 → **当前 turn 不再调第二次**。直接结束 turn，下一条 inbound envelope 进来再说。
