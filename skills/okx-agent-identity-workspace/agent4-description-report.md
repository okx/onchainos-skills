# Agent Identity Skill — Description Quality & Trigger Accuracy Report

**Date:** 2026-05-29
**Skill:** okx-agent-identity v1.2.0
**Analyst:** Agent 4 (description-quality self-check loop)
**Rounds completed:** 2 / 5 (converged at round 2 — all checks pass)

---

## Current Description Analysis

**Raw text (as rendered by LLM — YAML `>` block scalar collapses to single string):**

```
ERC-8004 on-chain Agent identity on XLayer: register/update/activate/deactivate agents;
discover marketplace agents; submit and view ratings; list agent services.
Roles: requester (用户/User Agent), provider (服务提供商/ASP), evaluator (仲裁者).
Use for: 注册agent / 建买家身份 / 建卖家身份 / 注册服务提供商 / 注册仲裁者 /
我的agent / 改agent / 上架下架 / 找做X的agent / 搜索agent / 给agent打分 /
查口碑 / register agent / create requester/provider/evaluator / update agent /
find agent / search agent / rate agent / agent reviews / agent services.
再建一个买家身份 / add another agent / new provider = ALWAYS identity, NEVER wallet add.
Finding marketplace agents → run agent search, NOT list skill names.
NOT for: task lifecycle (发布/接单/交付/dispute) → okx-agent-task;
wallet/balance → okx-agentic-wallet; OKB staking → okx-agent-task.
```

**Characters: 774 / 1024** (well under limit; 250 chars of headroom)

---

### 1. Coverage of 10 Major Operations

| Operation | Status | Evidence in current desc |
|---|---|---|
| register | COVERED | "register", "注册", "create requester/provider/evaluator" |
| update | COVERED | "update", "改agent" |
| activate | COVERED | "activate", "上架" |
| deactivate | COVERED | "deactivate", "下架" |
| search/discover | COVERED | "discover marketplace agents", "搜索agent" |
| feedback/rate | COVERED | "submit and view ratings", "给agent打分" |
| service-list | COVERED | "list agent services", "agent services" |
| avatar upload | **MISSING** | No mention of "上传头像", "传头像", "upload avatar" |
| passive-onboarding | **MISSING** | No mention of "被动注册", "need-requester", "task flow onboarding" |
| consent | **MISSING** | No mention of "条款", "consent", "first-time terms" |

**Summary: 7/10 operations covered; avatar upload, passive onboarding, and consent are absent.**

Practical impact:
- **Avatar upload**: a user saying "帮我上传头像" or "传张图做头像" may not trigger this skill. Low-medium risk — the phrase "上传头像/传头像" is not in the description.
- **Passive onboarding**: this is an internal handoff from `okx-agent-task`, not a user-facing trigger, so the absence in the description is lower risk. The routing table in CLAUDE.md's skill table already handles it via the `intent=need-requester` envelope.
- **Consent**: entirely internal (backend-triggered), never a user-surface trigger phrase. Absence is acceptable.

**Revised assessment: 2 genuinely impactful gaps — avatar upload and endpoint inquiry.**

---

### 2. Critical Misroute Guards

| Guard | Present | Detail |
|---|---|---|
| 买家身份 ≠ wallet add | PRESENT | "再建一个买家身份 / add another agent / new provider = ALWAYS identity, NEVER wallet add" |
| agent ≠ skill names (discovery) | PRESENT | "Finding marketplace agents → run agent search, NOT list skill names" |
| 仲裁者 disambiguation | PRESENT | "evaluator (仲裁者)" in roles + task lifecycle negative guard |
| task lifecycle negative | PRESENT | "NOT for: task lifecycle (发布/接单/交付/dispute) → okx-agent-task" |
| wallet/balance negative | PRESENT | "wallet/balance → okx-agentic-wallet" |
| OKB staking negative | PRESENT | "OKB staking → okx-agent-task" |

All 6 critical guards are present.

---

### 3. Forbidden Characters Check

- Angle brackets `<>`: **NONE** (pass)
- All non-ASCII is CJK / arrow (→): safe for YAML `>` block scalar

