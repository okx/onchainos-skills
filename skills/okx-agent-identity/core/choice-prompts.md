# Choice Prompts (numbered options)

Use whenever the user must pick from a **bounded set of 2–5 options**. Open-ended fields (Name, Description, Fee amount, feedback text) stay free-text. The concrete prompts for each scenario are written out verbatim in the relevant playbook files — this file defines the rules and guards.

## Rules

- **Also accept canonical spelling** as fallback: if user replies `A2MCP` instead of `1`, accept it. Primary ask is numeric.
- **Map the number before sending to the CLI.** `--role` accepts `1`/`2`/`3` aliases (`utils.rs:162-165`). `servicetype` and others do NOT — skill must translate `1→A2MCP`, `2→A2A` locally before CLI invocation.
- **One question per turn.** Never batch-ask multiple fields in one message.
- **Never use numbered options for open-ended fields.** Name, description, fee, feedback text are free-form.
- **Never force a menu for "what's next".** Post-success suggestions are always one declarative line, never a menu.
- If user replies outside the enumeration (`whatever` / `any is fine`), politely re-ask the numbered list once; never silently pick a default.

## When to use this pattern

Use numbered-options for: role selection on create, arbitrator intent disambiguation, existing provider pre-check (new vs update), servicetype choice (A2MCP vs A2A), "add another service?" loop, avatar upload path selection, which agent to use as `--creator-id`, and terms consent (agree / decline).

---

## One-Shot Capture

Silent support for users who dump everything at once (e.g. "register a provider called Alice, description is DeFi research, use default avatar").

### Rules

1. **Silent, not advertised.** Never say "you can also enter everything at once". One-shot is a fast path users discover naturally; the step-by-step Q&A remains the default surface.
2. **Capture only unambiguous values.** If the split is ambiguous ("Alice doing DeFi analysis" — is the name `Alice` or `Alice doing DeFi analysis`?), capture only the clearly-unambiguous part; leave the ambiguous field for the normal Q.
3. **Skip answered Q's silently.** If Q_k's field is already captured, skip Q_k without echoing "name is already Alice". The confirmation card shows everything at the end.
4. **Phase boundary is strict.** Identity-phase capture does NOT reach into service-phase fields. "provider called Alice, charges 10 USDT" → capture `name=Alice`, discard `fee=10`. When Phase 2 starts, MAY quote the earlier mention as a suggested default (not auto-fill).
5. **All fields captured → still render confirmation card.** Even if every required field was covered in one shot, the confirmation card is mandatory. Wait for explicit `execute` / `yes` before calling the CLI.
6. **Confirmation-step ambiguity.** If any captured value was edge-case (whitespace, punctuation), show it verbatim and let the user reject during confirmation. Do not "clean up" silently.
7. **One-shot + numbered choice combo.** If the one-shot utterance includes a choice field (e.g. "Type: A2MCP"), accept it. When asking a choice Q the user hasn't answered yet, still use the numbered-options pattern.

### Worked examples

- **A — partial, requester:** "register a buyer called Alice" → captures `role=requester`, `name=Alice`. Skip Q1 → Q2 (picture) → confirmation.
- **B — full, requester:** "register a buyer, name Alice, no avatar" → All Q's skipped → confirmation card directly.
- **C — ambiguous split:** "provider called Alice doing DeFi analysis" → captures `role=provider` only; name + description left for normal Q&A.
- **D — cross-phase leakage (strict rejection):** "provider called Alice, doing DeFi analysis, charges 10 USDT" → Phase-1 capture: `name=Alice`, `description=doing DeFi analysis`. **Fee=10 is discarded.** Phase 2 starts fresh with its own Q1.
