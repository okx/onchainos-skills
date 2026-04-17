# OnchainOS Skills Capability Matrix

This matrix gives contributors and integrators a quick map of what each skill does and how to compose them.

## Skill x Action Matrix

| Skill | Discover | Market Data | Portfolio | Swap Quote | Swap Execute | Simulate | Broadcast | Order Tracking |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| `okx-dex-token` | ✅ | ✅ (enriched analytics) | - | - | - | - | - | - |
| `okx-dex-market` | ✅ (signals/meme scan) | ✅ (raw price/kline/trades/index) | - | - | - | - | - | - |
| `okx-wallet-portfolio` | - | - | ✅ | - | - | - | - | - |
| `okx-dex-swap` | - | - | - | ✅ | ✅ (tx data generation) | - | - | - |
| `okx-onchain-gateway` | - | - | - | - | - | ✅ | ✅ | ✅ |

## Chain Coverage Snapshot

> Source: skill docs in `skills/*/SKILL.md` and project README.

| Capability | Coverage |
|---|---|
| Token / Market / Portfolio / Swap / Gateway core flows | 20+ chains (including XLayer, Solana, Ethereum, Base, BSC, Arbitrum, Polygon) |
| Smart-money signals (`market signal-*`) | Chain-dependent; discover via `onchainos market signal-chains` |
| Meme pump endpoints (`market memepump-*`) | Solana (501), BSC (56), X Layer (196), TRON (195) |
| `swap exactOut` mode | Ethereum (1), Base (8453), BSC (56), Arbitrum (42161) |

## Recommended Workflow Patterns

### 1) Research -> Trade
1. `token search` / `token price-info`
2. `market kline` / `market trades`
3. `swap quote`
4. `swap swap` (get tx data)
5. Sign locally
6. `gateway broadcast` -> `gateway orders`

### 2) Portfolio-first Trading
1. `portfolio all-balances`
2. `token price-info` (top holdings)
3. `swap quote` / `swap swap`
4. Sign + `gateway broadcast`

### 3) Safety-first Meme Flow
1. `market memepump-tokens`
2. `market memepump-token-details`
3. `market memepump-token-dev-info`
4. `market memepump-token-bundle-info`
5. `swap quote` only after user confirmation

## Notes for Contributors

- Keep this matrix in sync with command references in `skills/*/SKILL.md`.
- Prefer additive edits (new rows/columns) over rewrites.
- If a capability is chain-limited, document discovery command + explicit exceptions.
