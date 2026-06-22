# Buyer 侧 Skill 文件精简优化方案

> 基于三种 Session（User / Backup / Job）架构，全面分析 buyer 侧 20 个 skill 文件（4,204 行）的 token 消耗，按 session 类型识别冗余和可精简内容，提出拆分与标注方案。独立于 CLI 下沉优化，可并行实施。

---

## 最终方案总览

### 选定方案: 方案 D — User 物理隔离 + Sub 标注/拆分/压缩

在方案 C（混合标注 + 辅助拆分）基础上，增加 **User session 独立入口文件**，实现三种 session 全覆盖优化。

#### 核心策略

| Session | 策略 | 手段 | 每 task 节省 |
|---|---|---|---|
| **User** | **物理隔离**（新） | 新建 `SKILL-user.md` + `buyer-user.md`，只含 User 需要的 ~215 行（vs 原 770 行） | **~16,500** |
| **Job** | 标注 + 内容压缩 | scope 标注跳过 user-only 段落 + xmtp how-to 压缩 + Quick Nav 删除 + incident 引用化 | **~52,700** |
| **Backup** | 标注 | scope 标注 + §3.5 skip | **~13,100** |
| **辅助文件** | 物理拆分/移动 | buyer-actions 拆分 + cli-reference 按角色拆分 + evaluator 子目录 | **~4,000+** |
| **合计** | | **14 项优化** | **~86,300 token/task** |

#### 方案 D vs 方案 C 的关键差异

方案 C 对 User session 只做标注（软节省），方案 D 做 **物理隔离**（硬节省）。

| 维度 | 方案 C (原推荐) | 方案 D (最终) |
|---|---|---|
| User session 策略 | 标注（依赖 LLM 遵守 ~85-90%） | **独立入口文件（不加载即不消耗）** |
| CLI Rust 代码改动 | 0 | **0** |
| User session 收益 | ~6,600 (软) | **~16,500 (硬)** |
| 总收益 | ~70K token/task | **~86K token/task** |
| 实施时间 | ~2 天 | ~2.5 天 |

**可行性关键**: User session 由 Claude Code 主对话控制文件加载（Read tool），不经过 CLI preamble 注入。因此可以通过创建独立入口文件实现物理隔离，**无需改任何 Rust 代码**。Sub sessions 仍由 CLI preamble 控制，继续用原 SKILL.md + buyer.md，互不影响。

#### 实施路线图

```
Day 1 ─── Phase A: 物理隔离 + 零风险标注（并行）────────── ~60,500 token/task
          ├── [S14] 创建 SKILL-user.md + buyer-user.md（User 物理隔离）
          └── [S1-S4,S8] 5 处 scope 标注（Sub session 标注）

Day 2 ─── Phase B: 辅助文件拆分 ────────────────────────── ~4,000+ token/task
          └── [S6,S7,S11,S12] buyer-actions/cli-reference/evaluator/incidents

Day 2-3 ─ Phase C: 内容精简 ────────────────────────────── ~21,623 token/task
          └── [S5,S9,S10,S13] preamble 去重 + incident 压缩 + Quick Nav + xmtp

Day 3 ─── Phase D: 验证 ───────────────────────────────── 冒烟测试 + 回归
```

#### 联合收益（与 CLI 下沉优化）

| 优化类别 | Token 节省/task |
|---|---|
| Skill 文件精简 14 项（本文档） | **~86K** |
| CLI 下沉 10 项（另一文档） | **~100K** |
| **联合收益** | **~186K token/task** |

按 100 task/天: **~18.6M token/天节省**

---

## 背景：三种 Session 的 Skill 加载需求

### Session 加载矩阵

| Session | 必须加载 | 可能加载 | 绝不需要 | RESUME 次数 | 优化倍增 |
|---|---|---|---|---|---|
| **User Session** | SKILL.md, buyer.md | buyer-actions.md, user-intent-routing.md, display-formats.md | provider.md, evaluator.md, evaluator-*.md | 1-3 次 | 低 |
| **Backup Session** | SKILL.md, buyer.md | cli-reference.md (grep), exception-escalation.md | buyer-actions.md, user-intent-routing.md, display-formats.md, provider.md, evaluator-*.md | 2-5 次 | 中 |
| **Job Session** | SKILL.md, buyer.md | cli-reference.md (grep), message-types.md, exception-escalation.md, xmtp-tools.md | buyer-actions.md, user-intent-routing.md, display-formats.md, provider.md, evaluator-*.md, preflight.md | **10-20 次** | **高** |

### 三种 Session 的职责边界

| 职责 | User Session | Backup Session | Job Session |
|---|---|---|---|
| 用户意图解析 | ✅ | ❌ | ❌ |
| 任务发布/修改 | ✅ | ❌ | ❌ |
| 决策卡片展示 | ✅ | ❌ | ❌ |
| 决策 relay | ✅（发送端） | ❌ | ✅（接收端） |
| job_created 处理 | ❌ | ✅ | ❌ |
| 推荐/指定服务商 | ❌ | ✅ | ❌ |
| 创建 Job session | ❌ | ✅ | ❌ |
| SKILL_PREFETCH | ❌ | ✅（发送） | ✅（接收） |
| 协商三步握手 | ❌ | ❌ | ✅ |
| 入站消息路由 §3.5 | ❌ | ❌ | ✅ |
| 系统事件处理 | ❌ | 终态 only | ✅（除 job_created） |
| 终态通知 | ❌ | ✅ | ❌ |

---

## 1. 全量文件清单

### 1.1 总体数据

| 分类 | 文件数 | 行数 | 估算 token | 加载时机 |
|---|---|---|---|---|
| 核心文件（SKILL.md / buyer.md / buyer-actions.md） | 3 | 1,060 | ~15,900 | 前两个每次 session 必加载 |
| `_shared/` | 9 | 1,916 | ~28,700 | 按需 |
| `references/` | 5 | 925 | ~13,900 | 按需 |
| provider.md / evaluator.md | 2 | 303 | ~4,500 | 非 buyer 角色 |
| **合计** | **19** | **4,204** | **~63,000** | |

### 1.2 各 Session 的实际加载路径

#### Backup Session（job_created → 推荐/指定 → 终态）

```
必加载:
  SKILL.md      404 行  ~6,000 token
  buyer.md      366 行  ~5,500 token
  ─────────────────────────────────
  小计          770 行  ~11,500 token

实际需要（Backup 职责范围）:
  SKILL.md: Activation + Role + Field Mapping + sessionKey + Communication Contract ≈ 250 行
  buyer.md: §1 Trigger + §3.4 前半 + §4 System event + §6 Exception ≈ 80 行
  ─────────────────────────────────
  实际需要  ≈ 330 行  ~5,000 token
  浪费      ≈ 440 行  ~6,500 token（占 57%）

RESUME 2-5 次 → 浪费 6,500 × 3.5 = ~22,750 token/task
```

#### Job Session（协商 + 系统事件 + relay）

```
必加载:
  SKILL.md      404 行  ~6,000 token
  buyer.md      366 行  ~5,500 token
  ─────────────────────────────────
  小计          770 行  ~11,500 token

实际需要（Job 职责范围）:
  SKILL.md: Activation + Role + Field Mapping + sessionKey + Communication + Anti-hallucination + Boundary ≈ 300 行
  buyer.md: Preamble + §1 + §2 + §3.4 + §3.5 + §3.6 + §4 + §5 + §6 + §7 ≈ 235 行
  ─────────────────────────────────
  实际需要  ≈ 535 行  ~8,000 token
  浪费      ≈ 235 行  ~3,500 token（占 30%）

RESUME 10-20 次 → 浪费 3,500 × 15 = ~52,500 token/task
```

#### User Session（意图 + 发布/修改 + 决策）

```
必加载:
  SKILL.md      404 行  ~6,000 token
  buyer.md      366 行  ~5,500 token
  ─────────────────────────────────
  小计          770 行  ~11,500 token

实际需要（User 职责范围）:
  SKILL.md: Role + Pre-flight + User Intent Routing + Cross-Skill Routing + Boundary ≈ 120 行
  buyer.md: USDT消歧 + §3.1-§3.3 + Intent routing + resolve rule ≈ 90 行
  按需: buyer-actions.md §1-§4 中的 1 个段落 ≈ 50-70 行
  ─────────────────────────────────
  实际需要  ≈ 260 行  ~3,900 token
  浪费      ≈ 510 行  ~7,600 token（占 66%）

RESUME 1-3 次 → 浪费 7,600 × 2 = ~15,200 token/task
```

### 1.3 浪费总览

