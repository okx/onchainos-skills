# 支付方式差异

> 状态机本身与支付方式无关（见 [`state-machine.md`](./state-machine.md)），
> 本文档列出 **两种支付方式在各状态节点的动作差异**。

## 总览

| 模式 | 符号 | 适用场景 | 资金流向 |
|---|---|---|---|
| **Escrow（担保支付）** | `escrow` / `1` | 默认、推荐；双方互不信任的新关系 | 买家 confirm-accept 时资金锁进担保合约；complete 时合约自动放给卖家 |
| **x402（按需微支付）** | `x402` / `3` | 按次付费的 API / 服务调用 | 卖家 service-list 注册 HTTP endpoint；买家 GET 该 endpoint → 拿 402 challenge → 签 x402_pay → 重放 endpoint 同步取回交付物。**没有 paymentId** |

## 状态节点 × 支付方式对照

### `confirm-accept`（open → accepted）

| 模式 | 前置条件 | 买家 CLI | 链上副作用 |
|---|---|---|---|
| escrow | 卖家 apply 上链（provider_applied） | `onchainos agent confirm-accept <jobId> --provider <p> --payment-mode escrow` | 资金担保到合约；pre-accept 双签流程 |
| x402 | 无（自动匹配） | `... --payment-mode x402` | direct/accept 单签 + 自动触发 x402 支付流程（request → 402 → sign → replay） |

### `deliver`

| 模式 | 触发时机 | 说明 |
|---|---|---|
| escrow | accepted → submitted（执行任务后提交） | 标准流程：accepted 后卖家执行任务并交付 |
| x402 | accepted → submitted | 同 escrow |

CLI 命令（所有支付方式相同）：
```bash
onchainos agent deliver <jobId> --file "<url>" --message "<msg>"
```

### `complete`

| 模式 | 触发时机 | 买家 CLI | 资金动作 |
|---|---|---|---|
| escrow | submitted → completed（验收交付物后） | `onchainos agent complete <jobId>` | 合约 pre-complete 双签 → 自动释放担保款给卖家 |
| x402 | submitted → completed | `onchainos agent complete <jobId>` | 资金已在 accept 阶段完成，complete 仅变更状态 |

### `refuse`（submitted → refused，仅 escrow）

⚠️ **仅 escrow 支持拒绝**。x402 资金已在 accept 阶段支付完成。

escrow 买家拒绝：`onchainos agent reject <jobId> --reason "..."`

### `dispute raise` + 证据 + 裁决

仲裁流程与支付方式无关：
- raise：`onchainos agent dispute raise <jobId> --reason "..."`
- 上传链下证据：`onchainos agent dispute upload <jobId> --text "..." --image <path>`
- evaluator 投票 → job_completed（卖家胜）或 job_refunded（买家胜）

**资金结算**：裁决后按支付方式的规则执行（escrow 合约自动、x402 已付不涉及）。

## 安全性对比

| 维度 | escrow | x402 |
|---|---|---|
| 买家违约风险（收货后不付款）| ❌ 无（合约自动）| ❌ 无（已付）|
| 卖家违约风险 | 受 refuse / dispute 保护 | 受 refuse 保护（但 x402 资金已付）|
| 链上交易次数 | 多（pre + main + broadcast）| 最少 |
| gas 成本 | 高 | 低 |

**默认推荐 escrow**。x402 需要业务场景明确支持。
