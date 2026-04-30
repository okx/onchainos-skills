---
name: okx-growth-competition
description: "Use this skill for trading competitions exclusive to Agentic Wallet: 'list trading competitions', 'show available competitions', 'join trading contest', 'register for competition', 'check my competition status', 'view leaderboard', 'check my ranking', 'claim competition reward', 'what competitions can I join', 'did I win', 'winners list', 'show registered wallet', 'export wallet'. Covers the full lifecycle: discover → view rules → join → trade → check rank → claim reward. Do NOT use for: general DEX swaps (use okx-dex-swap), portfolio balance / PnL queries outside a competition (use okx-wallet-portfolio or okx-dex-market), wallet login or transaction history (use okx-agentic-wallet), or any non-competition trading activity."
license: MIT
metadata:
  author: okx
  version: "1.2.0"
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
| 1 | `onchainos competition list [--status 0\|1\|2] [--page-size N] [--page-num N]` | None | List Agentic Wallet exclusive competitions (default status=0, active only) |
| 2 | `onchainos competition detail --activity-id <id>` | None | Get rules, prize pool, chain, timeline |
| 3 | `onchainos competition rank --activity-id <id> --wallet <addr> --sort-type <5\|7\|8> [--limit N]` | None | Leaderboard + user rank |
| 4 | `onchainos competition user-status [--activity-id <id>] --evm-wallet <evm_addr> --sol-wallet <sol_addr>` | None | Check participation & reward status; uses chain-appropriate address (omit --activity-id to check all activities) |
| 5 | `onchainos competition join --activity-id <id> --evm-wallet <addr> --sol-wallet <addr> --chain-index <chain_id>` | Wallet login | Register for the competition |
| 6 | `onchainos competition claim --activity-id <id> --evm-wallet <addr> --sol-wallet <addr>` | Wallet login | Get reward calldata for on-chain submission |

`--status` (request filter): `0`=active, `1`=ended, `2`=all  
`activityStatus` (response field): **`3`=active, `4`=ended** — these are DIFFERENT values from the request filter  
`sort-type`: `5`=volume, `7`=realized PnL, `8`=boost token volume

## Output Rules

<NEVER>
**Never expose internal IDs to users — under ANY circumstance, in ANY format.** This applies to `activityId`, `chainIndex`, `accountId`, and any numeric/hash IDs in API responses. They are for internal tool-call parameters only. Identify activities to the user EXCLUSIVELY by `activityName` (or `shortName` if name is unavailable).
</NEVER>

**Forbidden patterns** (do NOT produce output like this):
- ❌ `Agentic Trading Contest (#107)`
- ❌ `#106 (agenticwallettest1)`
- ❌ A column titled "ID" / "活动ID" / "#"
- ❌ Any reference like "活动 #107" / "competition 107" / "id 107"

**Correct pattern**:
- ✅ `Agentic Trading Contest`
- ✅ When disambiguating two activities with the same name, append `chainName` (e.g. `Agentic Trading Contest (Solana)`), never the ID.

When the user asks to act on a specific activity (e.g. "claim Agentic Trading Contest"), match by `activityName` from the previous tool result and pass its `activityId` internally.

## Execution Flow

### Step 1 — Discover Competitions

```bash
onchainos competition list --status 0
```

Display as a numbered list. For each competition:
- **Name**: `name` field
- **Rewards**: `rewards`
- **Chain**: `chainName`
- **Time**: `startTime` ~ `endTime` (human-readable)
- **Link**: `https://web3.okx.com/boost/trading-competition/<shortName>`

