# Wallet Monitor

> Continuously poll wallet activity in-session and alert on new trades. Does not execute trades.

## Triggers

"帮我盯着这个钱包", "监控地址", "watch wallet", "盯着 [address]", "monitor this wallet"

## Required Skills

okx-dex-signal, okx-dex-token, okx-security

## Input

| Param             | Required | Default |
|-------------------|----------|---------|
| wallet_addresses  | Yes      | Max 10  |
| chain             | No       | Auto    |
| polling_interval  | No       | 60s     |

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
→ "看看 [symbol]"  |  → "用 [amount] [native_token] 买 [symbol]"
```

Multi-wallet convergence: `[MULTI-WALLET] {n} wallets bought {symbol}`

Exit when user says "停止监控".

## Actions

- → "看看 [symbol]" — triggers Token Research
- → "用 [amount] [native_token] 买 [symbol]" — triggers Safe Swap
- → "停止监控" — exits the loop

## Follow-up Workflows

Token Research (`workflows/token-research.md`), Safe Swap (`workflows/safe-swap.md`)
