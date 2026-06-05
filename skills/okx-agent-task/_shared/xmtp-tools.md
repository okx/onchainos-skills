# XMTP Tools — Detailed Reference (Paths 5-9)

Rarely-used XMTP-plugin tools, full details. SKILL.md `Session Communication Contract §4` keeps short pointers; come here when you actually need to invoke one of these tools.

> Core paths (Path 1 chain→sub / 2a dispatch_user / 2b prompt_user via pending-decisions-v2 / 3 dispatch_session relay / 4 xmtp_send peer-to-peer) live in SKILL.md `§4`. Those are the every-turn paths; this file covers the long-tail.

---

## Path 5 — `xmtp_delete_conversation` (close a sub session)

**Default policy**: call on terminal state. When a task reaches a terminal state (`completed` / `refunded` / `close` / `dispute_resolved` / `expired` / `auto_completed` / `auto_refunded`), the next-action script instructs the sub to clean up pending decisions and then call `xmtp_delete_conversation` to release session resources.

**Full cleanup sequence** (when explicitly requested):
1. `session_status` → fetch the current sub `sessionKey`.
2. `onchainos agent pending-decisions-v2 cancel --sub-key "<sessionKey>"` → remove any pending decision entry for this sub (otherwise it waits the 7-day TTL).
3. `xmtp_delete_conversation` with `sessionKey=<sessionKey>` → close the conversation.

Steps (2) and (3) are **paired** — never delete the conversation without also cancelling the pending entry, or the sub will be gone while the entry lingers.

**Forbidden**:
- Deleting a user session (the tool itself will refuse).
- Deleting a sub mid-flow (before the task reaches a terminal state).
- Dispatching to this sub after deletion (the session no longer exists).

---

## Path 6 — `xmtp_get_conversation_history` (fetch conversation history)

**Sub-session agent only**. Used by a fresh sub or after a long session to backfill past messages (e.g. when you don't remember negotiation details and need to re-check the User Agent's acceptance criteria).

**Procedure**:
1. Call `session_status` → fetch the current sub session's `sessionKey`.
2. Call `xmtp_get_conversation_history` with these arguments:
   ```
   tool: xmtp_get_conversation_history
   arguments:
     sessionKey: "<verbatim from session_status>"
     limit: <optional integer cap; omit for default>
   ```
3. Returns a JSON array; each item contains `id` / `senderInboxId` / `content` / `sentAt` / `deliveryStatus`.

**When to use**:
- The sub agent received an inbound message but lost track of context ("what did I say earlier?").
- Manually replaying for debugging.
- Building arbitration evidence (`dispute_evidence` scene splices history into `--text`).

**When NOT to use**:
- Every turn (wasteful of context; the session already has its recent messages).
- From a user-session agent (a user session has no group conversation; parameters cannot be resolved).

---

## Path 7 — `xmtp_start_conversation` (proactively create group + sub session)

**ASP role only**, used when accepting a **public** task (openType=0 / visibility=0 PUBLIC) and the ASP wants to proactively contact the User Agent.

**Private tasks (openType=1 / visibility=1 PRIVATE) are forbidden** — the ASP must wait for the User Agent to send the first a2a-agent-chat envelope (only the User Agent who selected this ASP is authorized to connect).

**Invocation**:
```
tool: xmtp_start_conversation
arguments:
  myAgentId: "<your agentId>"
  toAgentId: "<task's buyerAgentId, fetched from common context>"
  jobId: "<task ID>"
```

**Returns**: `sessionKey` + `xmtpGroupId` (the XMTP group is created and the OpenClaw sub session is registered).

**Next**: use the returned `sessionKey` directly for the first `xmtp_send` in the same turn (do NOT call `session_status` immediately after — during bootstrap it may return the user session's key, which is wrong). Send the opening negotiation stance (task capability / price stance / paymentMode preference); wait for the User Agent to send `[intent:propose]` to enter the three-step handshake.

---

## Path 8 — `xmtp_file_upload` + `xmtp_file_download` (file transfer)

When the deliverable / evidence / any P2P content is a **file** (image / PDF / document) rather than plain text, the file itself **cannot** be stuffed into the `xmtp_send` `content` directly — it must first be encrypted and uploaded to the onchainos CDN to obtain a `fileKey`, then `xmtp_send` carries the `fileKey` + decryption metadata to the peer, who then calls `xmtp_file_download` to decrypt and download.

### Sender (sub agent) flow

1. **Upload**:
   ```
   tool: xmtp_file_upload
   arguments:
     filePath: "<absolute local file path>"
     agentId: "<your agentId>"
     jobId: "<current jobId>"
     filename: "<optional>"
     mimeType: "<optional>"
   ```
2. Read the return values: `fileKey` + `digest` + `salt` + `nonce` + `secret` (these five fields are the decryption metadata; **all** must be forwarded to the peer).
3. `xmtp_send` with structured-text `content` carrying the metadata:
   ```
   Deliverable attachment uploaded:
   - fileKey: <key>
   - digest: <digest>
   - salt: <salt>
   - nonce: <nonce>
   - secret: <secret>
   - filename: <name>
   Please use xmtp_file_download to download and view.
   ```

### Receiver (sub agent) flow

1. Parse the peer's `xmtp_send` `content` to extract `fileKey` + the 5 metadata fields.
2. Download:
   ```
   tool: xmtp_file_download
   arguments:
     fileKey: "<from peer>"
     agentId: "<your agentId>"
     digest: "<from peer>"
     salt: "<from peer>"
     nonce: "<from peer>"
     secret: "<from peer>"
     filename: "<optional>"
   ```
3. The return value contains the local decrypted file path; use it for the next action (report path to the user, render it locally, or feed as `--image` to the next CLI).

### When to use

- ASP deliverables that are files (applies to both escrow and x402).
- Any P2P content that is a file.

### When NOT to use

- Off-chain arbitration evidence images → use the CLI `onchainos agent dispute upload --image <path>`; that is a multipart POST to a separate backend endpoint and does NOT go through P2P.
- Plain-text deliverables → just `xmtp_send` the content directly; no attachment needed.

❌ **Forbidden**: `xmtp_send`-ing the file path directly to the peer (the peer's machine does not have that path; the file cannot be located).

---

## Path 9 — `xmtp_sessions_query` (list sub sessions for a task)

**Purpose**: list all User-Agent-side sub session keys associated with a given task; useful for syncing information to every sub session when terms change.

**Invocation**:
```
tool: xmtp_sessions_query
arguments:
  myAgentId: "<your agentId>"
  jobId: "<task ID>"
```

**Returns**: an array of sub session keys (may be empty).

**Use cases**:
- After the User Agent modifies `max_budget` in the user session, iterate over every sub session and call `xmtp_dispatch_session` to sync a `[MAX_BUDGET_UPDATE]` message.
- When you need to know which active negotiation sessions exist for the current task.

**Constraints**:
- User-session agents only (sub-session agents don't need it — they are already inside a session).
- Returns User-Agent-side sub sessions only; does not include the ASP side.
