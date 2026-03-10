---
name: okx-onchain-gateway
description: "This skill should be used when the user asks to 'broadcast transaction', 'send tx', 'estimate gas', 'simulate transaction', 'check tx status', 'track my transaction', 'get gas price', 'gas limit', 'broadcast signed tx', or mentions broadcasting transactions, sending transactions on-chain, gas estimation, transaction simulation, tracking broadcast orders, or checking transaction status. Covers gas price, gas limit estimation, transaction simulation, transaction broadcasting, and order tracking across XLayer, Solana, Ethereum, Base, BSC, Arbitrum, Polygon, and 20+ other chains. Do NOT use for swap quote or execution - use okx-dex-swap instead. Do NOT use for general programming questions about transaction handling."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.2"
  homepage: "https://web3.okx.com"
---

# OKX Onchain Gateway CLI

6 commands for gas estimation, transaction simulation, broadcasting, and order tracking.

## Pre-flight Checks

Every time before running any `onchainos` command, always follow these steps in order. Do not echo routine command output to the user; only provide a brief status update when installing, updating, or handling a failure.

1. **Confirm installed**: Run `which onchainos`. If not found, install it:
   ```bash
   curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
   ```
   If the install script fails, ask the user to install manually following the instructions at: https://github.com/okx/onchainos-skills

2. **Check for updates**: Read `~/.onchainos/last_check` and compare it with the current timestamp:
   ```bash
   cached_ts=$(cat ~/.onchainos/last_check 2>/dev/null || true)
   now=$(date +%s)
   ```
   - If `cached_ts` is non-empty and `(now - cached_ts) < 43200` (12 hours), skip the update and proceed.
   - Otherwise (file missing or older than 12 hours), run the installer to check for updates:
     ```bash
     curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
     ```
     If a newer version is installed, tell the user and suggest updating their onchainos skills from https://github.com/okx/onchainos-skills to get the latest features.
3. If any `onchainos` command fails with an unexpected error during this
   session, try reinstalling before giving up:
   ```bash
   curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
   ```
4. Create a `.env` file in the project root to override the default API credentials (optional — skip this for quick start):
   ```
   OKX_API_KEY=          # or OKX_ACCESS_KEY
   OKX_SECRET_KEY=
   OKX_PASSPHRASE=
   ```

## Skill Routing

- For swap quote and execution → use `okx-dex-swap`
- For market prices → use `okx-dex-market`
- For token search → use `okx-dex-token`
- For wallet balances / portfolio → use `okx-wallet-portfolio`
- For transaction broadcasting → use this skill (`okx-onchain-gateway`)

## Quickstart

```bash
# Get current gas price on XLayer
onchainos gateway gas --chain xlayer

# Estimate gas limit for a transaction
onchainos gateway gas-limit --from 0xYourWallet --to 0xRecipient --chain xlayer

# Simulate a transaction (dry-run)
onchainos gateway simulate --from 0xYourWallet --to 0xContract --data 0x... --chain xlayer

# Broadcast a signed transaction
onchainos gateway broadcast --signed-tx 0xf86c...signed --address 0xYourWallet --chain xlayer

# Track order status
onchainos gateway orders --address 0xYourWallet --chain xlayer --order-id 123456789
```

## Chain Name Support

The CLI accepts human-readable chain names and resolves them automatically.

| Chain | Name | chainIndex |
|---|---|---|
| XLayer | `xlayer` | `196` |
| Solana | `solana` | `501` |
| Ethereum | `ethereum` | `1` |
| Base | `base` | `8453` |
| BSC | `bsc` | `56` |
| Arbitrum | `arbitrum` | `42161` |

## Command Index

| # | Command | Description |
|---|---|---|
| 1 | `onchainos gateway chains` | Get supported chains for gateway |
| 2 | `onchainos gateway gas --chain <chain>` | Get current gas prices for a chain |
| 3 | `onchainos gateway gas-limit --from ... --to ... --chain ...` | Estimate gas limit for a transaction |
| 4 | `onchainos gateway simulate --from ... --to ... --data ... --chain ...` | Simulate a transaction (dry-run) |
| 5 | `onchainos gateway broadcast --signed-tx ... --address ... --chain ...` | Broadcast a signed transaction |
| 6 | `onchainos gateway orders --address ... --chain ...` | Track broadcast order status |

## Cross-Skill Workflows

This skill is the **final mile** — it takes a signed transaction and sends it on-chain. It pairs with swap (to get tx data).

### Workflow A: Swap → Broadcast → Track

> User: "Swap 1 ETH for USDC and broadcast it"

```
1. okx-dex-swap     onchainos swap swap --from ... --to ... --amount ... --chain ethereum --wallet <addr>
       ↓ user signs the tx locally
2. okx-onchain-gateway  onchainos gateway broadcast --signed-tx <signed_hex> --address <addr> --chain ethereum
       ↓ orderId returned
3. okx-onchain-gateway  onchainos gateway orders --address <addr> --chain ethereum --order-id <orderId>
```

**Data handoff**:
- `tx.data`, `tx.to`, `tx.value`, `tx.gas` from swap → user builds & signs → `--signed-tx` for broadcast
- `orderId` from broadcast → `--order-id` param in orders query

