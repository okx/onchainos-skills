---
name: okx-growth-competition
description: "Use this skill for trading competitions exclusive to Agentic Wallet: 'list trading competitions', 'show available competitions', 'join trading contest', 'register for competition', 'check my competition status', 'view leaderboard', 'check my ranking', 'claim competition reward', 'what competitions can I join', '查看交易赛', '参加交易赛', '报名交易赛', '查看排名', '领取交易赛奖励', '我的竞赛状态'. Covers the full lifecycle: discover → view rules → join → trade → check rank → claim reward."
license: MIT
metadata:
  author: okx
  version: "1.1.0"
  homepage: "https://web3.okx.com"
---

# OKX Growth Competition — Trading Competition

Agentic Wallet exclusive trading competitions. Full lifecycle: list → detail → join → trade → rank → claim.

CLI reference: `references/cli-reference.md`

## Pre-flight

> Read `../okx-agentic-wallet/_shared/preflight.md`. If missing, read `_shared/preflight.md`.

## Command Index

| # | Command | Auth | Description |
|---|---------|------|-------------|
| 1 | `onchainos competition list [--status 0\|1\|2] [--page-size N] [--page-num N]` | None | List Agentic Wallet exclusive competitions |
| 2 | `onchainos competition detail --activity-id <id>` | None | Get rules, prize pool, chain, timeline |
| 3 | `onchainos competition rank --activity-id <id> --wallet <addr> --sort-type <5\|7\|8> [--limit N]` | None | Leaderboard + user rank |
| 4 | `onchainos competition user-status --activity-id <id> --wallet <addr>` | None | Check participation & reward status |
| 5 | `onchainos competition join --activity-id <id> --wallet <addr>` | Wallet login | Register (nickname auto: "Agentic....{last4}") |
| 6 | `onchainos competition claim --activity-id <id> --wallet <addr>` | Wallet login | Get reward calldata for on-chain submission |

`status`: `0`=active, `1`=ended, `2`=all (omit=all)
`sort-type`: `5`=volume, `7`=realized PnL, `8`=boost token volume

## Execution Flow

### Step 1 — Discover Competitions

```bash
onchainos competition list --status 0
```

Display: name, rewards, **chain** (`chainName`), start/end time, activity URL `https://web3.okx.com/boost/trading-competition/<shortName>`.

Ask: "Would you like to see details or join one?"

### Step 2 — View Details (if requested)

```bash
onchainos competition detail --activity-id <id>
```

Display (required fields):
- **Chain**: `chainName` (e.g. Arbitrum One)
- Rules: `tabDetails[].title` + `tabDetails[].desc`
- Prize pool: `prizePoolDistribution[].rules` (rank range → reward amount + `rewardUnit`)
- End time: `endTime` (format as human-readable date)

Ask: "Shall I register you for this competition?"

### Step 3 — Join (requires wallet login)

Resolve wallet address:
1. `onchainos wallet status` — check login
2. If not logged in → `onchainos wallet login` → verify OTP
3. `onchainos wallet addresses` → pick EVM address matching competition's `chainId`

Nickname is **automatically set** to `"Agentic....{last4 of address}"` — do NOT ask user to choose or modify.

```bash
onchainos competition join --activity-id <id> --wallet <addr>
```

On success: confirm registration with nickname shown. Suggest: "I can now trade according to competition rules — would you like me to start?"

On error `address limit reached`: inform user one address per account is allowed.

### Step 4 — Trade (delegate to okx-dex-swap)

When user asks to trade per competition rules:
- Resolve token pairs from `detail` response (participation requirements section)
- Execute swap via `onchainos swap execute` (see `okx-dex-swap` skill)
- Remind user: single-trade min $1, must use OKX DEX Aggregator routing

### Step 5 — Check Status & Rank

```bash
onchainos competition user-status --activity-id <id> --wallet <addr>
onchainos competition rank --activity-id <id> --wallet <addr> --sort-type 5
```

Display user's join status, current rank, estimated reward, distance to next rank tier.

Run both together when user asks about their competition status.

### Step 6 — Claim Reward

Check status first:
```bash
onchainos competition user-status --activity-id <id> --wallet <addr>
```

- `rewardStatus=0` (not won) → inform user, no claim needed
- `rewardStatus=1` (won) → proceed to claim
- `rewardStatus=2` (claimed) → already claimed
- `rewardStatus=3` (expired) → warn user reward has expired

```bash
onchainos competition claim --activity-id <id> --wallet <addr>
```

Response is an array of calldata objects. For each entry, submit via `onchainos wallet contract-call`:
```bash
onchainos wallet contract-call \
  --to <contractAddress> \
  --chain <chain> \
  --input-data <input> \
  --amt <value>
```

Map `chainId` to chain name using `onchainos wallet chains`. Report: token symbol, amount claimed, transaction hash(es).

## Status Codes

| Field | Value | Meaning |
|-------|-------|---------|
| status | 0 | Competition active (进行中) |
| status | 1 | Competition ended (已结束) |
| status | 2 | All (全部) |
| joinStatus | 0 | Not joined |
| joinStatus | 1 | Joined |
| rewardStatus | 0 | Not won |
| rewardStatus | 1 | Won, not claimed |
| rewardStatus | 2 | Claimed |
| rewardStatus | 3 | Reward expired |

## Error Handling

Any API error → show error message directly to user (do not retry automatically).

Common errors:
- `not logged in` → run `onchainos wallet login`
- `address limit reached` → one address per user per competition
- `not eligible for reward` (code 11002) → user did not win

## Output Language

Respond in the user's language based on their message language (Chinese if Chinese, English otherwise). CLI output is always JSON — translate field values when presenting to user.
