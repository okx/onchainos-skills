# No Polling / No Waiting / No Silent Extra Calls

> Cross-cutting rule. Applies to every command this skill issues. Reference from `SKILL.md`.

## The rule

One user intent = one CLI call. Show the result. Wait for the user to say what's next.

## What this forbids

1. **No silent "look before leap".** After a successful `create` / `update` / `activate` / `deactivate` / `feedback-submit`, do NOT chase it with a `get` to "confirm it landed". The command's own response is authoritative. **CLI-internal bounded wait** (e.g. `create` / `update` 内部 ≤5 s 的 tx-status 补全) **不属于此处禁令** —— 那是单条命令的实现细节；本规则约束的是 **skill / agent 层** 不要再叠加 `agent get` 之类的二次确认。
2. **No status polling.** Never `sleep` and re-query. Never loop `agent get` to watch a status transition.
3. **No automatic retry on business errors.** If the CLI returns a 4xx-class failure (invalid field, validation, `provider agents require at least one service; provide --service`, etc.), render the error card from `display-formats.md` §Error card and stop. The user decides the next step.
4. **No speculative side-queries.** Do not run `wallet status` / `agent get` / `agent search` "just to be safe" before the user's actual command. Pre-flight checks in `_shared/preflight.md` happen once per session; after that, trust the state. **Concrete examples**:
   - After `agent get --agent-ids <id>` returns the single-agent detail, do **NOT** chain `agent service-list --agent-id <id>` — the `services` array is already in the response (`items[0].services`). Do **NOT** chain `agent feedback-list --agent-id <id>` — the reputation aggregate `{ score, count }` is already in the response; pull the full review list **only if** the user says yes to the numbered-options prompt in `display-formats.md §Post-detail prompt`.
   - After `agent create` / `update` / `activate` / `deactivate` / `feedback-submit`, do NOT re-run `agent get` to "verify" — the command's own response is authoritative. Note: `create` / `update` already do a bounded internal poll against the hash→info endpoint and may include an `agent` sub-object in the response (see `display-formats.md` §2 / `cli-reference.md` §1); skill-layer code must NOT add its own retry on top.
5. **No splitting one ask into many CLI calls** unless the user's wording clearly asks for multiple operations ("把 #42 下架再改个头像" is two commands; "改头像" is one).

## What is allowed

- **Transient network retry**: one retry on 5xx / connection-reset. Second failure → surface the error.
- **User-initiated re-check**: if the user explicitly says "查一下到没到链上 / 确认一下生效了没", run `agent get --agent-ids <id>` once.
- **Dependency reads**: before `update`, you still run `agent get` — that's part of the mandatory 4-step flow, not polling.
- **Sanity reads inside create**: checking whether the user already has an agent of the requested role (the "pre-check existing" step of Core Flow) is a single read, not a loop.
- **Same-turn skill handoffs (whitelist)**: this rule is about CLI calls and self-querying. Loading a downstream skill file inside the same response and continuing with its instructions is **not** polling and is explicitly allowed for the paths enumerated in `SKILL.md §Step 4: Report Result and Stop`. Today the whitelist covers:
  - `agent create --role evaluator` → `/skills/okx-agent-task/evaluator.md` (staking)
  - `agent create --role requester` → `/skills/okx-agent-chat/after-agent-list-changed.md` (sync local agent list to OpenClaw; silent no-op outside OpenClaw runtime)
  - `agent create --role provider` → `/skills/okx-agent-chat/after-agent-list-changed.md` (same as above)
  - `agent activate --agent-id <id>` → `/skills/okx-agent-chat/after-agent-list-changed.md` (same as above; idempotent)
  - `agent deactivate --agent-id <id>` → `/skills/okx-agent-chat/after-agent-list-changed.md` (same as above; idempotent)

  These transition skill context — they do not requery the on-chain state of the just-completed write. **Passive Onboarding (`intent=need-requester`) is excluded** from this whitelist; it must hand strictly back to `okx-agent-task` with the contracted single line. Do not invent new same-turn handoffs outside the `§Step 4` whitelist.

## Error-card stance

When a write command fails, the recovery path is **always** through the user, not around them:

- Render the error card (see `display-formats.md` §Error card): single-line summary → 原因 → 下一步.
- Do NOT queue the retry. Do NOT pre-edit the command. Wait for the user to reply.
- The raw CLI `bail!` string lives in the card footer for debugging; the translation sits above it (see `troubleshooting.md`).

## Why this rule exists

Silent extra calls make every agent action feel slow and opaque. A user who says "下架 #42" expects one network round-trip and a line of confirmation — not a `get` + `deactivate` + `get` triple that the CLI printer has to unwind. Errors compound: a hidden pre-check that fails obscures the actual command the user wanted to run.

Treat each user message as a contract: execute exactly what they asked for, surface what happened, then stop.
