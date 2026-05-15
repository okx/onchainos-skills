# No Polling / No Waiting / No Silent Extra Calls

> Cross-cutting rule. Applies to every command this skill issues. Reference from `SKILL.md`.

## The rule

One user intent = one CLI call. Show the result. Wait for the user to say what's next.

## What this forbids

1. **No silent "look before leap".** After a successful `create` / `update` / `activate` / `deactivate` / `feedback-submit`, do NOT chase it with a `get` to "confirm it landed". The command's own response is authoritative. **CLI-internal bounded wait** (e.g. `create` / `update` 内部 ≤5 s 的 tx-status 补全) **不属于此处禁令** —— 那是单条命令的实现细节；本规则约束的是 **skill / agent 层** 不要再叠加 `agent get` 之类的二次确认。
2. **No status polling.** Never `sleep` and re-query. Never loop `agent get` to watch a status transition.
3. **No automatic retry on business errors.** If the CLI returns a 4xx-class failure (invalid field, validation, `provider agents require at least one service; provide --service`, etc.), render the error card from `display-formats.md` §Error card and stop. The user decides the next step.
4. **No speculative side-queries.** Do not run `wallet status` / `agent get` / `agent search` "just to be safe" before the user's actual command. Pre-flight checks in `_shared/preflight.md` happen once per session; after that, trust the state. **Concrete examples**:
   - After `agent get --agent-ids <id>` returns the single-agent detail, do **NOT** chain `agent service-list --agent-id <id>` — the `services` array is already in the response (`list[0].agentList[0].services` — the envelope is double-layer, see `cli-reference.md §3`). Do **NOT** chain `agent feedback-list --agent-id <id>` — the reputation aggregate `{ score, count }` is already in the response at `list[0].agentList[0].reputation`; pull the full review list **only if** the user says yes to the numbered-options prompt in `display-formats.md §Post-detail prompt`.
   - After `agent create` / `update` / `activate` / `deactivate` / `feedback-submit`, do NOT re-run `agent get` to "verify" — the command's own response is authoritative. Note: `create` / `update` already do a bounded internal poll against the hash→info endpoint and may include an `agent` sub-object in the response (see `display-formats.md` §2 / `cli-reference.md` §1); skill-layer code must NOT add its own retry on top.
5. **No splitting one ask into many CLI calls** unless the user's wording clearly asks for multiple operations ("把 #42 下架再改个头像" is two commands; "改头像" is one).

## What is allowed

- **Transient network retry**: one retry on 5xx / connection-reset. Second failure → surface the error.
- **User-initiated re-check**: if the user explicitly says "查一下到没到链上 / 确认一下生效了没", run `agent get --agent-ids <id>` once.
- **Dependency reads**: before `update`, you still run `agent get` — that's part of the mandatory 4-step flow, not polling.
- **Sanity reads inside create**: checking whether the user already has an agent of the requested role (the "pre-check existing" step of Core Flow) is a single read, not a loop.
- **Same-turn skill handoffs (whitelist)**: this rule is about CLI calls and self-querying. Loading a downstream skill file inside the same response and continuing with its instructions is **not** polling and is explicitly allowed for the paths enumerated in `SKILL.md §Step 4: Report Result and Stop`. That whitelist is the **single source of truth** for the trigger → downstream-file mapping (5 rows today: `evaluator → stake` plus four chat post-hook paths after `requester` / `provider` create and `activate` / `deactivate`). Do not mirror the row contents here — read them from `SKILL.md`; if anything changes, update the whitelist itself.

  These transition skill context — they do not requery the on-chain state of the just-completed write. **Passive Onboarding (`intent=need-requester`) is excluded** from this whitelist; it must hand strictly back to `okx-agent-task` with the contracted single line. Do not invent new same-turn handoffs outside the `§Step 4` whitelist.

## Error-card stance

When a write command fails, the recovery path is **always** through the user, not around them:

- Render the error card (see `display-formats.md` §Error card): single-line summary → 原因 → 下一步.
- Do NOT queue the retry. Do NOT pre-edit the command. Wait for the user to reply.
- The raw CLI `bail!` string lives in the card footer for debugging; the translation sits above it (see `troubleshooting.md`).

## Why this rule exists

Silent extra calls make every agent action feel slow and opaque. A user who says "下架 #42" expects one network round-trip and a line of confirmation — not a `get` + `deactivate` + `get` triple that the CLI printer has to unwind. Errors compound: a hidden pre-check that fails obscures the actual command the user wanted to run.

Treat each user message as a contract: execute exactly what they asked for, surface what happened, then stop.

## No Shell-Stitching of CLI Output (P0 — symmetric counterpart of "no polling")

The five rules above forbid **over-querying** (extra CLI calls). This rule forbids the symmetric failure: **under-querying — reading your own session log, writing bash parsers, and stitching together a response from `grep` / `sed` instead of re-invoking the CLI.** Empirically this is more damaging than polling because the stitched data **does not error out** — it silently turns into hallucinated values that look plausible to the user.

⛔ **Forbidden:**

- Reading your own session transcript / tool-result files (e.g. `~/.claude/projects/<sid>/tool-results/<tid>.txt`) to "reconstruct" backend data you already saw.
- Writing parser scripts (e.g. `/tmp/parse.sh`, `/tmp/extract_agents.py`) that `grep` / `sed` / `awk` over a captured CLI JSON response. **Especially toxic pattern:** `grep -A N '"agentId"' file | grep '"profileDescription"' | head -1` — in single-line JSON (the common case) this matches the *first* `profileDescription` in the whole response for *every* `agentId`, producing identical fields across rows. This was the direct root cause of TC-J8-001c hallucination.
- Caching one page's content in memory and "deriving" what page 2 / 3 / … should contain.
- Stitching multiple pages locally into a single "complete" table (e.g. concatenating page 1 + page 2 and presenting as "all 94"). Boundary errors (duplicate ids at the page-split, missing ids at the edge) are guaranteed and the user has no way to spot them.
- Sending `--page-size 100` to "get everything in one call" when the backend caps at 50 (`cli-reference.md §7`).

✅ **Correct path when a previous CLI response doesn't contain the next thing the user asked for:**

- Want page 2? → `onchainos agent search --query "<same>" --page <prev+1> --page-size <same>` and render that response directly.
- Want a specific row's full detail? → `onchainos agent get --agent-ids <id>` (single id is cheap).
- Need a different filter cut? → re-issue `agent search` with the new filter values; do not post-filter rows in bash.
- Output really is large? → render the first N rows + the language-and-case-matched continuation footer per `display-formats.md §6 Display Completeness`. ⛔ The two cases use **different** continuation phrases and must not be mixed:
  - **Case A — Backend pagination** (`envelope.total > page_size`, more pages exist server-side) → footer says `第 <page>/<total_pages> 页，继续翻页说 "下一页"。` / `Page <page>/<total_pages> — say "next page" to continue.` The next user action is another CLI call with `--page <prev+1>`.
  - **Case B — AI-side truncation** (`envelope.total ≤ page_size`, full result already in this response, AI chose to show top K) → footer says `说"更多" / "展开" / "全部"看剩 N-K 条` / `Say "more" / "show all" / "expand" for the remaining N-K`. The next user action is **rendering the remaining rows from this same in-context response** — no new CLI call.
  Do NOT silently summarize "总共 N 条" without quoting the actual rows you'd render — the moment you can't quote a specific `agentId` from the bucket you're summarizing, the bucket is fabrication.

**Self-test before emitting any table whose rows came from a CLI response:** for each row, can I point to **the exact field in the most-recent CLI tool-call result** that gave me this value? If the answer is "I derived it from an earlier turn / parsed it via bash / inferred from context" — **STOP**. Re-issue the CLI call and render that response.

The asymmetry vs. polling: polling adds latency and visible jitter (users notice). Shell-stitching produces clean-looking but wrong data (users don't notice until it matters). Both are banned; this one is the worse failure mode and must never ship.