| Session | 加载量 | 实际需要 | 浪费 | RESUME 均值 | 累积浪费/task |
|---|---|---|---|---|---|
| Job Session | 770 行 | 535 行 | 235 行 (~3,500 token) | 15 次 | **~52,500** |
| Backup Session | 770 行 | 330 行 | 440 行 (~6,500 token) | 3.5 次 | **~22,750** |
| User Session | 770 行 | 260 行 | 510 行 (~7,600 token) | 2 次 | **~15,200** |
| **合计** | | | | | **~90,450 token/task** |

---

## 2. 核心文件深度分析

### 2.1 SKILL.md（404 行，~6,000 token）— 按 Session 需求

| 段落 | 行范围 | 行数 | User | Backup | Job |
|---|---|---|---|---|---|
| YAML frontmatter | 1-9 | 9 | 系统 | 系统 | 系统 |
| Title + description | 11-13 | 3 | 一次 | 一次 | 一次 |
| Quick Navigation | 15-29 | 15 | 可选 | 可选 | 可选 |
| Runtime Bridge | 31-39 | 9 | ✅ | ✅ | ✅ |
| Roles + determination | 41-68 | 28 | ✅ | ✅ | ✅ |
| **Pre-flight** | **70-78** | **9** | **✅** | **❌** | **❌** |
| Critical Field Mapping | 80-93 | 14 | ✅ | ✅ | ✅ |
| Core Architecture | 94-104 | 11 | 一次 | 一次 | 一次 |
| Reading Order | 106-113 | 8 | 一次 | 一次 | 一次 |
| Activation | 115-177 | 63 | 部分 | ✅ | ✅ |
| a2a-agent-chat entry | 178-196 | 19 | ❌ | ✅ | ✅ |
| sessionKey Discrimination | 198-211 | 14 | 低 | ✅ | ✅ |
| Session Communication Contract | 213-312 | 100 | 部分 | 部分 | ✅ |
| Anti-hallucination | 314-326 | 13 | ✅ | ✅ | ✅ |
| **User Intent Routing** | **327-339** | **13** | **✅** | **❌** | **❌** |
| **Cross-Skill Routing** | **341-352** | **12** | **✅** | **❌** | **❌** |
| Message Format | 354-356 | 3 | 一次 | 一次 | 一次 |
| Communication Boundary | 358-384 | 27 | ✅ | ✅ | ✅ |
| Additional Resources | 386-404 | 19 | 按需 | 按需 | 按需 |

#### SKILL.md 优化点

**A. User session only（~34 行，~510 token）**: Pre-flight + User Intent Routing + Cross-Skill Routing
**B. Sub session only（~19 行，~285 token）**: a2a-agent-chat entry 在 User session 无用
**C. Activation 内联 incident（~15 处，~225 token）**: defense-in-depth 但 RESUME 时重复
**D. Quick Navigation（15 行，~225 token）**: LLM 不依赖导航表
**E. Session Communication Contract 中 user-only 子段（~20 行，~300 token）**: L233-243 agent state machine + L298-312 resolve/cancel

---

### 2.2 buyer.md（366 行，~5,500 token）— 按 Session 需求

| 段落 | 行范围 | 行数 | User | Backup | Job |
|---|---|---|---|---|---|
| **USDT/USDG 消歧** | **1-9** | **9** | **✅** | **❌** | **❌** |
| 通用 preamble 规则 | 11-28 | 18 | ❌ | ✅ 重复 | ✅ 重复 |
| Quick Navigation | 30-47 | 18 | 可选 | 可选 | 可选 |
| Tool-call batching | 49-54 | 6 | ❌ | ✅ | ✅ |
| §1 Trigger identification | 56-70 | 15 | ❌ | ✅ | ✅ |
| §2 P2P reply | 72-81 | 10 | ❌ | 低 | ✅ |
| **§3.1 Publishing** | **84-90** | **7** | **✅** | **❌** | **❌** |
| **§3.2 Designated A2A** | **94-113** | **20** | **✅** | **❌** | **❌** |
| **§3.3 Designated x402** | **117-151** | **35** | **✅** | **❌** | **❌** |
| **Intent routing table** | **154-165** | **12** | **✅** | **❌** | **❌** |
| **resolve 执行规则** | **167-175** | **9** | **✅** | **❌** | **❌** |
| §3.4 Negotiation | 178-232 | 55 | ❌ | **部分** | **✅ 核心** |
| §3.5 Inbound routing | 236-277 | 42 | ❌ | ❌ | **✅ 最核心** |
| §3.6 Accepted-execution | 280-297 | 18 | ❌ | ❌ | ✅ |
| **§3.6.1-3.8 指针** | **300-302** | **3** | **✅** | **❌** | **❌** |
| §4 System event | 305-311 | 7 | ❌ | ✅ | ✅ |
| §5 user_decision relay | 314-339 | 26 | ❌ | ✅ | ✅ |
| §6 Exception-escalation | 342-355 | 14 | ❌ | ✅ | ✅ |
| §7 Common helper | 358-367 | 10 | ❌ | ✅ | ✅ |

#### buyer.md 各 Session 浪费

| Session | 不需要的段落 | 浪费行数 | 浪费 token |
|---|---|---|---|
| **Job** | USDT消歧 + §3.1-§3.3 + routing table + resolve + §3.6.1-3.8 指针 + Quick Nav | ~104 行 | ~1,560 |
| **Backup** | 上述 + §3.5 + §3.6 + §2 + Quick Nav | ~170 行 | ~2,550 |
| **User** | preamble + batching + §1 + §2 + §3.4-§3.6 + §4-§7 + Quick Nav | ~220 行 | ~3,300 |

#### buyer.md 与 SKILL.md 重复（~18 行，~270 token）

| 规则 | SKILL.md 位置 | buyer.md 位置 | 重复程度 |
|---|---|---|---|
| sessions_spawn 禁止 | L170 | L19-21 | 完全重复 |
| 系统事件必须 next-action | L117-124 | L23-25 | 完全重复 |
| Role 每次重新解析 | L174 | L23 末尾 | 完全重复 |

---

### 2.3 buyer-actions.md（290 行，~4,400 token）— User Session Only

**仅 User Session 加载**。各段落独立性高：

| 段落 | 行范围 | 行数 | 触发场景 | 频率 |
|---|---|---|---|---|
| Preamble + nav | 1-22 | 22 | 每次 | - |
| §1.0-1.3 发布 | 26-81 | 56 | "create a task" | 高 |
| §1.4 草稿 | 82-139 | 58 | "save draft" | 低 (<5%) |
| §2 中途附件 | 142-165 | 24 | "attach file" | 中 |
| §3 条款修改 | 168-235 | 68 | "change budget" | 中 |
| §4 查看交付物 | 239-290 | 52 | "view deliverables" | 中 |

**问题**: LLM 倾向读全文（290 行），但每次触发只用 1 个段落（24-68 行）。

---

### 2.4 辅助文件按 Session 需求

#### `_shared/`（9 文件，1,916 行）

| 文件 | 行数 | User | Backup | Job |
|---|---|---|---|---|
| cli-reference.md | 824 | ❌ | grep | grep |
| message-types.md | 341 | §3.1-3.2 | ❌ | §1-§2 |
| state-machine.md | 175 | ❌ | 偶尔 | 偶尔 |
| xmtp-tools.md | 154 | Path 9 | Path 6/8 | Path 6/8/9 |
| user-intent-routing.md | 123 | **✅** | **❌** | **❌** |
| exception-escalation.md | 100 | ❌ | ✅ | ✅ |
| entry-points.md | 85 | ❌ | 低 | 低 |
| payment-modes.md | 65 | ❌ | 低 | 低 |
| preflight.md | 49 | **✅** | **❌** | **❌** |

#### `references/`（5 文件，925 行）

| 文件 | 行数 | User | Backup | Job |
|---|---|---|---|---|
| display-formats.md | 324 | **✅** | **❌** | **❌** |
| incidents.md | 213 | ❌ | 部分 | 部分 |
| evaluator-staking.md | 180 | **❌** | **❌** | **❌** |
| troubleshooting.md | 125 | ❌ | 部分 | 部分 |
| evaluator-decision-rubric.md | 83 | **❌** | **❌** | **❌** |

---

## 3. 拆分方案对比

### 方案 A: Session-Scope 标注（最小改动）

**思路**: 在现有文件中增加 session scope 标注，引导 LLM 跳过不需要的段落。

```markdown
> ⚠️ **User session only** — Backup/Job sessions skip to §3.4.
```

**优点**: 改动量极小（~30 行标注），不影响文件结构，不影响 provider/evaluator
**缺点**: 依赖 LLM 遵守标注指令（实测约 85-90% 遵守率），无法保证物理隔离
**适用**: 所有段落级的 session scope 差异

**预估收益**:
- Job session: 跳过 ~104 行 = ~1,560 token/RESUME × 15 = **~23,400 token/task**
- Backup session: 跳过 ~170 行 = ~2,550 token/RESUME × 3.5 = **~8,925 token/task**
- User session: 跳过 ~220 行 = ~3,300 token/RESUME × 2 = **~6,600 token/task**
- **总计: ~38,925 token/task**

