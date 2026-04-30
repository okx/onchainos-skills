# Onchain OS — Agentic Wallet CLI Reference

Complete parameter tables, return field schemas, and usage examples for all wallet commands (A-G).

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
| `policy` | Object \| Null | Policy settings for the active account (null when not logged in or no policy configured). See **Policy fields** below. |

#### Policy fields (inside `policy`)

| Field | Type | Description |
|---|---|---|
| `singleTxLimit` | String | Per-transaction USD limit (`"0"` = not set) |
| `singleTxFlag` | Boolean | Whether per-transaction limit is enabled |
| `dailyTransferTxLimit` | String | Daily transfer USD limit (`"0"` = not set) |
| `dailyTransferTxFlag` | Boolean | Whether daily transfer limit is enabled |
| `dailyTransferTxUsed` | String | Daily transfer amount already used (USD) |
| `dailyTradeTxLimit` | String | Daily trade USD limit (`"0"` = not set) |
| `dailyTradeTxFlag` | Boolean | Whether daily trade limit is enabled |
| `dailyTradeTxUsed` | String | Daily trade amount already used (USD) |

### A6. `onchainos wallet logout`

Logout and clear all stored credentials.

```bash
onchainos wallet logout
```

**Parameters:** None.

**Success response:** `{"ok": true, "data": {}}`

### A7. `onchainos wallet chains`

List all chains supported by the wallet, including chain names, IDs, and capabilities.

```bash
onchainos wallet chains
```

**Parameters:** None.

**Return fields** (per chain in array):

| Field | Type | Description |
|---|---|---|
| `alias` | String | Internal alias (e.g., `"eth"`, `"matic"`) — for internal use only |
| `chainIndex` | String | Chain index used in API responses (e.g., `"1"`) |
| `chainName` | String | Technical chain name (e.g., `"eth"`, `"matic"`) — may differ from display name |
| `isEvmChain` | Boolean | Whether this is an EVM-compatible chain |
| `realChainIndex` | String | **The value to pass to `--chain`** in wallet commands (e.g., `"1"` for Ethereum) |
| `showName` | String | **Human-readable display name** — always use this when showing chain names to users (e.g., `"Ethereum"`, `"Polygon"`, `"BNB Chain"`) |

> **Usage**: Use `showName` for user-facing display. Use `realChainIndex` for `--chain` parameters in wallet commands.

---

## B. Balance Commands

### B1. `onchainos wallet balance`

Query the authenticated wallet's token balances. Behavior varies by flags.

```bash
onchainos wallet balance [--all] [--chain <chain>] [--token-address <addr>] [--force]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--all` | No | false | Query all accounts' assets (uses batch endpoint) |
| `--chain` | No | all chains | Chain name or numeric ID (e.g. `ethereum` or `1`, `solana` or `501`, `xlayer` or `196`). Required when using `--token-address`. |
| `--token-address` | No | - | Single token contract address. Requires `--chain`. |
| `--force` | No | false | Bypass all caches, re-fetch wallet accounts + balances from API |

---

**Scenario 1: No flags — active account balance (default)**

Returns the active account's EVM/SOL addresses, all-chain token list, and total USD value.

| Field | Type | Description |
|---|---|---|
| `totalValueUsd` | String | Total USD value for the active account |
| `accountId` | String | Active account UUID |
| `accountName` | String | Active account name |
| `evmAddress` | String | EVM address for this account |
| `solAddress` | String | Solana address for this account |
| `accountCount` | Number | Total number of wallet accounts |
| `details` | Array | Token balance groups from the API, enriched with `usdValue` |

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

**Scenario 3: `--chain <chain>` (no `--token-address`) — chain-filtered balances**

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

**Scenario 4: `--chain <chain> --token-address <addr>` — specific token balance**

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

## C. Portfolio Commands

> Portfolio commands (`portfolio total-value`, `portfolio all-balances`, `portfolio overview`, etc.)
> are handled by the **okx-wallet-portfolio** skill. See that skill's cli-reference for full documentation.

---

## D. Send Command

### D1. `onchainos wallet send`

Send native tokens or contract tokens (ERC-20 / SPL) from the Agentic Wallet.

