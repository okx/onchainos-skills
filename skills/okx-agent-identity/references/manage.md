# Manage — activate · deactivate

Loaded when: user wants to publish (activate) or unpublish (deactivate) an agent `#N`.

These are pure state toggles. Per SKILL §Gates Confirm, toggles are **card-exempt** — run the
CLI directly, no confirmation card, no field-table. Per SKILL §Gates No-poll, never chase a
successful toggle with `agent get-agents`. Both successful toggles continue per SKILL §Step 5/6. Resolve
`#<id>` per the SKILL §Invariants #id ladder; keep no skill name and no `onchainos` literal in any
user-visible line (SKILL §UX Red Lines 1).

## deactivate

Run directly with the user's `#N`. Read only `success`.

```bash
# internal — not shown to the user
onchainos agent deactivate --agent-id <N>
```

- `success: true` → emit exactly ONE line (not a menu):
  `Unpublished — hidden from client lists. Say 'activate #<id>' to re-publish.`
  Then → Step 6 (per SKILL §Step 5/6). Do not re-query.
- `success: false` / `code != 0` → load `references/errors.md`.

## activate

```bash
# internal — not shown to the user
onchainos agent activate --agent-id <N> --preferred-language <BCP-47>
```

### Response — match in order

| Response shape | Action |
|---|---|
| `blockType: 1` + `agentRole` | Hard stop — not a provider. Emit (localized): agent #`<N>` is a `<roleLabel>`; only ASP (provider) identities support listing. |
| `activate` + `submitApproval` | Submitted for review → Step 6. |
| `activate.success: true` | Published → Step 6. |
| `activate.approvalStatus: 2` | Already under review. Stop, no Step 6, no poll. |
| `activate.success: false` (other) | Load `references/errors.md`. |
