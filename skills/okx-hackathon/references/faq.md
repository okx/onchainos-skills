# OKX.AI Trading Hackathon — FAQ

> Scope: common questions about `hackathon register` outside an active registration walkthrough (that flow is `registration.md`). Answers stay short and point at `registration.md` for the mechanics — that file is the single source for flags, templates, and error handling. If a question is answered in neither file, say so and defer to the backend's own error message rather than guessing.

**Q: What does my Trading ASP agent need to qualify?**
A: Three preconditions, all enforced by the backend: (1) a trading-type ASP, (2) offers a subscription service, (3) offers a 3-day free trial. The flow asks the user to confirm these before submitting, but the backend is the authoritative check and rejects registration if any is not met.

**Q: Which chain does this register against?**
A: X Layer, fixed by the CLI/MCP tool — no flag sets or returns it, and neither does the hackathon's internal activity id. Refer to the hackathon by name, never by that id (`../SKILL.md` Output Rules).

**Q: Do I need to fund my account before I register?**
A: No — the >300U-equivalent funding reminder is for before trading begins. `hackathon register` itself does not check or gate on balance.

**Q: What's the difference between the `web3` and `cefi` account types?**
A: `web3` registers with your current wallet's X Layer address and needs no UID; `cefi` additionally requires your OKX UID. `registration.md` Step 2 covers how each is collected and submitted.

**Q: What happens to my OKX UID?**
A: It is submitted with the registration and nothing else — not returned in the result, not printed to the terminal, and fully redacted in the local audit log.

**Q: Can I skip providing my wallet address?**
A: Yes, for both account types — it auto-resolves from your currently selected wallet account's X Layer (EVM) address. Pass it explicitly only to override.

**Q: My registration failed — what should I do?**
A: Read the `errorCode`, not the wording of the message:
- `hackathon_registration_rejected` — the backend evaluated the ASP and refused it. The returned `error` is the authoritative reason: translate it into the user's language and show it, keeping the same condition and required action. Do not soften it, generalise it, or substitute a guess for it.
- `hackathon_service_unavailable` — the request never reached the registration logic (connection error, timeout, 5xx, an error page). Nothing about the ASP was evaluated, so **NEVER** report an eligibility problem here — that sends the user off to "fix" a perfectly valid ASP. Retry shortly.
- Anything else is CLI-side validation — the error list at the end of `registration.md` says what to fix and retry.

**Q: The registration keeps getting rejected and the message mentions the activity, not my ASP.**
A: This CLI build is pinned to one specific hackathon. If the backend says the activity is missing, closed, or ended, that hackathon is over — upgrading `onchainos` picks up whatever the current activity is. It is not a problem with the ASP, and there is no flag to point the command at a different activity.

**Q: Can I register more than one agent, or list/update a registration afterward?**
A: `hackathon register` submits exactly one `--agent-id` per call, and there is no list/update/status subcommand in the current CLI or MCP surface. If asked, say it isn't supported today rather than guessing at a flow that doesn't exist.

**Q: What happens if I run `hackathon register` again for the same agent?**
A: The CLI does not track prior registration state client-side — it submits again, and the backend's response (success or a rejection message) is authoritative on whether a duplicate is allowed.

**Q: I asked to join a competition/trading cup — is this the right skill?**
A: No. This skill only enters an existing Trading ASP in the OKX.AI hackathon; joining a standard trading competition or cup is `competition join` in `okx-growth-competition`.
