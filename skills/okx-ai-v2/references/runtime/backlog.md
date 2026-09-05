# Runtime Backlog

Owns one-time decision-list reads and replies. It does not start a long poll.

| Intent | Route |
|---|---|
| `decision list`, `show decision list`, or full pending-decision queue | Run `onchainos agent pending-decisions-v2 list --format markdown` and follow the returned playbook verbatim. |
| Outstanding, unanswered, or unhandled decisions | Read [`watch-outdated-list.md`](watch-outdated-list.md). |
| Message history, unread task messages, or catch-up | Read [`watch.md`](watch.md); its history entry drains the unread backlog before waiting. |

When the context contains an active `[USER_DECISION_REQUEST]`, the user's reply
belongs to that card before any new free-text intent:

- For one visible card, run its pre-filled `resolve-prompt` command with the
  user's reply verbatim.
- For multiple cards, use an explicit Job ID or label to select the matching
  block. Ask which task only when the reply remains ambiguous.
- For a card originating from live Watch, follow
  [`watch.md` §Handling the user reply](watch.md#handling-the-user-reply--concurrency-safe-llmcontent-execution),
  including its claim and watch-resume rules.
