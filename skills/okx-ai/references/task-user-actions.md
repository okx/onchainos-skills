# User's User-Session Actions

> 🛑 **Entry boundary**: load this file only after `task-user-intent-routing.md` selects the action. Read the relevant execution boundary in `task-user-playbook.md` when the selected action requires it. 🌐 All user-facing content must match the user's language.
> 🛑 **Universal confirmation rule**: every modification MUST be confirmed individually before execution. Multiple changes in one sentence → split into steps, confirm each. ❌ Batch-executing = user cannot review.

---

## Quick Navigation

| Section | When to read |
|---|---|
| §2 Mid-task attachment | User wants to add files to an active task |
| §3 Terms changes | stop task |
| §4 View deliverables | User wants to see submitted deliverables |

---

## 2. Mid-task attachment (user session)

**Trigger**: add a file/image to a task / attach this to a job / upload a file to a task, or the user sends a file during an active task conversation (confirm intent first).

**Flow**:

1. **Task disambiguation**: **always confirm which task**, even if only one is active — ask the user to specify the jobId or pick from the list (`onchainos agent active-tasks` — already safe across all identities; falls back to `onchainos agent tasks --agent-id <myAgentId>` when a specific agent's history is needed).
2. 🛑 **Save locally via CLI**: `onchainos agent task-attach <jobId> --file <path>` — the CLI **internally checks the task status** before saving. If the task is in submitted or later state (status≥2), the CLI **rejects** the operation. **File size limit: 100 MB per file.**
   - **CLI returns error** → 🛑🛑🛑 **STOP immediately**. Inform the user that the task has entered the review/terminal phase and attachments can no longer be added. **Do NOT proceed to step 3.** **Do NOT save the file manually.**
   - **CLI returns success** → continue to step 3.
   - ❌ **ABSOLUTE PROHIBITION**: when `task-attach` returns an error, **forbidden** from using shell commands (`mkdir`, `cp`, `mv`) to save files or dispatching `[ATTACHMENT_ADDED]` to the sub session.
3. 🛑 **Forward to sub session (MUST NOT SKIP)**: dispatch via `okx-a2a session send` — the daemon resolves the active sub session from `--job-id` + `--to-agent-id`:
   ```bash
   okx-a2a session send \
     --job-id <jobId> --to-agent-id <providerAgentId> \
     --content "[ATTACHMENT_ADDED] <file path from task-attach output>" \
     --json
   ```
   ❌ Stopping after step 2 without dispatching = the attachment is stuck locally. ❌ Using any other prefix = sub session cannot recognize the message.
   - If no sub session exists (task not yet matched with a provider), tell the user the file is saved and will be forwarded once a provider is matched.
4. **Confirm to user**: inform the user the attachment has been saved and forwarded (or "saved and will be forwarded once matched").

---

## 3. Terms changes (user session)

> **Pre-condition**: the task is in the **Created** state (before Accepted). After Accepted, terms are locked and modification requests are refused.

🛑 **Priority rule**: user instruction > automated flow. Terms-change or stop from user → immediately interrupt and handle first.

### 3.1 Stop task

**Trigger**: "stop task" / "close task"

1. Run the read-only `onchainos agent refund-prepare <jobId>` and render the
   returned task/refund details and action. Fresh Refund V2 state decides whether
   this is a zero-price close, a paid direct refund, a trial cancellation, or a
   blocked contract gap.
2. Ask for explicit confirmation of the exact returned write action. On
   confirmation, execute its unchanged `operation` and `refundContextId` through
   `onchainos agent refund-execute ... --confirm`.

Never call legacy `agent close`; it is registered only to return deterministic
migration guidance and performs no network write.

### 3.2 Other non-terms input

User messages unrelated to terms → sync to the user session as context; do NOT trigger any API.

---

## 4. View deliverables (user session)

The user wants to see saved deliverables from completed or in-progress tasks.

> This section applies to both user and ASP roles. Use `--role user` or `--role asp` based on the current role.

**Trigger**: "view deliverables", "my deliverables", "list deliverables", "show deliverable for job X"

**Step 1 — Determine scope**:
- If the user specifies a jobId → single job query
- If the user says "all" / "list" / no specific job → list all

**Step 2 — Run the CLI** (substitute `<role>` with `user` or `asp`):

- Single job: `onchainos agent task-deliverable-list --job-id <jobId> --role <role>`
- All / search: `onchainos agent task-deliverable-list --role <role> [--search "<keyword>"]`

**Step 3 — Present results directly to the user** (🌐 translate labels to user's language):

- Single job: list each entry with `originalName`, `deliverableType`, `sizeBytes` (human-readable), absolute `path`, `savedAt`.
- All jobs: group by job (`title` + `jobId`), show `deliverableCount` + each file's `originalName` and absolute `path`.
- Empty → "No saved deliverables found."
- ⚠️ File paths MUST be absolute.
