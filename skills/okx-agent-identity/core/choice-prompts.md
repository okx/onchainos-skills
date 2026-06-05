# Choice Prompts (numbered options)

Use whenever the user must pick from a **bounded set of 2–5 options**. Open-ended fields (Name, Description, Fee amount, feedback text) stay free-text. The concrete prompts for each scenario are written out verbatim in the relevant playbook files — this file defines the rules and guards.

## Rules

- **Also accept canonical spelling** as fallback: if user replies `A2MCP` instead of `1`, accept it. Primary ask is numeric.
- **Map the number before sending to the CLI.** `--role` accepts `1`/`2`/`3` aliases (`utils.rs:162-165`). `servicetype` and others do NOT — skill must translate `1→A2MCP`, `2→A2A` locally before CLI invocation.
- **Batch-first, one-at-a-time fallback.** Render the phase overview as an explicit invitation to send everything at once (see §One-Shot Capture), capture whatever the user batches, then ask only the **remaining** fields — one field per *asking* turn. Never list multiple fields as an imperative ask in a single message.
- **Never use numbered options for open-ended fields.** Name, description, fee, feedback text are free-form.
- **Never force a menu for "what's next".** Post-success suggestions are always one declarative line, never a menu.
- If user replies outside the enumeration (`whatever` / `any is fine`), politely re-ask the numbered list once; never silently pick a default.

## When to use this pattern

Use numbered-options for: role selection on create, arbitrator intent disambiguation, existing provider pre-check (new vs update), servicetype choice (A2MCP vs A2A), avatar upload path selection, which agent to use as `--creator-id`, and terms consent (agree / decline). (The "add another service?" loop is **no longer a routine prompt** — providers default to a single service; see `playbooks/provider-services.md`.)

---

## One-Shot Capture

The **encouraged fast path** for users who give everything at once (e.g. "register a provider called Alice, description is DeFi research, default avatar, one API service named TVL Query, 10 USDT, https://…").

### Rules

1. **Encouraged via the overview.** The phase overview explicitly invites the user to "send it all in one message" (see `playbooks/provider.md §Collection overview`). When they do, capture everything in one parse. When they don't, fall back to asking the remaining fields one at a time.
2. **Capture only unambiguous values.** If the split is ambiguous ("Alice doing DeFi analysis" — is the name `Alice` or `Alice doing DeFi analysis`?), capture only the clearly-unambiguous part; leave the ambiguous field for a normal one-at-a-time ask.
3. **Skip captured fields silently.** If a field is already captured, do not re-ask and do not echo "name is already Alice". The confirmation card shows everything at the end.
4. **Batch across identity + service fields.** A single overview reply MAY carry both identity and service fields — capture all of them (service fields are **no longer discarded**). Still confirm choice fields (`servicetype`) explicitly — never infer the type from the service name. If the user batches multiple services, capture each.
4a. **Complex fields are validation-gated, not blindly captured.** The structured / high-risk fields — service `servicedescription` (must follow the 3-part structure) and `endpoint` (https + public + on-chain-permanent, anti-pattern blacklist) — are captured from a batch **only when the batched value passes validation**. When it fails, **peel that single field into its own focused follow-up step** (with the structure template / endpoint requirements inline) rather than accepting a bad value or deferring the failure to the confirmation card. Simple fields (`name` / `servicetype` / `fee`) never trigger a split. See `playbooks/provider-services.md §Field complexity tiers`.
5. **All fields captured → still render confirmation card.** Even if every required field was covered in one shot, the confirmation card is mandatory. Wait for explicit `execute` / `yes` before calling the CLI.
6. **Confirmation-step ambiguity.** If any captured value was edge-case (whitespace, punctuation), show it verbatim and let the user reject during confirmation. Do not "clean up" silently.
7. **One-shot + numbered choice combo.** If the utterance includes a choice field (e.g. "Type: A2MCP"), accept it. When asking a choice field the user hasn't answered yet, still use the numbered-options pattern.

### Worked examples

- **A — partial, requester:** "register a buyer called Alice" → captures `role=requester`, `name=Alice`. Ask only the remaining picture field (default unless raised) → confirmation.
- **B — full, requester:** "register a buyer, name Alice, no avatar" → all fields covered → confirmation card directly.
- **C — ambiguous split:** "provider called Alice doing DeFi analysis" → captures `role=provider` only; name + description left for one-at-a-time asks.
- **D — full provider batch:** "provider Alice, DeFi research, one API service TVL Query, query protocol TVL, 10 USDT, https://api.alice.xyz/mcp" → captures identity (`name`/`description`) **and** the service (`name`/`servicedescription`/`servicetype=A2MCP`/`fee=10`/`endpoint`) in one parse → straight to confirmation card. Avatar defaults; user can change it on the card.