### Workflow B: Simulate → Broadcast → Track

> User: "Simulate this transaction first, then broadcast if safe"

```
1. onchainos gateway simulate --from 0xWallet --to 0xContract --data 0x... --chain ethereum
       ↓ simulation passes (no revert)
2. onchainos gateway broadcast --signed-tx <signed_hex> --address 0xWallet --chain ethereum
3. onchainos gateway orders --address 0xWallet --chain ethereum --order-id <orderId>
```

### Workflow C: Gas Check → Swap → Broadcast

> User: "Check gas, swap for USDC, then send it"

```
1. onchainos gateway gas --chain ethereum                                    → check gas prices
2. okx-dex-swap     onchainos swap swap --from ... --to ... --chain ethereum --wallet <addr>
       ↓ user signs
3. onchainos gateway broadcast --signed-tx <signed_hex> --address <addr> --chain ethereum
4. onchainos gateway orders --address <addr> --chain ethereum --order-id <orderId>
```

## Operation Flow

### Step 1: Identify Intent

- Estimate gas for a chain → `onchainos gateway gas`
- Estimate gas limit for a specific tx → `onchainos gateway gas-limit`
- Test if a tx will succeed → `onchainos gateway simulate`
- Broadcast a signed tx → `onchainos gateway broadcast`
- Track a broadcast order → `onchainos gateway orders`
- Check supported chains → `onchainos gateway chains`

### Step 2: Collect Parameters

- Missing chain → recommend XLayer (`--chain xlayer`, low gas, fast confirmation) as the default, then ask which chain the user prefers
- Missing `--signed-tx` → remind user to sign the transaction first (this CLI does NOT sign)
- Missing wallet address → ask user
- For gas-limit / simulate → need `--from`, `--to`, optionally `--data` (calldata)
- For orders query → need `--address` and `--chain`, optionally `--order-id`

### Step 3: Execute

- **Gas estimation**: call `onchainos gateway gas` or `gas-limit`, display results
- **Simulation**: call `onchainos gateway simulate`, check for revert or success
- **Broadcast**: call `onchainos gateway broadcast` with signed tx, return `orderId`
- **Tracking**: call `onchainos gateway orders`, display order status

### Step 4: Suggest Next Steps

After displaying results, suggest 2-3 relevant follow-up actions:

| Just completed | Suggest |
|---|---|
| `gateway gas` | 1. Estimate gas limit for a specific tx → `onchainos gateway gas-limit` (this skill) 2. Get a swap quote → `okx-dex-swap` |
| `gateway gas-limit` | 1. Simulate the transaction → `onchainos gateway simulate` (this skill) 2. Proceed to broadcast → `onchainos gateway broadcast` (this skill) |
| `gateway simulate` | 1. Broadcast the transaction → `onchainos gateway broadcast` (this skill) 2. Adjust and re-simulate if failed |
| `gateway broadcast` | 1. Track order status → `onchainos gateway orders` (this skill) |
| `gateway orders` | 1. View price of received token → `okx-dex-market` 2. Execute another swap → `okx-dex-swap` |

Present conversationally, e.g.: "Transaction broadcast! Would you like to track the order status?" — never expose skill names or endpoint paths to the user.

## CLI Command Reference

### 1. onchainos gateway chains

Get supported chains for gateway. No parameters required.

```bash
onchainos gateway chains
```

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier (e.g., `"1"`, `"501"`) |
| `name` | String | Human-readable chain name (e.g., `"Ethereum"`) |
| `logoUrl` | String | Chain logo image URL |
| `shortName` | String | Chain short name (e.g., `"ETH"`) |

### 2. onchainos gateway gas

Get current gas prices for a chain.

```bash
onchainos gateway gas --chain <chain>
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--chain` | Yes | - | Chain name (e.g., `ethereum`, `solana`, `xlayer`) |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `normal` | String | Normal gas price (legacy) |
| `min` | String | Minimum gas price |
| `max` | String | Maximum gas price |
| `supporteip1559` | Boolean | Whether EIP-1559 is supported |
| `eip1559Protocol.suggestBaseFee` | String | Suggested base fee |
| `eip1559Protocol.baseFee` | String | Current base fee |
| `eip1559Protocol.proposePriorityFee` | String | Proposed priority fee |
| `eip1559Protocol.safePriorityFee` | String | Safe (slow) priority fee |
| `eip1559Protocol.fastPriorityFee` | String | Fast priority fee |

For Solana chains: `proposePriorityFee`, `safePriorityFee`, `fastPriorityFee`, `extremePriorityFee`.

### 3. onchainos gateway gas-limit

Estimate gas limit for a transaction.

```bash
onchainos gateway gas-limit --from <address> --to <address> --chain <chain> [--amount <amount>] [--data <hex>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--from` | Yes | - | Sender address |
| `--to` | Yes | - | Recipient / contract address |
| `--chain` | Yes | - | Chain name |
| `--amount` | No | `"0"` | Transfer value in minimal units |
| `--data` | No | - | Encoded calldata (hex, for contract interactions) |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `gasLimit` | String | Estimated gas limit for the transaction |

