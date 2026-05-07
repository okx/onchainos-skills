# 支付方式差异

> 状态机本身与支付方式无关（见 [`state-machine.md`](./state-machine.md)），
> 本文档列出 **三种支付方式在各状态节点的动作差异**。

⚠️ **`non_escrow` 和 `x402` 不是同义词，是两种独立的支付方式**：底层都用 EIP-3009 单签做支付凭证，但触发路径完全不同 —— `non_escrow` 出 `paymentId: a2a_xxx`（卖家先 `get-payment` 建 a2a-pay 付款单），`x402` 走 HTTP 402 challenge（卖家 service-list 注册 endpoint，买家直接 GET 拿 402 响应再签名重放）。**禁止**在任何对外 xmtp_send / 用户提示里写「非担保 x402 / non_escrow x402 / 非担保（x402）」之类的混合标签。

## 总览

| 模式 | 符号 | 适用场景 | 资金流向 |
|---|---|---|---|
| **Escrow（担保支付）** | `escrow` / `1` | 默认、推荐；双方互不信任的新关系 | 买家 confirm-accept 时资金锁进担保合约；complete 时合约自动放给卖家 |
| **Non-Escrow（非担保支付）** | `non_escrow` / `direct` / `2` | 买卖双方有信任基础；或小额任务 | 协商后卖家 `get-payment` 创建 a2a-pay charge 出 `paymentId: a2a_xxx` → 买家用 paymentId 跑 confirm-accept → a2a_pay EIP-3009 支付 → direct/accept 上链 |
| **x402（按需微支付）** | `x402` / `3` | 按次付费的 API / 服务调用 | 卖家 service-list 注册 HTTP endpoint；买家 GET 该 endpoint → 拿 402 challenge → 签 x402_pay → 重放 endpoint 同步取回交付物。**没有 paymentId** |

## 状态节点 × 支付方式对照

### `confirm-accept`（open → accepted）

| 模式 | 前置条件 | 买家 CLI | 链上副作用 |
|---|---|---|---|
| escrow | 卖家 apply 上链（provider_applied） | `onchainos agent confirm-accept <jobId> --provider <p> --payment-mode escrow` | 资金担保到合约；pre-accept 双签流程 |
| non_escrow | 卖家 create_payment_charge 生成账单，XMTP 发 paymentId | `... --payment-mode non_escrow --payment-id <paymentId>` | setPaymentMode(2) → a2a_pay EIP-3009 支付 → direct/accept 单签上链 |
| x402 | 无（自动匹配） | `... --payment-mode x402` | direct/accept 单签 + 自动触发 x402 支付流程（request → 402 → sign → replay） |

### `deliver`

| 模式 | 触发时机 | 说明 |
|---|---|---|
| escrow | accepted → submitted（执行任务后提交） | 标准流程：accepted 后卖家执行任务并交付 |
| non_escrow | completed → submitted（支付链路完成后提交） | 非担保：complete 在 accepted 后立即执行，卖家在 completed 状态后才提交交付物 |
| x402 | accepted → submitted | 同 escrow |

CLI 命令（所有支付方式相同）：
```bash
onchainos agent deliver <jobId> --file "<url>" --message "<msg>"
```

### `complete`

| 模式 | 触发时机 | 买家 CLI | 资金动作 |
|---|---|---|---|
| escrow | submitted → completed（验收交付物后） | `onchainos agent complete <jobId>` | 合约 pre-complete 双签 → 自动释放担保款给卖家 |
| non_escrow | accepted → completed（接单后立即） | `onchainos agent complete <jobId>` | direct/complete 单签上链（资金已在 accept 阶段通过 a2a_pay 支付） |
| x402 | submitted → completed | `onchainos agent complete <jobId>` | 资金已在 accept 阶段完成，complete 仅变更状态 |

### `refuse`（submitted → refused，仅 escrow）

⚠️ **仅 escrow 支持拒绝**。non_escrow 的 complete 在 accepted 后已执行，submitted 阶段只是收交付物通知，不支持 refuse。

escrow 买家拒绝：`onchainos agent reject <jobId> --reason "..."`

### `dispute raise` + 证据 + 裁决

仲裁流程与支付方式无关：
- raise：`onchainos agent dispute raise <jobId> --reason "..."`
- 上传链下证据：`onchainos agent dispute upload <jobId> --text "..." --image <path>`
- evaluator 投票 → job_completed（卖家胜）或 job_refunded（买家胜）

**资金结算**：裁决后按支付方式的规则执行（escrow 合约自动、non_escrow 买家手动补偿、x402 已付不涉及）。

## Provider 视角：付款单生成

| 模式 | 触发时机 | 付款单内容 |
|---|---|---|
| escrow | 协商达成后卖家 apply 上链，收到 `provider_applied` | 金额、币种、担保合约地址、paymentMode=escrow |
| non_escrow | 协商达成后卖家调 `create_payment_charge`（**不走 apply**） | 金额、币种、paymentId（a2a_xxx 格式），通过 XMTP 发给买家 |
| x402 | 无需付款单（自动匹配） | 金额、币种、**endpoint URL**、paymentMode=x402 |

non_escrow 获取付款单（卖家执行）：
```bash
onchainos agent get-payment <jobId> --token-symbol <USDT|USDG> --token-amount <金额> --payment-mode non_escrow --agent-id <agentId>
```

## 安全性对比

| 维度 | escrow | non_escrow | x402 |
|---|---|---|---|
| 买家违约风险（验收后不付款）| ❌ 无（合约自动）| ❌ 无（accept 阶段已付）| ❌ 无（已付）|
| 卖家违约风险（收钱不交付）| 受 refuse / dispute 保护 | ✅ 有（先付后交付，买家承担风险）| 受 refuse 保护（但 x402 资金已付）|
| 链上交易次数 | 多（pre + main + broadcast）| 少 | 最少 |
| gas 成本 | 高 | 中 | 低 |

**默认推荐 escrow**。其他模式需要业务场景明确支持。
