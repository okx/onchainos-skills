# OKX.AI Trading Hackathon — FAQ

> Scope: common questions about `hackathon register` outside an active registration walkthrough (that flow is `references/registration.md`). Every answer here is grounded in the CLI/backend contract documented there. If a question isn't answered in either file, say so and defer to the backend's own error `msg` rather than guessing.

**Q: What does my Trading ASP agent need to qualify?**
A: Three preconditions, all enforced by the backend: (1) a trading-type ASP, (2) offers a subscription service, (3) offers a 3-day free trial. The skill asks the user to confirm these before submitting, but the backend is the authoritative check and rejects registration if any is not met.

**Q: Which chain does this register against?**
A: X Layer (chain index `196`), for the current OKX.AI Trading Hackathon. The chain and the hackathon's internal activity id are both fixed by the CLI/MCP tool — no flag or param sets or returns either. Refer to the hackathon by name, never by its internal id (`../SKILL.md` Output Rules).

**Q: Do I need to fund my account before I register?**
A: No — the >300U-equivalent funding reminder is for before trading begins. `hackathon register` itself does not check or gate on balance.

**Q: What's the difference between `web3` and `cefi` account types?**
A: `web3` registers using your current wallet's X Layer address, no UID needed. `cefi` additionally requires your OKX UID; the X Layer address is still submitted alongside it. `--account-type` accepts only these two values.

**Q: Can I skip providing my wallet address?**
A: Yes, for both account types — `--address` auto-resolves from your currently selected wallet account's X Layer (EVM) address. Pass it explicitly only to override.

**Q: My registration failed — what should I do?**
A: Depends on where it failed:
- CLI-side validation (missing `--uid` for `cefi`, an address that doesn't validate for the chain) — fix the flag/value and retry.
- `not logged in` — run `onchainos wallet login`, then retry.
- A backend rejection (non-zero `code`) — the returned `msg` is the authoritative reason (commonly: ASP not trading-type, no subscription, or no 3-day trial). Surface that `msg` verbatim; do not paraphrase or invent a different reason.

**Q: Can I register more than one agent, or list/update a registration afterward?**
A: `hackathon register` submits exactly one `--agent-id` per call, and there is no list/update/status subcommand in the current CLI or MCP surface. If asked, say it isn't supported today rather than guessing at a flow that doesn't exist.

**Q: What happens if I run `hackathon register` again for the same agent?**
A: The CLI does not track prior registration state client-side — it submits again, and the backend's response (success or a rejection `msg`) is authoritative on whether a duplicate is allowed.

**Q: I asked to join a competition/trading cup — is this the right skill?**
A: No. This skill only registers a Trading ASP agent for the OKX.AI hackathon; joining a standard trading competition or cup is `competition join` in `okx-growth-competition`.
