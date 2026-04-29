# Shared Chain Name Support

> This file is shared across all onchainos skills.

The CLI accepts human-readable chain names and resolves them automatically.

When a wallet account is created via `onchainos wallet add`, the response's `addressList` enumerates every chain on which an address was generated for that account — the backend determines the full set at creation time, currently spanning 18+ chains across the EVM and Solana families (Ethereum, BNB Chain, Polygon, Arbitrum, Base, Optimism, X Layer, Avalanche, Linea, Scroll, zkSync, Sonic, Blast, Fantom, Monad, Conflux, Tempo, Solana, etc.). Treat that response as the source of truth — do not hard-code a chain count or list here.

For the up-to-date list of chains the CLI recognizes (with both name aliases and chainIndex), run `onchainos wallet chains`.
