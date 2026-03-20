# Onchain OS — Agentic Wallet CLI Reference

Complete parameter tables, return field schemas, and usage examples for all wallet commands (A-F).

---

## A. Account Commands (6 commands)

### A1. `onchainos wallet login [email]`

Start the login flow. With email: sends OTP; without email: silent AK login.

```bash
onchainos wallet login [email] [--locale <locale>]
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `email` | positional | No | Email address to receive OTP. Omit for silent AK login. |
| `--locale` | option | No | Language for the OTP email. AI should always infer from conversation context and include it: `zh-CN` (Chinese), `ja-JP` (Japanese), `en-US` (English/default). If unsure, default to `en-US`. |

**Return fields (email OTP — returns empty on success):**

```json
{ "ok": true, "data": {} }
```

**Return fields (silent login):**

| Field | Type | Description |
|---|---|---|
| `accountId` | String | Active account UUID |
| `accountName` | String | Human-readable account name |

### A2. `onchainos wallet verify <otp>`

Verify the OTP code received via email to complete login.

```bash
onchainos wallet verify <otp>
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `otp` | positional | Yes | 6-digit OTP code from email |

**Return fields:**

| Field | Type | Description |
|---|---|---|
| `accountId` | String | Active account UUID |
| `accountName` | String | Human-readable account name |

> Never expose sensitive fields (tokens, keys, certificates) to the user.

### A3. `onchainos wallet add`

Add a new wallet account under the logged-in user.

```bash
onchainos wallet add
```

**Parameters:** None.

> **Note:** Adding a wallet automatically switches to the new account. No need to run `wallet switch` manually.

**Return fields:**

| Field | Type | Description |
|---|---|---|
| `accountId` | String | New account UUID |
| `accountName` | String | Account name (e.g., "Wallet 2") |

### A4. `onchainos wallet switch <account_id>`

Switch the active wallet account.