```bash
onchainos wallet send \
  --readable-amount <amount> \
  --recipient <address> \
  --chain <chain> \
  [--from <address>] \
  [--contract-token <address>] \
  [--force] \
  [--gas-token-address <address>] \
  [--relayer-id <id>] \
  [--enable-gas-station]
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `--readable-amount` | string | One of | Human-readable amount (e.g. `"0.1"`, `"100"`). CLI converts to minimal units automatically. Preferred. |
| `--amt` | string | One of | Raw minimal units. Use only when explicitly known. Mutually exclusive with `--readable-amount`. |
| `--recipient` | string | Yes | Recipient address (0x-prefixed for EVM, Base58 for Solana) |
| `--chain` | string | Yes | Chain name or numeric ID (e.g. `ethereum` or `1`, `solana` or `501`, `bsc` or `56`) |
| `--from` | string | No | Sender address — defaults to selected account's address on the given chain |
| `--contract-token` | string | No | Token contract address for ERC-20 / SPL transfers. Omit for native token transfers. |
| `--force` | bool | No | Skip confirmation prompts from the backend (default false). Use when re-running a command after the user has confirmed a `confirming` response. |
| `--gas-token-address` | string | No | Gas Station: token contract address to pay gas (from confirming response tokenList). Second-phase call only. |
| `--relayer-id` | string | No | Gas Station: relayer ID (from confirming response tokenList). Second-phase call only. |
| `--enable-gas-station` | bool | No | Gas Station: first-time activation flag. When `--gas-token-address` is also given, sets it as default (Scene A option 1). When passed alone, enables without a default (Scene A option 2, backend auto-picks highest-balance token). |

**Return fields (normal):**

| Field | Type | Description |
|---|---|---|
| `txHash` | String | Broadcast transaction hash |

**Return fields (Gas Station auto-path — gasStationStatus ∈ {READY_TO_USE / PENDING_UPGRADE / REENABLE_ONLY} with hash non-empty):**

| Field | Type | Description |
|---|---|---|
| `txHash` | String | Broadcast transaction hash (may be empty; relayer returns async) |
| `orderId` | String | Order ID for async status query via `wallet history --chain <chain> --order-id <id>` (routes to `/order/detail`) |
| `gasStationUsed` | Boolean | `true` |
| `gasStationStatus` | String | Enum: READY_TO_USE / PENDING_UPGRADE / REENABLE_ONLY |
| `autoSelectedToken` | Boolean | Backend auto-selected the gas token |
| `serviceCharge` | String | Gas fee amount (integer, multiplied by token decimal) |
| `serviceChargeSymbol` | String | Gas fee token symbol (e.g. "USDT") |

**Confirming response (Gas Station FIRST_TIME_PROMPT or READY_TO_USE with default-insufficient — exit code 2):**

When Gas Station needs user input, the CLI returns a confirming response with the available token list in the `next` field. The `message` body distinguishes the two subcases:
- FIRST_TIME_PROMPT (Scene A) — first-time enable, 3-option decision tree
- READY_TO_USE with empty hash (Scene C) — default insufficient, 2-question decision tree

See `references/gas-station.md` Step 2 for Agent handling instructions.

**Return fields (Gas Station INSUFFICIENT_ALL):**

| Field | Type | Description |
|---|---|---|
| `gasStationUsed` | Boolean | `true` |
| `gasStationStatus` | String | `"INSUFFICIENT_ALL"` |
| `insufficientAll` | Boolean | `true` — all gas tokens insufficient |
| `gasStationTokenList` | Array | All items with `sufficient: false` |
| `fromAddr` | String | User address for deposit guidance |

**Return fields (Gas Station HAS_PENDING_TX):**

| Field | Type | Description |
|---|---|---|
| `gasStationUsed` | Boolean | `true` |
| `gasStationStatus` | String | `"HAS_PENDING_TX"` |
| `hasPendingTx` | Boolean | `true` — a previous Gas Station tx is still pending |

**Return fields (not routed through Gas Station — gasStationStatus=NOT_APPLICABLE):**

Same as regular `wallet send` output (`txHash` / `orderId`). `gasStationUsed=false`.

---

## D-GS. Gas Station Management Commands

### D-GS1. `onchainos wallet gas-station update-default-token`

Update the default gas payment token for Gas Station on a specific chain.

```bash
onchainos wallet gas-station update-default-token \
  --chain <chain> \
  --gas-token-address <address>
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `--chain` | string | Yes | Chain name or ID (e.g. `ethereum` or `1`) |
| `--gas-token-address` | string | Yes | Token contract address to set as default gas payment token |

### D-GS2. `onchainos wallet gas-station enable`

