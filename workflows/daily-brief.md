# Daily Brief

> Combine market pulse, smart money activity, new token launches, and portfolio alerts into one morning report.

## Keyword Glossary

> If the user's query contains Chinese text, read `references/keyword-glossary.md` for trigger mappings.

## Triggers

"daily brief", "morning brief", "market overview", "what's the market doing today"

## Required Skills

okx-dex-token, okx-dex-market, okx-dex-signal, okx-dex-trenches, okx-wallet-portfolio

## Input

| Param          | Required | Default |
|----------------|----------|---------|
| chain          | No       | Solana  |
| wallet_address | No       | —       |

## CLI

Agent-orchestrated — no single CLI composite. The workflow spans four cross-skill steps whose outputs are meant to be summarised in prose; the agent runs the atomic commands in each step directly.

## Steps

### Step 1 — Market pulse [required] (parallel)

```
onchainos market prices --tokens "<chainIndex>:<SOL_addr>,<chainIndex>:<BTC_addr>,<chainIndex>:<ETH_addr>"
onchainos token hot-tokens --chain <chain>
onchainos market kline --address <SOL_addr> --chain solana --bar 1D --limit 7
```

> `market prices` requires `chainIndex:address` format. `hot-tokens` returns up to 100 results — display top 10.

Present: SOL / BTC / ETH prices, SOL 7-day trend, top 10 trending tokens

### Step 2 — Smart money activity [recommended] (parallel)

```
onchainos signal list --chain <chain>
onchainos tracker activities --tracker-type smart_money --chain <chain>
```

> `tracker activities` requires `--tracker-type`.

Present: SM signal tokens grouped by wallet count, recent SM/KOL buys and sells

### Step 3 — New token activity [recommended] (sequential)

```
onchainos memepump tokens --chain <chain> --stage MIGRATED
```

> No `--limit` param. MIGRATED stage returns tokens within 72h of migration — display top 10.

Present: recently migrated tokens with holder count, volume, SM count

### Step 4 — Portfolio alerts [recommended] (conditional: wallet_address provided)

```
onchainos portfolio all-balances --address <wallet> --chains <chain>
onchainos market portfolio-overview --address <wallet> --chain <chain>
```

Present: holdings summary, overall PnL, notable 24h changes in held tokens

## Output Template

```
DAILY BRIEF — {chain} — {date}

MARKET
BTC ${x}  |  ETH ${x}  |  SOL ${x}
SOL 7D: {trend summary}

HOT TOKENS
#  Symbol    Price     24h%     Vol
1  {sym}     ${x}     {x}%     ${x}
...

SMART MONEY
Buying: {sym} ({n} wallets), {sym} ({n})...
Selling: {sym} ({n} wallets)...
Notable: {addr} bought ${x} of {sym}

NEW TOKENS
Token    MCap     Holders  SM
{sym}    ${x}     {n}      {n}
...

[If wallet provided]
PORTFOLIO
Total: ${x}  |  PnL(24h): ${x}
Moves: {sym} {+/-x}%, {sym} {+/-x}%
```

## Actions

- → "research [symbol]" — Token Research (`token-research.md`)
- → "what is smart money buying" — Smart Money Signals (`smart-money-signals.md`)
