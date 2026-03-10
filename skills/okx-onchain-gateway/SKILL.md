---
name: okx-onchain-gateway
description: "This skill should be used when the user asks to 'broadcast transaction', 'send tx', 'estimate gas', 'simulate transaction', 'check tx status', 'track my transaction', 'get gas price', 'gas limit', 'broadcast signed tx', or mentions broadcasting transactions, sending transactions on-chain, gas estimation, transaction simulation, tracking broadcast orders, or checking transaction status. Covers gas price, gas limit estimation, transaction simulation, transaction broadcasting, and order tracking across XLayer, Solana, Ethereum, Base, BSC, Arbitrum, Polygon, and 20+ other chains. Do NOT use for swap quote or execution - use okx-dex-swap instead. Do NOT use for general programming questions about transaction handling."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.3"
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

## Additional Resources

For detailed parameter tables, return field schemas, and usage examples for all 6 commands, consult:
- **`references/cli-reference.md`** — Full CLI command reference with params, return fields, and examples

To search for specific command details: `grep -n "onchainos gateway <command>" references/cli-reference.md`

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
