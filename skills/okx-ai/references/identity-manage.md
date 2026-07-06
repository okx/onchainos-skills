# Manage — activate · deactivate

Loaded when: user wants to publish (activate) or unpublish (deactivate) an agent `#N`.

These are pure state toggles. Per SKILL §Gates Confirm, toggles are **card-exempt** — run the
CLI directly, no confirmation card, no field-table. Per SKILL §Gates No-poll, never chase a
successful toggle with `agent get-agents`. Both successful toggles continue by running SKILL.md §Step 5/6 Step 6 as a required communication subflow. Resolve
`#<id>` per the SKILL §Invariants #id ladder.

## deactivate

Run directly with the user's `#N`. Read only `success`.

```bash
# internal — not shown to the user
onchainos agent deactivate --agent-id <N>
```

- `success: true` → emit exactly ONE line (not a menu):
  `Unpublished — hidden from client lists. Say 'activate #<id>' to re-publish.`
  Then → run SKILL.md §Step 5/6 Step 6 as a required communication subflow. Do not re-query.
- `success: false` / `code != 0` → load `identity-errors.md`.

## activate

```bash
# internal — not shown to the user
onchainos agent activate --agent-id <N> --preferred-language <BCP-47>
```

### Response — match in order

| Response shape | Action |
|---|---|
| `blockType: 1` + `agentRole` | Hard stop — not an ASP. Emit (localized): agent #`<N>` is a `<roleLabel>`; only ASP identities support listing. |
| `activate` + `submitApproval` | Submitted for review → run SKILL.md §Step 5/6 Step 6 as a required communication subflow. |
| `activate.success: true` | Published → run SKILL.md §Step 5/6 Step 6 as a required communication subflow. |
| `activate.approvalStatus: 2` | Already under review. Stop, no Step 6, no poll. |
| `activate.success: false` (other) | Load `identity-errors.md`. |
