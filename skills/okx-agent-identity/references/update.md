# Update flow — `update #N`

Loaded when: user wants to update an existing agent, or fix a rejected / QA-failed listing. Pairs with SKILL.md.

> **Rejected listing → update the same agent, never create new.** QA failure (§4 of register.md) or review rejection (`approvalStatus`/`approvalDisplayStatus: 5`): fix path is `agent update` on the existing id → re-activate. Never offer a new agent as remedy; only create if user explicitly insists after steer.

---

1. **`agent get --agent-ids <id>` FIRST — before collecting ANY change** → render the current detail card (§Invariants Verbatim-render contract). Never start editing from the user's words alone; always fetch current state first.
2. **Ownership check:** returned `ownerAddress` ≠ current wallet → STOP: "This agent doesn't belong to your current wallet."
3. **Collect changes** one field per turn. If the changed field is a service **endpoint**, apply the same rules as register.md §6 (must be `https://`, publicly reachable, ≤512 chars).
4. **QA on changed provider fields:** role = provider AND a QA-governed field changed → run `validate-listing` on the changed fields only; render findings inline (register.md §4 step 2). requester / evaluator skip QA.
5. **Update Diff card** (§Invariants diff variant — 3 columns `| Field | Current | New |`, unchanged → `(unchanged)`, changed New cell bold, real before→after values). Wait for **1** / 执行; no `agent update` before confirm.
6. **`--service` = WHOLESALE replacement:** rebuild the COMPLETE service list from current + diff; never send only the changed entry. **No-op guard:** if nothing changed → "No changes to submit." Don't call `agent update`; re-enter update Q&A. `--description ""` does NOT clear a description. Post-update:
   - `agent.approvalStatus == 2` (WS push payload, if the field is present) → "Update saved. Under review — once approved it will go live automatically. No further action needed." If the `agent` key or `approvalStatus` field is absent (WS push timed out or field not returned), fall through to the standard update success line per §10 update template.
   - step-1 detail showed `approvalDisplayStatus == 5` (not auto-resubmitted) → "Update saved — not yet resubmitted. Say 'activate #\<id\>' to send it for review."
   - else → "Update saved." → Step 6.
