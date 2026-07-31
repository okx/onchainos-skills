# OKX.AI Trading Hackathon — FAQ

> Scope: common questions about `hackathon register` that are not part of an active registration walkthrough (that flow is `references/registration.md`). Every answer here is grounded in the CLI/backend contract documented there. If a question isn't answered here or in `registration.md`, say so and defer to the backend's own error `msg` rather than guessing.

**Q: What does my Trading ASP agent need to qualify?**
A: Three preconditions, all enforced by the backend: (1) it must be a trading-type ASP, (2) it must offer a subscription service, and (3) it must offer a 3-day free trial. The skill asks the user to confirm these before submitting, but the backend is the authoritative check and rejects registration if any is not met.

**Q: Which chain and activity does this register against?**
A: X Layer (chain index `196`) and the current OKX.AI hackathon (activity id `5`). Both are fixed internally by the CLI/MCP tool — there is no flag or param to set or see them; they are not part of the CLI/MCP interface at all.

**Q: Do I need to fund my account before I register?**
A: No — the >300U-equivalent funding reminder is for before trading begins. `hackathon register` itself does not check or gate on balance.

**Q: What's the difference between `web3` and `cefi` account types?**
A: `web3` registers using your current wallet's X Layer address (auto-resolved, no UID needed). `cefi` additionally requires your OKX UID via `--uid`; the wallet's X Layer address is still auto-resolved and submitted alongside it. `--account-type` only accepts these two values.

**Q: Can I skip providing my wallet address?**
A: Yes for both account types — `--address` auto-resolves from your currently selected wallet account's X Layer (EVM) address when omitted. You only need to pass `--address` explicitly to override it.

**Q: My registration failed — what should I do?**
A: Depends on where it failed:
- A CLI-side validation error (e.g. missing `--uid` for `cefi`, or an address that doesn't validate for the chain) — fix the flag/value and retry.
- `not logged in` — run `onchainos wallet login`, then retry.
- A backend rejection (non-zero `code`) — the returned `msg` is the authoritative reason (commonly: ASP not trading-type, no subscription, or no 3-day trial). Surface that `msg` to the user verbatim; do not paraphrase or invent a different reason.

**Q: Can I register more than one agent, or list/update a registration afterward?**
A: `hackathon register` submits exactly one `--agent-id` per call, and there is no list/update/status subcommand in the current CLI or MCP surface. If a user asks for this, say it isn't supported today rather than guessing at a flow that doesn't exist.

**Q: What happens if I run `hackathon register` again for the same agent?**
A: The CLI does not track prior registration state client-side — it will submit the request again and the backend's response (success or a rejection `msg`) is authoritative on whether a duplicate registration is allowed.

**Q: I asked to join a competition/trading cup — is this the right skill?**
A: No. `hackathon register` is only for registering a Trading ASP agent for the OKX.AI hackathon. Joining a standard trading competition or cup is `competition join` in the `okx-growth-competition` skill — see the disambiguation table in `../SKILL.md`.
