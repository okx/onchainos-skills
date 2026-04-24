# competition CLI Reference

All commands: `onchainos competition <subcommand> [flags]`

---

## competition list

List Agentic Wallet exclusive trading competitions.

```
onchainos competition list [--status <3|4>] [--page-size <n>] [--page-num <n>]
```

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--status` | int | — | 0=active, 1=ended, 2=all; omit for all |
| `--page-size` | int | 10 | Results per page |
| `--page-num` | int | 1 | Page number (1-based) |

**Output:**
```json
{
  "ok": true,
  "data": {
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
    "totalCount": 2,
    "pageNum": 1,
    "pageSize": 10
  }
}
```

Activity URL: `https://web3.okx.com/boost/trading-competition/<shortName>`

---

## competition detail

Get competition rules, prize pool, and timeline.

```
onchainos competition detail --activity-id <id>
```

| Flag | Required | Description |
|------|----------|-------------|
| `--activity-id` | Yes | Activity ID from `competition list` |

**Output:** Competition object with `tabConfigs` array. Each tab has:
- `tab`: 1=volume, 3=realized PnL, 4=boost token volume
- `tabDetails[].title` / `tabDetails[].desc`: rules text
- `prizePoolDistribution[].rules[].interval` + `.reward`: rank range → reward amount
- `prizePoolDistribution[].rewardUnit`: reward token symbol

---

## competition rank

Get leaderboard and current user ranking.

```
onchainos competition rank --activity-id <id> --wallet <addr> --sort-type <type> [--limit <n>]
```

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--activity-id` | Yes | — | Activity ID |
| `--wallet` | Yes | — | User wallet address |
| `--sort-type` | Yes | 5 | 5=volume, 7=realized PnL, 8=boost token volume |
| `--limit` | No | 20 | Max leaderboard entries (max 100) |

**Output:**
```json
{
  "ok": true,
  "data": {
    "agenticActivity": true,
    "totalRewardToken": "1000000",
    "rewardTokenSymbol": "HIPPO",
    "myRankInfo": {
      "currentRank": 42,
      "nickName": "Agentic...abcd",
      "userTotal": "1250.5",
      "expectedRewards": "100",
      "format": 1,
      "rewardUnit": "HIPPO"
    },
    "allRankInfos": [ ... ],
    "rankUpdateTime": 1774359000638
  }
}
```

`format`: 1=number, 2=percentage, 3=token amount with unit

`rankUpdateTime`: milliseconds (13-digit timestamp)

`agenticActivity`: true = Agentic Wallet exclusive competition

`luckyRewardAmount`: per-tab lucky reward token amount (in each tab config object)

---

## competition user-status

Get user's participation and reward status.

```
onchainos competition user-status --activity-id <id> --wallet <addr>
```

| Flag | Required | Description |
|------|----------|-------------|
| `--activity-id` | Yes | Activity ID |
| `--wallet` | Yes | User wallet address |

**Output:**
```json
{
  "ok": true,
  "data": {
    "joinStatus": 1,
    "joinTime": 1742920000,
    "rewardStatus": 1,
    "claimTime": null,
    "rewardAmount": "10000",
    "rewardUnit": "HIPPO"
  }
}
```

| Field | Values |
|-------|--------|
| `joinStatus` | 0=not joined, 1=joined |
| `rewardStatus` | 0=not won, 1=won (unclaimed), 2=claimed, 3=expired |

---

## competition join

Register for a competition. **Requires wallet login.**

```
onchainos competition join --activity-id <id> --wallet <addr> [--nickname <name>]
```

| Flag | Required | Description |
|------|----------|-------------|
| `--activity-id` | Yes | Activity ID |
| `--wallet` | Yes | EVM wallet address to register |
| `--nickname` | No | Leaderboard display name (default: "Agentic…<last4>") |

**Output:**
```json
{ "ok": true, "data": { "joined": true, "activityId": "100", "walletAddress": "0x...", "nickname": "Agentic...abcd" } }
```

**Errors:**
- `not logged in` → run `onchainos wallet login`
- `address limit reached` → one address per user per competition allowed

---

## competition claim

Fetch reward calldata for on-chain submission. **Requires wallet login.**

```
onchainos competition claim --activity-id <id> --wallet <addr>
```

**Output:** Array of calldata objects — pass each to `onchainos gateway broadcast`:

```json
{
  "ok": true,
  "data": [{
    "contractAddress": "0x...",
    "chain": 42161,
    "input": "0xa9059cbb...",
    "tokenSymbol": "HIPPO",
    "tokenAmount": "10000000000000000000000",
    "tokenAddress": "0x...",
    "value": "0"
  }]
}
```

Submit each entry via `onchainos wallet contract-call`:
```bash
onchainos wallet contract-call \
  --to <contractAddress> \
  --chain <chain> \
  --input-data <input> \
  --amt <value>
```
Use `onchainos wallet chains` to map `chainId` (e.g. 42161) to chain name (e.g. "arbitrum").

**Errors:**
- code 11002 `not eligible for reward` → user did not win
