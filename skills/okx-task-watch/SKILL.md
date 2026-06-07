---
name: okx-task-watch
description: "Live user-session task-progress monitor for OKX A2A. Long-poll the user inbox via `okx-a2a user watch`; render notifications verbatim; claim items; relay `decision_request` per `llm_content`. Also drains backlog: any request to view past / missed / unread task messages is served by the same watch command (it drains SQLite-backed pending items first, then waits for new events). Claude Code / Codex only (gated by `CLAUDECODE=1` or `CODEX_THREAD_ID`); on Hermes / OpenClaw the client pushes notifications natively and no `okx-a2a` command exists — this skill stops with an unsupported-platform message. NOT a cron / scheduled / interval-polling task — `--timeout 300` IS the wait. Triggers: 监听任务进展 / 开始监听任务 / 关注任务进展 / 使用监听 skill 监听任务进展 / 帮我盯着任务 / 任务有进度就告诉我 / 任务有动静告诉我 / 开监听 / watch 任务 / 历史消息 / 历史记录 / 过去消息 / 之前的消息 / 帮我看看之前的历史消息 / 看下之前的消息 / 未读消息 / task watch / user watch / monitor task progress / keep me posted on tasks / watch tasks / start watching / show past messages / show message history / catch me up on tasks / unread task messages. Business actions (apply / deliver / dispute / quote / accept) belong to `okx-agent-task`; this skill only handles the watch loop."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.0"
  homepage: "https://web3.okx.com"
---

# OKX Task Watch

Live monitor for the user-session task inbox. Owns: triggers, the watch command, anti-cron rules, item dispatch (`notification` / `decision_request`), claim semantics, relay protocol, stop conditions.

Business actions (apply / deliver / dispute / quote / accept) belong to `okx-agent-task`. This skill only handles the watch loop.

## Triggers — MANDATORY ROUTING (NOT cron / scheduled / polling)

When ANY trigger phrase below matches, execute §Action. The watch command is a single long-poll call (waits up to 5 min per call for events); do **NOT** wrap in `/loop`, `Cron`, `$CODEX_HOME/automations`, `watch -n`, `sleep` loops, scheduler frameworks, or "set up a recurring job". Do **NOT** ask "how often should I check?" — `--timeout 300` IS the wait. Do **NOT** substitute `agent status` / `agent active-tasks` polling.

**Trigger phrases**:
- Chinese (live monitor): `监听任务进展` / `开始监听任务` / `关注任务进展` / `使用监听 skill 监听任务进展` / `帮我盯着任务` / `任务有进度就告诉我` / `任务有动静告诉我` / `开监听` / `watch 任务`
- Chinese (history / backlog drain): `历史消息` / `历史记录` / `过去消息` / `之前的消息` / `帮我看看之前的历史消息` / `看下之前的消息` / `未读消息`
- English (live monitor): `task watch` / `user watch` / `monitor task progress` / `keep me posted on tasks` / `watch tasks` / `start watching`
- English (history / backlog drain): `show past messages` / `show message history` / `catch me up on tasks` / `unread task messages`

> 📥 **Why "view history" routes here**: watch first drains the SQLite-backed pending queue (all unclaimed prior items) before long-polling for new events. A user asking for past / missed / unread messages is asking for the same drain — same command, same Dispatch flow. Do NOT route to `agent active-tasks` / `agent status` (those are summaries, not the actual notification bodies).

## Platform compatibility — Claude Code / Codex only

🛑 The `okx-a2a` CLI loop is only wired on **Claude Code** and **Codex** harnesses. On **Hermes** and **OpenClaw**, the client itself pushes task notifications natively — there is no `okx-a2a` command and no manual watch is needed.

Before §Action, gate on the same env-vars that `buyer/create.rs` / `buyer/draft.rs` use to decide whether to emit the `[Watch]` block:

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

Run:

```bash
okx-a2a user watch --once --json --timeout 300 --poll-ms 1000 --limit 50
```

When the call returns items, process each per §Dispatch below. After processing all items, re-enter the same command — the only exceptions are the §Stop condition triggers.

