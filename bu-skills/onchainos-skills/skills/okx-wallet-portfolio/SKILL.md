---
name: okx-wallet-portfolio
description: "This skill should be used when the user asks to 'check my wallet balance', 'show my token holdings', 'how much OKB do I have', 'what tokens do I have', 'check my portfolio value', 'view my assets', 'how much is my portfolio worth', 'what\\'s in my wallet', or mentions checking wallet balance, total assets, token holdings, portfolio value, remaining funds, DeFi positions, or multi-chain balance lookup. Supports XLayer, Solana, Ethereum, Base, BSC, Arbitrum, Polygon, and 20+ other chains. Do NOT use for general programming questions about balance variables or API documentation. Do NOT use when the user is asking how to build or integrate a balance feature into code."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.0"
  homepage: "https://web3.okx.com"
---

# OKX Wallet Portfolio

> **Note**: Wallet portfolio CLI commands are not yet available. This functionality is planned for a future release. In the meantime, other skills provide related capabilities — see Skill Routing below.

## Prerequisites

Before using other OKX skills, ensure the `onchainos` CLI is installed:

1. Check if `onchainos` is already available:
   ```bash
   which onchainos
   ```
2. If not found, install it:
   ```bash
   curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
   ```
3. Verify installation:
   ```bash
   onchainos --version
   ```
4. If the install script fails, ask the user to install manually following the instructions at: https://github.com/okx/onchainos-skills
5. Create a `.env` file in the project root to override the default API credentials (optional — skip this for quick start):
   ```
   OKX_API_KEY=
   OKX_SECRET_KEY=
   OKX_PASSPHRASE=
   ```

## Skill Routing

Since wallet portfolio CLI commands are not yet available, use the following skills for related tasks:

- For token prices / K-lines → use `okx-dex-market`
- For token search / metadata → use `okx-dex-token`
- For swap execution → use `okx-dex-swap`
- For transaction broadcasting → use `okx-onchain-gateway`

## Current Limitations

The following wallet portfolio operations are **not yet supported** via CLI:

- Check total asset value across chains
- View all token balances for a wallet address
- Query specific token balances
- Check supported chains for balance queries

When a user asks about wallet balance or portfolio, inform them that this feature is coming soon and suggest alternative workflows using the available skills listed above.

## Common Chain IDs

| Chain | Name | chainIndex |
|---|---|---|
| XLayer | `xlayer` | `196` |
| Solana | `solana` | `501` |
| Ethereum | `ethereum` | `1` |
| Base | `base` | `8453` |
| BSC | `bsc` | `56` |
| Arbitrum | `arbitrum` | `42161` |

**Address format note**: EVM addresses (`0x...`) work across Ethereum/BSC/Polygon/Arbitrum/Base etc. Solana addresses (Base58) and Bitcoin addresses (UTXO) have different formats. Do NOT mix formats across chain types.

## Amount Display Rules

- Token amounts in UI units (`1.5 ETH`), never base units (`1500000000000000000`)
- USD values with 2 decimal places
- Large amounts in shorthand (`$1.2M`)
- Sort by USD value descending
