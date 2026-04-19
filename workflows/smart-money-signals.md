# Smart Money Signals

> Collect smart money buy signals, aggregate by token, then run due diligence on each signal token.

## Triggers

"聪明钱在买什么", "跟单信号", "smart money", "聪明钱信号", "KOL在买什么"

## Required Skills

okx-dex-signal, okx-dex-token, okx-dex-trenches, okx-security

## Input

| Param | Required | Default |
|-------|----------|---------|
| chain | No       | Solana  |

## Steps

### Step 1 — Collect signals [required] (sequential)

```
onchainos signal list --chain <chain>
```

Aggregate by token: count distinct SM wallet addresses per token, sort descending by wallet count, take top 5–10.

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

- → "看看 [symbol]" — triggers Token Research
- → "用 [amount] [native_token] 买 [symbol]" — triggers Safe Swap

## Follow-up Workflows

Token Research (`workflows/token-research.md`), Safe Swap (`workflows/safe-swap.md`)
