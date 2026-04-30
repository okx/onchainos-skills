# Smart Money Signals

> Collect smart money buy signals, aggregate by token, then run due diligence on each signal token.

## Keyword Glossary

> If the user's query contains Chinese text, read `references/keyword-glossary.md` for trigger mappings.

## Triggers

"smart money", "what are whales buying", "copy trading signals", "what is smart money buying", "KOL buys"

## Required Skills

okx-dex-signal, okx-dex-token, okx-dex-trenches, okx-security

## Input

| Param | Required | Default |
|-------|----------|---------|
| chain | No       | Solana  |

## CLI

Run the complete workflow in one command:

```
onchainos workflow smart-money [--chain <chain>]
```

## Steps

### Step 1 — Collect signals [required] (sequential)

```
onchainos signal list --chain <chain>
```

Aggregate by token: count distinct SM wallet addresses per token, sort descending by wallet count, take top 5.

Present: token list with SM wallet count per token

### Step 2 — Per-token due diligence [required] (parallel per token, max 5)

For each top token:

```
onchainos token price-info --address <token> --chain <chain>
onchainos token advanced-info --address <token> --chain <chain>
onchainos security token-scan --tokens "<chainIndex>:<token>"
```

If `advanced-info.protocolId` is non-empty, also run in parallel:

```
onchainos memepump token-dev-info --address <token> --chain <chain>
onchainos memepump token-bundle-info --address <token> --chain <chain>
```

Present: per token — price, mcap, mint/freeze, honeypot, tax flags, dev rug history, bundle rate

## Output Template

```
SMART MONEY SIGNALS — {chain} — {timestamp}
Scanned: {n} signal tokens → Top {m} by SM wallet count

#1  {name} ({symbol})
    SM Wallets: {n}  |  Price: ${x}  |  MCap: ${x}
    Honeypot: {Y/N}  |  Tax: {x}/{x}%  |  Mint: {A/R}  |  Freeze: {A/R}
    [If protocolId non-empty]
    Dev Rugs: {n}  |  Dev Holding: {x}%  |  Bundle: {x}%

#2  {name} ({symbol})
    ...
```

## Actions

- → "research [symbol]" — Token Research (`token-research.md`)