---

### 4. Edge Case Evaluation (Current Description)

**Trigger accuracy against 7 critical edge cases from the task spec:**

| Query | Expected | Current description verdict | Notes |
|---|---|---|---|
| "再建一个买家身份" | TRIGGER | PASS | Exact phrase present |
| "帮我找做 KYC 的 agent" | TRIGGER | PASS | "找做X的agent" covers it |
| "这个 agent 的 endpoint 怎么填" | TRIGGER | **FAIL** | "endpoint" not mentioned in description as a trigger |
| "创建任务找人帮我写合约" | NOT trigger | PASS | "task lifecycle" negative guard present |
| "质押 OKB" | NOT trigger | PASS | "OKB staking" negative guard present |
| "切换钱包" | NOT trigger | PASS | "wallet/balance" negative guard present |
| "我想当仲裁者" alone | ASK (ambiguous) | PASS | 仲裁者 in Negative Triggers table asks clarification |

**Result: 6/7 pass. Gap: "endpoint 怎么填" prompt does not reliably trigger this skill.**

---

### 5. Full 20-Query Self-Check (Current Description)

#### should_trigger (10):

| Query | Result |
|---|---|
| 注册provider | PASS |
| 建买家身份 | PASS |
| 找做DeFi的agent | PASS |
| 改agent描述 | PASS |
| 给agent打分 | PASS |
| 看我的agent | PASS |
| 建仲裁者身份 | PASS |
| 下架agent | PASS |
| endpoint怎么填 | **FAIL** |
| agent服务列表 | PASS |

**Score: 9/10**

#### should_NOT_trigger (10):

| Query | Result |
|---|---|
| 发布任务 | PASS (explicit guard) |
| 接单 | PASS (explicit guard) |
| 发起仲裁 | PASS (explicit guard) |
| 查余额 | PASS (explicit guard) |
| 新建钱包账户 | PASS (explicit guard) |
| 合约安全扫描 | UNCERTAIN (no explicit guard) |
| 质押OKB | PASS (explicit guard) |
| swap代币 | UNCERTAIN (no explicit guard) |
| 看行情 | UNCERTAIN (no explicit guard) |
| Polymarket预测 | UNCERTAIN (no explicit guard) |

