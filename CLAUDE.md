# Project guidance

## Developing in this repository

This repository uses [`AGENTS.md`](AGENTS.md) as the canonical cross-agent
development guide.

Before editing any file:
1. Read `AGENTS.md`.
2. Classify the task as Skill-only, CLI-only, mixed, or guidance-only.
3. Follow the matching first-read workflow described there.

Claude-specific notes:
- Do not start editing before reading the matching rules and templates: for Skill
  work, `templates/` and `rules/skill-rules.md`; for CLI work,
  `rules/cli-io-rules.md`.
- State a short change plan before you edit, and keep changes minimal and scoped.
- After the change, run the validation that matches it and report the result.
- Do not add external document links or sensitive information to guidance files.