Turn Gas Station back on for a chain that was previously enabled. (Internal: DB flag flip only, no on-chain action. Requires prior on-chain setup — first-time activation happens via `wallet send` which bundles the setup with the first Gas Station broadcast. If the chain has never been activated, backend returns a msg in the response body — relay the backend msg verbatim, do NOT paraphrase with "7702" / "delegation" / "DB".) See `gas-station.md` User-Facing Reply Templates for user-facing wording.

```bash
onchainos wallet gas-station enable \
  --chain <chain>
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `--chain` | string | Yes | Chain name or ID (e.g. `ethereum` or `1`) |

### D-GS3. `onchainos wallet gas-station disable`

Turn Gas Station off for a chain; the chain reverts to paying gas with native token. (Internal: DB flag flip only, no on-chain action. On-chain state and `default_gas_token_address` are preserved so re-enabling later is instant.) See `gas-station.md` User-Facing Reply Templates for user-facing wording — **never paraphrase "DB flag" / "7702" / "delegation" into the reply**.

```bash
onchainos wallet gas-station disable \
  --chain <chain>
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `--chain` | string | Yes | Chain name or ID (e.g. `ethereum` or `1`) |

### D-GS4. `onchainos wallet gas-station status`

**Read-only Gas Station readiness probe.** Used by **third-party plugin pre-flight** — the agent runs this before invoking a plugin's on-chain command (e.g. `aave-v3-plugin --confirm supply ...`) to decide whether the chain needs first-time GS activation, re-enable, or is already ready. Never broadcasts. Safe to call repeatedly.