Example response format:
> There are 3 active trading competitions:
> 1. **XXX Trading Competition**, prize 50,000 HIPPO, chain Arbitrum One, 4/1 ~ 4/30, [View details🔗](https://web3.okx.com/boost/trading-competition/xxx)
> 2. ...

Ask: "Which competition would you like to view in detail, or would you like to register directly?"

### Step 2 — View Details (if requested)

```bash
onchainos competition detail --activity-id <id>
```

Display (required fields):
- **Name**: `name`
- **Chain**: `chainName` (`chainId`)
- **Time**: `startTime` ~ `endTime` (human-readable)
- **Rules** (grouped by Tab):
  - For each tab: `tabDetails[].title` + `tabDetails[].desc` (paragraphs separated by `\n`)
- **Prize pool** (grouped by Tab):
  - `prizePoolDistribution[].totalReward` + `rewardUnit` (total prize pool)
  - Per rank: `rules[].interval` → `reward` + `rewardUnit`

Example response format:
> The total prize pool for **XXX Competition** is 100,000 HIPPO. The top performer by ROI receives 500 HIPPO, and the top performer by realized PnL receives 300 HIPPO. There is also a participation prize — any account with effective trading volume above $100 has a chance to share the pool.
> Note: the competition ends at **2025-04-30 08:00**.
> Would you like me to register you for this competition?

### Step 3 — Join (requires wallet login)

**Resolve wallet addresses:**
1. `onchainos wallet status` — check login; if not logged in → `onchainos wallet login`
2. Use the currently active account's EVM address (XLayer) and SOL address

Get `chainIndex` from `competition detail` → `chainIndex` field.

```bash
onchainos competition join --activity-id <id> --evm-wallet <evm_addr> --sol-wallet <sol_addr> --chain-index <chain_id>
```

**On success:**
> Registration successful. All trades from your Solana address [first4...last4] and XLayer address [first4...last4] will count toward the competition. You can ask the Agent for your ranking at any time.

**On error `address limit reached`:**
> Registration failed: this wallet account is already registered and cannot register again. Please switch to your registered account to trade.

**On error containing `region` / `not available in your region`:**
> Registration failed: service is not available in your region. Please switch to a supported region and try again.

**On any other error:**
> Operation failed. Please contact customer support.

**Duplicate join guard:** Before calling `join`, run `user-status` to check if `joinStatus=1`. If already joined, block and show the "already registered" message above without calling the API.

### Step 4 — Trade (delegate to okx-dex-swap)

When user asks to trade per competition rules:

**Case A — User does NOT provide a CA (only token name/symbol):**
1. Search via `onchainos token search` to resolve the CA.
2. Confirm with user before proceeding:
   > Just to confirm, the CA for token ** is ***. Is that correct?
3. Wait for user to confirm. Only proceed after explicit "yes".
4. Then follow **Case B** below.

**Case B — User provides a CA directly:**
1. **Risk warning:**
   > Please note that token prices can be highly volatile and trading may result in losses. Do you understand the risk and want to proceed?
2. Wait for explicit confirmation before proceeding.
3. **Execute swap** via `onchainos swap execute` (see `okx-dex-swap` skill).
4. Report: "Done — your trade has been submitted." + tx hash.

**Competition constraints per trade:**
- Single-trade min $1 (orders below $1 are not counted)
- Token pairs must match competition rules from `detail` response

### Step 5 — Check Status & Rank

#### Check participation status

```bash
onchainos competition user-status --evm-wallet <evm_addr> --sol-wallet <sol_addr>                       # all activities
onchainos competition user-status --activity-id <id> --evm-wallet <evm_addr> --sol-wallet <sol_addr>   # single
```

Display: join status, join time, reward status, reward amount.

- If `rewardStatus=1` (won, not claimed): proactively ask "You have won a reward. Would you like me to claim it for you?"
- If `rewardStatus=3` (expired): "Your reward has expired and can no longer be claimed."

#### Check leaderboard (full board)

```bash
onchainos competition rank --activity-id <id> --wallet <addr> --sort-type <type> --limit 20
```

Display top N entries. For each entry: rank, nickname (masked), score (`userTotal`), estimated reward.

Example response:
> Here is the Top 20 leaderboard by ROI:
> Rank 1, address Agen...abcd, ROI +125.3%, estimated reward 500 HIPPO
> Rank 2, ...

#### Check user's own rank

Run `rank` and look at `myRankInfo`:
- **On leaderboard** (`currentRank > 0`):
  > Your rank by realized PnL is #25, estimated reward 100 HIPPO.
- **Not on leaderboard** (`currentRank=0` or missing):
  > Your address is not on the leaderboard. ROI / realized PnL must reach at least XX to qualify. You will receive a participation reward — the exact amount will be announced after the competition ends.

`userTotal` meaning by `sort-type`: `5`=trade volume, `7`=realized PnL, `8`=boost token volume.  
`format`: `1`=number, `2`=percentage, `3`=token amount with unit.

### Step 6 — Claim Reward

Check status first:
```bash
onchainos competition user-status --activity-id <id> --evm-wallet <evm_addr> --sol-wallet <sol_addr>
```

| `rewardStatus` | Action |
|---|---|
| 0 | Not won — inform user, no claim needed |
| 1 | Won — proceed to claim |
| 2 | Already claimed |
| 3 | Expired — "Your reward has expired and can no longer be claimed" |

```bash
onchainos competition claim --activity-id <id> --evm-wallet <evm_addr> --sol-wallet <sol_addr>
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

**On claim error (code 11002 `not eligible for reward`):** "You did not win a reward and cannot claim."  
**On any other error:** "Operation failed. Please contact customer support."

## Additional Flows

### Query Registered Wallet

When user asks "show my registered address" or similar:

1. `onchainos wallet status` — get active account and addresses
2. Run `onchainos competition user-status --evm-wallet <evm_addr> --sol-wallet <sol_addr>` (all activities, omit `--activity-id`)
3. Find entries where `joinStatus=1`
4. For each matched entry, present: competition name (`activityName`) + chain (`chainName`) + masked address (first4...last4). Use chain to determine which address was used (EVM or SOL).

If multiple entries match, list all of them.

Example (single):
> Your Account 1 is registered for **XXX Trading Competition**. Registered address: Solana address DeEV...Fbx.

Example (multiple):
> Your Account 1 is registered for the following trading competitions:
> - **XXX Trading Competition** (Solana): DeEV...Fbx
> - **YYY Trading Competition** (XLayer): 0x1234...abcd

If no entry has `joinStatus=1`:
> You are not currently registered for any trading competition.

### Wallet Export Guard

When the user requests to export the Agentic Wallet:

1. Check `onchainos competition user-status --evm-wallet <evm_addr> --sol-wallet <sol_addr>` for any active competition
2. If any `joinStatus=1`:
   > Your wallet is registered for an Agentic Wallet trading competition. Exporting the wallet will forfeit your eligibility for this competition. Please confirm whether you want to proceed with the export.
3. Only proceed with export if the user explicitly confirms.

## Status Codes

### `--status` filter parameter (input only)

| Value | Meaning |
|-------|---------|
| 0 | Active competitions (default) |
| 1 | Ended competitions |
| 2 | All competitions |

### Response field values

| Field | Value | Meaning |
|-------|-------|---------|
| status | 3 | Competition active |
| status | 4 | Competition ended |
| joinStatus | 0 | Not joined |
| joinStatus | 1 | Joined |
| rewardStatus | 0 | Not won |
| rewardStatus | 1 | Won, not claimed |
| rewardStatus | 2 | Claimed |
| rewardStatus | 3 | Reward expired |

## Error Handling

| Error | Response |
|-------|----------|
| `not logged in` | Run `onchainos wallet login` |
| `address limit reached` | Registration failed: this wallet account is already registered and cannot register again |
| code 11002 `not eligible for reward` | You did not win a reward and cannot claim |
| `region` / `not available in your region` | Registration failed: service is not available in your region. Please switch to a supported region and try again. |
| Any other error | Operation failed. Please contact customer support. |

## Output Language

Respond in the user's language (Chinese if Chinese, English otherwise). CLI output is always JSON — translate field values when presenting to user.