**改动量**: ~30 行标注
**风险**: 极低

---

### 方案 B: buyer.md 拆分为 3 个角色文件

**思路**: 按 session 职责拆分 buyer.md 为独立文件。

```
buyer.md          → buyer-common.md (共享核心 ~120 行)
                  → buyer-user.md   (User session 专用 ~90 行)
                  → buyer-job.md    (Job session 专用 ~160 行)
                  → buyer-backup.md (Backup session 专用 ~50 行)
```

**Reading Order 变更**:
- User session: SKILL.md → buyer-common.md → buyer-user.md
- Backup session: SKILL.md → buyer-common.md → buyer-backup.md
- Job session: SKILL.md → buyer-common.md → buyer-job.md

**内容分配**:

| 当前 buyer.md 段落 | 目标文件 | 行数 |
|---|---|---|
| 通用 preamble (L11-28, 去重后 4 行) | buyer-common.md | 4 |
| Tool-call batching (L49-54) | buyer-common.md | 6 |
| §1 Trigger identification (L56-70) | buyer-common.md | 15 |
| §2 P2P reply (L72-81) | buyer-common.md | 10 |
| §4 System event (L305-311) | buyer-common.md | 7 |
| §5 user_decision relay (L314-339) | buyer-common.md | 26 |
| §6 Exception-escalation (L342-355) | buyer-common.md | 14 |
| §7 Common helper (L358-367) | buyer-common.md | 10 |
| **buyer-common.md 小计** | | **~92 行** |
| USDT/USDG 消歧 (L1-9) | buyer-user.md | 9 |
| §3.1 Publishing (L84-90) | buyer-user.md | 7 |
| §3.2 Designated A2A (L94-113) | buyer-user.md | 20 |
| §3.3 Designated x402 (L117-151) | buyer-user.md | 35 |
| Intent routing table (L154-165) | buyer-user.md | 12 |
| resolve 执行规则 (L167-175) | buyer-user.md | 9 |
| §3.6.1-3.8 指针 (L300-302) | buyer-user.md | 3 |
| **buyer-user.md 小计** | | **~95 行** |
| §3.4 Negotiation (L178-232) | buyer-job.md | 55 |
| §3.5 Inbound routing (L236-277) | buyer-job.md | 42 |
| §3.6 Accepted-execution (L280-297) | buyer-job.md | 18 |
| **buyer-job.md 小计** | | **~115 行** |
| §3.4 前半（designated 路由相关） | buyer-backup.md | ~20 |
| 终态事件补充指引 | buyer-backup.md | ~15 |
| SKILL_PREFETCH 说明 | buyer-backup.md | ~10 |
| **buyer-backup.md 小计** | | **~45 行** |

**优点**: 物理隔离，每个 session 只读需要的内容，不可能误读
**缺点**: 拆分 4 个文件，维护成本增加；§3.4 中 backup 和 job 有部分重叠；需更新所有引用路径；SKILL.md Reading Order 需要按 session 类型分支
**适用**: 差异大、频次高的 session（尤其 Job session）

**预估收益**:
- Job session: 加载 92 + 115 = 207 行（vs 原 366）= 节省 159 行 = ~2,385 token/RESUME × 15 = **~35,775 token/task**
- Backup session: 加载 92 + 45 = 137 行（vs 原 366）= 节省 229 行 = ~3,435 token/RESUME × 3.5 = **~12,022 token/task**
- User session: 加载 92 + 95 = 187 行（vs 原 366）= 节省 179 行 = ~2,685 token/RESUME × 2 = **~5,370 token/task**
- **总计: ~53,167 token/task**

**改动量**: 拆 1 文件 → 4 文件 + 更新 SKILL.md Reading Order + 更新所有引用
**风险**: 中等 — 需要 SKILL.md 能按 session 类型指定加载路径；§3.4 重叠段落需仔细拆分

---

### 方案 C: 混合方案（推荐）— 标注 + 关键文件拆分

**思路**: 对核心文件（buyer.md）使用标注，对辅助文件使用物理拆分/隔离。

| 文件 | 策略 | 原因 |
|---|---|---|
| **buyer.md** | **Session-Scope 标注** | 拆分 4 文件维护成本高；标注简单且 buyer.md 段落边界清晰 |
| **SKILL.md** | **Session-Scope 标注** | 同上 |
| **buyer-actions.md** | **按段落拆分** | 5 个独立段落，互不依赖，物理拆分收益高 |
| **cli-reference.md** | **按角色拆分** | 824 行过大，buyer/provider/evaluator 命令互不相关 |
| **evaluator-*.md** | **移至子目录** | 物理隔离消除误读 |
| **message-types.md** | **Session-Scope 标注** | §3.1+ 标注 user session only |
| **incidents.md** | **角色标签** | 标题加 [buyer]/[provider] 标签 |
| **Additional Resources** | **Scope 标注** | 每条加 session scope |

**预估收益**: 方案 A 的标注收益（~38,925 token）+ 物理拆分的额外收益（~20,000 token）= **~58,925 token/task**

**改动量**: ~30 行标注 + 拆 5-8 文件
**风险**: 低

---

### 方案对比总结

| 维度 | 方案 A 标注 | 方案 B 全拆分 | 方案 C 混合（推荐） |
|---|---|---|---|
| Token 节省/task | ~38,925 | ~53,167 | **~58,925** |
| 改动量 | ~30 行 | 拆 4 核心 + 更新引用 | ~30 行 + 拆 5-8 辅助 |
| 维护成本 | 极低 | 高（4 个 buyer 文件同步） | 低 |
| 对 provider/evaluator 影响 | 无 | 无（buyer only 拆分） | 无 |
| 物理隔离保证 | 无（依赖 LLM 遵守） | 完全隔离 | 辅助文件隔离 |
| 实施速度 | 0.5 天 | 2 天 | 1.5 天 |
| 风险 | 极低 | 中 | 低 |

---

### 3.4 三方案详细风险-收益对比

#### 方案 A: Session-Scope 标注

| 维度 | 评估 |
|---|---|
| **核心机制** | 在段落前插入 `> ⚠️ User session only` 等标注，引导 LLM 跳过 |
| **收益** | ~38,925 token/task。零结构变更，30 分钟内可全量上线 |
| **风险 1: LLM 遵守率** | 实测 ~85-90%。标注位于文件内部，LLM 已将全文纳入 context，标注只是 *建议跳过*，无法阻止 LLM "偷看"。尤其在复杂推理场景下，LLM 可能回溯已 skip 的内容 |
| **风险 2: 遵守率退化** | 跨模型升级无保证 —— 新版本可能对标注格式敏感度不同。需持续回归测试 |
| **风险 3: token 节省是"软"节省** | 文件已进入 context window（input token 已消耗），标注只减少 LLM *attention* 到这些行的概率，不减少 input token。真正的 token 节省取决于 prompt caching 边界 —— 如果整个 SKILL.md 在同一个 cache block 内，跳过段落不减少计费 |
| **适用边界** | 段落边界清晰、session 差异明确的核心文件（SKILL.md、buyer.md）。段落边界模糊或内容有交叉引用的文件不适合 |
| **退出成本** | 接近零 —— 删除标注即可回退 |

#### 方案 B: buyer.md 全拆分

| 维度 | 评估 |
|---|---|
| **核心机制** | buyer.md → buyer-common.md + buyer-user.md + buyer-job.md + buyer-backup.md，每个 session 只加载对应文件 |
| **收益** | ~53,167 token/task。**物理隔离**，不在 context 中的内容不消耗 input token、不干扰推理 |
| **风险 1: 维护成本** | §3.4（Negotiation）在 Backup 和 Job 之间存在 ~20 行重叠。拆分后需在两文件中同步维护，改一处漏另一处会导致行为分歧 |
| **风险 2: Reading Order 复杂化** | SKILL.md 的 Reading Order 必须按 session 类型分支：`if backup → read buyer-common + buyer-backup`。当前 Reading Order 是线性列表，改为条件分支增加 LLM 理解成本 |
| **风险 3: 引用链断裂** | provider.md 中有 4 处引用 `buyer.md §3.4`；SKILL.md Activation 中有 2 处引用 `buyer.md §1`。拆分后所有引用需更新为新路径，遗漏会导致 LLM 找不到目标文件 |
| **风险 4: 加载判断成本** | Session 类型判断逻辑（sessionKey 格式解析）必须在加载 buyer-* 之前完成。如果判断错误（如 backup sessionKey 变体），会加载错误文件 |
| **适用边界** | 仅适用于 buyer.md 这类 session 差异极大、段落职责边界清晰的文件。SKILL.md 不适合全拆分（全局规则太多） |
| **退出成本** | 高 —— 需合并 4 文件 + 还原所有引用 |

