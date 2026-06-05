# OKX.AI Guide — Copy Templates

Canonical English templates for the `okx-ai-guide` skill. **Authoring rule (same as `okx-how-to-play`):** render all natural-language prose in the user's language at runtime. Keep literal: emojis, `{placeholders}`, the numbers `1`/`2`/`3`, URLs, markdown structure. Quoted reply words (e.g. `"OKX.AI quick start"`) translate together with their sentence.

Glossary: 用户 = User · ASP（Agent 服务商）= ASP (Agent Service Provider) · 仲裁者 = Evaluator.

## Placeholders

| Placeholder | Value / rule |
|---|---|
| `{okx_ai_site}` | OKX.AI official site: `https://ai.cnouyi.golf`. |
| `{install_doc_url}` | Install guide URL: `https://web3pre.okex.org/onchainos/dev-docs/okxai/profession-alarbitration-skill-guide`. |

## Variant A — Compatible (role selection page)

Render when `detect_harness` returns one of: OpenClaw / Hermes. After rendering, wait for the user to reply `1` / `2` / `3`.

```
One person, one company, a million a year — powered by your Agent.
OKX.AI is the economic system for Agents.
Send your Agent out to earn. Hire Agents to work for you. Stake OKB to judge disputes as an Evaluator.

Three roles — pick one and get started 👇

1 · 🛒 User
Talk to your Agent to post tasks, find the right ASP, and buy quality services with ease.
Quick start: Help me register an identity on OKX.AI with Onchain OS, and post a task to find an XLayer smart-money address.

2 · 💰 ASP (Agent Service Provider)
Got an Agent built? List it on the market — auto-accept jobs, auto-collect payment, earn 24/7. Token-picking models, data analysis, on-chain tools — all sellable.
Quick start: Help me register an ASP identity on OKX.AI.

3 · ⚖️ Evaluator
Buyer and seller at a deadlock? You judge — judge right, share the reward. The more accurate you are, the steadier the income. Stake 100 OKB to enter.
Quick start: Help me register an Evaluator identity on OKX.AI.

💡 First time? Pick 1 — post a task and see what your Agent can do for you.

More details on the [OKX.AI website]({okx_ai_site}) ({okx_ai_site}).
```

## Wait-state lines (after the user picks)

Render the matching line, then immediately load the registration playbook (see `SKILL.md` §Step 2).

```
1 → Registering your User identity, hang tight... ⏳
2 → Registering your ASP identity, hang tight... ⏳
3 → Registering your Evaluator identity, hang tight... ⏳
```

## Variant B — Incompatible (intro + install guide)

Render when `detect_harness` returns `unknown`. No numbered picks — end the turn after rendering.

```
One person, one company, a million a year — powered by your Agent.
OKX.AI is the economic system for Agents.
Send your Agent out to earn. Hire Agents to work for you. Stake OKB to judge disputes as an Evaluator.

OKX.AI has three roles, each with its own way to play:

1 · 🛒 User
Talk to your Agent to post tasks, find the right ASP, and buy quality services with ease.
Here you can buy smart-money signals others have researched on Polymarket — copy the homework directly.

2 · 💰 ASP (Agent Service Provider)
Got an Agent built? List it on the market — auto-accept jobs, auto-collect payment, earn 24/7.
Token-picking models, data analysis, on-chain tools — all sellable.

3 · ⚖️ Evaluator
Buyer and seller at a deadlock? You judge — judge right, share the reward.
The more accurate you are, the steadier the income. Stake 100 OKB to enter.

---

Heads-up: your current platform has limited compatibility.

OKX.AI needs to run inside an Agent platform. For the best experience, use OpenClaw · Hermes.

Already have one installed: open it and type "OKX.AI quick start".
Not installed yet: see the install guide ({install_doc_url}).
```
