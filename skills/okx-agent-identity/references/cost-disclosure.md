# Cost Disclosure (P0)

Fires whenever the user asks about fees / gas / 抽成 / "扣不扣钱".

Source of truth: OKX Agent platform PRD §1.7 / §F0.7. Never derive from training data.

## Phase-1 gas policy

**所有链上动作 OKX 全包网络手续费 — 用户钱包不扣一分钱:**

| 操作 | 费用 |
|---|---|
| 创建 agent / mint NFT (`agent create`) | ✅ OKX 全包 |
| 编辑 agent 字段 (`agent update`) | ✅ OKX 全包 |
| 上架 / 下架 (`activate` / `deactivate`) | ✅ OKX 全包（下架不上链） |
| 评价 (`agent feedback-submit`) | ✅ OKX 全包 |

User Agents paying service fees go through `okx-agent-task` settlement — out of scope here.

## Platform commission

**无平台抽成 (zero platform fee).** The ASP sets the `service fee` and keeps 100%. OKX takes no cut.

## Standard line (PRD 文案约束 — render verbatim when topical)

Quote at least once per session, ideally before the first agent-creating mutation:

> 中文: 「**OKX 替你出手续费（在区块链上做事的成本），钱包不扣一分钱；OnchainOS Agentic Wallet 替你直接签好交易，整个过程你的钱包都不用动。**」
>
> English: "**OKX covers all transaction fees on your behalf (the cost of doing things on the blockchain), so your wallet is not charged a cent. OnchainOS Agentic Wallet signs the transaction for you — your wallet stays untouched throughout.**"

## Forbidden phrasings

- ❌ "文档中未明确说明 gas 费用" / "未明确" / "未涉及"
- ❌ "需要在实际创建时才能看到准确的 gas 预估"
- ❌ "建议查看官方文档 / 联系 OKX 客服 / 在 XLayer 区块浏览器查看"
- ❌ Fabricated fee categories: "平台服务费 X USDT" / "调度费" / "管理费" / "执行管理费"
- ❌ Soft-hallucination wrappers: "假设例子 / 我的推测 / 实际可能完全不同 / 这只是一个示例"
- ❌ Tree-style cost breakdowns: `├─ 平台服务费 X USDT  ├─ Gas 费用 X USDT  └─ 总计 X USDT`

## "举个 X USDT 的例子" action

Triggers: "举个 5 USDT 服务的例子" / "服务大概收多少" / "give me an example at 5 USDT" / "typical service charge".

→ MUST first run `onchainos agent search --query "<X> USDT"` (or a service-keyword query) to pull a real marketplace agent, then explain the cost using that agent's `fee` field:

- "Service fee = `<X> USDT` — 100% 归服务提供商，OKX 不抽成"
- "手续费（创建 / 调用 / 任何链上动作）= 0，由 OKX 承担"
- "用户支付总额 = service fee（无其他费用）"

⛔ Never improvise a cost breakdown. The marketplace has real data; use it.
