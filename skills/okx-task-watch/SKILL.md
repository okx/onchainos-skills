---
name: okx-task-watch
description: "ACTIVATE for OKX A2A user-session task-progress flows: live monitoring via `okx-a2a user watch` (returns unread events backlog + long-polls for new ones; destructive read — returned items do not reappear) or outstanding-decision listing via `okx-a2a user outdated-list` (un-`check`ed `decision_request` items only). Claude Code / Codex only (`CLAUDECODE=1` or `CODEX_THREAD_ID`); other platforms stop with an unsupported-platform message. Triggers: 监听任务进展 / 开始监听任务 / 关注任务进展 / 使用监听 skill 监听任务进展 / 帮我盯着任务 / 任务有进度就告诉我 / 任务有动静告诉我 / 开监听 / watch 任务 / 历史消息 / 历史记录 / 过去消息 / 之前的消息 / 帮我看看之前的历史消息 / 看下之前的消息 / 未读消息 / 未决策 / 待决策 / 没有决策 / 未处理 / 待处理 / 没有处理 / task watch / user watch / monitor task progress / keep me posted on tasks / watch tasks / start watching / show past messages / show message history / catch me up on tasks / unread task messages / outstanding decisions / pending decisions / unhandled decisions / what am I missing. Business actions (apply / deliver / dispute / quote / accept) belong to `okx-agent-task`."
license: Apache-2.0
metadata:
  author: okx
  version: "3.20.0-beta"
  homepage: "https://web3.okx.com"
---

# OKX Task Watch

Live monitor for the user-session task inbox. Owns: triggers, the watch command, anti-cron rules, item dispatch (`notification` / `decision_request`), claim semantics, relay protocol, stop conditions.

Business actions (apply / deliver / dispute / quote / accept) belong to `okx-agent-task`. This skill only handles the watch loop.

## Triggers — MANDATORY ROUTING (NOT cron / scheduled / polling)

When ANY trigger phrase below matches, execute §Action. The watch command is a single long-poll call (the call itself waits for events); do **NOT** wrap in `/loop`, `Cron`, `$CODEX_HOME/automations`, `watch -n`, `sleep` loops, scheduler frameworks, or "set up a recurring job". Do **NOT** ask "how often should I check?" — the long-poll IS the wait. Do **NOT** substitute another command for polling.

**Trigger phrases**:
- Chinese (live monitor): `监听任务进展` / `开始监听任务` / `关注任务进展` / `使用监听 skill 监听任务进展` / `帮我盯着任务` / `任务有进度就告诉我` / `任务有动静告诉我` / `开监听` / `watch 任务`
- Chinese (history / backlog drain): `历史消息` / `历史记录` / `过去消息` / `之前的消息` / `帮我看看之前的历史消息` / `看下之前的消息` / `未读消息`
- Chinese (continuation — clarify first, see §Continuation triggers): `继续监听` / `继续盯着` / `继续 watch` / `接着监听` / `再监听一下` / `继续监听任务`
- English (live monitor): `task watch` / `user watch` / `monitor task progress` / `keep me posted on tasks` / `watch tasks` / `start watching`
- English (history / backlog drain): `show past messages` / `show message history` / `catch me up on tasks` / `unread task messages`
- English (continuation — clarify first, see §Continuation triggers): `keep watching` / `continue watching` / `resume monitoring`

> ⚠️ **Continuation triggers are a special case** — they do NOT immediately call watch. They imply the user wants to keep watching some specific task, but the intent is ambiguous (which task? or all of them?). See §Continuation triggers below for the clarification flow.