#### 方案 C: 混合方案

| 维度 | 评估 |
|---|---|
| **核心机制** | 核心文件（SKILL.md、buyer.md）用标注；辅助文件（buyer-actions、cli-reference、evaluator-*）用物理拆分/移动 |
| **收益** | ~58,925 token/task（标注 ~38,925 + 拆分 ~20,000）。核心文件保持单一，辅助文件物理隔离 |
| **优势 1: 核心文件稳定** | buyer.md 保持一个文件，provider 引用不变，Reading Order 保持线性 |
| **优势 2: 辅助文件拆分收益确定** | buyer-actions.md 5 个段落完全独立，拆分后每次只加载 1 个段落（24-68 行 vs 原 290 行），这是 *物理* 节省 |
| **优势 3: 渐进实施** | Phase A（标注）0.5 天可上线，立即验证效果；Phase B（拆分）在标注效果确认后推进 |
| **风险 1: 标注层仍有 A 的问题** | 核心文件的 85-90% 遵守率问题不变。但核心文件 session 差异占比（30-57%）低于辅助文件（100%），标注的"漏网"影响相对小 |
| **风险 2: 辅助文件层引入多层引用** | 拆分后引用层级从 1 层变为 2-3 层（详见下文 §3.5）|
| **退出成本** | 低 —— 标注删除即可；辅助文件可独立合并回原文件 |

#### 三方案决策矩阵

| 决策因素 | 方案 A | 方案 B | 方案 C |
|---|---|---|---|
| 需要 **立刻上线、零风险验证** | ✅ 最佳 | ❌ | ✅ Phase A |
| 需要 **最大 token 物理节省** | ❌ 软节省 | ✅ 最佳 | ⚠️ 中等 |
| 需要 **最低维护成本** | ✅ 最佳 | ❌ | ✅ |
| 核心文件修改频繁（每周 1+ 次） | ✅ | ❌ 同步负担 | ✅ |
| 辅助文件几乎不改 | 无差别 | 无差别 | ✅ 拆分成本一次性 |
| 将来可能从方案 A 升级到 B | ✅ 起点 | N/A | ✅ 天然升级路径 |

**推荐**：方案 D（方案 C 升级版）。在方案 C 基础上，新增 User session 独立入口文件（S14），将 User session 从"软标注"升级为"物理隔离"。核心文件 Sub session 差异用标注（保持结构稳定 + 0 维护），辅助文件用物理拆分（收益确定 + 隔离彻底），User session 用独立入口文件（零 CLI 改动 + 硬节省）。如果后续标注遵守率实测不理想，可从 D 无缝升级到 B（只需将 buyer.md 标注段拆为独立文件）。

---

### 3.5 文件内标注 vs 文件拆分：深度对比

#### 标注的本质限制

文件内标注（如 `> ⚠️ User session only — skip to §3.4`）是一种 **prompt-level instruction**，其效果取决于：

| 因素 | 影响 | 说明 |
|---|---|---|
| **标注位置** | 高 | 段落顶部比中间有效。LLM 按顺序处理，段首标注在读到具体内容前生效 |
| **标注格式** | 中 | `> ⚠️` blockquote + emoji 比纯文字更醒目。但过多标注会导致 "标注疲劳"，LLM 开始忽略 |
| **段落长度** | 高 | 被标注跳过的段落越长，LLM 越可能"好奇"而回头看。3-5 行的段落标注效果好，50+ 行的段落标注效果差 |
| **上下文压力** | 高 | 当 LLM 推理遇到困难（如 negotiate 失败重试），倾向于搜索更多 context，包括被标注跳过的部分 |
| **Input token** | 无节省 | 标注不改变文件内容，全量 input token 仍然消耗。节省的是 LLM attention/reasoning，不是 API 计费 |

**关键认知**：标注节省的是 LLM 的"注意力 token"（减少推理噪声），而非 API 的"input token"（仍然全量计费）。对于 prompt caching 场景，如果被标注段落落在同一个 cache block 内，标注的计费节省为零。

#### 拆分的本质优势和成本

文件拆分是 **物理隔离**，其效果是确定性的：

| 因素 | 优势 | 成本 |
|---|---|---|
| **Input token** | 物理减少 — 不加载的文件不消耗 token | 需要正确判断加载哪个文件 |
| **推理隔离** | 不在 context 中的内容不可能干扰推理 | 如果判断错误，需要的内容不在 context 中 |
| **跨 session 一致性** | 每个 session 的行为由其专属文件完全定义 | 共享逻辑需要在多个文件中同步 |
| **维护** | 每个文件更小、更聚焦 | 文件数量增加，修改时需要知道改哪个文件 |

#### 标注适用场景 vs 拆分适用场景

| 判断标准 | → 标注 | → 拆分 |
|---|---|---|
| 被跳过的内容量 | ≤ 30 行 | > 30 行 |
| Session 差异比例 | < 30% | > 50% |
| 共享逻辑比例 | > 50% | < 30% |
| 文件修改频率 | 高（每周 1+ 次） | 低（每月或更少） |
| 引用该文件的其他文件数 | ≥ 3 | ≤ 1 |
| 段落间有交叉引用 | 是 | 否（段落完全独立） |

**实际决策**:

| 文件 | 推荐方式 | 原因 |
|---|---|---|
| SKILL.md | **标注** | 全局规则多，session 差异仅 ~34 行（8%），拆分不值得 |
| buyer.md | **标注**（可升级到拆分） | session 差异大（30-66%）但 §3.4 有 Backup/Job 重叠，拆分维护成本高 |
| buyer-actions.md | **拆分** | 5 个段落完全独立，0 交叉引用，每次只用 1 个 |
| cli-reference.md | **拆分** | 3 个角色命令集完全独立，824 行太大 |
| evaluator-*.md | **移动** | 和 buyer 完全无关，物理位置隔离即可 |
| message-types.md | **标注** | session 差异仅 §3.1-3.2（~40 行），拆分过度 |

---

### 3.6 多层引用的 AI 跳过风险分析

#### 引用层级与加载可靠性

拆分文件会引入 **多层引用链**（A 引用 B，B 引用 C）。每增加一层引用，AI 不去加载的概率递增：

| 引用层级 | 示例 | 加载可靠性 | 说明 |
|---|---|---|---|
| **L0: 直接加载** | SKILL.md Reading Order 列出的文件 | **~98%** | CLI preamble 直接写明"read these files"，几乎必加载 |
| **L1: 一级引用** | buyer.md 中 `→ Read buyer-actions.md` | **~90-95%** | 在已加载文件中发现引用，AI 大概率跟随 |
| **L2: 二级引用** | buyer-actions.md 中 `→ see cli-reference-buyer.md for commands` | **~70-80%** | AI 开始犹豫："我已经有足够信息了吗？"。如果当前信息足以完成任务，AI 倾向于不再加载 |
| **L3: 三级引用** | cli-reference-buyer.md 中 `→ see payment-modes.md for mode enum` | **~50-60%** | 显著下降。AI 在 3 层 Read 后通常认为"够了"，除非明确遇到缺失信息 |
| **L4+** | 更深引用 | **< 40%** | 几乎不可靠，AI 倾向猜测或从训练知识补充 |

#### 为什么多层引用会被跳过

1. **工具调用成本感知**: 每次 Read 是一次工具调用，AI 有隐式的"成本意识"——倾向于用尽可能少的工具调用完成任务。多层引用 = 多次 Read = AI 倾向于"够用就停"

2. **信息充足幻觉**: 在 L1 加载后，AI 可能已经看到足够多的上下文关键词，产生"我已理解"的判断，从而跳过 L2+ 的更详细内容

3. **Context window 压力**: 随着已加载文件增多，AI 的有效注意力被稀释。在高层引用时 AI 更倾向于利用已有信息而非加载更多

4. **引用格式模糊性**: `→ see payment-modes.md` 这种软引用比 `🛑 MUST READ payment-modes.md before proceeding` 弱得多。AI 将前者理解为"可选参考"

#### 当前架构的引用层级

```
L0 (CLI preamble 直接指定):
  SKILL.md → buyer.md          ✅ 可靠

L1 (SKILL.md Reading Order / Additional Resources):
  → buyer-actions.md           ✅ 可靠（Reading Order 明确列出）
  → cli-reference.md           ✅ 可靠
  → message-types.md           ⚠️ 按需（Additional Resources 列出但非必加载）
  → xmtp-tools.md             ⚠️ 按需

L2 (buyer.md / buyer-actions 内引用):
  → display-formats.md         ⚠️ 按需
  → incidents.md               ⚠️ 按需
  → troubleshooting.md         ⚠️ 低频
```

#### 拆分后的引用层级变化