```bash
onchainos wallet switch <account_id>
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `account_id` | positional | Yes | Account UUID to switch to |

**Success response:** `{"ok": true, "data": {}}`

### A5. `onchainos wallet status`

Show current login status and active account.

```bash
onchainos wallet status
```

**Parameters:** None.

**Return fields:**

| Field | Type | Description |
|---|---|---|
| `email` | String | Logged-in email (empty if not logged in) |
| `loggedIn` | Boolean | Whether a session is active |
| `currentAccountId` | String | Active account UUID |
| `currentAccountName` | String | Active account name |
| `accountCount` | Number | Total number of wallet accounts (0 if not logged in) |

### A6. `onchainos wallet logout`

Logout and clear all stored credentials.

```bash
onchainos wallet logout
```

**Parameters:** None.

**Success response:** `{"ok": true, "data": {}}`

---

## B. Balance Commands

### B1. `onchainos wallet balance`

Query the authenticated wallet's token balances. Behavior varies by flags.

```bash
onchainos wallet balance [--all] [--chain <chainId>] [--token-address <addr>] [--force]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--all` | No | false | Query all accounts' assets (uses batch endpoint) |
| `--chain` | No | all chains | Chain ID / `realChainIndex` (e.g., `1` for Ethereum, `501` for Solana, `196` for XLayer). Required when using `--token-address`. |
| `--token-address` | No | - | Single token contract address. Requires `--chain`. |
| `--force` | No | false | Bypass all caches, re-fetch wallet accounts + balances from API |

---

**Scenario 1: No flags — account overview (default)**

Returns all accounts with EVM/SOL addresses and per-account USD totals.

| Field | Type | Description |
|---|---|---|
| `totalValueUsd` | String | Total value across all accounts |
| `accounts[]` | Array | Account list |
| `accounts[].accountId` | String | Account UUID |
| `accounts[].accountName` | String | Account name |
| `accounts[].evmAddress` | String | EVM address for this account |
| `accounts[].solAddress` | String | Solana address for this account |
| `accounts[].totalValueUsd` | String | Per-account total USD value |
| `accounts[].isActive` | Boolean | Whether this is the currently selected account |

---

**Scenario 2: `--all` — batch balance for all accounts**

Returns `totalValueUsd` plus a `details` map of per-account balance cache entries.

| Field | Type | Description |
|---|---|---|
| `totalValueUsd` | String | Summed total USD value across all accounts |
| `details` | Object | Map of `accountId` → balance cache entry |
| `details.<accountId>.totalValueUsd` | String | Per-account total USD value |
| `details.<accountId>.updatedAt` | Number | Unix timestamp of last cache update |
| `details.<accountId>.data` | Array | Raw token balance data for this account |

---

**Scenario 3: `--chain <chainId>` (no `--token-address`) — chain-filtered balances**

Returns token balances for the active account on the specified chain.

| Field | Type | Description |
|---|---|---|
| `totalValueUsd` | String | Total USD value on that chain |
| `details` | Array | Token balance groups from the API, enriched with `usdValue` |
| `details[].tokenAssets[]` | Array | Tokens on this chain |
| `details[].tokenAssets[].chainIndex` | String | Chain identifier |
| `details[].tokenAssets[].symbol` | String | Token symbol (e.g., `"ETH"`) |
| `details[].tokenAssets[].balance` | String | Token balance in UI units |
| `details[].tokenAssets[].usdValue` | String | Token value in USD |
| `details[].tokenAssets[].tokenContractAddress` | String | Contract address (empty for native) |
| `details[].tokenAssets[].tokenPrice` | String | Token price in USD |

---

**Scenario 4: `--chain <chainId> --token-address <addr>` — specific token balance**

Returns balance data for a single token. No `totalValueUsd` at top level.

| Field | Type | Description |
|---|---|---|
| `details` | Array | Token balance groups, enriched with `usdValue` (same shape as Scenario 3) |

---

### B — Input / Output Examples

**User says:** "Show all my accounts' assets"

```bash
onchainos wallet balance --all
# -> Display:
#   ◆ All Accounts · Balance                           Total $5,230.00
#
#     Account 1                                          $3,565.74
#     Account 2                                          $1,664.26
```

---

**User says:** "Show my balance"

```bash
onchainos wallet balance
# -> Display:
#   ◆ Wallet 1 · Balance                               Total $1,565.74
#
#     XLayer (AA)                                          $1,336.00
#     Ethereum                                               $229.74
#
#     No tokens on: Base · Arbitrum One · Solana · ...
```

---

**User says:** "Check my balance for token 0x3883ec... on Ethereum"

```bash
onchainos wallet balance --chain 1 --token-address "0x3883ec817f2a080cb035b0a38337171586e507be"
# -> Display:
#   ◆ Wallet 1 · Token Detail
#
#     XYZ (Ethereum)    1,500.00    $750.00
```

---

## C. Portfolio Commands (9 commands)

### C1. `onchainos portfolio chains`

Get supported chains for balance queries. No parameters required.

```bash
onchainos portfolio chains
```

**Return fields:**

| Field | Type | Description |
|---|---|---|
| `name` | String | Chain name (e.g., `"XLayer"`) |
| `logoUrl` | String | Chain logo URL |
| `shortName` | String | Chain short name (e.g., `"OKB"`) |
| `chainIndex` | String | Chain unique identifier (e.g., `"196"`) |

### C2. `onchainos portfolio supported-chains`

Get supported chains for portfolio PnL endpoints. No parameters required.

```bash
onchainos portfolio supported-chains
```

**Return fields:**

| Field | Type | Description |
|---|---|---|
| `name` | String | Chain name (e.g., `"Ethereum"`) |
| `logoUrl` | String | Chain logo URL |
| `shortName` | String | Chain short name |
| `chainIndex` | String | Chain unique identifier (e.g., `"1"`) |

### C3. `onchainos portfolio total-value`

Get total asset value for a wallet address.

```bash
onchainos portfolio total-value --address <address> --chains <chains> [--asset-type <type>] [--exclude-risk <bool>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--chains` | Yes | - | Chain names or IDs, comma-separated (e.g., `"xlayer,solana"` or `"196,501"`) |
| `--asset-type` | No | `"0"` | `0`=all, `1`=tokens only, `2`=DeFi only |
| `--exclude-risk` | No | `true` | `true`=filter risky tokens, `false`=include. Only ETH/BSC/SOL/BASE. Note: `all-balances` and `token-balances` use `"0"`/`"1"` instead of boolean. |

**Return fields:**

| Field | Type | Description |
|---|---|---|
| `totalValue` | String | Total asset value in USD |

### C4. `onchainos portfolio all-balances`

Get all token balances for a wallet address.

```bash
onchainos portfolio all-balances --address <address> --chains <chains> [--exclude-risk <value>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--chains` | Yes | - | Chain names or IDs, comma-separated, max 50 |
| `--exclude-risk` | No | `"0"` | `0`=filter out risky tokens (default), `1`=include. Only ETH/BSC/SOL/BASE |

**Return fields** (per token in `tokenAssets[]`):

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier |
| `tokenContractAddress` | String | Token contract address |
| `symbol` | String | Token symbol (e.g., `"OKB"`) |
| `balance` | String | Token balance in UI units (e.g., `"10.5"`) |
| `rawBalance` | String | Token balance in base units (e.g., `"10500000000000000000"`) |
| `tokenPrice` | String | Token price in USD |
| `isRiskToken` | Boolean | `true` if flagged as risky |

### C5. `onchainos portfolio token-balances`

Get specific token balances for a wallet address.

```bash
onchainos portfolio token-balances --address <address> --tokens <tokens> [--exclude-risk <value>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--tokens` | Yes | - | Token list: `"chainIndex:tokenAddress"` pairs, comma-separated. Use empty address for native token (e.g., `"196:"` for native OKB). Max 20 items. |
| `--exclude-risk` | No | `"0"` | `0`=filter out (default), `1`=include |

**Return fields**: Same schema as `all-balances` (`tokenAssets[]`).

### C6. `onchainos portfolio overview`

Get wallet-level PnL summary and trading behaviour metrics.

```bash
onchainos portfolio overview --address <address> --chain <chain> [--time-frame <frame>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--chain` | Yes | - | Chain name or ID (e.g., `ethereum`, `solana`, `xlayer`) |
| `--time-frame` | No | `7d` | `1d`, `3d`, `7d`, `1m`, `3m` |

**Return fields:**

| Field | Type | Description |
|---|---|---|
| `realizedPnlUsd` | String | Realized PnL in USD |
| `unrealizedPnlUsd` | String | Unrealized PnL in USD |
| `totalPnlUsd` | String | Total PnL in USD |
| `totalPnlPercent` | String | Total PnL as a percentage |
| `winRate` | String | Ratio of profitable sells (e.g., `"0.65"` = 65%) |
| `buyTxCount` | String | Number of buy transactions |
| `sellTxCount` | String | Number of sell transactions |
| `preferredMarketCap` | String | Most-traded market cap bucket (`1`-`5`, small->large) |
| `topPnlTokenList[]` | Array | Top performing tokens in the period |

### C7. `onchainos portfolio dex-history`

Get wallet DEX transaction history with cursor pagination.

```bash
onchainos portfolio dex-history --address <address> --chain <chain> [--limit <n>] [--cursor <cursor>] [--token <address>] [--tx-type <types>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--chain` | Yes | - | Chain name or ID |
| `--limit` | No | `20` | Page size (1-100) |
| `--cursor` | No | - | Pagination cursor from previous response (omit for first page) |
| `--token` | No | - | Filter by token contract address |
| `--tx-type` | No | all | Transaction type(s), comma-separated: `1`=buy, `2`=sell, `3`=transfer-in, `4`=transfer-out, `0`=all |

**Return fields:**

| Field | Type | Description |
|---|---|---|
| `cursor` | String | Next-page cursor (empty when no more pages) |
| `historyList[]` | Array | Transaction records |
| `historyList[].type` | String | Transaction type (`1`-`4`) |
| `historyList[].timestamp` | String | Transaction time (Unix ms) |
| `historyList[].tokenContractAddress` | String | Token involved |

### C8. `onchainos portfolio recent-pnl`

Get paginated list of recent per-token PnL records.

```bash
onchainos portfolio recent-pnl --address <address> --chain <chain> [--limit <n>] [--cursor <cursor>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--chain` | Yes | - | Chain name or ID |
| `--limit` | No | `20` | Page size (1-100) |
| `--cursor` | No | - | Pagination cursor from previous response |

**Return fields:**

| Field | Type | Description |
|---|---|---|
| `cursor` | String | Next-page cursor (empty when no more pages) |
| `pnlList[]` | Array | Token PnL records |
| `pnlList[].tokenSymbol` | String | Token symbol |
| `pnlList[].tokenContractAddress` | String | Token contract address |
| `pnlList[].realizedPnl` | String | Realized PnL in USD |
| `pnlList[].unrealizedPnl` | String | Unrealized PnL in USD |
| `pnlList[].totalPnl` | String | Total PnL in USD |
| `pnlList[].buyTxCount` | String | Buy transaction count |
| `pnlList[].sellTxCount` | String | Sell transaction count |
| `pnlList[].tokenBalanceAmount` | String | Current token amount held |
| `pnlList[].lastActiveTimestamp` | String | Last activity timestamp (Unix ms) |

### C9. `onchainos portfolio token-pnl`

Get latest PnL snapshot for a specific token in a wallet.

```bash
onchainos portfolio token-pnl --address <address> --chain <chain> --token <token>
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--chain` | Yes | - | Chain name or ID |
| `--token` | Yes | - | Token contract address |

**Return fields:**

| Field | Type | Description |
|---|---|---|
| `tokenSymbol` | String | Token symbol |
| `tokenContractAddress` | String | Token contract address |
| `realizedPnl` | String | Realized PnL in USD |
| `unrealizedPnl` | String | Unrealized PnL in USD |
| `totalPnl` | String | Total PnL in USD |
| `buyAvgPrice` | String | Average buy price in USD |
| `sellAvgPrice` | String | Average sell price in USD |
| `buyTxCount` | String | Buy transaction count |
| `sellTxCount` | String | Sell transaction count |
| `tokenBalance` | String | Current position value in USD |
| `tokenBalanceAmount` | String | Current token amount (`"0"` = fully closed position) |
| `lastActiveTimestamp` | String | Last activity timestamp (Unix ms) |

### C — Input / Output Examples

**User says:** "Check my wallet total assets on XLayer and Solana"

```bash
onchainos portfolio total-value --address 0xYourWallet --chains "xlayer,solana"
# -> Display: Total assets $12,345.67
```

**User says:** "Show all tokens in my wallet"

```bash
onchainos portfolio all-balances --address 0xYourWallet --chains "xlayer,solana,ethereum"
# -> Display:
#   OKB:  10.5 ($509.25)
#   USDC: 2,000 ($2,000.00)
#   USDT: 1,500 ($1,500.00)
#   ...
```

**User says:** "Only check USDC and native OKB balances on XLayer"

```bash
onchainos portfolio token-balances --address 0xYourWallet --tokens "196:,196:0x74b7f16337b8972027f6196a17a631ac6de26d22"
# -> Display: OKB: 10.5 ($509.25), USDC: 2,000 ($2,000.00)
```

**User says:** "Show my PnL on Ethereum for the last month"

```bash
onchainos portfolio overview --address 0xYourWallet --chain ethereum --time-frame 1m
# -> Display: Total PnL $+1,234.56 | Win rate: 65% | Buys: 42 | Sells: 28
```

**User says:** "What tokens did I buy on Ethereum recently?"

```bash
onchainos portfolio dex-history --address 0xYourWallet --chain ethereum --tx-type 1 --limit 20
# -> Display: list of buy transactions with token, amount, timestamp
```

**User says:** "How much profit have I made on USDC on Ethereum?"

```bash
onchainos portfolio token-pnl \
  --address 0xYourWallet \
  --chain ethereum \
  --token 0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48
# -> Display: Realized PnL $+500.00 | Unrealized $+12.50 | Avg buy $1.00 | Avg sell $1.001
```

---

## D. Send Command

### D1. `onchainos wallet send`

Send native tokens or contract tokens (ERC-20 / SPL) from the Agentic Wallet.

```bash
onchainos wallet send \
  --amount <amount> \
  --receipt <address> \
  --chain <chainId> \
  [--from <address>] \
  [--contract-token <address>] \
  [--force]
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `--amount` | string | Yes | Amount in UI units (e.g. "0.01" for 0.01 ETH) |
| `--receipt` | string | Yes | Recipient address (0x-prefixed for EVM, Base58 for Solana) |
| `--chain` | string | Yes | Chain ID / `realChainIndex` (e.g. "1" for Ethereum, "501" for Solana, "56" for BSC) |
| `--from` | string | No | Sender address — defaults to selected account's address on the given chain |
| `--contract-token` | string | No | Token contract address for ERC-20 / SPL transfers. Omit for native token transfers. |
| `--force` | bool | No | Skip confirmation prompts from the backend (default false). Use when re-running a command after the user has confirmed a `confirming` response. |

**Return fields:**

| Field | Type | Description |
|---|---|---|
| `txHash` | String | Broadcast transaction hash |

---

## E. History Command (2 modes)

### E1. List Mode (no `--tx-hash`)

Browse the transaction order list for the current or specified account.

```bash
onchainos wallet history \
  [--account-id <id>] \
  [--chain <chainId>] \
  [--begin <ms_timestamp>] \
  [--end <ms_timestamp>] \
  [--page-num <cursor>] \
  [--limit <n>] \
  [--order-id <id>] \
  [--uop-hash <hash>]
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `--account-id` | string | No | Account ID to query. Defaults to the currently selected account. |
| `--chain` | string | No | Chain ID / `realChainIndex` (e.g. "1" for Ethereum, "501" for Solana). Resolved to chainIndex internally. |
| `--begin` | string | No | Start time filter (millisecond timestamp) |
| `--end` | string | No | End time filter (millisecond timestamp) |
| `--page-num` | string | No | Page cursor for pagination |
| `--limit` | string | No | Number of results per page |
| `--order-id` | string | No | Filter by specific order ID |
| `--uop-hash` | string | No | Filter by user operation hash |

**Return fields:**

| Field | Type | Description |
|---|---|---|
| `cursor` | String | Next-page cursor (empty when no more pages) |
| `orderList[]` | Array | Transaction records |
| `orderList[].txHash` | String | Transaction hash |
| `orderList[].txStatus` | String | Status code (see table below) |
| `orderList[].txTime` | String | Transaction time (Unix ms) |
| `orderList[].txCreateTime` | String | Order creation time (Unix ms) |
| `orderList[].from` | String | Sender address |
| `orderList[].to` | String | Recipient address |
| `orderList[].direction` | String | `"send"` or `"receive"` |
| `orderList[].chainSymbol` | String | Chain symbol (e.g., `"ETH"`) |
| `orderList[].coinSymbol` | String | Token symbol |
| `orderList[].coinAmount` | String | Token amount |
| `orderList[].serviceCharge` | String | Gas fee |
| `orderList[].confirmedCount` | String | Confirmation count |
| `orderList[].hideTxType` | String | Hidden tx type flag |
| `orderList[].repeatTxType` | String | Repeat tx type |
| `orderList[].assetChange[]` | Array | Net asset changes |
| `orderList[].assetChange[].coinSymbol` | String | Token symbol |
| `orderList[].assetChange[].coinAmount` | String | Token amount |
| `orderList[].assetChange[].direction` | String | `"in"` or `"out"` |

**List mode example response:**

```json
{
  "ok": true,
  "data": [
    {
      "cursor": "next_page_token",
      "orderList": [
        {
          "txHash": "0xabc123...",
          "txStatus": "1",
          "txTime": "1700000000000",
          "txCreateTime": "1700000000000",
          "from": "0xSender...",
          "to": "0xRecipient...",
          "direction": "send",
          "chainSymbol": "ETH",
          "coinSymbol": "ETH",
          "coinAmount": "0.01",
          "serviceCharge": "0.0005",
          "confirmedCount": "12",
          "hideTxType": "0",
          "repeatTxType": "",
          "assetChange": [
            {
              "coinSymbol": "ETH",
              "coinAmount": "0.01",
              "direction": "out"
            }
          ]
        }
      ]
    }
  ]
}
```

### E2. Detail Mode (with `--tx-hash`)

Look up a specific transaction by its hash.

```bash
onchainos wallet history \
  --tx-hash <hash> \
  --chain <chainId> \
  --address <addr> \
  [--account-id <id>] \
  [--order-id <id>] \
  [--uop-hash <hash>]
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `--tx-hash` | string | Yes | Transaction hash to look up |
| `--chain` | string | Yes | Chain ID / `realChainIndex` where the transaction occurred (e.g. "1" for Ethereum, "501" for Solana) |
| `--address` | string | Yes | Wallet address that sent/received the transaction |
| `--account-id` | string | No | Account ID. Defaults to the currently selected account. |
| `--order-id` | string | No | Order ID filter |
| `--uop-hash` | string | No | User operation hash filter |

**Return fields (detail mode):**

| Field | Type | Description |
|---|---|---|
| `txHash` | String | Transaction hash |
| `txTime` | String | Transaction time (Unix ms) |
| `txStatus` | String | Status code (see table below) |
| `failReason` | String | Failure reason (empty if success) |
| `direction` | String | `"send"` or `"receive"` (mapped from `txType`) |
| `repeatTxType` | String | Repeat tx type |
| `from` | String | Sender address |
| `to` | String | Recipient address |
| `chainSymbol` | String | Chain symbol |
| `chainIndex` | String | Chain identifier |
| `coinSymbol` | String | Token symbol |
| `coinAmount` | String | Token amount |
| `serviceCharge` | String | Gas fee |
| `confirmedCount` | String | Confirmation count |
| `explorerUrl` | String | Block explorer URL for the transaction |
| `hideTxType` | String | Hidden tx type flag |
| `input[]` | Array | Input asset changes |
| `input[].name` | String | Token name |
| `input[].amount` | String | Amount |
| `input[].direction` | String | Direction |
| `output[]` | Array | Output asset changes |
| `output[].name` | String | Token name |
| `output[].amount` | String | Amount |
| `output[].direction` | String | Direction |

**Detail mode example response:**

```json
{
  "ok": true,
  "data": [
    {
      "txHash": "0xabc123...",
      "txTime": "1700000000000",
      "txStatus": "1",
      "failReason": "",
      "direction": "send",
      "repeatTxType": "",
      "from": "0xSender...",
      "to": "0xRecipient...",
      "chainSymbol": "ETH",
      "chainIndex": "1",
      "coinSymbol": "ETH",
      "coinAmount": "0.01",
      "serviceCharge": "0.0005",
      "confirmedCount": "12",
      "explorerUrl": "https://etherscan.io/tx/0xabc123...",
      "hideTxType": "0",
      "input": [
        { "name": "ETH", "amount": "0.01", "direction": "in" }
      ],
      "output": [
        { "name": "ETH", "amount": "0.01", "direction": "out" }
      ]
    }
  ]
}
```

### Transaction Status Values

| `txStatus` | Meaning |
|---|---|
| `0` | Pending |
| `1` | Success |
| `2` | Failed |
| `3` | Pending confirmation |

---

## F. Contract Call Command

### F1. `onchainos wallet contract-call`

Call a smart contract on an EVM chain or Solana program with TEE signing and automatic broadcasting.

```bash
onchainos wallet contract-call \
  --to <contract_address> \
  --chain <chainId> \
  [--value <amount>] \
  [--input-data <hex_calldata>] \
  [--unsigned-tx <base58_tx>] \
  [--gas-limit <number>] \
  [--from <address>] \
  [--aa-dex-token-addr <address>] \
  [--aa-dex-token-amount <amount>] \
  [--mev-protection] \
  [--jito-unsigned-tx <jito_base58_tx>] \
  [--force]
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `--to` | string | Yes | Contract address to interact with |
| `--chain` | string | Yes | Chain ID / `realChainIndex` (e.g. "1" for Ethereum, "501" for Solana, "56" for BSC) |
| `--value` | string | No | Native token amount to send with the call (default "0"). In UI units (e.g., "0.01" for 0.01 ETH). |
| `--input-data` | string | Conditional | EVM call data (hex-encoded, e.g. "0xa9059cbb..."). **Required for EVM chains.** |
| `--unsigned-tx` | string | Conditional | Solana unsigned transaction data (base58). **Required for Solana.** |
| `--gas-limit` | string | No | Gas limit override (EVM only). If omitted, the CLI estimates gas automatically. |
| `--from` | string | No | Sender address — defaults to the selected account's address on the given chain. |
| `--aa-dex-token-addr` | string | No | AA DEX token contract address (for AA DEX interactions). |
| `--aa-dex-token-amount` | string | No | AA DEX token amount (for AA DEX interactions). |
| `--mev-protection` | bool | No | Enable MEV protection (default false). Supported on Ethereum, BSC, Base, and Solana. On Solana, `--jito-unsigned-tx` is also required. |
| `--jito-unsigned-tx` | string | No | Jito unsigned transaction data (base58) for Solana MEV protection. **Required when `--mev-protection` is used on Solana.** |
| `--force` | bool | No | Skip confirmation prompts from the backend (default false). Use when re-running a command after the user has confirmed a `confirming` response. |

> Either `--input-data` (EVM) or `--unsigned-tx` (Solana) must be provided. The CLI will fail if neither is present.

**Return fields:**

| Field | Type | Description |
|---|---|---|
| `txHash` | String | Broadcast transaction hash |
