# P2-3: service-match + designated 路由合并 — 详细分析

## 一、当前流程（逐调用追踪）

### 1.1 指定服务商 + x402 路径（最常见的快速路径）

```
LLM Turn 0: 收到 system event {event: "job_created", jobId: "abc"}
  ├── 读 SKILL.md → 判断 role → 读 buyer.md
  └── 调用: onchainos agent next-action --jobid abc --event job_created --role buyer --agentId 100
                                                                    ↓
CLI Turn 0 返回: match_provider.rs::job_created()
  ├── 检测到 designated provider (dp_id=802)
  └── 输出: job_created_with_designated_provider()
      ├── preamble_medium (~800 token)         ←── 重复开销
      ├── Step 0: xmtp_dispatch_user 通知用户   (~200 token)
      └── route_only() 路由指令                 (~500 token)
          ├── D-Step 1: `onchainos agent designated-route --provider 802 --job-id abc --agent-id 100`
          └── D-Step 2: 根据 route 值调 next-action --event designated_x402|designated_a2a|designated_error
      总输出: ~1500 token
                                                                    ↓
LLM Turn 1: 执行 playbook
  ├── 调用 xmtp_dispatch_user（通知用户任务已上链）
  ├── 调用 designated-route CLI                    ← CLI 调用 #2
  │     └── CLI 内部: POST /asp/service/match API → 返回 JSON
  │         {route: "x402", endpoint: "https://...", feeAmount: "0.01", feeTokenSymbol: "USDT", ...}
  ├── LLM 读 JSON → 映射 route="x402" → event="designated_x402"     ← 确定性逻辑，LLM 浪费推理
  └── 调用 next-action --event designated_x402 --provider 802       ← CLI 调用 #3（额外轮次！）
                                                                    ↓
CLI Turn 1 返回: designated.rs::branch_x402()
  ├── preamble（已附在外层，此处仅 body）
  └── DX-Step 1: x402-validate 命令 + 5 分支决策树    (~1000 token)
      ├── result=="x402_invalid" → pending-decisions-v2 模板
      ├── result=="input_required" → 构造 body 重试
      ├── result=="price_mismatch" → pending-decisions-v2 模板
      ├── result=="over_budget" → pending-decisions-v2 模板
      └── result=="pass" → A-Step 3: set-payment-mode
      总输出: ~1800 token（含 preamble ~2600 token）
                                                                    ↓
LLM Turn 2: 执行 branch_x402 playbook
  ├── 调用 x402-validate CLI                                       ← CLI 调用 #4
  │     └── CLI 内部: 并行执行 x402-check + fetch_task_budget → 返回 JSON
  │         {result: "pass", amountHuman: "0.01", tokenSymbol: "USDT", endpoint: "...", ...}
  ├── LLM 读 JSON → 映射 result="pass" → 执行 A-Step 3             ← 确定性逻辑
  └── 调用 set-payment-mode CLI                                    ← CLI 调用 #5
      └── 返回 {confirming: true} 或 {alreadySet: true}
```

### 1.2 指定服务商 + a2a 路径

```
（Turn 0 同上）
                                                                    ↓
LLM Turn 1:
  ├── xmtp_dispatch_user
  ├── designated-route → {route: "a2a", ...}
  ├── LLM 映射 route="a2a" → event="designated_a2a"                 ← 确定性逻辑
  └── next-action --event designated_a2a --provider 802              ← 额外轮次！
                                                                    ↓
CLI 返回: designated.rs::branch_a2a()
  └── B-Step 0~2: 建群 + SKILL_PREFETCH + 首条询价消息              (~1500 token)
                                                                    ↓
LLM Turn 2: 执行 branch_a2a
  ├── session_status
  ├── xmtp_start_conversation
  ├── xmtp_dispatch_session (SKILL_PREFETCH)
  └── xmtp_send (首条询价)
```

### 1.3 调用时序图

```
                 LLM                          CLI
                  │                            │
  T0 ────────────►│ next-action job_created    │
                  │───────────────────────────►│
                  │     playbook (1500 tok)     │
                  │◄───────────────────────────│
                  │                            │
  T1 ────────────►│ xmtp_dispatch_user         │
                  │ designated-route            │───► /asp/service/match API (~2s)
                  │◄───────────────────────────│     JSON result
                  │                            │
      ┌───────────│ LLM 推理: route→event 映射 │     ← 浪费 ~100 token + ~1s
      │           │                            │
      └──────────►│ next-action designated_x402│     ← 额外 CLI 调用
                  │───────────────────────────►│
                  │   branch playbook (1800tok)│     ← 额外 playbook 输出
                  │◄───────────────────────────│
                  │                            │
  T2 ────────────►│ x402-validate              │───► x402-check + budget (~1.5s)
                  │◄───────────────────────────│     JSON result
                  │                            │
      ┌───────────│ LLM 推理: result→步骤映射   │     ← 浪费 ~100 token + ~0.5s
      └──────────►│ set-payment-mode           │───► on-chain tx
                  │◄───────────────────────────│
```

