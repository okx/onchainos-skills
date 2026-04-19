# Token Research

> Pull price, contract, security, holders, top traders, and smart money signals for any token in one flow.

## Triggers

"帮我看看这个币", "analyze token", "这个币怎么样", "research" + address, "这个币安全吗", "查一下这个代币"

## Required Skills

okx-dex-token, okx-security, okx-dex-swap, okx-dex-signal, okx-dex-trenches

## Input

| Param         | Required | Default     |
|---------------|----------|-------------|
| token_address | Yes      | —           |
| chain         | No       | Auto-detect |

## Steps

### Step 1 — Core data [required] (parallel)

Prefer composite command if available:

```
onchainos token report --address <addr> --chain <chain>
```

Fallback — run all 4 in parallel:

```
onchainos token info --address <addr> --chain <chain>
onchainos token price-info --address <addr> --chain <chain>
onchainos token advanced-info --address <addr> --chain <chain>
onchainos security token-scan --tokens "<chainIndex>:<addr>"
```

> Token liquidity comes from `price-info.liquidity`. `security token-scan` returns boolean flags only; combine with `advanced-info.tokenTags` for tax info.

Present: name, symbol, age (from `advanced-info.createTime`), price, mcap, 24h vol, 24h change, honeypot, buy/sell tax flags, mint/freeze authority, liquidity, LP burned %

### Step 2 — On-chain structure [recommended] (parallel)

```
onchainos token holders --address <addr> --chain <chain>
onchainos token cluster-overview --address <addr> --chain <chain>
onchainos token top-trader --address <addr> --chain <chain>
onchainos signal list --chain <chain> --token-address <addr>
```

> `token holders` returns Top 100 (no `--limit`). `cluster-overview` may 500 for brand-new tokens — skip gracefully if unavailable.

Present: holder count, Top 10 holding %, tag distribution (SM / Whale / Insider), linked cluster groups + supply %, top trader PnL breakdown (profitable / losing / holding / exited), SM signal wallet count

### Step 3 — Launchpad supplement [recommended] (conditional: `contract.protocolId` from Step 1 is non-empty)

```
onchainos memepump token-details --address <addr> --chain <chain>
onchainos memepump token-dev-info --address <addr> --chain <chain>
onchainos memepump token-bundle-info --address <addr> --chain <chain>
onchainos memepump similar-tokens --address <addr> --chain <chain>
```

> Skip entirely when `protocolId` is empty (token is not from a launchpad).

Present: bonding curve progress, dev tokens created, dev rug count, dev holding %, bundle rate, dev's other projects

## Output Template

```
TOKEN: {symbol} ({chain})
Address: {addr}  |  Age: {n}d

--- PRICE & MARKET ---
Price: ${x}  |  MCap: ${x}  |  24h Vol: ${x}
1h: {x}%  |  4h: {x}%  |  24h: {x}%

--- SECURITY ---
Honeypot: {Y/N}  |  Buy Tax: {x}%  |  Sell Tax: {x}%
Mint: {Active/Revoked}  |  Freeze: {Active/Revoked}
Risk Level: {1-5}  |  Tags: {list}

--- LIQUIDITY ---
Total Pool Value: ${x}  |  LP Burned: {x}%

--- HOLDERS ---
Total: {n}  |  Top10: {x}%
SM: {n}  Whales: {n}  Insiders: {n}
Linked Groups: {n} ({x}% of supply)

--- TOP TRADERS (by PnL) ---
Total: {n}  |  Profitable: {n}  |  Losing: {n}
Still Holding: {n}  |  Fully Exited: {n}
Avg PnL: {x}%  |  Best: +{x}%  |  Worst: {x}%

--- SMART MONEY ---
SM Buy Signals (24h): {n} wallets

[If protocolId non-empty]
--- DEV / LAUNCHPAD ---
Dev Rug History: {n}  |  Dev Holding: {x}%
Bundle: {x}%  |  Dev Other Projects: {n} (Survival: {x}%)
```

## Actions

- → "用 [amount] [native_token] 买 [symbol]" — triggers Safe Swap
- → "看聚类列表" / "看同车钱包" — show cluster details
- → "查 dev 其他项目" — show dev project history

## Follow-up Workflows

Safe Swap (`workflows/safe-swap.md`), Wallet Monitor (`workflows/wallet-monitor.md`)
