# `okx-a2a user watch` — full protocol

> Loaded from the SKILL.md "Live task-progress monitor" stub. SKILL.md owns triggers + the entry-point command + anti-cron rules; **this file owns everything that happens AFTER `user watch` returns an item** (kind dispatch, claim semantics, relay protocol, stop condition, terminal signals).

## Dispatch by `kind`

A returned item is always one of two `kind`s, handled completely differently:

- **`kind == notification`** — **two MANDATORY steps in this exact order, both required every time**:
  1. **Render `user_content` verbatim** to the user. ❌ Do NOT paraphrase, summarize, shorten, prepend `[notification:]` / "you have a new update:", or replace structured fields (jobId, amount, deadline) with handwaves like "your task". Translation to the user's language follows LOCALIZATION_PREFIX rules **only** — every data value, label, and line break must survive. Do **not** parse `llm_content` for this kind.
  2. **Immediately claim the item** to remove it from the pending queue:
     ```bash
     okx-a2a user check --todo-ids <todoId> --json
     ```
     ⚠️ **Skipping step 2 = the item stays `pending` and resurfaces on every subsequent watch wake-up**, spamming the user with duplicates. Step 2 is non-optional — render AND claim, in that order, every time.

  After step 2, **re-enter `okx-a2a user watch ...`** — no relay, no `llm_content` thinking.

- **`kind == decision_request`** — render `user_content` to the user verbatim (same anti-paraphrase rules as `notification` above), **and treat `llm_content` as the current turn's instruction set to think about and execute**. The user's reply is the input to that thinking.

### `decision_request`: rendering choices

Each JSON item already carries a `choices` array auto-derived by the CLI from `user_content` (recognizing `请回复「xxx」` / `请选择` followed by a numbered or lettered list). If `choices` is missing or empty, parse `user_content` yourself by the same rules and always append `自定义回复`. `decision_request` items must always allow an open-ended reply even when no parsed choices exist.

Choice semantics: `保留` / `稍后` / `暂不` / `skip` → keep pending; everything else → reply (treated as the user's verbatim answer to this item, which triggers `llm_content` thinking via the flow below).

### `decision_request`: handling the user reply — concurrency-safe relay

1. User picks `保留` / `skip` → **do NOT** claim; leave the item pending. **Then re-enter `okx-a2a user watch --once --json --timeout 300 --poll-ms 1000 --limit 50`** — keeping an item pending is not a stop signal.
2. Otherwise claim first: `okx-a2a user check --todo-ids <id> --json`.
3. On `handled` → **execute the relay per `llm_content`'s instructions**. `llm_content` itself tells you which command to run, which target to relay to, and how to assemble the payload — just follow it. **Do NOT** semantically interpret the user's reply (no provider picking, no session creation, no XMTP solicitation), and do not bypass `llm_content` through any other path. Hand the relay off to the target session and do not wait for the target sub to finish.
4. On `alreadyHandled` → tell the user "this item was processed in another window"; **then re-enter `okx-a2a user watch ...`** (the watch session continues — only the duplicate item is dropped). Do not execute the relay again.
5. Claim succeeded but relay failed → create a new `okx-a2a user notify` with the failure reason and a retry command; **do NOT** flip the original item back to pending. **Then re-enter `okx-a2a user watch ...`**.

🛑 **After every non-terminal `decision_request` outcome (steps 1, 3, 4, 5 above), the watch loop continues — re-enter `okx-a2a user watch --once --json --timeout 300 --poll-ms 1000 --limit 50` exactly as a fresh wake.** Do NOT stop just because the relay completed / the user picked `skip` / the item turned out duplicate. The only valid exit is via §Stop condition below.

🛑 **User-session authority boundary**: while handling a `decision_request` item, the user session is only a **relay endpoint**, not a business executor. The user's reply (`956`, `1`, `关闭`, `approve`, …) is the verbatim answer to that item — it must not be reinterpreted as a new "pick a provider / start negotiation / create a group / solicit a quote" intent. In the user session, **never** execute: `okx-a2a session create` / `okx-a2a xmtp-send` / `xmtp_start_conversation` / `xmtp_send` / `onchainos agent next-action` / `agent common context` / `agent recommend` / `agent service-list`. Those business steps belong to the target job/session after it has received the relay.

## Stop condition

🛑 **The ONLY valid stop conditions:**
- A **terminal item** (signal list below) was just processed in this watch turn, **AND** the post-terminal check (steps 1–3 below) confirms no other active tasks.
- The user explicitly says stop — e.g. `停止监听` / `不用监听了` / `stop watching` / `unsubscribe`.

🚫 **NOT stop conditions** — every one of these is a normal wake cycle; **immediately re-enter `okx-a2a user watch --once --json --timeout 300 --poll-ms 1000 --limit 50`**:
- Watch returned an empty result / no items.
- Watch timed out (`--timeout 300` elapsed) without any event.
- `user_attention.changed` did not fire for a while / SQLite read returned nothing.
- A non-terminal `notification` or `decision_request` was just handled.

⚠️ The most common bug is treating "fetched data → no rows → stop". That is **wrong** — empty just means no events arrived during this 300s window. Re-enter watch.

**Terminal signal — heuristic on `user_content` only.** A watch item is treated as a candidate terminal when its `user_content` contains any of these phrases: `本任务流程结束` / `任务完成` / `已验收通过` / `已退款` / `已关闭` / `已超时` / `已失败` (or the equivalent translated forms — `task completed` / `refunded` / `closed` / `expired` / `failed` / `dispute resolved`, etc.). This is a best-effort text match against the queue item only; `okx-a2a user watch` does NOT expose a structured `event` field, so do **not** look for or invent one.

⚠️ Heuristic over-trigger is harmless — false positives (e.g. `任务完成进度 50%`) only cause one extra `agent active-tasks` round-trip in the post-terminal flow below; they cannot incorrectly stop the watch because the **real** stop gate is "active-tasks is empty" (step 3), not the text match.

**After handling a candidate terminal item** (this is the ONLY path that may lead to a real stop):
1. `okx-a2a user list --json --limit 50` — if any are still pending, process those first, then re-enter watch.
2. Empty queue → `onchainos agent active-tasks`.
3. `totalTasks: 0` / `tasks: []` → briefly tell the user "no other active tasks; monitoring ends" and **stop** (this is the only "real stop" exit).
4. Still has active non-terminal tasks → re-enter `okx-a2a user watch ...`.
