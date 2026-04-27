# Wallet Monitor

> Continuously poll wallet activity in-session and alert on new trades. Does not execute trades.

## Keyword Glossary

> If the user's query contains Chinese text, read `references/keyword-glossary.md` for trigger mappings.

## Triggers

"watch wallet", "monitor this wallet", "watch [address]", "alert me when this wallet trades"

## Required Skills

okx-dex-signal, okx-dex-token, okx-security

## Input

| Param            | Required | Default |
|------------------|----------|---------|
| wallet_addresses | Yes      | Max 10  |
| chain            | No       | Auto    |
| polling_interval | No       | 60s     |

## CLI

Agent-orchestrated — no single CLI composite. The workflow is a polling loop with conditional per-event enrichment, so a composite command would need streaming output and stateful diffing across ticks. For a background WebSocket session use the ws-based variant (`wallet-monitor-ws.md`).

## Steps

### Step 1 — Setup [required] (sequential)

Confirm monitoring address list and interval with the user.

### Step 2 — Poll loop [required] (sequential, repeating every `interval` seconds)

```
onchainos tracker activities --tracker-type multi_address --wallet-address <wallet> --chain <chain>
```

> `--tracker-type` is required. Multiple addresses comma-separated, max 20.

Diff against previous poll to detect new transactions. On new buy:

```
onchainos token price-info --address <new_token> --chain <chain>
onchainos security token-scan --tokens "<chainIndex>:<new_token>"
```

Alert format:

```
[{time}] ALERT — {label/addr}
{Buy/Sell} {symbol} — ${amount}
Price: ${x}  |  MCap: ${x}
Honeypot: {Y/N}  |  Tax: {x}/{x}%
→ "research [symbol]"  |  → "buy [amount] [native_token] of [symbol]"
```

Multi-wallet convergence: `[MULTI-WALLET] {n} wallets bought {symbol}`

Exit when user says "stop monitoring".

## Actions

- → "research [symbol]" — Token Research (`token-research.md`)
- → "stop monitoring" — exits the loop