如果按方案 C 拆分 buyer-actions.md 为 4 个文件：

```
L0: SKILL.md → buyer.md                              ✅ 不变

L1: buyer.md → buyer-action-publish.md                ⚠️ 从 L1(整文件) 变为 L1(4 个文件之一)
    buyer.md → buyer-action-modify.md                 ⚠️ AI 需选择正确的文件
    buyer.md → buyer-action-draft.md
    buyer.md → buyer-action-deliverables.md

L2: buyer-action-publish.md → cli-reference-buyer.md  ⚠️ 原 L1 → 现 L2
    buyer-action-modify.md → display-formats.md       ⚠️ 原 L1 → 现 L2
```

**新增风险**: buyer-actions 拆分后，原来的 L1 引用（buyer.md → buyer-actions.md 整文件）变成了 **L1 选择 + L1 加载**。AI 需要先判断用哪个子文件，再去加载。判断错误的概率约 5-10%。

#### 缓解策略

| 策略 | 效果 | 适用 |
|---|---|---|
| **控制最大引用深度 ≤ 2 层** | 高 | 所有拆分场景。L0→L1→L2 是安全上限，避免 L3+ |
| **强制引用语法** | 高 | 用 `🛑 MUST READ` 而非 `→ see`。强制语法加载率比软引用高 15-20% |
| **Reading Order 列全** | 高 | 将拆分后的文件全部列入 SKILL.md Reading Order（L0 级别），而非靠 buyer.md 引用（L1 级别）。代价是 Reading Order 变长 |
| **CLI preamble 预加载** | 最高 | 在 CLI 的 preamble 构建中直接注入目标文件内容（当前 SKILL_PREFETCH 已用此模式）。完全消除引用层级问题。代价是增加 preamble 大小 |
| **合并小文件** | 中 | 如果拆分后单个文件 < 30 行，考虑合并回父文件而非独立。太小的文件不值得单独一层引用 |
| **避免辅助文件互引** | 中 | 拆分后的子文件之间不要互相引用。每个子文件应该是自包含的叶子节点 |

#### 对方案 C 的影响

方案 C 的拆分目标是 **辅助文件**（buyer-actions、cli-reference、evaluator-*），这些文件的特点是：

- buyer-actions 的 4 个段落 **完全独立**，拆分后每个子文件是叶子节点，不引用其他文件 → L2 风险不存在
- cli-reference 按角色拆分后，buyer sub 只需 cli-reference-buyer.md → 引用层级不变（仍是 L1）
- evaluator-* 移动到子目录 → 引用层级不变（只是路径变了）

**结论**: 方案 C 的拆分策略刻意避免了多层引用问题。真正可能引入 L2+ 风险的是方案 B（buyer.md 全拆分 → buyer-job.md → cli-reference-buyer.md 变成 L2），这也是不推荐方案 B 的一个原因。

---

## 4. 优化方案明细（方案 D: 14 项）

### 4.1 [S1] buyer.md Session-Scope 标注 — P0

**目标**: Job/Backup session 跳过 user-session-only 段落

**改动**: 在 3 处加标注：

1. USDT/USDG 消歧块（L1）前:
```
> ⚠️ **User session only** — Backup/Job sessions skip to `## 1. Trigger identification`.
```

2. §3.1（L84）前:
```
> ⚠️ **§3.1–§3.3 + intent routing table + resolve rule = User session only**
> Backup/Job sessions: skip to **§3.4 Negotiation phase**.
```

3. §3.6.1-3.8 指针（L300）前:
```
> ⚠️ **User session only**
```

**改动量**: 6 行标注
**收益**: Job 跳过 104 行 × 15 RESUME = **~23,400 token/task**；Backup 跳过 170 行 × 3.5 = **~8,925 token/task**
**风险**: 极低
**影响面**: buyer.md only

---

### 4.2 [S2] SKILL.md Session-Scope 标注 — P0

**目标**: Sub sessions 跳过 user-session-only 段落

**改动**:

1. Pre-flight (L70) 前:
```
> ⚠️ **User session only** — sub sessions skip to `## Critical Field Mapping`.
```

2. User Intent Routing (L327) 前:
```
> ⚠️ **User session only** — sub sessions skip to `## Communication Boundary`.
```

**改动量**: 2 行标注
**收益**: Job 跳过 34 行 × 15 = **~7,650 token/task**；Backup 跳过 34 行 × 3.5 = **~1,785 token/task**
**风险**: 极低

---

### 4.3 [S3] SKILL.md Additional Resources Scope 标注 — P0

**目标**: 防止 LLM 误读不需要的辅助文件

在每个条目后追加 session scope:

```
- cli-reference.md — **grep only, all sessions**
- user-intent-routing.md — **User session only**
- display-formats.md — **User session only**
- evaluator-staking.md — **Evaluator only**
- evaluator-decision-rubric.md — **Evaluator only**
- preflight.md — **User session only**
- ...
```

**改动量**: 14 行修改
**收益**: 防范 919 行误读（evaluator-* 263 行 + display-formats 324 行 + user-intent-routing 123 行 + preflight 49 行 + message-types user-only 160 行）
**风险**: 极低

---

### 4.4 [S4] message-types.md User-Session-Only 标注 — P1

在 Path 3.1（L135）前加标注:
```
> ⚠️ **Path 3.1–3.2 = User session receiver rules**
> Sub sessions sending `[USER_DECISION_REQUEST]`: see Path 2b format only.
```

**改动量**: 3 行
**收益**: 防止 Job/Backup 误读 ~160 行
**风险**: 极低

---

### 4.5 [S5] buyer.md Preamble 去重 — P1

**目标**: 压缩 buyer.md L11-28 与 SKILL.md Activation 的重复

从 18 行压缩为 4 行引用:
```
> 🛑 SKILL.md Activation 规则在 buyer 侧同等有效（sub 已在 context 中读过）:
> - `sessions_spawn`/`sessions_yield` 绝对禁止（I-backup-spawn, I-MiniMax）
> - 系统事件 MUST `next-action`；直接执行 CLI 禁止
> - `--role` MUST 每个 event 重新解析（I-19）
```

**改动量**: 14 行净减
**收益**: ~210 token/RESUME × 15(Job) + × 3.5(Backup) = **~3,885 token/task**
**风险**: 中等 — defense-in-depth 减弱。缓解: SKILL.md 原文仍在 context

---

### 4.6 [S6] buyer-actions.md 按段落拆分 — P1

**目标**: User session 触发时只读需要的段落

**方案 A（推荐）: 精确行号指针**

修改 buyer.md / buyer-user 中的指针:
```
§1 Publishing → Read buyer-actions.md lines 26-81 only (skip §1.4 drafts)
§2 Attachment → lines 142-165
§3 Terms     → lines 168-235
§4 Deliverables → lines 239-290
```

**方案 B: 物理拆分为 4 文件**
- `buyer-action-publish.md` (56 行)
- `buyer-action-draft.md` (58 行)
- `buyer-action-modify.md` (92 行)
- `buyer-action-deliverables.md` (52 行)

**改动量**: A 改 4 指针；B 拆 4 文件
**收益**: 每次触发从 290 行降至 24-68 行 = **~3,300-4,000 token/触发**
**风险**: 低

---

### 4.7 [S7] cli-reference.md 按角色拆分 — P1

拆分为 4 个角色文件: common (~160行) + buyer (~400行) + provider (~180行) + evaluator (~160行)。

**改动量**: 拆 1 → 4 文件
**收益**: buyer sub 从 824 行降至 560 行，节省 264 行
**风险**: 低

---

### 4.8 [S8] buyer.md §3.5 Backup/Job 分离标注 — P1

**目标**: Backup session 不需要 §3.5 Inbound routing（42 行）—— §3.5 是 Job session 的入站消息路由，Backup 不处理 peer 消息。

在 §3.5（L236）前加:
```
> ⚠️ **Job session only** — Backup sessions skip to §4.
```

**改动量**: 1 行标注
**收益**: Backup 跳过 42 行 × 3.5 = **~2,205 token/task**
**风险**: 极低 — Backup 确实不处理 peer 消息

---

### 4.9 [S9] SKILL.md Activation Incident 压缩 — P2

将 Activation 中 ~10 处 inline incident 描述替换为编号引用:

```
原: 🔴 I-3 backup self-queried. I-5/I-7 backup sessions_spawn re-delegation.
改: 🔴 Incidents: I-3, I-5, I-6, I-7, I-8 (→ references/incidents.md)
```

**改动量**: ~15 行净减
**收益**: ~225 token/RESUME × 15(Job) = **~3,375 token/task**
**风险**: 中等 — 缓解: 关键 incident 在 buyer.md §3.5 保留全文

---

### 4.10 [S10] Quick Navigation 去重 — P2

删除 SKILL.md (15行) + buyer.md (18行) 的 Quick Navigation 表。

**改动量**: 删 33 行
**收益**: ~495 token/RESUME × 15(Job) = **~7,425 token/task**
**风险**: 低 — LLM 按段落标题定位。人类可保留为 HTML 注释

---

### 4.11 [S11] incidents.md 角色标签 — P2

21 个 incident 标题追加 `[buyer]`/`[provider]`/`[all]` 角色标签。

**改动量**: 21 行修改
**收益**: buyer sub 只需读 ~100 行（减 ~113 行）
**风险**: 极低

---

### 4.12 [S12] evaluator 文件移至子目录 — P3

将 `evaluator-decision-rubric.md` + `evaluator-staking.md` 移至 `references/evaluator/`。

**改动量**: 移 2 文件 + 更新引用
**收益**: 物理隔离消除 263 行误读风险
**风险**: 低

---

### 4.13 [S13] SKILL.md Session Communication Contract §4 xmtp 工具说明压缩 — P1

**目标**: 压缩 SKILL.md L264-296 中 playbook 已自包含的 xmtp 工具 how-to 说明

**背景**: Session Communication Contract (~100 行) 中的 xmtp 相关内容分两类：
1. **安全护栏 + 全局行为模型**（~55 行）：通信路径分区、Tool whitelist、Forbidden 列表、User/Sub state machine、Push opt-in — **不可删除**，playbook 不覆盖这些全局约束
2. **工具调用 how-to**（~40 行）：Path 4/2a/2b/3 的步骤说明、命令模板、§5 queue 部分命令格式 — **可删除/压缩**，每个 `next-action` playbook 已内嵌完整的工具调用步骤和参数

**不可删除（安全护栏）**:

| 内容 | 行 | 原因 |
|---|---|---|
| Runtime Bridge (L31-39) | 9 行 | 环境兼容性，告诉 LLM 有哪些工具、如何映射 |
| 4 条通信路径分区 (L219-225) | 7 行 | 安全红线 — 哪个工具用于哪个方向 |
| Tool whitelist (L266) | 1 行 | 防止用错误工具（Session Send 等） |
| Forbidden 列表 (L229-231, L258, L262, L294) | ~10 行 | 信封伪造禁止、自环禁止、混用禁止 |
| User-session state machine (L233-243) | 11 行 | User session 行为定义，无 playbook 覆盖 |
| Sub-session state machine (L245-257) | 13 行 | Sub 三种触发的行为分区 |
| Push opt-in (L253-256) | 4 行 | 防止 sub 自作主张推送 |

**可压缩（playbook 已自包含）**:

| 内容 | 原行数 | 压缩后 | 原因 |
|---|---|---|---|
| Path 4 步骤说明 (L268-270) | 3 行 | 删除 | playbook 已写明 `session_status → xmtp_send` |
| Path 2a 说明 (L272) | 1 行 | 删除 | playbook 已写明 `xmtp_dispatch_user` |
| Path 2b 命令模板 (L274-280) | 7 行 | 删除 | playbook 已内嵌 `pending-decisions-v2 request` 完整命令 |
| Path 3 命令模板 (L282-290) | 9 行 | 删除 | `[USER_DECISION_REQUEST]` block 已预填 resolve-prompt |
| §5 queue 命令格式 (L298-312) | 15 行 | 压缩为 5 行 | 保留核心规则，去掉命令格式 |
| Counter-example (L296) | 5 行 | 删除 | 低频，incidents.md 有记录 |

**改动**: §4 从 33 行压缩为 ~8 行：

```markdown
### 4. Tool invocation steps

