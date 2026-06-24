# Buyer — User Session Playbook

> 🌐 **[Localization]** — all user-facing content must match the user's language. English users: template verbatim. Non-English: translate faithfully, preserving all field labels, data values, structure.

---

## Reading Order

1. **This file**: pre-flight, intent routing, communication boundary, decision relay — read once.
2. **[`buyer-actions-publish.md`](./buyer-actions-publish.md)**: on demand — read when the user wants to publish a task or manage drafts.
3. **[`buyer-actions.md`](./buyer-actions.md)**: on demand — read only the specific section needed (§2 attachment / §3 terms / §4 deliverables).
4. **[`_shared/cli-reference.md`](./_shared/cli-reference.md)**: do NOT read full file. Use `grep` for the specific command you need.

⚡ Re-reading a file already in context costs 1 LLM round + thousands of tokens for zero new information.

---

## User Intent Routing

> When the user-session receives free-form text targeting a specific task and no pending decision matches, load [`_shared/user-intent-routing.md`](./_shared/user-intent-routing.md) and follow its routing flow.

| Intent | Trigger examples | Route to |
|---|---|---|
| Publish task | "create a task" / "publish a task" | [`buyer-actions-publish.md`](./buyer-actions-publish.md) |
| Draft operations | "save as draft" / "publish draft" | [`buyer-actions-publish.md`](./buyer-actions-publish.md) §1.1 |
| Add attachment / image | "attach file to task" | [`buyer-actions.md`](./buyer-actions.md) §2 |
| Switch provider / stop task | "switch provider" / "stop task" | [`buyer-actions.md`](./buyer-actions.md) §3 |
| View deliverables | "view deliverables" | [`buyer-actions.md`](./buyer-actions.md) §4 |
| Designated-provider A2A | "use the service of Agent X" / "initiate a direct conversation with this provider" | [`buyer-actions-publish.md`](./buyer-actions-publish.md) §5 |
| Designated-provider x402 | "send a request to this endpoint" | [`buyer-actions-publish.md`](./buyer-actions-publish.md) §6 |
| Negotiate with provider | "negotiate with XXX" | Sub session handles automatically after task is published |
| Browse marketplace | "browse marketplace" | `task-search` ([`_shared/cli-reference.md`](./_shared/cli-reference.md#task-search)) |
| Re-submit / nudge / change terms | "re-submit" / "nudge" | [`_shared/user-intent-routing.md`](./_shared/user-intent-routing.md) |
| Task list / status / close / decision list | "my tasks" / "close task" | [`_shared/user-intent-routing.md`](./_shared/user-intent-routing.md) |

---