# Shared Chain Name Support

> This file is shared across all onchainos skills.

The CLI accepts human-readable chain names and resolves them automatically.

## Wallet address creation (6 chains)

The following 6 chains support **wallet address creation** (i.e., you can generate a wallet address on these chains):

| Chain | Name | chainIndex |
|---|---|---|
| XLayer | `xlayer` | `196` |
| Solana | `solana` | `501` |
| Ethereum | `ethereum` | `1` |
| Base | `base` | `8453` |
| BSC | `bsc` | `56` |
| Arbitrum | `arbitrum` | `42161` |

> **Note**: The wallet supports interacting with 17+ chains beyond this list (e.g., Polygon, Avalanche, Optimism).
> Run `onchainos wallet chains` for the full list of supported chains.

## Gas Station supported chains and tokens (11 EVM chains)

Authoritative matrix for Gas Station. Use this when the Agent needs chain display names, native token symbols, or the set of stablecoins accepted on each chain.

| chainIndex | Display name | Native symbol | USDT | USDC | USDG |
|---|---|---|---|---|---|
| `1` | Ethereum | ETH | ✓ | ✓ | ✓ |
| `56` | BNB Chain | BNB | ✓ | ✓ | |
| `8453` | Base | ETH | ✓ | ✓ | |
| `42161` | Arbitrum One | ETH | ✓ | ✓ | |
| `137` | Polygon | MATIC | ✓ | ✓ | |
| `10` | Optimism | ETH | ✓ | ✓ | |
| `1030` | Conflux eSpace | CFX | ✓ | ✓ | |
| `59144` | Linea | ETH | ✓ | ✓ | |
| `534352` | Scroll | ETH | ✓ | ✓ | |
| `10143` | Monad | MON | ✓ | ✓ | |
| `146` | Sonic EVM | S | | ✓ | |

> **Always derive the per-tx token set from the Phase 1 response's `gasStationTokenList`** — it's backend-authoritative and already scoped to the current chain. The table above is for reference only (FAQ answers, chain-list questions, unsupported-chain detection). chainIndex values for Conflux / Monad / Sonic are included for completeness; verify against `onchainos wallet chains` before relying on them for dispatch.

**Related rules** (see `references/gas-station.md`):
- Gas Station only triggers on the 11 chains above; for any other chain the backend returns `gasStationUsed=false` and the default native-gas flow runs.
- Per-chain setup is one-time and happens automatically inside the first Gas Station transaction on that chain.
- Every Gas Station state (enable flag, default gas token) is scoped to `(account, chain)` — switching a chain doesn't affect others.
