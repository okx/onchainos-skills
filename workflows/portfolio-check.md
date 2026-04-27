# Portfolio Check

> Show wallet token balances, total value, and per-token PnL.

## Keyword Glossary

> If the user's query contains Chinese text, read `references/keyword-glossary.md` for trigger mappings.

## Triggers

"portfolio", "check my holdings", "my wallet", "what tokens do I have", "my assets"

## Required Skills

okx-wallet-portfolio, okx-dex-market, okx-dex-token

## Input

| Param          | Required | Default       |
|----------------|----------|---------------|
| wallet_address | Yes      | —             |
| chain          | No       | All supported |

## CLI

The CLI composite covers Step 1 (overview) only:

```
onchainos workflow portfolio --address <addr> [--chains <chains>]
```

Step 2 (per-token detail) is optional and agent-orchestrated — loop the atomic commands below over each holding returned by Step 1.

## Steps

### Step 1 — Overview [required] (parallel)

```
onchainos portfolio all-balances --address <wallet> --chains <chain>
onchainos portfolio total-value --address <wallet> --chains <chain>
onchainos market portfolio-overview --address <wallet> --chain <chain>
```

> `all-balances` and `total-value` use `--chains` (plural).

Present: total value, token balances, PnL, win rate

### Step 2 — Per-token detail [recommended] (parallel per holding, agent-orchestrated)

For each held token:

```
onchainos market portfolio-token-pnl --address <wallet> --chain <chain> --token <addr>
onchainos token price-info --address <addr> --chain <chain>
```

> `portfolio-token-pnl` uses `--token` (not `--token-address`). Not covered by `onchainos workflow portfolio` — run these per holding when a deeper per-token view is requested.

Present: per token — price, 24h change, realized / unrealized PnL, avg cost

## Output Template

```
PORTFOLIO — {short_addr}
Total: ${x}  |  PnL(30d): ${x}  |  Win Rate: {x}%

--- HOLDINGS ---
#1  {sym}  Balance: {n}  |  Value: ${x}  |  24h: {x}%  |  PnL: ${x}  |  AvgCost: ${x}
#2  {sym}  ...
```

## Actions

- → "research [symbol]" — Token Research (`token-research.md`)
