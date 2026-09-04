# Progression Contract

Use this envelope when a CLI result requires continuation:

```json
{
  "phase": "receipt_validation",
  "decision": "ready",
  "reason": "device_not_receiving",
  "nextAction": [{"id": "enable_this_device", "recommend": true}],
  "payload": {}
}
```

- `phase`: current business stage.
- `decision`: `ready`, `blocked`, or `requires_user_input`.
- `reason`: machine-readable reason.
- `nextAction`: currently allowed stable actions.
- `payload`: current facts.

Route by `decision`; never infer state from human-readable text.