## 二、浪费点分析

### 2.1 `next-action designated_*` 是纯浪费

| 项目 | 说明 | Token 开销 |
|------|------|-----------|
| LLM 读 designated-route JSON | 解析 route/errorType 字段 | ~50 input |
| LLM 路由映射推理 | "route=x402 → event=designated_x402" 这是查表 | ~100 output |
| `next-action designated_*` CLI 调用 | CLI 只是调用 `branch_x402()`/`branch_a2a()`/`branch_error()` | 0（CLI 本身很快） |
| CLI 返回 branch playbook | 含 preamble + body | ~1800~2600 input |
| LLM 读取 branch playbook | 再次读 preamble + 理解 body | 全量 input |
| **合计浪费** | | **~2000~2700 token** |

额外的时间开销：
- LLM 推理延迟：~1~2s（读 JSON + 路由 + 构造 next-action 命令）
- CLI 执行延迟：~100ms（next-action 本身很快，仅生成字符串）
- LLM 读取新 playbook：~1~2s
- **合计额外延迟：~2~4s**

### 2.2 `x402-validate` 的决策树输出也有冗余

designated_x402 playbook 输出了完整的 5 分支决策树（~1000 token），但 `x402-validate` CLI 已经在内部完成了全部判断，只返回一个 `result` 字段。LLM 读决策树只是为了将 `result` 映射到对应操作——这也是确定性逻辑。

## 三、优化方案

### 方案 A: `designated-route --with-playbook`（推荐，最小改动）

**思路**：让 `designated-route` 在返回路由结果后，直接附带匹配的 branch playbook。LLM 不再需要调用 `next-action designated_*`。

**改动范围**：

1. **`common/mod.rs::handle_designated_route()`** — 增加 `--with-playbook` 参数
2. 当 `--with-playbook` 存在时，在确定 route 后，调用 `designated::branch_x402()` / `branch_a2a()` / `branch_error()` 生成 playbook 文本
3. 在 JSON 输出中增加 `"playbook"` 字段

```rust
// common/mod.rs — handle_designated_route 修改
pub async fn handle_designated_route(
    provider_id: &str,
    target_endpoint: Option<&str>,
    job_id: &str,
    agent_id: &str,
    with_playbook: bool,    // 新增参数
) -> Result<()> {
    // ... 现有逻辑不变 ...
    
    // 在 crate::output::success(result) 之前：
    if with_playbook {
        let short_id = short_job_id(job_id);
        let playbook = match route_str {
            "x402" => buyer::flow_negotiate::designated::branch_x402(
                job_id, agent_id, &short_id, &provider_agent_id,
            ),
            "a2a" => {
                let title = fetch_task_title(job_id, agent_id).await.unwrap_or_default();
                buyer::flow_negotiate::designated::branch_a2a(
                    job_id, agent_id, &short_id, &provider_agent_id, &title,
                )
            },
            _ => buyer::flow_negotiate::designated::branch_error(
                job_id, agent_id, &short_id, &provider_agent_id,
            ),
        };
        result["playbook"] = serde_json::json!(playbook);
    }
    
    crate::output::success(result);
    Ok(())
}
```

4. **`match_provider.rs::route_only()`** — 修改指令

```diff
- onchainos agent designated-route --provider {dp_id} --job-id {job_id} --agent-id {agent_id}
+ onchainos agent designated-route --provider {dp_id} --job-id {job_id} --agent-id {agent_id} --with-playbook
```

```diff
- **D-Step 2 — call `next-action` with the matching branch pseudo-event:**
- | `route` value | `errorType` | next-action `--event` |
- |---|---|---|
- ...
- Execute:
- ```bash
- onchainos agent next-action --jobid {job_id} --event <from table above> --role buyer --agentId {agent_id} --provider {dp_id}
- ```
+ **D-Step 2 — follow the `playbook` field in the response directly.**
+ The `playbook` field contains the matching branch instructions. Execute them verbatim.
+ 🛑 Do NOT call `next-action --event designated_*` — the playbook is already inline.
```

