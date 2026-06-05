# Cost Disclosure (P0)

Fires whenever the user asks about fees / gas / commission / "will I be charged".

Source of truth: OKX Agent platform. Never derive from training data.

## Phase-1 gas policy

**All on-chain actions: OKX fully covers the network transaction fees — the user's wallet is not charged a cent:**

| Operation | Fee |
|---|---|
| Create agent / mint NFT (`agent create`) | ✅ Covered by OKX |
| Edit agent fields (`agent update`) | ✅ Covered by OKX |
| Activate / deactivate (`activate` / `deactivate`) | ✅ Covered by OKX (deactivate is not on-chain) |
| Feedback (`agent feedback-submit`) | ✅ Covered by OKX |

User Agents paying service fees go through `okx-agent-task` settlement — out of scope here.

## Platform commission

**Zero platform fee.** The ASP sets the `service fee` and keeps 100%. OKX takes no cut.

## Standard line

Quote at least once per session, ideally before the first agent-creating mutation:

> "**OKX covers all transaction fees on your behalf (the cost of doing things on the blockchain), so your wallet is not charged a cent. OnchainOS Agentic Wallet signs the transaction for you — your wallet stays untouched throughout.**"

## Forbidden phrasings

- ❌ "The doc doesn't explicitly say what gas costs" / "not specified" / "not covered"
- ❌ "You'll only see an accurate gas estimate at actual creation time"
- ❌ "Check the official docs / contact OKX support / look it up on the XLayer block explorer"
- ❌ Fabricated fee categories: "Platform service fee X USDT" / "Dispatch fee" / "Management fee" / "Execution fee"
- ❌ Soft-hallucination wrappers: "Hypothetical example / my guess / actual values may differ / this is just an example"
- ❌ Tree-style cost breakdowns: `├─ Platform service fee X USDT  ├─ Gas fee X USDT  └─ Total X USDT`

## "Give me an example at X USDT" action

Triggers: "give me an example at 5 USDT" / "typical service charge".

→ MUST first run `onchainos agent search --query "<X> USDT"` (or a service-keyword query) to pull a real marketplace agent, then explain the cost using that agent's `fee` field:

- "Service fee = `<X> USDT` — 100% goes to the service provider, OKX takes no cut"
- "Transaction fees (create / call / any on-chain action) = 0, covered by OKX"
- "Total user payment = service fee (no other fees)"

⛔ Never improvise a cost breakdown. The marketplace has real data; use it.