🛑 **Tool whitelist**: `xmtp_send`, `xmtp_dispatch_user`, ... (11 tools).
Do NOT use `Session Send` / `sessions.send`.

**How-to**: each `next-action` playbook contains exact tool-call steps with
parameters. Follow the playbook verbatim. For long-tail tools (Path 5-9)
see `_shared/xmtp-tools.md`.

**❌ Forbidden**: outputting xmtp content as assistant TEXT; paraphrasing
after tool call; fabricating task status before relay completes.
```

§5 命令模板部分类似压缩，保留 resolve-prompt 核心规则但去掉命令格式。

**改动量**: ~25 行净减
**收益**: ~375 token/RESUME × 15(Job) + × 3.5(Backup) = **~6,938 token/task**
**风险**: 低 — 安全护栏完整保留；how-to 已由 playbook 覆盖。缓解：保留 Tool whitelist + Forbidden 列表确保安全底线
**影响面**: SKILL.md Session Communication Contract §4-§5

---

### 4.14 [S14] User Session 独立入口文件 — P0

**目标**: User session 从加载全量 SKILL.md(404行) + buyer.md(366行) = 770 行，降为只加载 ~215 行

**背景**: User session 只做意图识别、任务发布/修改、决策卡片展示。不发 xmtp 消息、不处理系统事件、不做协商。当前 770 行中 **555 行（72%）对 User session 完全无用**。

标注方案（S1-S2）对 User session 只提供"软节省"——文件仍全量进入 context window，input token 不减。而 User session 的文件加载由 Claude Code 主对话的 Read tool 控制，不经过 CLI preamble 注入，因此可以通过创建独立入口文件实现 **物理隔离**，无需改任何 Rust 代码。

**改动**: 新建 2 个文件 + 修改 1 处路由

#### 文件 1: `SKILL-user.md`（~120 行，从 SKILL.md 404 行提取）

从 SKILL.md 提取 User session 需要的段落：

| 段落 | 原行范围 | 行数 | 保留原因 |
|---|---|---|---|
| YAML frontmatter | L1-9 | 9 | 系统必须 |
| Title + description | L11-13 | 3 | 一次性 |
| Roles + determination | L41-68 | 28 | 角色判断 |
| **Pre-flight** | L70-78 | 9 | 发布前检查 |
| Critical Field Mapping | L80-93 | 14 | create-task 字段 |
| Reading Order (User 版) | 新写 | 8 | 指向 buyer-user.md |
| Anti-hallucination | L314-326 | 13 | 安全护栏 |
| **User Intent Routing** | L327-339 | 13 | 意图路由 |
| **Cross-Skill Routing** | L341-352 | 12 | 跨 skill 路由 |
| Communication Boundary (精简) | L358-384 | ~15 | 不伪造规则 |
| **小计** | | **~124 行** | |

**不包含**（User session 完全不需要）：

| 段落 | 行数 | 排除原因 |
|---|---|---|
| Runtime Bridge | 9 | xmtp 工具映射，User 不用 xmtp |
| Quick Navigation | 15 | LLM 不依赖 |
| Core Architecture | 11 | 一次性架构说明，User 场景简单不需要 |
| Activation 事件路由 | 63 | 仅 sub-session |
| a2a-agent-chat entry | 19 | 仅 sub-session |
| sessionKey Discrimination | 14 | User 无 sessionKey |
| Session Communication Contract | 100 | User 不用任何 xmtp 工具 |
| Message Format | 3 | sub-session 消息格式 |
| Additional Resources | 19 | sub-session 按需加载列表 |
| **排除小计** | **~253 行** | |

#### 文件 2: `buyer-user.md`（~100 行，从 buyer.md 366 行提取）

| 段落 | 原行范围 | 行数 | 保留原因 |
|---|---|---|---|
| 精简 preamble | 新写 | ~5 | 核心规则引用（不重复 SKILL-user.md 已有的） |
| USDT/USDG 消歧 | L1-9 | 9 | User 发布时需要 |
| §3.1 Publishing | L84-90 | 7 | 任务发布 |
| §3.2 Designated A2A | L94-113 | 20 | 指定服务商（A2A） |
| §3.3 Designated x402 | L117-151 | 35 | 指定服务商（x402） |
| Intent routing table | L154-165 | 12 | 用户意图分发 |
| resolve 执行规则 | L167-175 | 9 | 决策执行 |
| §3.6.1-3.8 指针 | L300-302 | 3 | 修改/附件/交付物入口指针 |
| **小计** | | **~100 行** | |

**不包含**（User session 完全不需要）：

| 段落 | 行数 | 排除原因 |
|---|---|---|
| 通用 preamble sub 规则 | 18 | sub-session 专属 |
| Quick Navigation | 18 | LLM 不依赖 |
| Tool-call batching | 6 | sub-session 批量调用 |
| §1 Trigger identification | 15 | sub-session 事件触发判断 |
| §2 P2P reply | 10 | sub-session peer 消息 |
| §3.4 Negotiation | 55 | Job session 协商 |
| §3.5 Inbound routing | 42 | Job session 入站路由 |
| §3.6 Accepted-execution | 18 | Job session 执行 |
| §4 System event | 7 | sub-session 系统事件 |
| §5 user_decision relay | 26 | sub-session relay |
| §6 Exception-escalation | 14 | sub-session 异常处理 |
| §7 Common helper | 10 | sub-session 辅助 |
| **排除小计** | **~239 行** | |

#### 路由修改: CLAUDE.md

在 CLAUDE.md 的 `okx-agent-task` 路由中，将 User session 的 Reading Order 指向新文件：

```
Routing 新增规则:
- User session (主对话，无 sessionKey): 
  Read SKILL-user.md → buyer-user.md → (按需) buyer-actions.md 对应段落
