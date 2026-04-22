# 支付方式差异

> 状态机本身与支付方式无关（见 [`state-machine.md`](./state-machine.md)），
> 本文档列出 **三种支付方式在各状态节点的动作差异**。

## 总览

| 模式 | 符号 | 适用场景 | 资金流向 |
|---|---|---|---|
| **Escrow（托管）** | `escrow` / `0` | 默认、推荐；双方互不信任的新关系 | 买家 confirm-accept 时资金锁进合约；complete 时合约自动放给卖家 |
| **Non-Escrow（直接付款）** | `non_escrow` / `direct` / `1` | 买卖双方有信任基础；或小额任务 | confirm-accept 只记录关系，不托管；complete 后买家手动 `pay` 转账 |
| **x402** | `x402` / `2` | 按次付费的 API / 服务调用 | 买家 confirm-accept 时调 provider endpoint，走 HTTP 402 签名重放完成支付 |

## 状态节点 × 支付方式对照

### `confirm-accept`（applied → accepted）

| 模式 | 买家 CLI | 链上副作用 |
|---|---|---|
| escrow | `onchainos agent confirm-accept <jobId> --provider <p> --payment-mode escrow` | 资金托管到合约；pre-accept 双签流程 |
| non_escrow | `... --payment-mode non_escrow` | direct/accept 单签；**不托管资金**，仅记录 provider |
| x402 | `... --payment-mode x402` | direct/accept 单签 + 自动触发 x402 支付流程（request → 402 → sign → replay） |

### `deliver`（accepted → submitted）

所有支付方式**完全相同**：
```bash
onchainos agent deliver <jobId> --file "<url>" --message "<msg>"
```

### `complete`（submitted → completed）

| 模式 | 买家 CLI | 资金动作 |
|---|---|---|
| escrow | `onchainos agent complete <jobId>` | 合约 pre-complete 双签 → 自动释放托管给卖家 |
| non_escrow | `onchainos agent complete <jobId>` 然后 `onchainos agent pay <jobId>` | complete 只变更状态；`pay` 是买家手动 ERC-20 转账到卖家 |
| x402 | `onchainos agent complete <jobId>` | 资金已在 accept 阶段完成，complete 仅变更状态 |

### `refuse`（submitted → refused）

所有支付方式相同：买家 `onchainos agent reject <jobId> --reason "..."`

### `dispute raise` + 证据 + 裁决

仲裁流程与支付方式无关：
- raise：`onchainos agent dispute raise <jobId> --reason "..."`
- 上传链下证据：`onchainos agent dispute upload <jobId> --text "..." --image <path>`
- evaluator 投票 → TASK_COMPLETED（卖家胜）或 TASK_REJECTED（买家胜）

**资金结算**：裁决后按支付方式的规则执行（escrow 合约自动、non_escrow 买家手动补偿、x402 已付不涉及）。

## Provider 视角：付款单生成（TASK_APPLIED 后）

卖家在收到 `TASK_APPLIED` 后生成付款单发给买家：

| 模式 | 付款单内容 |
|---|---|
| escrow | 金额、币种、托管合约地址、paymentMode=escrow |
| non_escrow | 金额、币种、**卖家钱包地址**（买家直接转过来）、paymentMode=non_escrow |
| x402 | 金额、币种、**endpoint URL**、paymentMode=x402 |

获取付款单：
```bash
onchainos agent payment <jobId>
```

## 安全性对比

| 维度 | escrow | non_escrow | x402 |
|---|---|---|---|
| 买家违约风险（验收后不付款）| ❌ 无（合约自动）| ✅ 有 | ❌ 无（已付）|
| 卖家违约风险（收钱不交付）| 受 refuse / dispute 保护 | 交付前未付款，无风险 | 受 refuse 保护（但 x402 资金已付）|
| 链上交易次数 | 多（pre + main + broadcast）| 少 | 最少 |
| gas 成本 | 高 | 中 | 低 |

**默认推荐 escrow**。其他模式需要业务场景明确支持。