Internally probes Phase 1 diagnostic via a 0-amount native self-transfer (the same call the regular `wallet send` would make on its first phase, but here we deliberately don't proceed past it).

```bash
onchainos wallet gas-station status \
  --chain <chain> \
  [--from <address>]
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `--chain` | string | Yes | Chain name or ID |
| `--from` | string | No | Sender address; defaults to selectedAccountId |

Response:

```json
{
  "ok": true,
  "data": {
    "chainId": "42161",
    "chainName": "arb_eth",
    "fromAddress": "0xd13c...a136",
    "gasStationActivated": false,
    "gasStationDefaultToken": null,
    "gasStationStatus": "FIRST_TIME_PROMPT",
    "recommendation": "ENABLE_GAS_STATION",
    "hasPendingTx": false,
    "insufficientAll": false,
    "tokenList": [
      { "symbol": "USDC", "feeTokenAddress": "0xaf88...5831", "relayerId": "fcfc...3c87",
        "balance": "1.49", "serviceCharge": "0.026", "sufficient": true }
    ]
  }
}
```

`recommendation` enum:

| Value | Agent action |
|---|---|
| `READY` | Chain has sufficient native gas, or GS already active. Proceed directly to plugin invocation. |
| `ENABLE_GAS_STATION` | First-time. Render Scene A → user picks → run `wallet gas-station setup` → re-invoke plugin. |
| `REENABLE_GAS_STATION` | User previously disabled GS. Render Scene B' → user picks → `setup` → re-invoke. |
| `PENDING_UPGRADE` | Chain not yet 7702-delegated. Render Scene A' → user picks → `setup` (carries 7702 material) → re-invoke. |
| `INSUFFICIENT_ALL` | No stablecoin has enough balance. Tell user to top up. Do NOT invoke plugin. |
| `HAS_PENDING_TX` | A pending GS tx blocks new ones. Tell user to wait. Do NOT invoke plugin. |

### D-GS5. `onchainos wallet gas-station setup`

**Standalone first-time activation.** Decoupled from `wallet send` so the agent can activate Gas Station *before* invoking a third-party plugin. The plugin (which always passes `--force` internally) will then succeed transparently because GS is already active on the chain.

Internally drives a 1-minimal-unit self-transfer of the picked gas token with `--enable-gas-station --force`. Backend Phase 2 returns 712 hash (and `authHashFor7702` if the chain still needs 7702 upgrade); CLI signs and broadcasts. The carrier transfer is from-self to from-self, so net value movement = 0; only the GS service charge is consumed.

**Pre-condition**: the agent has already obtained user consent via Scene A / B' / A' (see `gas-station.md`). This command does NOT prompt — it executes the activation that the user has already approved.

```bash
onchainos wallet gas-station setup \
  --chain <chain> \
  --gas-token-address <addr> \
  --relayer-id <relayer_id> \
  [--from <address>]
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `--chain` | string | Yes | Chain name or ID |
| `--gas-token-address` | string | Yes | Token address picked by the user from `tokenList` |
| `--relayer-id` | string | Yes | Relayer ID paired with `--gas-token-address` |
| `--from` | string | No | Sender address; defaults to selectedAccountId |

Idempotency:
- Already activated, same default → returns `{gasStationActivated: true, alreadyActivated: true}` without broadcasting.
- Already activated, different default → switches via `update-default-token` and returns `{alreadyActivated: true, defaultTokenSwitched: true}`.
- Not yet activated → drives the carrier transfer; on success returns the wallet send response (`{txHash, orderId, gasStationUsed: true, serviceCharge, ...}`).

---

## E. History Command (2 modes)

Routing:
- If any of `--tx-hash` / `--order-id` / `--uop-hash` is provided → **Detail mode** → `/priapi/v5/wallet/agentic/order/detail` (precise single record)
- Otherwise → **List mode** → `/priapi/v5/wallet/agentic/order/list` (browse paged list)

### E1. List Mode (browse paged list)

Browse the transaction order list for the current or specified account. Use when the user wants to see recent transactions without knowing a specific identifier.

```bash
onchainos wallet history \
  [--account-id <id>] \
  [--chain <chain>] \
  [--begin <ms_timestamp>] \
  [--end <ms_timestamp>] \
  [--page-num <cursor>] \
  [--limit <n>]
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `--account-id` | string | No | Account ID to query. Defaults to the currently selected account. |
| `--chain` | string | No | Chain name or numeric ID (e.g. `ethereum` or `1`, `solana` or `501`). Resolved to chainIndex internally. |
| `--begin` | string | No | Start time filter (millisecond timestamp) |
| `--end` | string | No | End time filter (millisecond timestamp) |
| `--page-num` | string | No | Page cursor for pagination |
| `--limit` | string | No | Number of results per page |

> Note: `--order-id` / `--tx-hash` / `--uop-hash` are **not** accepted in list mode — providing any of them routes to detail mode automatically.

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

### E2. Detail Mode (single order lookup)

Look up a specific transaction by any of: `--order-id`, `--tx-hash`, or `--uop-hash`. Triggered whenever **any** of those flags is present.

**Preferred for Gas Station**: right after a GS broadcast, the user has the `orderId` but `txHash` is returned asynchronously by the relayer — use `--order-id` to poll status without waiting for the hash.

```bash
# Query by orderId (recommended right after broadcast)
onchainos wallet history \
  --chain <chain> \
  --order-id <id> \
  [--account-id <id>]

# Query by txHash (once relayer returns hash / for non-GS transactions)
onchainos wallet history \
  --chain <chain> \
  --tx-hash <hash> \
  [--address <addr>] \
  [--account-id <id>]

# Query by user-operation hash
onchainos wallet history \
  --chain <chain> \
  --uop-hash <hash> \
  [--account-id <id>]
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `--chain` | string | Yes | Chain name or numeric ID where the transaction occurred (e.g. `ethereum` or `1`, `solana` or `501`) |
| `--order-id` | string | No* | Order ID returned by broadcast. Any one of `--order-id` / `--tx-hash` / `--uop-hash` must be provided to enter detail mode. |
| `--tx-hash` | string | No* | Transaction hash (may not be available yet for GS transactions — prefer `--order-id` in that case) |
| `--uop-hash` | string | No* | User operation hash |
| `--address` | string | No | Wallet address hint (optional; backend filters by identifier above) |
| `--account-id` | string | No | Account ID. Defaults to the currently selected account. |

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
  --chain <chain> \
  [--amt <amount>] \
  [--input-data <hex_calldata>] \
  [--unsigned-tx <base58_tx>] \
  [--gas-limit <number>] \
  [--from <address>] \
  [--aa-dex-token-addr <address>] \
  [--aa-dex-token-amount <amount>] \
  [--mev-protection] \
  [--jito-unsigned-tx <jito_base58_tx>] \
  [--biz-type <biz_type>] \
  [--strategy <strategy>] \
  [--force]
```

| Parameter | Type | Required | Description                                                                                                                                     |
|---|---|---|---|
| `--to` | string | Yes | Contract address to interact with                                                                                                               |
| `--chain` | string | Yes | Chain name or numeric ID (e.g. `ethereum` or `1`, `solana` or `501`, `bsc` or `56`)                                                             |
| `--amt` | string | No | Native token amount in minimal units — whole number, no decimals (default "0"). See SKILL.md `--amt` section for conversion rules.              |
| `--input-data` | string | Conditional | EVM call data (hex-encoded, e.g. "0xa9059cbb..."). **Required for EVM chains.**                                                                 |
| `--unsigned-tx` | string | Conditional | Solana unsigned transaction data (base58). **Required for Solana.**                                                                             |
| `--gas-limit` | string | No | Gas limit override (EVM only). If omitted, the CLI estimates gas automatically.                                                                 |
| `--from` | string | No | Sender address — defaults to the selected account's address on the given chain.                                                                 |
| `--aa-dex-token-addr` | string | No | AA DEX token contract address (for AA DEX interactions).                                                                                        |
| `--aa-dex-token-amount` | string | No | AA DEX token amount (for AA DEX interactions).                                                                                                  |
| `--mev-protection` | bool | No | Enable MEV protection (default false). Supported on Ethereum, BSC, Base, and Solana. On Solana, `--jito-unsigned-tx` is also required.          |
| `--jito-unsigned-tx` | string | No | Jito unsigned transaction data (base58) for Solana MEV protection. **Required when `--mev-protection` is used on Solana.**                      |
| `--biz-type` | string | No | Transaction category (`transfer`,`dex`, `defi`, `dapp`)                                                                                         |
| `--strategy` | string | No | Strategy name                                                                                                                                   |
| `--force` | bool | No | Skip confirmation prompts from the backend (default false). Use when re-running a command after the user has confirmed a `confirming` response. |

> Either `--input-data` (EVM) or `--unsigned-tx` (Solana) must be provided. The CLI will fail if neither is present.

**Return fields:**

| Field | Type | Description |
|---|---|---|
| `txHash` | String | Broadcast transaction hash |

---

## G. Sign Message Command

### G1. `onchainos wallet sign-message`

Sign a message using the TEE-backed session key. Supports personalSign (EIP-191, EVM + Solana) and EIP-712 typed structured data (EVM only).

```bash
onchainos wallet sign-message \
  --chain <chain> \
  --message <message> \
  [--type <type>] \
  --from <address> \
  [--force]
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `--chain` | string | Yes | Chain name or numeric ID (e.g. `ethereum` or `1`, `solana` or `501`, `bsc` or `56`) |
| `--message` | string | Yes | Message to sign. For `personal`: arbitrary string. For `eip712`: JSON string of the typed data. |
| `--type` | string | No | Signing type: `personal` (default, EVM + Solana) or `eip712` (EVM only). |
| `--from` | string | Yes | Sender address — the address whose private key is used to sign. |
| `--force` | bool | No | Skip confirmation prompts from the backend (default false). Use when re-running a command after the user has confirmed a `confirming` response. |

> **Note:** Using `--type eip712` with `--chain 501` (Solana) will return an error. EIP-712 is only supported on EVM chains.

**Return fields (EVM chains):**

| Field | Type | Description |
|---|---|---|
| `signature` | String | The resulting signature (hex-encoded, as returned by the API) |

**Return fields (Solana, chain 501):**

| Field | Type | Description |
|---|---|---|
| `signature` | String | The resulting signature (base58-encoded, converted from hex) |
| `publicKey` | String | The signer's public address (the `--from` address) |

### G — Input / Output Examples

**User says:** "Sign this message on Ethereum: Hello World"

```bash
onchainos wallet sign-message --chain 1 --from 0xYourAddress --message "Hello World"
# -> personalSign (EVM). message.value is hex-encoded.
#   Signature: 0xabcdef1234567890...
```

---

**User says:** "Sign this message on Solana"

```bash
onchainos wallet sign-message --chain 501 --from SoLYourAddress --message "Hello World"
# -> personalSign (Solana). message.value is base58-encoded.
#   Signature: 3xB7mK9v... (base58)
#   PublicKey: SoLYourAddress
```

---

**User says:** "Sign this EIP-712 typed data on Ethereum"

```bash
onchainos wallet sign-message --chain 1 --from 0xYourAddress --type eip712 --message '{"types":{"EIP712Domain":[{"name":"name","type":"string"}],"Mail":[{"name":"contents","type":"string"}]},"primaryType":"Mail","domain":{"name":"Example"},"message":{"contents":"Hello"}}'
# -> eip712 (EVM only). Solana is NOT supported for eip712.
#   Signature: 0x1234abcd5678ef90...
```
