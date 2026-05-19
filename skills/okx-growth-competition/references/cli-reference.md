# competition CLI Reference

All commands: `onchainos competition <subcommand> [flags]`

---

## competition list

List Agentic Wallet exclusive trading competitions.

```
onchainos competition list [--status <0|1|2>] [--page-size <n>] [--page-num <n>]
```

**API**: `GET /priapi/v1/dapp/agentic/competition/list`

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--status` | int | — | 0=active, 1=ended, 2=all; omit for all |
| `--page-size` | int | 10 | Results per page |
| `--page-num` | int | 1 | Page number (1-based) |

**Output:**
```json
{
  "availableCompetitions": [
    {
      "id": 100,
      "shortName": "hippo",
      "name": "HIPPO Trading Competition",
      "rewards": "50000 HIPPO",
      "startTime": 1742913600,
      "endTime": 1743432000,
      "chainId": 196,
      "chainName": "X Layer",
      "status": 3
    }
  ],
  "totalCount": 2
}
```

**Note**: Response `status` field uses different values from the query param:
- Query param: `0`=active, `1`=ended, `2`=all
- Response field: `3`=active, `4`=ended

Activity URL: `https://web3.okx.com/boost/trading-competition/<shortName>`

---

## competition detail

Get competition rules, prize pool, and timeline.

```
onchainos competition detail --activity-id <id>
```

**API**: `GET /priapi/v1/dapp/agentic/competition/detail`

| Flag | Required | Description |
|------|----------|-------------|
| `--activity-id` | Yes | Activity ID from `competition list` |

**Output:** Competition object. Key fields:
- `chainId` / `chainName`: the activity's **primary chain** — it is BOTH a trading chain (its trades count toward the competition standing) AND the **claim chain** (rewards / activity contract live here).
- `participateChainIds`: array of **additional trading chains** beyond `chainId` (e.g. with `chainId=196` and `participateChainIds=[501]`, trades on both X Layer and Solana count). Returned by **both `list` and `detail`** endpoints. May be empty on activities created before the field was added. Trading-eligibility = `{chainId} ∪ participateChainIds` (dedup). Claim path = `chainId` only.
- `startTime` / `endTime`: 10-digit Unix timestamps (raw — kept for backward compat, not recommended for display)
- `startTimeFormatted` / `endTimeFormatted`: pre-formatted UTC+8 strings (`yyyy-MM-dd HH:mm:ss`, e.g. `"2026-05-07 18:00:00"`) — **use these for display**, just append ` (UTC+8)` for the timezone suffix; do not recompute from epoch
- `tabConfigs[]`: one entry per leaderboard tab
  - `tab`: `1`=volume, `3`=realized PnL, `4`=boost token volume
  - `tabDetails[].title` / `tabDetails[].desc`: rules text (paragraphs separated by `\n`)
  - `prizePoolDistribution[].rules[].interval`: rank range (e.g. `"1"`, `"4-10"`)
  - `prizePoolDistribution[].rules[].reward`: reward amount for that range
  - `prizePoolDistribution[].rewardUnit`: reward token symbol
  - `prizePoolDistribution[].totalReward`: current total prize pool
  - `prizePoolDistribution[].rewardType`: `5`=volume pool, `7`=PnL pool, `8`=boost token pool
  - `rankFieldConfig[]`: column definitions for the leaderboard table

---

## competition rank

Get leaderboard and current user ranking.

```
onchainos competition rank --activity-id <id> [--wallet <addr>] --sort-type <type> [--limit <n>]
```

**API**: `GET /priapi/v1/dapp/agentic/competition/rank`

