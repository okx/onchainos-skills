# ASP (Agent Service Provider) Actions

This file only covers the content **specific** to the ASP role. Generic rules (envelope shapes / tool usage / anti-hallucination / push-to-user-session opt-in / communication boundary) all live in `SKILL.md`.

> **Fully gas-free**: every on-chain action by the ASP (`apply` / `deliver` / arbitration / refund / claim, etc.) goes through the platform's paymaster, so **the user's wallet never needs any gas / native balance**. **Do not** prompt the user to "prepare gas / reserve gas / check balance", and **do not** factor gas reserves into any amount suggestion.

The task state machine has moved into the CLI (`onchainos agent next-action`) — **you do not need to memorize the steps for every status**. On any system event (chain event / user-decision relay from the user session), call `next-action` and execute its output.

---

## Peer Message Routing (ASP sub session)

When the ASP sub session receives a peer message from the User Agent (`sender.role == 1`), match by `[intent:*]` marker:

| Marker | Action |
|---|---|
| `[intent:attachment]` | Extract all 6 encryption fields from the message and pass them in `--message`: `next-action --role provider --agentId <yours> --message '{"event":"buyer_attachment_received","jobId":"<jobId>","fileKey":"<fileKey>","digest":"<digest>","salt":"<salt>","nonce":"<nonce>","secret":"<secret>","filename":"<filename>"}'` → CLI downloads + saves in-process; follow the returned playbook. |
| `[intent:confirm]` | The User Agent confirmed acceptance → `next-action --role provider --agentId <yours> --message '{"event":"negotiate_confirmed","jobId":"<jobId>"}'` |
| Other / no marker | Negotiation or execution discussion → `next-action --role provider --agentId <yours> --message '{"event":"negotiate_reply","jobId":"<jobId>"}'` |

> 🛑 **`[intent:attachment]` fields**: all 6 fields (`fileKey`, `digest`, `salt`, `nonce`, `secret`, `filename`) are REQUIRED in the `--message` JSON. Copy each value in FULL from the inbound message — do NOT truncate or abbreviate. Missing fields → CLI falls back to the slower LLM playbook.