**优化后时序**：

```
                 LLM                          CLI
                  │                            │
  T0 ────────────►│ next-action job_created    │
                  │───────────────────────────►│
                  │     playbook (1500 tok)     │
                  │◄───────────────────────────│
                  │                            │
  T1 ────────────►│ xmtp_dispatch_user         │
                  │ designated-route            │
                  │   --with-playbook           │───► /asp/service/match API (~2s)
                  │◄───────────────────────────│     JSON + playbook inline
                  │                            │
                  │ (直接读 playbook 字段)       │     ← 省掉了路由推理 + next-action 调用
                  │                            │
  T2 ────────────►│ x402-validate              │───► x402-check + budget (~1.5s)
                  │◄───────────────────────────│
                  │ set-payment-mode            │───► on-chain tx
                  │◄───────────────────────────│
```

**改动量**：~60 行 Rust + ~20 行 playbook 文本

---

### 方案 B: `designated-route-full`（更激进，合并 x402-validate）

**思路**：在方案 A 基础上，x402 路径时 CLI 连 x402-validate 也一并执行，直接返回最终操作指令。

```
designated-route-full --provider 802 --job-id abc --agent-id 100
```

**内部流程**：
1. POST /asp/service/match → 确定路由
2. 如果 route=x402:
   - 自动调用 x402-check (endpoint 校验)
   - 自动调用 fetch_task_budget (预算检查)
   - 返回最终结论: `{route: "x402", validation: "pass", nextCommand: "onchainos agent set-payment-mode abc --payment-mode x402 --token-symbol USDT --token-amount 0.01 --endpoint https://..."}`
3. 如果 route=a2a: 返回 a2a branch playbook
4. 如果 route=error: 返回 error branch playbook

```rust
// 伪代码
pub async fn handle_designated_route_full(...) -> Result<()> {
    // Step 1: service match
    let route_result = call_service_match(provider_id, job_id, agent_id).await?;
    
    match route_result.route.as_str() {
        "x402" => {
            // Step 2: x402 validate (内部执行)
            let validate_result = handle_x402_validate_internal(
                &route_result.endpoint,
                agent_id,
                job_id,
                &route_result.fee_amount,
                &route_result.fee_token,
            ).await?;
            
            match validate_result.result.as_str() {
                "pass" => {
                    // 直接返回 set-payment-mode 命令
                    output::success(json!({
                        "route": "x402",
                        "action": "set-payment-mode",
                        "command": format!("onchainos agent set-payment-mode {} --payment-mode x402 --token-symbol {} --token-amount {} --endpoint {}",
                            job_id, validate_result.token_symbol, validate_result.amount_human, route_result.endpoint),
                        "playbook": "执行上面的 command，然后根据输出判断...",
                    }));
                }
                "x402_invalid" | "over_budget" | "price_mismatch" => {
                    // 直接返回 pending-decisions-v2 command
                    output::success(json!({
                        "route": "x402",
                        "action": "user_decision",
                        "validation": validate_result.result,
                        "playbook": branch_x402_error_playbook(...),
                    }));
                }
                "input_required" => {
                    output::success(json!({
                        "route": "x402",
                        "action": "input_required",
                        "fields": validate_result.fields,
                        "playbook": "需要 LLM 从 task description 提取参数...",
                    }));
                }
            }
        }
        "a2a" => {
            output::success(json!({
                "route": "a2a",
                "playbook": branch_a2a(job_id, agent_id, ...),
            }));
        }
        "error" => {
            output::success(json!({
                "route": "error",
                "errorType": route_result.error_type,
                "playbook": branch_error(job_id, agent_id, ...),
            }));
        }
    }
}
```

**优化后时序（x402 pass 路径）**：

```
                 LLM                          CLI
                  │                            │
  T0 ────────────►│ next-action job_created    │
                  │───────────────────────────►│
                  │     playbook               │
                  │◄───────────────────────────│
                  │                            │
  T1 ────────────►│ xmtp_dispatch_user         │
                  │ designated-route-full       │───► service/match + x402-check + budget
                  │                            │     (并行, ~2.5s)
                  │◄───────────────────────────│
                  │                            │     JSON: {route:"x402", validation:"pass",
                  │                            │            command:"set-payment-mode ..."}
                  │                            │
                  │ set-payment-mode            │───► on-chain tx
                  │◄───────────────────────────│
```

