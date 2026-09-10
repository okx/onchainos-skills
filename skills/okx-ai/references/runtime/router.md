# Runtime Continuation Router

Use this router only for a bound Runtime continuation when the upstream
reference, structured action, or CLI result has not already named the final
leaf. Never enter from standalone free text or re-enter when an upstream
reference links the final leaf directly.

Read exactly one selected reference:

| Bound context or action | Reference |
|---|---|
| A task sub-session must create a durable User decision | `decision-request.md` |
| The User replies to a concrete surfaced decision | `decision-relay.md` |
| A business leaf selected task-scoped A2A send/receive mechanics | `transport.md` |
| An owning leaf routes a concrete runtime failure | `recovery.md` |
| A terminal action or workflow explicitly requires cleanup | `cleanup.md` |
| A selected communication operation requires command details | `cli-reference.md` |

Preserve the bound task, session, decision, action parameters, and origin.
Never infer an internal operation from prose or preload sibling files. A
missing mapping is a coverage failure—report it and stop.
