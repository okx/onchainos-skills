# Safe Swap

> Safety check + quote + execute a token swap with MEV protection. Security rules follow okx-security SKILL.md.

## Keyword Glossary

> If the user's query contains Chinese text, read `references/keyword-glossary.md` for trigger mappings.

## Triggers

"buy [token]", "sell [token]", "swap", "trade [token] for [token]", "exchange [token]"

## Required Skills

okx-security, okx-dex-token, okx-dex-swap, okx-onchain-gateway

## Input

| Param          | Required | Default |
|----------------|----------|---------|
| token_in       | Yes      | —       |
| token_out      | Yes      | —       |
| amount         | Yes      | —       |
| chain          | Yes      | —       |
| wallet_address | Yes      | —       |
| slippage       | No       | auto    |

## Steps

### Step 1 — Pre-trade data [required] (parallel)

Prefer composite command if available:

```
onchainos swap safe-quote --chain <chain> --from <in> --to <out> --readable-amount <amount>
```

Fallback — run all 3 in parallel:

```
onchainos security token-scan --tokens "<chainIndex>:<token_out>"
onchainos token advanced-info --address <token_out> --chain <chain>
onchainos swap quote --chain <chain> --from <in> --to <out> --readable-amount <amount>
```

> `swap quote` uses `--from`/`--to` (not `--from-token`/`--to-token`). `--readable-amount` handles decimals automatically.

Present: honeypot, tax rates, mint/freeze, riskControlLevel, quote amount, price impact, gas

### Step 2 — Present & confirm [required] (sequential)

Display all Step 1 data. Apply risk controls per `okx-security` SKILL.md:
- **BLOCK**: auto-abort and tell user the reason
- **WARN**: show risk data, wait for explicit user confirmation
- **PASS**: display data and continue

### Step 3 — Simulate [recommended] (conditional: user requests simulation)

```
onchainos gateway simulate --from <wallet> --to <contract> --data <calldata> --chain <chain>
onchainos security tx-scan --from <wallet> --chain <chain> --data <calldata>
```

> `calldata` comes from `swap quote` response `txData` field.

Present: expected asset changes, gas, abnormal approvals or transfers

### Step 4 — Execute [required, after user confirmation] (sequential)

> Only supported with agentic wallet.

```
onchainos swap execute --chain <chain> \
  --from <in> --to <out> --readable-amount <amount> \
  --wallet <wallet> --slippage <slippage> --mev-protection
```

> `--mev-protection` routes Solana via Jito, EVM via Flashbots.

### Step 5 — Confirm [required] (sequential)

```
onchainos gateway orders --address <wallet> --chain <chain>
```

> Use tx hash from Step 4 if returned. `--address` (not `--wallet`).

Present: tx hash, status, actual amount received, gas used

## Output Template

```
PRE-TRADE
Target: {symbol} ({chain})
Honeypot: {Y/N}  |  Risk Level: {1-5}
Buy Tax: {x}%  |  Sell Tax: {x}%
Mint: {A/R}  |  Freeze: {A/R}
Quote: {x} {in} → {y} {out}
Price Impact: {x}%  |  Gas: {x} {native}
[Confirm?]

RESULT
Sold: {x} {in}  →  Received: {y} {out}
Tx: {hash}  |  Status: {status}  |  Gas: {x}
```

## Actions

- → "watch this token" — triggers Wallet Monitor
- → "check my portfolio" — triggers Portfolio Check

## Follow-up Workflows

Portfolio Check (`workflows/portfolio-check.md`), Wallet Monitor (`workflows/wallet-monitor.md`)
