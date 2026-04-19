<!-- workflows/TEMPLATE.md — Workflow document authoring guide -->

## Format

Every workflow doc must include all sections below, in order:

```
# {Name}
> {one-liner}
## Triggers
## Required Skills
## Input
## Steps
### Step N — {name} [required | recommended] (parallel | sequential | conditional: {condition})
## Output Template
## Actions
## Follow-up Workflows
```

## Rules

- **Step labels**: mark each step `[required]` or `[recommended]`, and `(parallel)` / `(sequential)` / `(conditional: <condition>)`
- **Token budget**: single doc ≤ 1.5K tokens
- **CLI-first**: use only `onchainos` CLI commands — do not reference MCP tool names
- **Language**: English body, Chinese trigger phrases are allowed in Triggers
- **Security rules**: follow token risk controls defined in `okx-security` SKILL.md — do not redefine them
- **Script mode**: all CLI commands support `--format json`; Output Template targets conversational mode, script mode consumes JSON directly
