# Agent guidance

This file is the canonical development guide for all coding agents in this
repository. `CLAUDE.md` only adds Claude Code specifics on top of it.

## Development flow

Follow these steps for every development change.

### 1. Align the requirement

Before treating a requirement as clear, explain the supplied requirement or
technical documents to the user: the goal, scope, and expected behavior. Use
`AskUserQuestion` to obtain the user's confirmation. Then compare that
confirmed understanding with the code. If the current behavior and the expected
behavior differ or are ambiguous, use `AskUserQuestion` to align with the user.
Do not start development until the user confirms the requirement.

### 2. Read the applicable development rules

After the requirement is clear, classify the change and finish the matching
pre-read below before creating or modifying a file.

| Change type | Required first step |
| --- | --- |
| Skill-only | Read `rules/skill-rules.md`, then inspect `templates/` and open the template that matches the target file type |
| CLI-only | Read `rules/cli-io-rules.md` |
| Mixed Skill + CLI | Read both sets of rules, and inspect `templates/` for the Skill files |
| Guidance-only (`AGENTS.md`, `CLAUDE.md`, `rules/`) | Read the source rules that the guidance describes, then change only the guidance files |

If you are unsure which row applies, treat the change as mixed.

### 3. Develop and verify

Implement the clear requirement. Repair only unit or integration tests that are
directly related to it. Do not fix unrelated test failures as part of the
change.

### 4. Independently review the change

Start an independent sub-agent code review after development. The reviewer must
apply the CLI and Skill rules in this file to the changed scope.

## Development domains

This repository has two development domains. A change can belong to one of them
or to both.

| Domain | What it covers | Rules to read first |
| --- | --- | --- |
| Skill development | `SKILL.md`, `references/**`, `_shared/**`, routing tables, Skill rules (including how Skills describe CLI commands), and the page templates in [`templates/`](templates/) | [`rules/skill-rules.md`](rules/skill-rules.md) and [`templates/README.md`](templates/README.md) |
| CLI development | CLI behavior under `cli/` and the CLI input/output rules (readable flag names, shared parsing helpers, JSON output) | [`rules/cli-io-rules.md`](rules/cli-io-rules.md) |

## Skill development

When you create or modify a Skill file (`SKILL.md`, a Router / Business-Policy /
CLI / Troubleshooting / Shared Reference, or a WebSocket Protocol Reference),
work template-first:

1. Open the relevant template under [`templates/`](templates/) before you write.
2. Choose the closest template for the target file type, using the page-type
   table in [`templates/README.md`](templates/README.md).
3. Keep the structure and the responsibilities that the template defines.
4. Do not invent a new structure when a template exists. If no template fits,
   say so in your change description instead of improvising.
5. Write the text by the [Writing Styles](#writing-styles) below.

If `rules/skill-rules.md` and a template differ on page structure, the template wins.

## CLI development

CLI-related work is governed by the CLI input/output rules in
[`rules/cli-io-rules.md`](rules/cli-io-rules.md). Read them before you change CLI
flags, input validation, or command output, and before you document any of these
in a Skill. Do not add CLI requirements that are not in those rules. When
CLI input or output changes, update the Skill pages that describe it in the same
change.

## Writing Styles

Write Skill text so that readers who are not native English speakers can
understand it and act on it quickly.

### Language

- Use one idea per sentence. State one main action or one judgment in each sentence.
- Use plain words and the active voice. Start action steps with a verb such as
  Run, Check, Ask, Show, or Stop.
- Keep sentences simple. Avoid unneeded abbreviations, nested clauses, double
  negatives, and strings of MUST NOT.
- Keep technical terms, CLI commands, field names, code, and fixed enum values in
  English. Write everything else in natural, clear language.
- Explain a new term once. When a specialized term first appears, add one short
  sentence that says what it does.
- Say each rule once. Do not repeat a rule to sound strict. Put a shared
  constraint in one place, and link to it or refer to it briefly elsewhere.

Example:

| Hard to follow | Easy to follow |
| --- | --- |
| Do not proceed with the broadcast without the user's confirmation not having been obtained. | Ask the user to confirm. Broadcast only after the user confirms. |

### Organizing information

Choose the Markdown structure by content:

| Content | Use |
| --- | --- |
| Steps that must run in order | Ordered list |
| Conditions, inputs, notes, or parallel options | Unordered list |
| A comparison of options, states, or field mappings | Short table |
| Commands, JSON, return values, and copyable templates | Code block |

When a paragraph contains three or more independent conditions, prohibitions,
fields, or actions, rewrite it as a list. Keep a causal sequence in prose or an
ordered list.

## Review checklist

- [ ] Skill files follow the closest template under `templates/`.
- [ ] CLI-related text follows `rules/cli-io-rules.md` and adds no new CLI requirements.
