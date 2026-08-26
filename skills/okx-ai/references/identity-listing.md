# Identity listing — activate · deactivate

These pure state toggles are **card-exempt** — run the CLI directly, without a confirmation card or
field table; never chase a successful toggle with `agent get-agents`. Use the user's `#<id>`.

## deactivate

Run the `deactivate` form in `identity-cli-reference.md` directly with the user's `#N`. Read only
`success`.

- `success: true` → emit exactly ONE line (not a menu):
  `Unpublished — hidden from client lists. Say 'activate #<id>' to re-publish.`
  Do not re-query. Then run the communication-init flow in [`chat-comm-init.md`](chat-comm-init.md) to sync the agent-list change (deactivate has no CLI-level readiness gate).
- `success: false` / `code != 0` → load `identity-errors.md`.

## activate

Invoke the `activate` form in `identity-cli-reference.md` with the user's `#N` and locked language.

### Response — match in order

| Response shape | Action |
|---|---|
| `blockType: 1` + `agentRole` | Hard stop — not an ASP. Emit (localized): agent #`<N>` is a `<roleLabel>`; only ASP identities support listing. |
| `submitApproval.success: true` | Emit `Submitted for review.` **Stop.** No query or poll. |
| `submitApproval.success: false` | If its response matches `identity-errors.md`, follow that rule; otherwise emit `Failed to submit for listing review.` plus the raw line and `You can try again later.` **Stop.** |
| `activate.approvalStatus: 2` | Emit `Your agent is under review — usually ready within 24h; once approved it appears on the marketplace.` **Stop.** Do not submit again, query, or poll. |
| `activate.success: true` | Published. |
| `activate.success: false` (other) | Load `identity-errors.md`. |
