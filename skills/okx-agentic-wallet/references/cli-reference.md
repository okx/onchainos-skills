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
  [--force]
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
  [--chain <chain>] \
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
| `--chain` | string | No | Chain name or numeric ID (e.g. `ethereum` or `1`, `solana` or `501`). Resolved to chainIndex internally. |
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
  --chain <chain> \
  --address <addr> \
  [--account-id <id>] \
  [--order-id <id>] \
  [--uop-hash <hash>]
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `--tx-hash` | string | Yes | Transaction hash to look up |
| `--chain` | string | Yes | Chain name or numeric ID where the transaction occurred (e.g. `ethereum` or `1`, `solana` or `501`) |
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
  [--force]
```

| Parameter | Type | Required | Description |
|---|---|---|---|
| `--to` | string | Yes | Contract address to interact with |
| `--chain` | string | Yes | Chain name or numeric ID (e.g. `ethereum` or `1`, `solana` or `501`, `bsc` or `56`) |
| `--amt` | string | No | Native token amount in minimal units — whole number, no decimals (default "0"). See SKILL.md `--amt` section for conversion rules. |
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
