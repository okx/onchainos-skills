# Shared Chain Name Support

> This file is shared across all onchainos skills.

The CLI accepts human-readable chain names and resolves them automatically.

## Wallet address creation (7 chains)

The following 7 chains support **wallet address creation** (i.e., you can generate a wallet address on these chains):

| Chain | Name | chainIndex |
|---|---|---|
| XLayer | `xlayer` | `196` |
| XLayer Testnet | `xlayer_test` | `1952` |
| Solana | `solana` | `501` |
| Ethereum | `ethereum` | `1` |
| Base | `base` | `8453` |
| BSC | `bsc` | `56` |
| Arbitrum | `arbitrum` | `42161` |

> **Note**: The wallet supports interacting with 17+ chains beyond this list (e.g., Polygon, Avalanche, Optimism).
> Run `onchainos wallet chains` for the full list of supported chains.

## Gas Station supported chains and tokens (Solana only)

Authoritative matrix for Gas Station. Use this when the Agent needs chain display name, native token symbol, or the set of stablecoins accepted.

| chainIndex | Display name | Native symbol | USDT | USDC | USDG |
|---|---|---|---|---|---|
| `501` | Solana | SOL | ✓ | ✓ | ✓ |

> **Always derive the per-tx token set from the response's `gasStationTokenList`** — it's backend-authoritative. The table above is for reference only (FAQ answers, unsupported-chain detection).

**Related rules** (see `references/gas-station.md`):
- Gas Station only triggers on Solana; for any other chain the backend returns `gasStationUsed=false` and the default native-gas flow runs.
- No account upgrade and no per-chain setup is required — first-time activation is just a DB flag flip after the user consents.
- Token selection priority: balance descending; on ties, USDT > USDC > USDG.
- Gas Station does NOT support: native SOL transfers, Jito Bundle transactions, single-tx value > 100,000 U.
- Every Gas Station state (enable flag, default gas token) is scoped to `(account, Solana)`.
