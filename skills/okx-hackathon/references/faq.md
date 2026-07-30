# OKX.AI Trading Hackathon — FAQ

> Scope: common questions about `hackathon register` outside an active registration walkthrough (that flow is `references/registration.md`). Every answer here is grounded in the CLI/backend contract documented there. If a question isn't answered in either file, say so and defer to the backend's own error `msg` rather than guessing.

**Q: What does my Trading ASP agent need to qualify?**
A: Three preconditions, all enforced by the backend: (1) a trading-type ASP, (2) offers a subscription service, (3) offers a 3-day free trial. The skill asks the user to confirm these before submitting, but the backend is the authoritative check and rejects registration if any is not met.

**Q: Which chain does this register against?**
A: X Layer (chain index `196`), for the current OKX.AI Trading Hackathon. The chain and the hackathon's internal activity id are both fixed by the CLI/MCP tool — no flag or param sets or returns either. Refer to the hackathon by name, never by its internal id (`../SKILL.md` Output Rules).

**Q: Do I need to fund my account before I register?**
A: No — the >300U-equivalent funding reminder is for before trading begins. `hackathon register` itself does not check or gate on balance.

**Q: What's the difference between `web3` and `cefi` account types?**
A: `web3` registers using your current wallet's X Layer address, no UID needed. `cefi` additionally requires your OKX UID; the X Layer address is still submitted alongside it. `--account-type` accepts only these two values, lowercase and exactly spelled — a differently cased `CeFi` is rejected outright on both the CLI and the MCP tool rather than quietly falling back to `web3`.

**Q: What happens to my OKX UID?**
A: It is submitted with the registration and nothing else. It is not returned in the result, not printed to the terminal, and fully redacted in the local audit log. Passing `--uid` on a `web3` registration is rejected rather than silently ignored, so a UID can never end up attached to the wrong account type.

**Q: Can I skip providing my wallet address?**
A: Yes, for both account types — `--address` auto-resolves from your currently selected wallet account's X Layer (EVM) address. Pass it explicitly only to override.

**Q: My registration failed — what should I do?**
A: Read the `errorCode`, not the wording of the message:
- `hackathon_registration_rejected` — the backend evaluated the ASP and refused it. The returned `error` is the authoritative reason; surface it verbatim and do not paraphrase, translate, or substitute a guess for it.
- `hackathon_service_unavailable` — the request never reached the registration logic (connection error, timeout, 5xx, an error page). Nothing about the ASP was evaluated, so do **not** report an eligibility problem. Retry shortly.
- No `errorCode`, or `invalid_input` — CLI-side validation (missing `--uid` for `cefi`, `--uid` passed on a `web3` registration, a mis-cased account type, an address that doesn't validate, a blank/mangled `--agent-id`). Fix the flag and retry.
- `not logged in` — run `onchainos wallet login`, then retry.

**Q: The registration keeps getting rejected and the message mentions the activity, not my ASP.**
A: This CLI build is pinned to one specific hackathon. If the backend says the activity is missing, closed, or ended, that hackathon is over — upgrading `onchainos` picks up whatever the current activity is. It is not a problem with the ASP, and there is no flag to point the command at a different activity.

**Q: Can I register more than one agent, or list/update a registration afterward?**
A: `hackathon register` submits exactly one `--agent-id` per call, and there is no list/update/status subcommand in the current CLI or MCP surface. If asked, say it isn't supported today rather than guessing at a flow that doesn't exist.

**Q: What happens if I run `hackathon register` again for the same agent?**
A: The CLI does not track prior registration state client-side — it submits again, and the backend's response (success or a rejection `msg`) is authoritative on whether a duplicate is allowed.

**Q: I asked to join a competition/trading cup — is this the right skill?**
A: No. This skill only enters an existing Trading ASP in the OKX.AI hackathon; joining a standard trading competition or cup is `competition join` in `okx-growth-competition`.
