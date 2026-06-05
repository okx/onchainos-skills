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

  3. **Resume watching** — call `okx-a2a user watch --once --json --timeout 300 --poll-ms 1000 --limit 50` again. No relay, no `llm_content` thinking.

- **`kind == decision_request`** — render `user_content` to the user verbatim (same anti-paraphrase rules as `notification` above), **and treat `llm_content` as the current turn's instruction set to think about and execute**. The user's reply is the input to that thinking.

### `decision_request`: rendering choices

Each JSON item already carries a `choices` array auto-derived by the CLI from `user_content` (recognizing `请回复「xxx」` / `请选择` followed by a numbered or lettered list). If `choices` is missing or empty, parse `user_content` yourself by the same rules and always append `自定义回复`. `decision_request` items must always allow an open-ended reply even when no parsed choices exist.

Choice semantics: `保留` / `稍后` / `暂不` / `skip` → keep pending; everything else → reply (treated as the user's verbatim answer to this item, which triggers `llm_content` thinking via the flow below).

### `decision_request`: handling the user reply — concurrency-safe relay

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
- A `decision_request` was just handled — relay completed (step 3) / `alreadyHandled` (step 4) / claim-succeeded-but-relay-failed (step 5). **Note**: `保留` / `skip` (step 1) is a STOP, listed above.
- Watch returned 0 items (empty result / `--timeout 300` elapsed with no events) — re-enter watch and keep waiting.
