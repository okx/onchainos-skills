# Update flow — `update #N`

Loaded when: user wants to update an existing agent, or fix a rejected / QA-failed listing. Pairs with SKILL.md.

> **Rejected listing → update the same agent, never create new.** QA failure (§4 of register.md) or review rejection (`approvalStatus`/`approvalDisplayStatus: 5`): fix path is `agent update` on the existing id → re-activate. Never offer a new agent as remedy; only create if user explicitly insists after steer.

---

1. **`agent get --agent-ids <id>` FIRST — before collecting ANY change** → render the current detail card (§Invariants Verbatim-render contract). Never start editing from the user's words alone; always fetch current state first.
2. **Ownership check:** returned `ownerAddress` ≠ current wallet → STOP: "This agent doesn't belong to your current wallet."
3. **Collect changes** one field per turn. If the changed field is a service **endpoint**, apply the same rules as register.md §6 (must be `https://`, publicly reachable, ≤512 chars). **Do NOT run `validate-listing` while collecting** — QA is a single batch pass that happens in step 4 *after* all changes are gathered (never per-field, never per-service).
4. **QA — single batch pass, identical to register.md §4.** role = provider AND a QA-governed field changed → after ALL changes are collected, run `validate-listing` **exactly once** on the **complete resulting listing** (merge current + changes: `--role provider --name <new-or-current> --description <new-or-current> --service '[… the full rebuilt service array per step 6 …]'`). Then follow register.md §4 steps 2–4 verbatim: render the findings card as **suggestions only** (do not apply any `fix` yet) → ask the two-choice confirmation (1. apply suggested fixes / 2. I'll revise myself) → on confirm apply once / on self-revise collect new values → **never re-run `validate-listing`**. requester / evaluator skip QA. Resolved values then flow into the step-5 diff card.
5. **Update Diff card** (§Invariants diff variant — 3 columns `| Field | Current | New |`, unchanged → `(unchanged)`, changed New cell bold, real before→after values). Wait for **1** / 执行; no `agent update` before confirm.
6. **`--service` = WHOLESALE replacement:** rebuild the COMPLETE service list from current + diff; never send only the changed entry. **No-op guard:** if nothing changed → "No changes to submit." Don't call `agent update`; re-enter update Q&A. `--description ""` does NOT clear a description. Post-update:
   - `agent.approvalStatus == 2` (WS push payload, if the field is present) → "Update saved. Under review — once approved it will go live automatically. No further action needed." If the `agent` key or `approvalStatus` field is absent (WS push timed out or field not returned), fall through to the standard update success line per §10 update template.
   - step-1 detail showed `approvalDisplayStatus == 5` (not auto-resubmitted) → "Update saved — not yet resubmitted. Say 'activate #\<id\>' to send it for review."
   - else → "Update saved." → Step 6.