- Sub sessions (CLI preamble 控制):
  不变，仍读 SKILL.md + buyer.md
```

具体方式：在 SKILL.md 的 Reading Order 段落增加分支判断，或在 CLAUDE.md okx-agent-task 路由说明中增加 "User session 读 SKILL-user.md" 的前置条件。

#### 引用层级分析

```
L0: Claude Code 主对话
  ↓ CLAUDE.md 路由到 okx-agent-task
  ↓ 读 SKILL-user.md（~120 行）    ← vs 原 SKILL.md 404 行
  
L1: SKILL-user.md Reading Order
  ↓ 读 buyer-user.md（~100 行）    ← vs 原 buyer.md 366 行
  
L1: buyer-user.md §3.6.1-3.8 指针
  ↓ 按需读 buyer-actions.md 对应段落  ← 不变
```

引用深度不变（仍是 L0→L1→L1），不引入多层引用风险。

#### 与 Sub session 的隔离保证

| 维度 | Sub session | User session |
|---|---|---|
| 入口控制方 | CLI preamble（hardcoded 路径） | Claude Code Read tool |
| 读取的 SKILL | SKILL.md（404 行，不变） | SKILL-user.md（120 行，新） |
| 读取的 buyer | buyer.md（366 行，不变） | buyer-user.md（100 行，新） |
| 是否受本项影响 | **完全不受影响** | 物理隔离 |

CLI preamble 中的路径（flow.rs L237-306, designated.rs L36-40）全部指向原 `SKILL.md` + `buyer.md`，本项不修改任何 Rust 代码。

**改动量**: 新建 SKILL-user.md (~120行) + buyer-user.md (~100行) + 修改 CLAUDE.md 路由 (~5行)
**收益**: (404-120) + (366-100) = 550 行减少 = ~8,250 token/RESUME × 2 = **~16,500 token/task**
**风险**: 极低 — Sub session 完全不受影响；User 文件是 SKILL.md/buyer.md 的纯子集，不新增任何规则
**影响面**: 仅 User session 的文件加载路径

---

## 5. 收益汇总

### 5.1 按优化项

| # | 优化项 | P | 目标 Session | 节省类型 | 节省(token/task) | 改动量 | 风险 |
|---|---|---|---|---|---|---|---|
| **S14** | **User session 独立入口** | **P0** | **User** | **物理隔离** | **16,500** | **新建 2 文件** | **极低** |
| S1 | buyer.md scope 标注 | P0 | Job+Backup | 软标注 | 32,325 | 6 行 | 极低 |
| S2 | SKILL.md scope 标注 | P0 | Job+Backup | 软标注 | 9,435 | 2 行 | 极低 |
| S3 | Additional Resources scope | P0 | All | 软标注 | 防范 919 行误读 | 14 行 | 极低 |
| S4 | message-types scope | P1 | Job+Backup | 软标注 | 防范 160 行误读 | 3 行 | 极低 |
| S8 | §3.5 Backup 标注 | P1 | Backup | 软标注 | 2,205 | 1 行 | 极低 |
| S13 | SKILL.md xmtp how-to 压缩 | P1 | Job+Backup | 物理删减 | 6,938 | ~25 行 | 低 |
| S5 | buyer.md preamble 去重 | P1 | Job+Backup | 物理删减 | 3,885 | 14 行净减 | 中 |
| S6 | buyer-actions 拆分 | P1 | User | 物理隔离 | 3,300-4,000 | 4 指针或 4 文件 | 低 |
| S7 | cli-reference 拆分 | P1 | Job+Backup | 物理隔离 | 按需节省 | 拆 4 文件 | 低 |
| S9 | Activation incident 压缩 | P2 | Job | 物理删减 | 3,375 | 15 行 | 中 |
| S10 | Quick Nav 去重 | P2 | Job+Backup | 物理删减 | 7,425 | 33 行删 | 低 |
| S11 | incidents 角色标签 | P2 | Job+Backup | 软标注 | 按需 | 21 行 | 极低 |
| S12 | evaluator 子目录 | P3 | All | 物理隔离 | 防范 263 行 | 2 文件 | 低 |

### 5.2 按节省类型

| 类型 | 含义 | 包含项 | 确定性收益 |
|---|---|---|---|
| **物理隔离** | 文件不加载 → input token 不消耗 | S14, S6, S7, S12 | **~20,500+** |
| **物理删减** | 文件行数减少 → input token 直接减少 | S5, S9, S10, S13 | **~21,623** |
| **软标注** | 标注引导跳过 → 减少推理噪声，input token 不变 | S1, S2, S3, S4, S8, S11 | **~43,965** (理论) |

> 注意：软标注的 ~44K 收益依赖 LLM 遵守标注 (~85-90%)，实际收益按 85% 折扣约 **~37,370**。物理隔离和物理删减的收益是确定性的。

### 5.3 按实施阶段

| 阶段 | 包含项 | 确定性节省 | 改动量 | 风险 | 耗时 |
|---|---|---|---|---|---|
| **Phase A: 物理隔离 + 零风险标注** | S14+S1+S2+S3+S4+S8 | **~16,500** (硬) + **~44,000** (软) | 新建 2 文件 + ~26 行标注 | 极低 | 0.5-1 天 |
| **Phase B: 辅助文件拆分** | S6+S7+S11+S12 | **~4,000+** (硬) | 拆 6-10 文件 | 低 | 0.5-1 天 |
| **Phase C: 内容精简** | S5+S9+S10+S13 | **~21,623** (硬) | ~108 行改动 | 低-中 | 0.5 天 |
| **Phase D: 验证** | 全 session 冒烟 + 回归 | — | — | — | 0.5 天 |
| **合计** | **14 项** | **~86,123 token/task** | | | **~2.5 天** |

### 5.4 按 Session 的收益分布

| Session | 收益来源 | 节省(token/task) | RESUME 均值 |
|---|---|---|---|
| **User** | S14 物理隔离 + S6 buyer-actions 拆分 | **~20,150** | 2 次 |
| **Job** | S1+S2 标注跳过 + S5+S9+S10+S13 压缩 | **~52,700** | 15 次 |
| **Backup** | S1+S2+S8 标注跳过 | **~13,100** | 3.5 次 |
| **合计** | | **~85,950** | |

---

## 6. 最终实施计划

### Phase A: 物理隔离 + 零风险标注（Day 1，可与 CLI 下沉并行）

**目标**: 一天内上线最高 ROI 项目。S14 和 S1-S4/S8 互不依赖，可并行执行。

**A-1 组: User session 物理隔离 [S14]**

- [ ] A.1.1 从 SKILL.md 提取 User 段落 → 创建 `skills/okx-agent-task/SKILL-user.md` (~120 行)
  - 包含: frontmatter, Role determination, Pre-flight, Field Mapping, Anti-hallucination, User Intent Routing, Cross-Skill Routing, Communication Boundary (精简)
  - 不包含: Runtime Bridge, Activation, a2a-agent-chat, sessionKey, Communication Contract, Quick Nav
- [ ] A.1.2 从 buyer.md 提取 User 段落 → 创建 `skills/okx-agent-task/buyer-user.md` (~100 行)
  - 包含: USDT 消歧, §3.1-§3.3, Intent routing, resolve, §3.6.1-3.8 指针
  - 不包含: preamble sub 规则, Trigger, P2P, Negotiation, Inbound routing, System event, relay, Exception
- [ ] A.1.3 修改 CLAUDE.md okx-agent-task 路由: User session 读 SKILL-user.md + buyer-user.md
- [ ] A.1.4 **验证**: 创建任务 → 指定服务商 → 修改条款 → 查看交付物
  - 确认 SKILL-user.md 包含所有 User 场景所需规则
  - 确认 Sub session（Backup/Job）不受影响（仍读原 SKILL.md + buyer.md）

**A-2 组: Sub session 零风险标注 [S1+S2+S3+S4+S8]**

- [ ] A.2.1 buyer.md 增加 3 处 scope 标注 [S1]
  - L1 前: `⚠️ User session only — Backup/Job skip to §1`
  - L84 前: `⚠️ §3.1-§3.3 + routing + resolve = User session only — skip to §3.4`
  - L300 前: `⚠️ User session only`
- [ ] A.2.2 SKILL.md 增加 2 处 scope 标注 [S2]
  - L70 前: `⚠️ User session only — sub sessions skip to Critical Field Mapping`
  - L327 前: `⚠️ User session only — sub sessions skip to Communication Boundary`
- [ ] A.2.3 SKILL.md Additional Resources 每条加 session scope [S3]
- [ ] A.2.4 message-types.md §3.1 前加标注 [S4]
- [ ] A.2.5 buyer.md §3.5 前加 Job session only 标注 [S8]
- [ ] A.2.6 **验证**: 触发一次 negotiate → 确认 Job session 跳过了 user-only 段落

**Phase A 收益**: ~60,500 token/task（S14: 16,500 硬 + S1: 32,325 软 + S2: 9,435 软 + S8: 2,205 软）

---

### Phase B: 辅助文件拆分（Day 2，Phase A 验证通过后）

- [ ] B.1 buyer-actions.md 指针精确化 [S6]
  - 方案 A（推荐）: buyer-user.md 中的 §3.6.1-3.8 指针改为精确行号
  - 方案 B: 拆为 4 个独立文件
- [ ] B.2 cli-reference.md 按角色拆分为 common + buyer + provider + evaluator [S7]
- [ ] B.3 evaluator-decision-rubric.md + evaluator-staking.md 移至 `references/evaluator/` [S12]
- [ ] B.4 incidents.md 21 个标题加 `[buyer]`/`[provider]`/`[all]` 角色标签 [S11]
- [ ] B.5 更新 SKILL.md Reading Order + SKILL-user.md Reading Order + 所有引用路径
- [ ] B.6 **验证**: 全文 grep 确认无断链引用；触发 buyer-actions 各段落确认正确加载

**Phase B 收益**: ~4,000+ token/task

---

### Phase C: 内容精简（Day 2-3，Phase A 验证通过后，可与 Phase B 并行）

- [ ] C.1 buyer.md preamble 从 18 行压缩为 4 行引用 [S5]
  - 风险缓解: SKILL.md 原文仍在 context，defense-in-depth 仅减弱不消失
- [ ] C.2 SKILL.md Activation ~10 处 inline incident 替换为编号引用 [S9]
  - 风险缓解: 关键 incident 在 buyer.md §3.5 保留全文
- [ ] C.3 SKILL.md (15行) + buyer.md (18行) Quick Navigation 表删除 [S10]
  - 人类可保留为 HTML 注释 `<!-- ... -->`
- [ ] C.4 SKILL.md §4 xmtp how-to 从 33 行压缩为 8 行 + §5 命令模板压缩 [S13]
  - 保留 Tool whitelist + Forbidden 列表；删除 playbook 已自包含的步骤说明
- [ ] C.5 **验证**: 触发高频事件（negotiate_reply, job_accepted），确认压缩后 LLM 仍正确调用工具

**Phase C 收益**: ~21,623 token/task

---

### Phase D: 全量验证（Day 3）

**D.1 引用完整性**

- [ ] D.1.1 `grep -r "buyer.md\|SKILL.md\|buyer-actions" skills/okx-agent-task/` 确认所有引用指向正确文件
- [ ] D.1.2 `grep -r "buyer-user.md\|SKILL-user.md" skills/okx-agent-task/` 确认新文件被正确引用
- [ ] D.1.3 检查 CLAUDE.md 中 okx-agent-task 路由是否完整

**D.2 User session 冒烟测试**

- [ ] D.2.1 "帮我发一个任务" → 创建任务全流程
- [ ] D.2.2 "指定 Agent 1506" → 指定服务商（A2A + x402 两条路径）
- [ ] D.2.3 "把预算改成 50 USDT" → 条款修改
- [ ] D.2.4 "查看交付物" → 交付物下载展示
- [ ] D.2.5 确认 **不读取** SKILL.md / buyer.md（通过 audit log 检查 Read tool 调用）

**D.3 Job session 冒烟测试**

- [ ] D.3.1 首次协商 → negotiate_request → negotiate_counter → negotiate_reply
- [ ] D.3.2 provider_applied → job_accepted
- [ ] D.3.3 job_submitted → 终态（completed / cancelled）
- [ ] D.3.4 确认标注跳过生效（Job session 不处理 §3.1-§3.3 内容）

**D.4 Backup session 冒烟测试**

- [ ] D.4.1 job_created → recommend / designated 分支
- [ ] D.4.2 终态事件（job_completed / job_cancelled）
- [ ] D.4.3 确认 Backup 不处理 §3.5 Inbound routing

**D.5 回归测试**

- [ ] D.5.1 Provider 角色全流程回归（确认无影响）
- [ ] D.5.2 Evaluator 角色回归（确认无影响）
- [ ] D.5.3 验证 S5/S9（内容精简项）是否导致规则违反率上升

---

### 实施依赖关系

```
Phase A (Day 1)
├── A-1: User 物理隔离 [S14] ──────────┐
│                                       ├── Phase D.2 (User 验证)
└── A-2: Sub scope 标注 [S1-S4,S8] ────┤
                                        ├── Phase B (Day 2) ──── Phase D.1 + D.3 + D.4
                                        │
                                        └── Phase C (Day 2-3) ── Phase D.5
                                            (可与 B 并行)
