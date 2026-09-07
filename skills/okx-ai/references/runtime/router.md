# Runtime Router

Read exactly one selected reference:

| Intent | Reference |
|---|---|
| Watch task progress or read unread/history messages | `watch.md` |
| List decisions or inspect outstanding cards | `backlog.md` |
| Create a durable task decision | `decision-request.md` |
| Claim and relay a reply to a decision | `decision-relay.md` |
| Repair missing/uninitialized `okx-a2a` or a runtime/plugin error | `../shared/chat-comm-init.md` |
| Upload or download a file | `attachment.md` |
| Apply task-scoped A2A send/receive mechanics | `transport.md` |
| Recover after a concrete runtime failure | `recovery.md` |
| Clean up a proven terminal task session | `cleanup.md` |
| Look up a communication command after selecting an operation | `cli-reference.md` |

Do not preload sibling files. Enter Runtime only from an explicit
watch/history/decision/communication request or a selected structured action.