## Anti-patterns

- Do NOT use `/loop`, Cron, `$CODEX_HOME/automations`, `watch -n`, `sleep` loops, or any self-rolled polling around `onchainos agent status` / `agent active-tasks`.
- There is **NO** `task watch` / `onchainos task watch` / `agent task watch` subcommand — do not invent one.
- Do NOT pass `--from-now`. Watch **must** first drain SQLite-backed pending items, **then** wait for new changes.
- Do NOT pass `--job-id` — **never** watch a single task. `user watch` is a user-session-wide monitor; narrowing to one job defeats its purpose and misses cross-task events.

## Dispatch by `kind`

A returned item is always one of two `kind`s, handled completely differently.

### `kind == notification` — three MANDATORY steps in this exact order, every time

1. **Render `user_content` verbatim** to the user. The assistant message MUST literally contain `<item.user_content>` (translation to the user's language follows LOCALIZATION_PREFIX rules only — every data value, label, address, amount, deadline, and line break must survive). ❌ Do NOT paraphrase, summarize, shorten, prepend `[notification:]` / "you have a new update:", or replace structured fields with handwaves like "your task". Do **not** parse `llm_content` for this kind. The mid-handling message is **not** a "work-progress update" — when you are handling a `notification`, your assistant message's job IS to display the notification body; it cannot only describe what you are about to do next.
   - 🛑 **Forbidden substitution phrases** (any of these in place of the body = violation): `收到「…」的通知` / `收到通知` / `我看到通知了` / `通知说……` / `我会标记已读` / `received notification` / `I'll mark it read` / `noted the notification` / any wrapper-only sentence with no body verbatim. These describe the act of receiving instead of displaying — strictly forbidden.
2. **Claim the item** to remove it from the pending queue:
   ```bash
   okx-a2a user check --todo-ids <todoId> --json
   ```
   🛑 **Hard self-check before claim**: the assistant message *immediately preceding* this `okx-a2a user check` call MUST contain the item's `user_content` verbatim. If it does not (because you summarized, skipped, or only described what you're about to do), **abort the claim**, render the body first in a new assistant message, then claim. Skipping step 1 = the user never sees the notification; skipping step 2 = the item stays `pending` and resurfaces on every subsequent watch wake-up, spamming duplicates.
3. **Resume watching** — call `okx-a2a user watch --once --json --timeout 300 --poll-ms 1000 --limit 50` again. No relay, no `llm_content` thinking.

**Multi-item ordering** — when `okx-a2a user watch` returns N `notification` items, process them as a per-item loop, NOT a batch:

```
for each item:
  render user_content verbatim  →  okx-a2a user check --todo-ids <thisId>  →  next item
```

❌ Do NOT batch-claim multiple `--todo-ids` in one call unless every one of those items already had its `user_content` rendered verbatim in a preceding assistant message. The render-before-claim invariant is per-item.

**Counter-example** (real incident — do NOT repeat):

- ❌ Wrong (substitution): assistant says `收到"正在连接服务商"的通知。我会标记已读。` → runs `okx-a2a user check --todo-ids …` — the body (`【北京未来一天天气查询】（0x49fa…b3f8） — 正在连接指定服务商（866）。`) was never shown; the user only sees a wrapper sentence about the act of receiving, then the item is gone.
- ✅ Correct: assistant message contains `[正在连接服务商]【北京未来一天天气查询】（0x49fa…b3f8） — 正在连接指定服务商（866）。` (full verbatim body, including bracketed marker, job title, jobId fragment, and counterparty id), THEN runs `okx-a2a user check --todo-ids …`.

### `kind == decision_request`

Render `user_content` to the user verbatim (same anti-paraphrase rules as `notification` above), **and treat `llm_content` as the current turn's instruction set to think about and execute**. The user's reply is the input to that thinking.

#### Rendering choices

