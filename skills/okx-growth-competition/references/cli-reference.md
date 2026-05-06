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
      "chainId": 42161,
      "chainName": "Arbitrum One",
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
- `chainId` / `chainName`: competition chain
- `startTime` / `endTime`: 10-digit Unix timestamps
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
onchainos competition rank --activity-id <id> --wallet <addr> --sort-type <type> [--limit <n>]
```

**API**: `GET /priapi/v1/dapp/agentic/competition/rank`

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--activity-id` | Yes | — | Activity ID |
| `--wallet` | Yes | — | User wallet address |
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
onchainos competition user-status [--activity-id <id>] --wallet <addr>
```

**API**: `GET /priapi/v1/dapp/agentic/competition/userStatus`

| Flag | Required | Description |
|------|----------|-------------|
| `--activity-id` | No | Activity ID; omit to check **all** activities (active + ended) |
| `--wallet` | Yes | User wallet address |

When `--activity-id` is omitted, the CLI calls `competition list --status 2` first to get all activity IDs, then queries `userStatus` for each and returns an array with activity metadata merged in.

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
    "chainName": "BNB Chain",
    "activityStatus": 4,
    "userStatus": { "joinStatus": 1, "rewardStatus": 1, "rewardAmount": "45", ... }
  }
]
```

| Field | Values |
|-------|--------|
| `joinStatus` | 0=not joined, 1=joined |
| `rewardStatus` | 0=not won, 1=won (unclaimed), 2=claimed, 3=expired |

`rewardAmount`, `rewardUnit`, `winnerDownUrl` only present when `rewardStatus >= 1`.

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

**Output:** aggregate result with reward metadata, successful txHashes, and any per-entry failures:

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
    "failed": []
  }
}
```

Internally the command:
1. Calls `competition_user_status` to verify `rewardStatus == 1` (won, unclaimed). Bails with a plain error if 0/2/3.
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