> 📥 **Why "view history" routes here**: watch is a **destructive read** of the event stream — each call returns the full backlog of unread events accumulated since the last call (e.g. while no one was watching), then long-polls for new ones. A user asking for past / missed / unread messages is asking to drain that backlog — same command, same Dispatch flow. Do NOT route to `agent active-tasks` / `agent status` (those are summaries, not the actual notification bodies). For un-replied `decision_request` items specifically (which `watch` already consumed but the user hasn't `check`ed), see §"Pull outstanding `decision_request` items".

## Platform compatibility — Claude Code / Codex only

🛑 The `okx-a2a` CLI is only wired on **Claude Code** and **Codex** harnesses. On **Hermes** and **OpenClaw**, the client itself pushes task notifications natively — there is no `okx-a2a` command and no manual watch is needed.

Before §Action, gate on environment variables:

```bash
detect_watch_support() {
  if [ "${CLAUDECODE:-}" = "1" ]; then
    echo "Claude"
  elif [ -n "${CODEX_THREAD_ID:-}" ]; then
    echo "Codex"
  else
    echo "unsupported"
  fi
}
detect_watch_support
```

- Output ∈ {`Claude`, `Codex`} → proceed to §Action.
- Output = `unsupported` → **stop**. Tell the user (localize to their language): "当前平台不支持 `okx-a2a` 监听 —— 任务通知会由客户端直接推送，无需手动开监听。" / "This platform doesn't support `okx-a2a`; task notifications are delivered natively by the client — no manual watch needed." Do NOT run any `okx-a2a` command.

## Action

### Continuation triggers — recall last jobId, then rearm

If the user's message matched a **continuation-style** phrase (`继续监听` / `继续盯着` / `继续 watch` / `接着监听` / `再监听一下` / `继续监听任务` / `keep watching` / `continue watching` / `resume monitoring`), the user means "keep watching the task we were already tracking" — they expect scoped monitoring on the same jobId, not a fresh global watch.

**Step 1 — Recall the jobId from this conversation's transcript.** Search in this order, take the FIRST hit:

1. The most recent CLI `[Watch]` block emitted earlier in this conversation (the jobId is the `--job-id <X>` value in its `okx-a2a user watch ...` command).
2. The most recent successful `agent create-task` / `agent publish-draft` stdout (jobId printed as `jobId: 0x...`).
3. The most recent jobId referenced in any rendered `notification` / `decision_request` in this conversation.

**Step 2 — Route by recall result**:

- **jobId found** → enter scoped session. **Do NOT emit §Banner** (the user already knows what they're tracking — a banner here is redundant ceremony). Just run `okx-a2a user watch --once --json --poll-ms 1000 --limit 50 --job-id <X>`. The sticky `--job-id <X>` applies for the rest of this session per §Session-scoped sticky.
- **No jobId found** → fall back to a global session. The behaviour diverges from the user's "keep watching" intent, so **DO emit §Banner** (it's the only signal the user has that the watch was rearmed as global rather than scoped). Then run `okx-a2a user watch --once --json --poll-ms 1000 --limit 50` (no `--job-id`). Do not ask the user — a continuation phrase plus no recoverable jobId is treated the same as a fresh `task watch` entry.

### 🛑 Banner before entering watch

**Decide by entry, not by "is this the first watch in this turn".** Look at **what triggered** the `okx-a2a user watch` call — not whether it's the first watch invocation in the current turn.

**Entries that REQUIRE the banner (only these two)**:

1. **Trigger-phrase entry** — this turn's user message matched a §Triggers phrase (e.g. `监听任务进展` / `历史消息` / `task watch`). **Exception**: a continuation-style phrase (`继续监听` / `keep watching` / ...) only triggers the banner when the recall fails and the watch falls back to global — see §Continuation triggers for the full rule.
2. **CLI `[Watch]` block entry** — a command earlier in this turn emitted a `[Watch]` block in stdout: a hint block that starts with `[Watch]` and instructs the current call to run `okx-a2a user watch ...` (typical sample: `` [Watch] Per `okx-task-watch` SKILL.md, start the monitor now: ``, output by `agent create-task` / `agent publish-draft`).

Any watch call that does not match one of these two entries **must NOT** emit the banner (including dispatch resume, wake fire, post-`pending-decisions-v2 resolve` relay — all session-continuation paths).

**How to send**: emit the exact canonical banner as a standalone **user-visible assistant message** (the message that appears in chat as the AI's reply to the user — NOT tool stdout, thinking blocks, or internal annotations the user cannot see).

| Chat language | Exact string (verbatim) |
|---|---|
| Chinese | `🔔 监听已启动，如果有历史消息，我们将先逐个处理，新任务进展会及时通知。` |
| English | `🔔 Watch started — any backlog will be processed first, then you'll be notified of new task events as they arrive.` |
| Other | Translate the English line; keep the leading 🔔 and the two-clause structure (started + backlog-first + then-new). |

❌ Violation examples:

- Saying `我现在开始监听` / `I'll start watching now` (or any paraphrase) **without** the exact canonical string in the same assistant message.
- Calling the watch tool before the banner has appeared.
- Embedding the banner inside Bash tool stdout / thinking block / tool-call arguments — these locations are invisible to the user, so the banner was not actually delivered.
- Emitting the banner on a re-entry path (resume after notification/decision_request handling, wake fire, post-relay resume) — these are not new entries.

### Run watch

```bash
okx-a2a user watch --once --json --poll-ms 1000 --limit 50
```

When the call returns items, process each per §Dispatch below. After processing all items, re-enter the same command (no banner) — the only exceptions are the §Stop condition triggers.

### Session-scoped `--job-id` (sticky)

If this watch session started from the CLI `[Watch]` block (the only path that puts `--job-id <X>` on the first call), **`--job-id <X>` is sticky for the entire session**. Wherever this skill shows the bare command `okx-a2a user watch --once --json --poll-ms 1000 --limit 50`, append `--job-id <X>` literally — including:

- §Dispatch notification resume
- §Dispatch decision_request resume (outcomes 3 / 4 / 5)
- §Re-enter after processing

The session ends when §Stop condition fires, or when the user starts a **new** watch via a §Triggers phrase — that new session is global, no `--job-id`.

## Anti-patterns

- Do NOT use `/loop`, Cron, `$CODEX_HOME/automations`, `watch -n`, `sleep` loops, or any self-rolled polling around `onchainos agent status` / `agent active-tasks`.
- There is **NO** `task watch` / `onchainos task watch` / `agent task watch` subcommand — do not invent one.
- Do NOT pass `--from-now`. By default watch returns the full backlog of unread events first, then long-polls for new ones; `--from-now` skips the backlog and silently drops any event the user hasn't seen yet (watch is destructive read — those events are gone for good).
- Do NOT pass `--job-id` **except in the post-publish `[Watch]` block**. `user watch` is a user-session-wide monitor by default; narrowing to one job defeats its purpose and misses cross-task events. The single exception is the CLI `[Watch]` block emitted by `agent create-task` / `agent publish-draft`, which intentionally narrows the first watch call to the freshly-published `jobId` so the user only sees that task's notifications immediately after publish. Trigger-phrase entries (e.g. `监听任务进展` / `task watch`) and any §Dispatch re-entry must still run watch **without** `--job-id`.
- 🛑 **Run `okx-a2a user watch` / `okx-a2a user outdated-list` exactly as written. Do NOT append `| grep` / `| tail` / `| head` / `| awk` / `| sed` / `| jq` / shell redirects.** Both commands emit a single structured JSON document — any pipe/truncation breaks the JSON and silently drops items. If output looks noisy with `[DEBUG]` lines mixed in, those belong on stderr and never affect the JSON on stdout; do not "clean" stdout. Pipe = data loss.

## Dispatch by `kind`

A returned item is always one of two `kind`s, handled completely differently.

### `kind == notification` — paste, then resume

A `notification` is a status update aimed at the **human user**, not a topic for you to respond to. Treat it like a text snippet you're quoting into a document: copy it, do not rewrite it.

**Step 1 — Paste `userContent` as a markdown blockquote.** The `>` prefix is the contract: "this is quoted content, not my own words".

```
> <item.userContent>
```

Every character of `userContent` must survive.

**Step 2 — Resume watching.** Call `okx-a2a user watch --once --json --poll-ms 1000 --limit 50` again (append the sticky `--job-id <X>` per §Session-scoped sticky if applicable).

**Multi-item ordering** — when watch returns N notifications, paste each `userContent` as its own blockquote in order, then run one resume call.

> 💡 `notification` items are auto-consumed by `watch` (destructive read — they will not appear in any later `watch` call). Do **NOT** call `okx-a2a user check --todo-ids …` for notifications; that command is for `decision_request` items only.

**Worked example** — given `userContent = "[Connecting Provider] Weather query for Beijing — 1 day (0x49fa…b3f8) — Connecting to the designated provider (agentId=866)."`:

✅ Correct — paste into a blockquote and show it to the user, nothing else added:

```
> [Connecting Provider] Weather query for Beijing — 1 day (0x49fa…b3f8) — Connecting to the designated provider (agentId=866).
```

❌ Wrong — any sentence *about* the notification instead of pasting it (`received a notification about provider connection` / `your task is connecting to provider 866` / `noted the connecting-provider update` — even if it cites parts of the body): you described the act of receiving, you did not paste the content.

### `kind == decision_request`

Paste `userContent` into your assistant message as a markdown blockquote (same copy-not-rewrite rule as §notification above), **and treat `llmContent` as the current turn's instruction set to think about and execute**. The user's reply is the input to that thinking. Do not reason over `userContent` itself — it is content for the user, not instructions for you.

#### Rendering choices

Each JSON item already carries a `choices` array auto-derived by the CLI from `userContent` (recognizing `请回复「xxx」` / `请选择` followed by a numbered or lettered list). If `choices` is missing or empty, parse `userContent` yourself by the same rules and always append `自定义回复`. `decision_request` items must always allow an open-ended reply even when no parsed choices exist.

Choice semantics: `保留` / `稍后` / `暂不` / `skip` → keep pending; everything else → reply (treated as the user's verbatim answer to this item, which triggers `llmContent` thinking via the flow below).

#### Schedule a 2-minute auto-timeout wake — before ending the turn

> ⚠️ **Skip this step for scoped sessions** (watch running with `--job-id <X>`, entered via the CLI `[Watch]` block or continuation-trigger recall). A scoped session is focused on a single task that the user is actively tracking — if they pause to think, they will come back on their own, and an auto-resume would just churn watch against a task that may already be terminal. Only **global sessions** (no `--job-id`, entered via a §Triggers phrase like `监听任务进展` / `task watch` or continuation-trigger global fallback) need the wake to keep the user-session-wide monitor alive across idle periods.

After rendering `userContent` and listing choices, but **before ending the turn**, schedule a 2-minute **one-shot** wake so the watch loop self-resumes if the user goes idle. Pick the tool by platform (same env vars as §Platform compatibility).

> **Terminology**: the handle returned by either tool is called the **wake id** in this skill.

🛑 **Critical for both platforms**: the `prompt` field must be passed **verbatim** as the exact English string below. Do **NOT** localize it, paraphrase it, or rewrite it based on the current `decision_request`'s content (e.g. do not turn it into "请选择服务商" / "回复 1 或 1727"). The wake fires in a fresh turn; the agent reading the wake prompt must see a generic, content-free "re-enter watch" instruction so it routes back into this skill — a contextualized prompt confuses both the scheduler view and the next-turn agent.

- **Claude Code** (`CLAUDECODE=1`):
  ```
  CronCreate(
    recurring: false,
    cron: "<minute> <hour> <DoM> <Mon> *",
    prompt: "Pending decision_request auto-timeout reached. Re-enter watch now: okx-a2a user watch --once --json --poll-ms 1000 --limit 50"
  )
  ```
  Field notes (do **not** inline these into the call):
  - `cron`: standard 5-field expression set to **now + 2 minutes in local time**. Example: if now is 14:28 local, use `30 14 <today_DoM> <today_Mon> *`.

  Remember the returned **wake id** (it stays in the assistant transcript and is visible in the next turn).

- **Codex** (`CODEX_THREAD_ID` non-empty):
  ```
  codex_app.automation_update(
    mode: "create",
    kind: "heartbeat",
    destination: "thread",
    rrule: "DTSTART:<YYYYMMDDTHHMMSS>\nRRULE:FREQ=MINUTELY;COUNT=1",
    prompt: "Pending decision_request auto-timeout reached. Re-enter watch now: okx-a2a user watch --once --json --poll-ms 1000 --limit 50",
    status: "ACTIVE"
  )
  ```
  Field notes (do **not** inline these into the call):
  - `rrule`: iCalendar RRULE syntax — exactly two lines joined by a literal `\n`:
      1. `DTSTART:<YYYYMMDDTHHMMSS>` — UTC basic format (e.g. `20260607T143000`) set to **now + 2 minutes in UTC**.
      2. `RRULE:FREQ=MINUTELY;COUNT=1` — fires exactly once (the `COUNT=1` guarantees one-shot semantics).

  Remember the returned **wake id**.

If the scheduling tool is unavailable (unknown tool / returns an error) → **skip silently** and end the turn. The user can re-trigger watch manually if they ignore the item.

**When the wake fires (user idle 2 min)**: its prompt runs `okx-a2a user watch ...` in a fresh turn, which resumes monitoring for **new** events. The original `decision_request` item is **not** re-surfaced by watch — it was already consumed when it first appeared (watch is destructive read). But because the user never `check`ed it, it remains in the outstanding-decisions queue and can be retrieved on demand via `okx-a2a user outdated-list` (see §"Pull outstanding `decision_request` items"). No extra logic needed here.

#### Handling the user reply — concurrency-safe relay

0. **First step (always)** — cancel the auto-timeout wake scheduled in the previous turn (best-effort):
   - Claude Code: `CronDelete(<wake id>)`
   - Codex: `codex_app.automation_update(mode: "delete", id: <wake id>)`
   - If the **wake id** is not visible in the assistant transcript (context compacted) or the cancel call errors → **skip and proceed**. Do NOT search for the wake by name/prompt match. A stale wake firing afterwards is harmless: it just re-enters watch to monitor new events; the already-handled `decision_request` item does **not** reappear in watch (it was consumed on the original return — watch is destructive read).

1. User picks `保留` / `skip` → **do NOT** claim; the item stays in the outstanding-decisions queue (un-`check`ed) and can be retrieved later via `okx-a2a user outdated-list` (triggers: `未决策` / `pending decisions`). **STOP the watch loop immediately** — briefly tell the user (localize per LOCALIZATION_PREFIX rules; keep `未决策` / `pending decisions` / `监听任务进展` / `task watch` unchanged): "Item kept on hold; watch loop ended. Say `未决策` / `pending decisions` to see all unhandled decisions, or `监听任务进展` / `task watch` to resume monitoring new events." The user explicitly chose to defer; honor that and stop background monitoring.
2. Otherwise claim first: `okx-a2a user check --todo-ids <id> --json`.
3. On `handled` → **execute the relay per `llmContent`'s instructions**. `llmContent` itself tells you which command to run, which target to relay to, and how to assemble the payload — just follow it. **Do NOT** semantically interpret the user's reply (no provider picking, no session creation, no XMTP solicitation), and do not bypass `llmContent` through any other path. Hand the relay off to the target session and do not wait for the target sub to finish.
4. On `alreadyHandled` → tell the user "this item was processed in another window"; **then re-enter `okx-a2a user watch --once --json --poll-ms 1000 --limit 50`** (append the sticky `--job-id <X>` per §Session-scoped sticky if applicable) (the watch session continues — only the duplicate item is dropped). Do not execute the relay again.
5. Claim succeeded but relay failed → create a new `okx-a2a user notify` with the failure reason and a retry command; **do NOT** flip the original item back to pending. **Then re-enter `okx-a2a user watch --once --json --poll-ms 1000 --limit 50`** (append the sticky `--job-id <X>` per §Session-scoped sticky if applicable).

🛑 **After `decision_request` outcomes 3, 4, 5 above, resume watching** — call `okx-a2a user watch --once --json --poll-ms 1000 --limit 50` again (append the sticky `--job-id <X>` per §Session-scoped sticky if applicable). Outcome 1 (`保留` / `skip`) is a hard STOP — see §Stop condition. Do NOT stop in outcomes 3/4/5 just because the relay completed / the item turned out duplicate / the relay failed.

🛑 **User-session authority boundary**: while handling a `decision_request` item, the user session is only a **relay endpoint**, not a business executor. The user's reply (`956`, `1`, `关闭`, `approve`, …) is the verbatim answer to that item — it must not be reinterpreted as a new "pick a provider / start negotiation / create a group / solicit a quote" intent. In the user session, **never** execute: `okx-a2a session create` / `okx-a2a xmtp-send` / `xmtp_start_conversation` / `xmtp_send` / `onchainos agent next-action` / `agent common context` / `agent recommend` / `agent service-list`. Those business steps belong to the target job/session after it has received the relay.

## Pull outstanding `decision_request` items — `okx-a2a user outdated-list`

User-initiated query, separate from the live watch loop. When the user explicitly asks to see decision_request items they have **not yet replied to** (rendered previously but no choice picked), surface all of them in one shot.

### Triggers
- Chinese: `未决策` / `待决策` / `没有决策` / `未处理` / `待处理` / `没有处理`
- English: `outstanding decisions` / `pending decisions` / `unhandled decisions` / `what am I missing`

### Action

```bash
okx-a2a user outdated-list --json
```

Returns the set of `decision_request` items the user has **not yet `check`ed** (i.e. watch has already surfaced them but the user never committed a reply). These items stay in the outstanding-decisions queue until `okx-a2a user check --todo-ids …` commits a decision. (Notifications are not included — watch consumes them on return and they have no outstanding state.)

### Rendering — batch, not per-item

Unlike watch's per-item flow, render **all returned items in a single assistant message**:

1. Number each item (`1`, `2`, `3`, ...) so the user can disambiguate.
2. For each item, paste its `userContent` as a markdown blockquote (same copy-not-rewrite rule as §`kind == notification` / §`kind == decision_request` above — no wrapper sentences, no summarization, no cross-item merging).
3. After the last item, append this disambiguation hint **exactly once** (translate to the user's language per LOCALIZATION_PREFIX rules; keep the literal token `JobID` and the examples unchanged):
   `💡 When replying, use either form to indicate which item you're answering: (1) list index + answer, e.g. "1 关闭" / "2: approve" / "3 — 956"; (2) JobID prefix + answer, e.g. "JobID 0x49fa — 1" (first 6 chars of jobId).`
4. End turn. Do **NOT** auto-re-enter `watch` or any other command — `outdated-list` is a one-shot query, not a loop.

### Handling the user's reply

Route in the following order:

1. **Reply starts with a list index** (digit `1` / `2` / `3` / ..., followed by separator `:` / `—` / space / newline, or standing alone):
   - Map the index back to the Nth `decision_request` rendered.
   - The text after the index (if any) is the verbatim answer for that item.
   - If the user sent only an index with no answer content (e.g. just `1`), **ask the user to supplement the answer** ("Please add your reply for decision 1, e.g. `1 关闭` / `1 956` / `1 自定义回复`") rather than guessing.

2. **Reply starts with `JobID <prefix>`** (or variants `JobID <prefix> — <answer>` / `<prefix>: <answer>`, etc.):
   - Match `<prefix>` against the listed items' jobIds (first 6 chars of jobId).
   - The text after the prefix is the verbatim answer for that item.

3. **Only one item is outstanding** → no ambiguity; treat the reply as belonging to that item whether or not it carries an index / prefix.

4. **Multiple outstanding items AND the reply carries neither an index nor a prefix** → ask the user to re-send using one of the forms above.

### Anti-patterns
- Do NOT call `okx-a2a user watch` for this intent — `watch` long-polls; `outdated-list` is a snapshot.
- Do NOT auto-re-enter any command after rendering. Wait for the user's reply (either an index or a JobID prefix is accepted).
- Do NOT schedule a 2-minute wake here — the wake belongs to the live watch flow for fresh `decision_request` items, not to a static list.
- Do NOT render items one by one across multiple assistant messages — batch them into a single message.

## Stop condition

🛑 **The ONLY valid stop conditions:**
- **User picks `保留` / `稍后` / `暂不` / `skip` on a `decision_request`** — item stays in the outstanding-decisions queue (un-`check`ed) and can be retrieved later via `outdated-list`. The watch loop ends here because the user explicitly chose to defer; honor that.
- The user explicitly says stop — e.g. `停止监听` / `不用监听了` / `stop watching` / `unsubscribe`.

### Re-enter after processing

After processing all returned items, **always** call `okx-a2a user watch --once --json --poll-ms 1000 --limit 50` again (append the sticky `--job-id <X>` per §Session-scoped sticky if applicable) to resume watching. The only exceptions are the stop conditions listed above.

🚫 **NOT stop conditions** — every one of these requires re-entering watch:

- A `notification` was just rendered (auto-consumed by watch — no claim step exists for notifications).
- A `notification` whose content contains `[Job Completed]` / `[Job Auto-Completed]` — **the task's terminal state ≠ the watch loop's terminal state**.
- A `decision_request` was just handled — relay completed (step 3) / `alreadyHandled` (step 4) / claim-succeeded-but-relay-failed (step 5). **Note**: `保留` / `skip` (step 1) is a STOP, listed above.
- Watch returned 0 items (empty result / long-poll elapsed with no new events) — re-enter watch and keep waiting.
