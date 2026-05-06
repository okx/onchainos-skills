# Token Research

> Pull price, contract, security, holders, top traders, and smart money signals for any token in one flow.

## Keyword Glossary

> If the user's query contains Chinese text, read `references/keyword-glossary.md` for trigger mappings.

## Triggers

"analyze token", "research [address]", "is this token safe", "what is this token", "token deep dive"

## Required Skills

okx-dex-token, okx-security, okx-dex-signal, okx-dex-trenches, okx-dex-social

## Input

| Param         | Required | Default     |
|---------------|----------|-------------|
| token_address | One of address or query required | — |
| query         | One of address or query required | — |
| chain         | No       | Auto-detect |

## CLI

Run with a contract address:

```
onchainos workflow token-research --address <addr> [--chain <chain>]
```

Run with a token symbol or name (returns top 5 matches for selection):

```
onchainos workflow token-research --query <symbol> [--chain <chain>]
```

When `--query` is used, the CLI returns up to 5 search results with index, symbol, name, address, chain, price, and market cap. The agent should present these to the user, let them pick one, then re-invoke with `--address` using the selected token's address.

## Steps

### Step 0 — Token resolution [conditional: --query provided]

If the user provided a symbol/name instead of a contract address:

```
onchainos workflow token-research --query <symbol> --chain <chain>
```

Present the top 5 results to the user in a numbered list. Once the user selects a token, continue from Step 1 with the selected address.

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

> `token holders` defaults to 20 results; pass `--limit 100` for top 100. `cluster-overview` may 500 for brand-new tokens — skip gracefully if unavailable.

Present: holder count, Top 10 holding %, tag distribution (SM / Whale / Insider), linked cluster groups + supply %, top trader PnL breakdown (profitable / losing / holding / exited), SM signal wallet count

### Step 3 — Social signals [recommended] (parallel)

```
onchainos social token-vibe-timeline --chain <chain> --token-address <addr> --period 1
onchainos social token-top-kols --chain <chain> --token-address <addr> --sort-by 1 --period 1 --limit 10
```

> Vibe is keyed by chain + contract address (already known from Step 1). Skip gracefully if either returns empty (cold token / no KOL chatter yet).
>
> If the token is a major-cap coin with a well-known symbol (BTC / ETH / SOL / etc.), also pull symbol-level news + sentiment in parallel:
>
> ```
> onchainos social coin-sentiment --coins <symbol> --period 1
> onchainos social news-by-coin --coins <symbol> --sort-by 2 --limit 5
> ```

Present: vibe score + 24h change rate, top 10 KOLs by engagement (with first-mention links if present), bullish/bearish ratio, top 5 hot headlines (when symbol-level data is pulled)

### Step 4 — Launchpad supplement [recommended] (conditional: `contract.protocolId` from Step 1 is non-empty)

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

--- SOCIAL ---
Vibe (24h): {x}/100 ({+/-x}% vs prev)
Mentions: {n} ({+/-x}%)  |  Engagement: {n} ({+/-x}%)
Top KOLs: @{handle1} ({n}k followers), @{handle2} ({n}k), ...
[If symbol-level data pulled]
Sentiment: {bull%}/{bear%}  |  Recent headlines: {n}

[If protocolId non-empty]
--- DEV / LAUNCHPAD ---
Dev Rug History: {n}  |  Dev Holding: {x}%
Bundle: {x}%  |  Dev Other Projects: {n} (Survival: {x}%)
```

## Actions

- → "show cluster list" / "show co-invested wallets" — show cluster details
- → "show dev projects" — show dev project history
- → "watch this token" — Wallet Monitor (`wallet-monitor.md`)