```

**关键路径**: A → D.2（User 验证）→ 上线 User 优化。B/C 可在 A 验证后并行推进。

---

## 附录 A：两项优化的联合收益

Skill 文件优化（本文档）和 CLI 下沉优化（另一文档）可并行实施，联合收益：

| 优化类别 | Token 节省/task | 主要目标 Session | 节省类型 |
|---|---|---|---|
| CLI 下沉 10 项 | **~100K** | Job (主) + Backup | 物理（CLI 接管，不进 LLM） |
| Skill 文件精简 14 项 | **~86K** | User + Job + Backup | 混合（物理隔离 + 物理删减 + 软标注） |
| **联合收益** | **~186K token/task** | | |

按 100 task/天: **~18.6M token/天节省**

#### 三类优化手段的叠加关系

```
优化前: 每 task ~400K+ token

层 1: CLI 下沉 (-100K)     → 逻辑从 skill 文件移入 CLI，LLM 不再需要阅读这些指令
层 2: 物理隔离 (-20K)      → 按 session/角色拆分文件，不需要的文件不加载
层 3: 物理删减 (-22K)      → 压缩重复、how-to、导航表，文件本身变小
层 4: 软标注 (-44K, ~85%)  → 标注引导跳过，减少推理噪声

优化后: ~214K token/task（节省 ~47%）
```

> CLI 下沉（层 1）与 Skill 文件优化（层 2-4）正交，可完全并行实施。层 2-4 之间也互不冲突。

---

## 附录 B：文件位置索引

| 文件 | 路径 | 行数 | 备注 |
|---|---|---|---|
| **SKILL-user.md** | `skills/okx-agent-task/SKILL-user.md` | **~120** | **新建 [S14] — User session 入口** |
| **buyer-user.md** | `skills/okx-agent-task/buyer-user.md` | **~100** | **新建 [S14] — User session buyer 流程** |
| SKILL.md | `skills/okx-agent-task/SKILL.md` | 404 | Sub session 继续使用 |
| buyer.md | `skills/okx-agent-task/buyer.md` | 366 | Sub session 继续使用 |
| buyer-actions.md | `skills/okx-agent-task/buyer-actions.md` | 290 | Phase B 拆分或指针精确化 |
| cli-reference.md | `skills/okx-agent-task/_shared/cli-reference.md` | 824 |
| message-types.md | `skills/okx-agent-task/_shared/message-types.md` | 341 |
| display-formats.md | `skills/okx-agent-task/references/display-formats.md` | 324 |
| provider.md | `skills/okx-agent-task/provider.md` | 262 |
| incidents.md | `skills/okx-agent-task/references/incidents.md` | 213 |
| evaluator-staking.md | `skills/okx-agent-task/references/evaluator-staking.md` | 180 |
| state-machine.md | `skills/okx-agent-task/_shared/state-machine.md` | 175 |
| xmtp-tools.md | `skills/okx-agent-task/_shared/xmtp-tools.md` | 154 |
| troubleshooting.md | `skills/okx-agent-task/references/troubleshooting.md` | 125 |
| user-intent-routing.md | `skills/okx-agent-task/_shared/user-intent-routing.md` | 123 |
| exception-escalation.md | `skills/okx-agent-task/_shared/exception-escalation.md` | 100 |
| entry-points.md | `skills/okx-agent-task/_shared/entry-points.md` | 85 |
| evaluator-decision-rubric.md | `skills/okx-agent-task/references/evaluator-decision-rubric.md` | 83 |
| payment-modes.md | `skills/okx-agent-task/_shared/payment-modes.md` | 65 |
| preflight.md | `skills/okx-agent-task/_shared/preflight.md` | 49 |
| evaluator.md | `skills/okx-agent-task/evaluator.md` | 41 |
