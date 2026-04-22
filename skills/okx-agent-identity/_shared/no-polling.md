# No Polling / No Waiting / No Silent Extra Calls

> Cross-cutting rule. Applies to every command this skill issues. Reference from `SKILL.md`.

## The rule

One user intent = one CLI call. Show the result. Wait for the user to say what's next.

## What this forbids

1. **No silent "look before leap".** After a successful `create` / `update` / `activate` / `deactivate` / `feedback-submit`, do NOT chase it with a `get` to "confirm it landed". The command's own response is authoritative.
2. **No status polling.** Never `sleep` and re-query. Never loop `agent get` to watch a status transition.
3. **No automatic retry on business errors.** If the CLI returns a 4xx-class failure (invalid field, validation, `provider agents require at least one service; provide --service`, etc.), render the error card from `display-formats.md` §6 and stop. The user decides the next step.
4. **No speculative side-queries.** Do not run `wallet status` / `agent get` / `agent search` "just to be safe" before the user's actual command. Pre-flight checks in `_shared/preflight.md` happen once per session; after that, trust the state.
5. **No splitting one ask into many CLI calls** unless the user's wording clearly asks for multiple operations ("把 #42 下架再改个头像" is two commands; "改头像" is one).

## What is allowed

- **Transient network retry**: one retry on 5xx / connection-reset. Second failure → surface the error.
- **User-initiated re-check**: if the user explicitly says "查一下到没到链上 / 确认一下生效了没", run `agent get --agent-ids <id>` once.
- **Dependency reads**: before `update`, you still run `agent get` — that's part of the mandatory 4-step flow, not polling.
- **Sanity reads inside create**: checking whether the user already has an agent of the requested role (the "pre-check existing" step of Core Flow) is a single read, not a loop.

## Error-card stance

When a write command fails, the recovery path is **always** through the user, not around them:

- Render the error card (see `display-formats.md` §6): single-line summary → 原因 → 下一步.
- Do NOT queue the retry. Do NOT pre-edit the command. Wait for the user to reply.
- The raw CLI `bail!` string lives in the card footer for debugging; the translation sits above it (see `troubleshooting.md`).

## Why this rule exists

Silent extra calls make every agent action feel slow and opaque. A user who says "下架 #42" expects one network round-trip and a line of confirmation — not a `get` + `deactivate` + `get` triple that the CLI printer has to unwind. Errors compound: a hidden pre-check that fails obscures the actual command the user wanted to run.

Treat each user message as a contract: execute exactly what they asked for, surface what happened, then stop.
