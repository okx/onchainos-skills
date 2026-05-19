# Wallet Analysis

> Pull a wallet's performance metrics, trading behavior, current holdings, and recent activity.

## Keyword Glossary

> If the user's query contains Chinese text, read `references/keyword-glossary.md` for trigger mappings.

## Triggers

"analyze wallet", "check this address", "is this wallet worth following", "what's this wallet's trading style"

## Required Skills

okx-dex-market, okx-wallet-portfolio, okx-dex-signal

## Input

| Param          | Required | Default     |
|----------------|----------|-------------|
| wallet_address | Yes      | —           |
| chain          | No       | Auto-detect |

## CLI

Run the complete workflow in one command:

```
onchainos workflow wallet-analysis --address <addr> [--chain <chain>]
```

## Steps

### Step 1 — Performance [required] (parallel)

```
onchainos market portfolio-overview --address <wallet> --chain <chain> --time-frame 3
onchainos market portfolio-overview --address <wallet> --chain <chain> --time-frame 4
onchainos portfolio all-balances --address <wallet> --chains <chain>
```

> `--time-frame`: 1=1D, 2=3D, 3=7D, 4=1M, 5=3M. `portfolio all-balances` uses `--chains` (plural).

Present: 7d vs 30d PnL, win rate, realized profit, trade count, current holdings

### Step 2 — Trading behavior [recommended] (sequential)

```
onchainos market portfolio-recent-pnl --address <wallet> --chain <chain>
```

Present: per-token PnL, trading frequency

### Step 3 — Recent activity [recommended] (sequential)

```
onchainos tracker activities --tracker-type multi_address --wallet-address <wallet> --chain <chain>
```

Present: most recent trades — time, token, direction, amount

## Output Template

```
WALLET: {short_addr} ({chain})

PERFORMANCE
           7d         30d
PnL:       ${x}       ${x}
Win Rate:  {x}%       {x}%
Realized:  ${x}       ${x}
Trades:    {n}        {n}

HOLDINGS
Token   Balance  Value    Unrealized
{sym}   {n}      ${x}    ${x}
...

BEHAVIOR
Avg Hold: {duration}  |  Avg Size: ${x}  |  Freq: {n}/day
Most Traded: {sym1}, {sym2}, {sym3}

TOKEN PnL
Token   Realized  Unrealized
{sym}   ${x}      ${x}
...

RECENT
Time    Token   Action  Amount
{time}  {sym}   Buy     ${x}
...
```

## Actions

- → "watch [address]" — Wallet Monitor (`wallet-monitor.md`)
- → "research [token_they_hold]" — Token Research (`token-research.md`)