Each JSON item already carries a `choices` array auto-derived by the CLI from `user_content` (recognizing `请回复「xxx」` / `请选择` followed by a numbered or lettered list). If `choices` is missing or empty, parse `user_content` yourself by the same rules and always append `自定义回复`. `decision_request` items must always allow an open-ended reply even when no parsed choices exist.

Choice semantics: `保留` / `稍后` / `暂不` / `skip` → keep pending; everything else → reply (treated as the user's verbatim answer to this item, which triggers `llm_content` thinking via the flow below).

#### Handling the user reply — concurrency-safe relay

1. User picks `保留` / `skip` → **do NOT** claim; leave the item pending. **STOP the watch loop immediately** — briefly tell the user "已保留该项为 pending，监听结束；需要时再说一声「监听任务进展」即可重新打开". Do NOT re-enter watch here — `watch` is required to first drain SQLite-backed pending items, so re-entering would immediately return the same kept item and infinite-loop the prompt.
2. Otherwise claim first: `okx-a2a user check --todo-ids <id> --json`.
3. On `handled` → **execute the relay per `llm_content`'s instructions**. `llm_content` itself tells you which command to run, which target to relay to, and how to assemble the payload — just follow it. **Do NOT** semantically interpret the user's reply (no provider picking, no session creation, no XMTP solicitation), and do not bypass `llm_content` through any other path. Hand the relay off to the target session and do not wait for the target sub to finish.
4. On `alreadyHandled` → tell the user "this item was processed in another window"; **then re-enter `okx-a2a user watch ...`** (the watch session continues — only the duplicate item is dropped). Do not execute the relay again.
5. Claim succeeded but relay failed → create a new `okx-a2a user notify` with the failure reason and a retry command; **do NOT** flip the original item back to pending. **Then re-enter `okx-a2a user watch ...`**.

🛑 **After `decision_request` outcomes 3, 4, 5 above, resume watching** — call `okx-a2a user watch --once --json --timeout 300 --poll-ms 1000 --limit 50` again. Outcome 1 (`保留` / `skip`) is a hard STOP — see §Stop condition. Do NOT stop in outcomes 3/4/5 just because the relay completed / the item turned out duplicate / the relay failed.

🛑 **User-session authority boundary**: while handling a `decision_request` item, the user session is only a **relay endpoint**, not a business executor. The user's reply (`956`, `1`, `关闭`, `approve`, …) is the verbatim answer to that item — it must not be reinterpreted as a new "pick a provider / start negotiation / create a group / solicit a quote" intent. In the user session, **never** execute: `okx-a2a session create` / `okx-a2a xmtp-send` / `xmtp_start_conversation` / `xmtp_send` / `onchainos agent next-action` / `agent common context` / `agent recommend` / `agent service-list`. Those business steps belong to the target job/session after it has received the relay.

## Stop condition

🛑 **The ONLY valid stop conditions:**
- **User picks `保留` / `稍后` / `暂不` / `skip` on a `decision_request`** — item stays pending (un-claimed), but the watch loop ends here. Re-entering would re-drain the same pending item and infinite-loop the prompt; the user has to re-trigger watch when they're ready.
- The user explicitly says stop — e.g. `停止监听` / `不用监听了` / `stop watching` / `unsubscribe`.

### Re-enter after processing

After processing all returned items, **always** call `okx-a2a user watch --once --json --timeout 300 --poll-ms 1000 --limit 50` again to resume watching. The only exceptions are the stop conditions listed above.

🚫 **NOT stop conditions** — every one of these requires re-entering watch:
- A `notification` was just rendered + claimed.
- A `notification` whose content contains `[Job Completed]` / `[Job Auto-Completed]` — **the task's terminal state ≠ the watch loop's terminal state**. After completion, the sub agent still auto-rates the ASP and pushes a rating notification; stopping here = the user never sees the rating result.
- A `decision_request` was just handled — relay completed (step 3) / `alreadyHandled` (step 4) / claim-succeeded-but-relay-failed (step 5). **Note**: `保留` / `skip` (step 1) is a STOP, listed above.
- Watch returned 0 items (empty result / `--timeout 300` elapsed with no events) — re-enter watch and keep waiting.