> The backend takes either `accountId` (self-query) or `walletAddress` (cross-user query) — never both. Omit `--wallet` to query your own rank; the command loads `accountId` from the active wallet session. Pass `--wallet` only to query someone else's rank; the address chain (EVM `0x...` else Solana) must match the activity chain or the command errors out (no silent wrong-chain query).

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--activity-id` | Yes | — | Activity ID |
| `--wallet` | No | (uses active account's `accountId` instead) | Optional wallet address — pass to query someone else's rank (chain-validated against the activity). |
| `--sort-type` | Yes | 1 | Currently observed: 1=PnL% (realized ROI), 7=PnL (realized profit). Future activities may add more — discover via `competition detail` → `tabConfigs[].rankFieldConfig[].sortValueMap.descend`. |
| `--limit` | No | 20 | Max entries in `allRankInfos` (max 100; applied client-side) |

**Output:**
```json
{
  "myRankInfo": {
    "currentRank": 42,
    "nickName": "Agentic...abcd",
    "userTotal": "1250.5",
    "expectedRewards": "100",
    "format": 1,
    "rewardUnit": "HIPPO"
  },
  "allRankInfos": [ ... ],
  "rankUpdateTime": 1774359000638,
  "agenticActivity": true,
  "totalRewardToken": "1000000",
  "rewardTokenSymbol": "HIPPO"
}
```

`format`: `1`=number, `2`=percentage, `3`=token amount with unit

`userTotal` meaning is dictated by the activity's `tabConfigs[].rankFieldConfig[]` — read `title` (display name) and `key` (internal field) from there. Currently observed metrics: PnL% (`pnl`, sort-type 1), PnL (`realizedProfit`, sort-type 7).

`rankUpdateTime`: milliseconds (13-digit timestamp)

---

## competition user-status

Get user's participation and reward status.

```
onchainos competition user-status [--activity-id <id>]
```

**API**:
- Single activity (`--activity-id` provided) → `GET /priapi/v1/dapp/agentic/competition/userStatus`
- All activities (`--activity-id` omitted) → `GET /priapi/v1/dapp/agentic/competition/batchUserStatus` (chunked at 20 ids per call, results merged transparently)

> The CLI sends `accountId` (loaded from the local wallet session) as the API identity, NOT a wallet address. One `accountId` covers every chain in the competition's `participateChainIds` — no chain picking, no wallet args. The batch endpoint replaces per-activity loops with a single (chunked) round-trip.

| Flag | Required | Description |
|------|----------|-------------|
| `--activity-id` | No | Activity ID; omit to check **all** activities (active + ended) |

When `--activity-id` is omitted, the CLI calls `competition list --status 2` first to get all activity IDs, then queries them via the **batch** endpoint (`batchUserStatus`, chunked at 20 ids per call) and returns an array with activity metadata merged in — fewer round-trips than the old loop-and-call approach.

Per-activity `userStatus` payload from the batch endpoint also includes extra fields not present in the single-activity response: `joinedAddress`, `winnerDownUrl`, `needContact`.

**Output (single activity):**
```json
{
  "joinStatus": 1,
  "joinTime": 1742920000,
  "rewardStatus": 1,
  "claimTime": null,
  "rewardAmount": "10000",
  "rewardUnit": "HIPPO",
  "winnerDownUrl": "https://..."
}
```

**Output (all activities — no --activity-id):**
```json
[
  {
    "activityId": 106,
    "activityName": "XXX Trading Competition",
    "shortName": "xxx",
    "chainName": "Solana",
    "activityStatus": 4,
    "userStatus": { "joinStatus": 1, "rewardStatus": 1, "rewardAmount": "45", ... }
  }
]
```

| Field | Values |
|-------|--------|
| `joinStatus` | 0=not joined, 1=joined |
| `rewardStatus` | 0=not won, 1=won (unclaimed), 2=claimed, 3=expired, 4=pending draw (winners not yet announced) |

`rewardAmount`, `rewardUnit`, `winnerDownUrl` only present when `rewardStatus=1` or `2` (a winner has been determined).

---

## competition join

Register for a competition. **Requires wallet login.**

```
onchainos competition join --activity-id <id> --evm-wallet <evm_addr> --sol-wallet <sol_addr>
```

**API**: `POST /priapi/v5/wallet/agentic/competition/join`

**Extra header**: `OK-ACCESS-PROJECT: 4d156bf0c61130f2692d097ecb68dbe4`

| Flag | Required | Description |
|------|----------|-------------|
| `--activity-id` | Yes | Activity ID |
| `--evm-wallet` | Yes | EVM wallet address (XLayer) |
| `--sol-wallet` | Yes | Solana wallet address |

**Request body fields** (built automatically):

| Field | Source |
|-------|--------|
| `activityId` | `--activity-id` |
| `evmAddress` | `--evm-wallet` |
| `solAddress` | `--sol-wallet` |
| `nickname` | Auto: `"Agentic....{last4 of evm}"` |
| `accountId` | `wallet_store.selected_account_id` (from login session) |

**API response**: `{ "code": 0, "data": null }` — CLI constructs a confirmation object:
```json
{ "joined": true, "activityId": "100", "evmAddress": "0x...", "solAddress": "...", "nickname": "Agentic....abcd" }
```

**Errors:**
- `not logged in` → run `onchainos wallet login`
- `address limit reached` → one address per user per competition
- region blocked → "service is not available in your region"

---

## competition claim

**Atomic** claim flow: pre-checks `rewardStatus`, fetches calldata, signs each entry with the TEE session, broadcasts on-chain, and returns txHash array. **Requires wallet login.**

```
onchainos competition claim --activity-id <id> --evm-wallet <evm_addr> --sol-wallet <sol_addr>
```

**API**: `POST /priapi/v5/wallet/agentic/competition/claim` (called internally; output is post-broadcast txHashes, not raw calldata)

**Extra header**: `OK-ACCESS-PROJECT: 4d156bf0c61130f2692d097ecb68dbe4`

| Flag | Required | Description |
|------|----------|-------------|
| `--activity-id` | Yes | Activity ID |
| `--evm-wallet` | Yes | EVM wallet address |
| `--sol-wallet` | Yes | Solana wallet address |

**Output:** aggregate result with reward metadata, successful txHashes, and any per-entry failures. Also surfaces `needContact` (true for top-tier winners who have not yet shared a contact method), plus the activity/account/wallet identifiers needed by the downstream `submit-contact` flow:

```json
{
  "ok": true,
  "data": {
    "rewardAmount": "460",
    "rewardUnit": "PYBOBO",
    "totalEntries": 1,
    "succeeded": [{
      "contractAddress": "7KRu...",
      "chain": "501",
      "txHash": "5abc...",
      "orderId": "..."
    }],
    "failed": [],
    "needContact": false,
    "activityId": "107",
    "accountId": "5747d742-...",
    "joinedAddress": "0x8e3f..."
  }
}
```

Internally the command:
1. Calls `competition_user_status` to verify `rewardStatus == 1` (won, unclaimed). Bails with a plain error if 0 (not won), 2 (already claimed), 3 (expired), or 4 (pending draw — winners not announced yet).
2. Calls the claim API to fetch unsigned calldata for each entry.
3. For Solana entries: extracts the unsigned tx bytes from `tx.data` (Buffer JSON shape) and base58-encodes them locally — empirically `base58CallData` is empty in real responses, so this fallback is always taken.
4. For EVM entries: takes the 0x-prefixed `input` directly.
5. Pipes each entry through `wallet contract-call` (TEE session signing + broadcast) and collects the resulting txHash.

**Errors:**
- code 11002 `not eligible for reward` → user did not win
- code 11003 → activity not found / status mismatch
- code 11008 → reward already claimed or claim window expired
- code 1860402 → backend failed to assemble the transaction; retry, then escalate
- "Sui-chain reward claims are not yet supported" → user must claim from the Sui-compatible wallet UI

---

## competition submit-contact

Record a contact method for top-tier winners (Top 10 on PnL% / PnL leaderboards). Called **only** after a `competition claim` that returned `needContact: true`, and only when the user has affirmatively shared a contact value. **Requires wallet login.**

```
onchainos competition submit-contact --activity-id <id> --contact-type <type> --contact-value <text>
```

**API**: `POST /priapi/v5/wallet/agentic/competition/submitContact`

**Extra header**: `OK-ACCESS-PROJECT: 4d156bf0c61130f2692d097ecb68dbe4`

| Flag | Required | Description |
|------|----------|-------------|
| `--activity-id` | Yes | Activity ID |
| `--contact-type` | Yes | One of: `Telegram`, `WeChat`, `Email`, `Twitter` (case-sensitive — backend rejects other values) |
| `--contact-value` | Yes | The contact value (max 256 chars). e.g. `@username` for Telegram/Twitter, the WeChat ID, the email address |

`accountId` and `walletAddress` are resolved internally: accountId comes from the local wallet store, walletAddress is looked up from `joinedAddress` via a fresh `batchUserStatus` call (ensures the address we submit matches the one the user actually joined with).

**Output:**
```json
{
  "ok": true,
  "data": {
    "submitted": true,
    "activityId": "107",
    "contactType": "Telegram"
  }
}
```

**Errors:**
- `contactType must be one of: Telegram, WeChat, Email, Twitter` → caller typo; backend rejects anything else
- `contactValue exceeds 256 character limit` → trim before retry
- `not registered for activity X` → user never joined; submit-contact only makes sense post-claim
- `Refresh token expired` → re-login required