### 4. onchainos gateway simulate

Simulate a transaction (dry-run).

```bash
onchainos gateway simulate --from <address> --to <address> --data <hex> --chain <chain> [--amount <amount>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--from` | Yes | - | Sender address |
| `--to` | Yes | - | Recipient / contract address |
| `--data` | Yes | - | Encoded calldata (hex) |
| `--chain` | Yes | - | Chain name |
| `--amount` | No | `"0"` | Transfer value in minimal units |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `intention` | String | Transaction intent description |
| `assetChange[]` | Array | Asset changes from the simulation |
| `assetChange[].symbol` | String | Token symbol |
| `assetChange[].rawValue` | String | Raw amount change |
| `gasUsed` | String | Gas consumed in simulation |
| `failReason` | String | Failure reason (empty string = success) |
| `risks[]` | Array | Risk information |

### 5. onchainos gateway broadcast

Broadcast a signed transaction.

```bash
onchainos gateway broadcast --signed-tx <tx> --address <address> --chain <chain>
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--signed-tx` | Yes | - | Fully signed transaction (hex for EVM, base58 for Solana) |
| `--address` | Yes | - | Sender wallet address |
| `--chain` | Yes | - | Chain name |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `orderId` | String | OKX order tracking ID (use for order status queries) |
| `txHash` | String | On-chain transaction hash |

### 6. onchainos gateway orders

Track broadcast order status.

```bash
onchainos gateway orders --address <address> --chain <chain> [--order-id <id>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--chain` | Yes | - | Chain name |
| `--order-id` | No | - | Specific order ID (from broadcast response) |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `cursor` | String | Pagination cursor for next page |
| `orders[]` | Array | List of order objects |
| `orders[].orderId` | String | OKX order tracking ID |
| `orders[].txHash` | String | On-chain transaction hash |
| `orders[].chainIndex` | String | Chain identifier |
| `orders[].address` | String | Wallet address |
| `orders[].txStatus` | String | Transaction status: `1` = Pending, `2` = Success, `3` = Failed |
| `orders[].failReason` | String | Failure reason (empty if successful) |

## Input / Output Examples

**User says:** "What's the current gas price on XLayer?"

```bash
onchainos gateway gas --chain xlayer
# → Display:
#   Base fee: 0.05 Gwei
#   Max fee: 0.1 Gwei
#   Priority fee: 0.01 Gwei
```

**User says:** "Simulate this swap transaction before I send it"

```bash
onchainos gateway simulate --from 0xYourWallet --to 0xDexContract --data 0x... --chain xlayer --amount 1000000000000000000
# → Display:
#   Simulation: SUCCESS
#   Estimated gas: 145,000
#   Intent: Token Swap
```

**User says:** "Broadcast my signed transaction"

```bash
onchainos gateway broadcast --signed-tx 0xf86c...signed --address 0xYourWallet --chain xlayer
# → Display:
#   Broadcast successful!
#   Order ID: 123456789
#   Tx Hash: 0xabc...def
```

**User says:** "Check the status of my broadcast order"

```bash
onchainos gateway orders --address 0xYourWallet --chain xlayer --order-id 123456789
# → Display:
#   Order 123456789: Success (txStatus=2)
#   Tx Hash: 0xabc...def
#   Confirmed on-chain
```

## Edge Cases

- **MEV protection**: Broadcasting through OKX nodes may offer MEV protection on supported chains.
- **Solana special handling**: Solana signed transactions use **base58** encoding (not hex). Ensure the `--signed-tx` format matches the chain.
- **Chain not supported**: call `onchainos gateway chains` first to verify.
- **Node return failed**: the underlying blockchain node rejected the transaction. Common causes: insufficient gas, nonce too low, contract revert. Retry with corrected parameters.
- **Wallet type mismatch**: the address format does not match the chain (e.g., EVM address on Solana chain).
- **Network error**: retry once, then prompt user to try again later
- **Region restriction (error code 50125 or 80001)**: do NOT show the raw error code to the user. Instead, display a friendly message: `⚠️ Service is not available in your region. Please switch to a supported region and try again.`
- **Transaction already broadcast**: if the same `--signed-tx` is broadcast twice, the API may return an error or the same `txHash` — handle idempotently.

## Amount Display Rules

- Gas prices in Gwei for EVM chains (`18.5 Gwei`), never raw wei
- Gas limit as integer (`21000`, `145000`)
- USD gas cost estimate when possible
- Transaction values in UI units (`1.5 ETH`), never base units

## Global Notes

- **This skill does NOT sign transactions** — it only broadcasts pre-signed transactions
- Amounts in parameters use **minimal units** (wei/lamports)
- Gas price fields: use `eip1559Protocol.suggestBaseFee` + `proposePriorityFee` for EIP-1559 chains, `normal` for legacy
- EVM contract addresses must be **all lowercase**
- The CLI resolves chain names automatically (e.g., `ethereum` → `1`, `solana` → `501`)
- The CLI handles authentication internally via environment variables — see Prerequisites step 4 for default values
