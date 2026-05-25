# Choice Prompts (numbered options)

Use whenever the user must pick from a **bounded set of 2–5 options**. Open-ended fields (Name, Description, Fee amount, feedback text) stay free-text.

## Templates

**Chinese:**
```
<一句话提问>
  1. <选项 1 的标签> — <一行解释，可选>
  2. <选项 2 的标签> — <一行解释，可选>
  3. <选项 3 的标签> — <一行解释，可选>
回复数字 1/2/3。
```

**English:**
```
<One-line question>
  1. <Option 1 label> — <one-line explanation, optional>
  2. <Option 2 label> — <one-line explanation, optional>
  3. <Option 3 label> — <one-line explanation, optional>
Reply with a number: 1/2/3.
```

## Rules

- **Also accept canonical spelling** as fallback: if user replies `A2MCP` instead of `1`, accept it. Primary ask is numeric.
- **Map the number before sending to the CLI.** `--role` accepts `1`/`2`/`3` aliases (`utils.rs:162-165`). `servicetype` and others do NOT — skill must translate `1→A2MCP`, `2→A2A` locally before CLI invocation.
- **One question per turn.** See `_shared/no-polling.md` and `role-playbook.md` one-question rule.
- **Never use numbered options for open-ended fields.** Name, description, fee, feedback text are free-form.
- **Never force a menu for "what's next".** Post-success suggestions (§8 of `display-formats.md`) are always one line, never a menu.
- If user replies outside the enumeration (`都可以` / `随便`), politely re-ask the numbered list once; never silently pick a default.

## Usage map

| Scenario | Location |
|---|---|
| Role selection on `create` | `SKILL.md §Core Flow: agent create` gate 1 |
| Arbitrator intent disambiguation | `SKILL.md §Negative Triggers` |
| Existing provider pre-check (new vs update) | `references/role-playbook.md §Pre-check` |
| servicetype (A2MCP vs A2A) | `references/role-provider.md` Phase 2 S3 |
| "Add another service?" loop gate | `references/role-provider.md` Phase 2 S6 |
| Avatar upload path | `references/avatar-upload.md §Policy` |
| Which agent to use as `--creator-id` | `references/feedback-guide.md` Step 2 |
| Terms consent (agree / decline) | `references/consent-guide.md` Step 2 |
