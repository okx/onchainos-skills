# Heartbeat

No background tasks are configured by default.

Respond to heartbeat polls with `HEARTBEAT_OK`.

## Optional scheduled tasks

To add a scheduled task, add an entry to the `tasks` array in `manifest.json`.
The template store uses this task format:

```json
"tasks": [
  {
    "name": "daily-brief",
    "schedule": { "kind": "cron", "expr": "0 9 * * *" },
    "payload": {
      "kind": "agentTurn",
      "text": "Run the daily brief workflow for Solana and share a morning summary."
    }
  }
]
```

Common schedules:
- `"0 9 * * *"` — 9am daily
- `"0 */4 * * *"` — every 4 hours
- `"0 9 * * 1"` — every Monday at 9am