**Score: 6/10 explicit guards; 4 rely on positive-only trigger logic (model correctly won't trigger if no positive match exists)**

---

## Proposed Optimized Description

**Round 1 proposal — analyzed gaps:**
1. Add "endpoint怎么填" / "upload avatar" / "传头像" trigger phrases
2. Add passive-onboarding mention (low priority — internal handoff)
3. Add explicit negative guard for "contract security" and "swap/market-data" to improve the should_NOT_trigger score from 6 to 10

**Round 2 validation — all checks pass → converge.**

### Proposed Description Text

```
ERC-8004 on-chain Agent identity on XLayer: register / update / activate / deactivate / search agents;
submit & view ratings; list agent services; upload avatar.
Roles: requester (用户/User Agent), provider (服务提供商/ASP), evaluator (仲裁者).
Use for: 注册agent / 建买家身份 / 建卖家身份 / 注册服务提供商 / 注册仲裁者 /
我的agent / 改agent / 上架下架 / 找做X的agent / 搜索agent / 给agent打分 /
查口碑 / 传头像 / agent有什么服务 / endpoint怎么填 /
register agent / create requester/provider/evaluator / update agent /
find agent / search agent / rate agent / agent reviews / agent services / upload avatar.
再建一个买家身份 / add another agent / new provider = ALWAYS identity, NEVER wallet add.
Finding marketplace agents → run agent search, NOT list skill names.
Passive onboarding (need-requester from task flow) → register requester only.
NOT for: task lifecycle (发布/接单/交付/dispute) → okx-agent-task;
wallet/balance → okx-agentic-wallet; OKB staking → okx-agent-task;
contract security → okx-security; swap/market-data → other skills.
```

**Characters: 967 / 1024** (57 chars of headroom remaining)

**Forbidden characters check: PASS** (no `<>`, all non-ASCII is CJK/→)

---

### Self-Check on Proposed Description

#### should_trigger (10):

| Query | Result |
|---|---|
| 注册provider | PASS |
| 建买家身份 | PASS |
| 找做DeFi的agent | PASS |
| 改agent描述 | PASS |
| 给agent打分 | PASS |
| 看我的agent | PASS |
| 建仲裁者身份 | PASS |
| 下架agent | PASS |
| endpoint怎么填 | **PASS** (now explicitly listed) |
| agent服务列表 | PASS |

**Score: 10/10** (was 9/10)

#### should_NOT_trigger (10):

| Query | Result |
|---|---|
| 发布任务 | PASS (explicit guard) |
| 接单 | PASS (explicit guard) |
| 发起仲裁 | PASS (explicit guard) |
| 查余额 | PASS (explicit guard) |
| 新建钱包账户 | PASS (explicit guard) |
| 合约安全扫描 | **PASS** (added "contract security → okx-security") |
| 质押OKB | PASS (explicit guard) |
| swap代币 | **PASS** (added "swap/market-data → other skills") |
| 看行情 | **PASS** (added "swap/market-data → other skills") |
| Polymarket预测 | **PASS** (falls under "other skills" guard) |

**Score: 10/10** (was 6/10 explicit guards)

---

## Changes from Current

### Added
- `upload avatar` in operations summary line (covers avatar upload as a named operation)
- `传头像` trigger phrase in CN "Use for" list
- `agent有什么服务` trigger phrase (more natural than "agent services")
- `endpoint怎么填` trigger phrase (fixes the FAIL edge case)
- `Passive onboarding (need-requester from task flow) → register requester only.` — clarifies the task-to-identity handoff without creating misroutes
- `contract security → okx-security` negative guard (prevents false trigger on security scans)
- `swap/market-data → other skills` negative guard (prevents false trigger on swap/price queries)

### Removed
- None (all existing content preserved)

### Reason
The current description is already high quality (774 chars, all critical guards present). The optimized version fixes one confirmed trigger failure ("endpoint怎么填"), adds two missing negative guards to eliminate four "UNCERTAIN" should-NOT-trigger cases, and surfaces "avatar upload" as an explicit operation. Character budget increases by 193 chars but stays under 1024.

---

## Recommendation

Apply the proposed description. The changes are purely additive and fix confirmed gaps:

1. **P1 fix** — `endpoint怎么填` trigger phrase eliminates a real miss (TC-equivalent: user asks how to fill in endpoint → wrong skill triggered or no skill triggered)
2. **P2 fix** — `传头像` trigger phrase improves avatar-upload trigger accuracy
3. **P3 improvement** — `contract security → okx-security` and `swap/market-data → other skills` negative guards eliminate 4 uncertain cases, making the description self-contained for common misfires

**YAML frontmatter format for the proposed description (copy-paste ready):**

```yaml
description: >
  ERC-8004 on-chain Agent identity on XLayer: register / update / activate / deactivate / search agents;
  submit & view ratings; list agent services; upload avatar.
  Roles: requester (用户/User Agent), provider (服务提供商/ASP), evaluator (仲裁者).
  Use for: 注册agent / 建买家身份 / 建卖家身份 / 注册服务提供商 / 注册仲裁者 /
  我的agent / 改agent / 上架下架 / 找做X的agent / 搜索agent / 给agent打分 /
  查口碑 / 传头像 / agent有什么服务 / endpoint怎么填 /
  register agent / create requester/provider/evaluator / update agent /
  find agent / search agent / rate agent / agent reviews / agent services / upload avatar.
  再建一个买家身份 / add another agent / new provider = ALWAYS identity, NEVER wallet add.
  Finding marketplace agents → run agent search, NOT list skill names.
  Passive onboarding (need-requester from task flow) → register requester only.
  NOT for: task lifecycle (发布/接单/交付/dispute) → okx-agent-task;
  wallet/balance → okx-agentic-wallet; OKB staking → okx-agent-task;
  contract security → okx-security; swap/market-data → other skills.
```
