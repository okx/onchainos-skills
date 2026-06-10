# Manage — activate · deactivate

Loaded when: user wants to publish (activate) or unpublish (deactivate) an agent `#N`.

These are pure state toggles. Per SKILL §Gates Confirm, toggles are **card-exempt** — run the
CLI directly, no confirmation card, no field-table. Per SKILL §Gates No-poll, never chase a
successful toggle with `agent get`. Both successful toggles continue per SKILL §Step 5/6. Resolve
`#<id>` per the SKILL §Invariants #id ladder; keep no skill name and no `onchainos` literal in any
user-visible line (SKILL §UX Red Lines 1).

## deactivate [eval 20]

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

Run directly with the user's `#N`. Read `success` + `approvalStatus` + top-level `code`.

```bash
# internal — not shown to the user
onchainos agent activate --agent-id <N>
```

Branch on the response:

- **`success: true`** → `Published — discoverable on the marketplace.` → Step 6.
- **`success: false` + `approvalStatus` `1` OR `5`** → listing needs provider QA. First **load the
  live listing** so QA has real values to check: run `agent get --agent-ids <N>` (name / description)
  and `agent service-list --agent-id <N>` (the service array). This re-listing fetch is an allowed
  multi-call exception, not a poll. Then run `validate-listing` (provider scope) on those values:
  - QA **pass** → run `agent submit-approval --agent-id <N> --preferred-language <lang>` automatically
    (skill-internal, never shown to the user, per SKILL §Commands) → then Step 6. `<lang>` = the
    conversation language as a BCP-47 tag (e.g. `zh-CN` / `en-US`) so backend review messages match the
    user's language; omit the flag if the language is unclear (backend then uses its default).
  - QA **fail** → report the findings inline (per SKILL §Invariants Verbatim-render contract) and **stop**.
    Do not submit.
  - For `approvalStatus 5`, treat it identically to `1`: do **not** surface a rejection, a
    rejection card, or `rejectReason` — proceed silently to the QA + submit path.

  ```bash
  # internal — not shown to the user
  onchainos agent get --agent-ids <N>            # fetch name / description
  onchainos agent service-list --agent-id <N>    # fetch the service array
  onchainos agent validate-listing --role provider --name "<name>" --description "<desc>" --service '[…]'
  onchainos agent submit-approval --agent-id <N> --preferred-language <conversation BCP-47, e.g. zh-CN>
  ```
- **`success: false` + `approvalStatus` `2`** → `Under review — your listing is being checked;
  you'll be discoverable once it's approved.` Stop. No Step 6, no poll.
- **top-level `code: "81602"`** → load `references/errors.md` and stop (use its softened "blocked by the platform" wording — don't echo internal labels).
- **any other `success: false`** (whitelist `10016`, region `50125`/`80001`, or an unrecognized code) → load `references/errors.md` and match the row there; don't interpret it generically.