**对比原流程省掉了**：
- ❌ LLM 读 designated-route JSON + 路由推理
- ❌ `next-action designated_x402` CLI 调用
- ❌ LLM 读 branch_x402 playbook (~1800 token)
- ❌ LLM 读 x402-validate JSON + 分支判断推理
- ❌ `x402-validate` CLI 调用（CLI 内部完成）

**改动量**：~200 行 Rust

---

### 方案 C: 上移到 `next-action job_created` 内部（最激进）

**思路**：让 `next-action --event job_created` 本身就调用 service-match API，直接返回最终的 branch playbook。

**问题**：
- `next-action` 的 `generate_next_action()` 当前是同步函数（返回 `String`）
- 调用 service-match API 需要 async
- 需要重构 `generate_next_action()` 为 async，影响面大

**结论**：改动量太大（~500 行），不推荐作为第一步。可作为长期重构目标。

## 四、方案对比

| | 方案 A | 方案 B | 方案 C |
|---|---|---|---|
| **省掉的 CLI 调用** | 1 次 (next-action designated_*) | 2 次 (next-action + x402-validate) | 3 次 (designated-route + next-action + x402-validate) |
| **省掉的 LLM 轮次** | 0（同一 turn 内） | 0~1 | 1 |
| **Token 节省** | ~2,500/任务 | ~4,500/任务（x402 路径） | ~5,500/任务 |
| **延迟节省** | ~2~3s | ~4~6s | ~5~8s |
| **Rust 改动量** | ~60 行 | ~200 行 | ~500 行 |
| **风险** | 极低 | 低（x402-validate 逻辑复制） | 中（async 重构） |
| **兼容性** | 向后兼容（新 flag） | 新命令（可并存） | 需要改函数签名 |

## 五、推荐路径

### Phase 1: 实施方案 A（1 天）

最小改动，快速见效：

1. `common/mod.rs::handle_designated_route()` 增加 `with_playbook: bool` 参数
2. 当 `with_playbook=true` 时，在 JSON output 中增加 `"playbook"` 字段
3. `designated.rs::route_only()` 修改指令为 `--with-playbook`
4. 删除 D-Step 2 路由表，替换为 "follow playbook field directly"

**验证**：
- designated + x402 路径：LLM 应在 1 个 turn 内完成 dispatch_user + designated-route + 读 playbook + x402-validate + set-payment-mode
- designated + a2a 路径：LLM 应在 1 个 turn 内完成 dispatch_user + designated-route + 读 playbook + 建群 + SKILL_PREFETCH + 首条消息
- designated + error 路径：LLM 应在 1 个 turn 内完成 dispatch_user + designated-route + 读 playbook + pending-decisions-v2

### Phase 2: 实施方案 B（3 天，可选）

在方案 A 生效后，如果 x402 路径仍有明显的 LLM 推理浪费，再合并 x402-validate：

1. 新增 `handle_designated_route_full()` 异步函数
2. x402 路径内部串联 x402-check + budget 校验
3. pass 路径直接输出 set-payment-mode 命令
4. 非 pass 路径输出对应的 pending-decisions-v2 playbook

## 六、详细实现（方案 A）

### 6.1 需修改的文件

```
cli/src/commands/agent_commerce/task/common/mod.rs      ← handle_designated_route 增加参数
cli/src/commands/agent_commerce/task/buyer/mod.rs        ← clap 命令增加 --with-playbook flag
cli/src/commands/agent_commerce/task/buyer/flow_negotiate/
  ├── match_provider.rs                                  ← route_only() 修改指令文本
  └── designated.rs                                      ← route_only() 修改指令文本
```

### 6.2 handle_designated_route 修改

```rust
// common/mod.rs

pub async fn handle_designated_route(
    provider_id: &str,
    target_endpoint: Option<&str>,
    job_id: &str,
    agent_id: &str,
    with_playbook: bool,    // 新增
) -> Result<()> {
    // ... 现有逻辑完全不变 ...
    
    // 在每个 crate::output::success(result) 调用前，
    // 如果 with_playbook == true，附加 playbook 字段：
    
    // 例如 x402 分支（line 794-822）：
    // 在 crate::output::success(result) 之前增加：
    if with_playbook {
        let short_id = util::short_job_id(job_id);
        let playbook_text = crate::commands::agent_commerce::task::buyer
            ::flow_negotiate::designated::branch_x402(
                job_id, agent_id, &short_id, &provider_agent_id,
            );
        result["playbook"] = serde_json::json!(playbook_text);
    }
    
    // a2a 分支（line 745-757）类似：
    if with_playbook {
        let short_id = util::short_job_id(job_id);
        // 需要 title — 可从 task detail 获取，或传入
        let title_display = /* 从 API 或参数获取 */;
        let playbook_text = crate::commands::agent_commerce::task::buyer
            ::flow_negotiate::designated::branch_a2a(
                job_id, agent_id, &short_id, &provider_agent_id, &title_display,
            );
        result["playbook"] = serde_json::json!(playbook_text);
    }
    
    // error 分支（line 823-834）类似
}
```

