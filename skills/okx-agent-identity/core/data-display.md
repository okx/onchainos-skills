# Amount Display Rules

## Service fee

- Format: **USDT numeric string up to 6 decimal places** (e.g. `1.234567`, `10`, `0.5`, `0`). Always show as "`N USDT`" to the user.
- **A2MCP**: `fee` is required. Pass user-typed value verbatim to CLI.
- **A2A**: `fee` is optional. If empty, CLI sends `"fee": ""` (key always present per `core/cli-create.md §1`). Render as:
  - Non-empty: `<N> USDT` (same as A2MCP).
  - Empty / absent: `免费` / `free` in user language. In confirm/diff cards where space allows: `（未填，双方自行协商）` / `(skipped — negotiated directly)`.

## Evaluator stake

Owned by `okx-agent-task`. **Never hardcode the amount here.** Point users to `/skills/okx-agent-task/references/evaluator-staking.md`.

## EVM addresses

Display all lowercase.

## Reputation stars (0.00–5.00)

Backend wire format is 0–100 integers. Conversion rule: `score / 20`, up to 2 decimal places. Because wire unit = 0.05 stars, no further rounding needed after division.

Examples: `0→0`, `66→3.3`, `67→3.35`, `70→3.5`, `89→4.45`, `92→4.6`, `100→5`.

**Conversion responsibility by endpoint:**

| Endpoint | Field | Who converts | Skill action |
|---|---|---|---|
| `agent search` | `list[*].feedbackRate` | Backend (already 0–5 Double) | Render directly — ⛔ no `/20` |
| `agent feedback-list` | `average`, `items[*].score`, `list[*].score` | CLI (`utils::convert_feedback_list_scores`) | Render directly — ⛔ no `/20` |
| `agent feedback-submit` | `--score` input | CLI (`utils::parse_stars_arg`, ×20) | Pass user's stars straight to `--score` — ⛔ no multiplication |
| `agent get` | `list[*].agentList[*].reputation.score` | ⚠️ Skill-side (raw 0–100) | Divide by 20, up to 2 dp |

**No-data / zero**: `agent search` feedbackRate follows this two-way rule:
- `feedbackRate` is `null`, absent, or `== 0` → render `暂无评分` / `No rating yet` (no reviews have been submitted yet)
- `feedbackRate > 0` → render `★ <feedbackRate>` (up to 2 decimal places, trailing zeros trimmed)

For `agent get` list view, `reputation.score` follows the same intent: `score == 0` or absent after `/20` conversion → `暂无评分` / `No rating yet`.

⛔ Never render `92 / 100` / `85 分` or the raw 0–100 integer in any user-visible cell or message.
