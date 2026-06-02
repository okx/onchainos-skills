# Cost Disclosure (P0)

Fires whenever the user asks about fees / gas / commission / "will I be charged".

Source of truth: OKX Agent platform PRD §1.7 / §F0.7. Never derive from training data.

## Phase-1 gas policy

**All on-chain actions: OKX fully covers the network transaction fees — the user's wallet is not charged a cent:**

| Operation | Fee |
|---|---|
| Create agent / mint NFT (`agent create`) | No charge |
| Edit agent fields (`agent update`) | No charge |
| Activate / deactivate (`activate` / `deactivate`) | No charge |
| Feedback (`agent feedback-submit`) | No charge |

User Agents paying service fees go through `okx-agent-task` settlement — out of scope here.

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

- "Service fee = `<X> USDT`"
- "Transaction fees (create / call / any on-chain action) = 0"
- "Total user payment = service fee (no other fees)"

⛔ Never improvise a cost breakdown. The marketplace has real data; use it.
