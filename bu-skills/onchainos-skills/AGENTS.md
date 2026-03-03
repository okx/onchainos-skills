# onchainos Skills — Agent Instructions

This is an **onchainos skill collection** providing 5 skills for on-chain operations: token search, market data, wallet balance, swap execution, and transaction broadcasting across 20+ blockchains.

## Available Skills

| Skill | Purpose | When to Use |
|-------|---------|-------------|
| okx-wallet-portfolio | Wallet balance and portfolio value | User asks about wallet holdings, token balances, portfolio value, remaining funds |
| okx-dex-market | Prices, K-line charts, trade history | User asks for token prices, candlestick data, trade logs, index prices |
| okx-dex-swap | DEX swap execution | User wants to swap, trade, buy, or sell tokens on-chain |
| okx-dex-token | Token search, metadata, rankings | User searches for tokens, wants trending rankings, holder distribution, market cap |
| okx-onchain-gateway | Gas estimation, tx simulation, broadcasting | User wants to broadcast tx, estimate gas, simulate, or track tx status |

## Skill Discovery

Skills are in the `skills/` directory. Each skill contains a `SKILL.md` with:

- YAML frontmatter (name, description, metadata)
- Full API reference with endpoints, parameters, and response schemas
- Code examples (TypeScript)
- Cross-skill workflow documentation
- Edge cases and error handling