### 6.3 route_only() 修改

```rust
// designated.rs::route_only()

pub(crate) fn route_only(job_id: &str, agent_id: &str, _short_id: &str, dp_id: &str, endpoint: Option<&str>) -> String {
    let endpoint_flag = match endpoint.filter(|s| !s.is_empty()) {
        Some(ep) => format!(" --endpoint {ep}"),
        None => String::new(),
    };
    format!("\
             🎯 **Designated ASP**: {dp_id}\n\
             ⚠️ The persisted designated-provider file has already been removed by the CLI.\n\n\
             **D-Step 1 — query ASP route + get branch playbook (single CLI call):**\n\
             ```bash\n\
             onchainos agent designated-route --provider {dp_id} --job-id {job_id} --agent-id {agent_id}{endpoint_flag} --with-playbook\n\
             ```\n\
             Response includes `route`, provider info, and a `playbook` field.\n\n\
             **D-Step 2 — execute the `playbook` field directly.**\n\
             The `playbook` field contains the complete branch instructions for the matched route.\n\
             Follow it verbatim — do NOT call `next-action --event designated_*`.\n\n\
             🛑 Multi-service selection: if `services` array is present, match the intended service first \
             (see task description), then use that service's endpoint/fee for subsequent steps.\n")
}
```

### 6.4 需要注意的问题

1. **`branch_a2a()` 需要 `title_display`**：当前 `handle_designated_route()` 没有 task title 信息。需要从 task detail API 获取，或在 CLI 参数中传入 `--title`（推荐后者，避免额外 API 调用）。

2. **`branch_a2a()` 调用 `list_attachment_paths()`**：这个函数读本地文件系统，在 `handle_designated_route()` 的 async context 中可能需要 spawn_blocking。不过该函数很轻量（只是 readdir），直接调用即可。

3. **Playbook 文本可能很长**（branch_a2a ~1500 token, branch_x402 ~1000 token, branch_error ~800 token）：JSON 中嵌入大段文本需要正确转义。`serde_json::json!()` 会自动处理。

4. **向后兼容**：不带 `--with-playbook` 时行为完全不变。旧版 SKILL.md 的 LLM 不受影响。

5. **LLM 理解成本**：从"读 JSON→查表→调 next-action→读 playbook"变成"读 JSON 中的 playbook 字段→执行"，对 LLM 来说更简单，不需要额外的路由推理。

## 七、Token 节省详算

### 当前（x402 pass 路径）

| 步骤 | Output token | Input token | 说明 |
|------|-------------|------------|------|
| designated-route JSON | ~200 | ~200 | CLI 输出 |
| LLM 路由推理 | ~100 | 0 | route→event 映射 |
| next-action designated_x402 调用 | 0 | 0 | CLI 命令本身 |
| branch_x402 playbook 接收 | 0 | ~2600 | preamble + body |
| LLM 读 playbook + 决定执行 | ~50 | 0 | 理解 playbook |
| **合计（额外开销）** | **~350** | **~2800** | **~3150 token** |

### 优化后

| 步骤 | Output token | Input token | 说明 |
|------|-------------|------------|------|
| designated-route --with-playbook JSON | ~200 | ~1200 | 含 playbook 字段 |
| LLM 读 playbook 字段 | ~50 | 0 | 直接执行 |
| **合计** | **~250** | **~1200** | **~1450 token** |

### 节省

- **Token**: ~1700/任务（仅 x402 pass 路径；error/a2a 路径类似）
- **延迟**: ~2~3s（省掉 next-action CLI 调用 + LLM 推理轮次）
- **LLM 轮次**: 不变（优化在同一 turn 内生效），但 turn 内工具调用减少 1 次

> 注：之前文档估算 ~3000 token/任务是包含了 preamble 重复的全量计算。精确来说核心节省是 ~1700 token + ~2~3s 延迟。如果结合 P0 preamble 去重，总节省会更大。
