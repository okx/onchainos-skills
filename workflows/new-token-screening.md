# New Token Screening

> Scan Launchpad new tokens, enrich top results with safety and dev data, surface actionable candidates.

## Keyword Glossary

> If the user's query contains Chinese text, read `references/keyword-glossary.md` for trigger mappings.

## Triggers

"scan new tokens", "new token screening", "pump.fun tokens", "what's new on pump.fun", "meme token scan"

## Required Skills

okx-dex-trenches, okx-security, okx-dex-token

## Input

| Param                  | Required | Default     |
|------------------------|----------|-------------|
| chain                  | No       | Solana      |
| protocol               | No       | All         |
| min_holders            | No       | CLI default |
| min_bonding_percent    | No       | CLI default |
| top10_hold_percent_max | No       | CLI default |

Filter params are passed through to `memepump tokens` CLI. Users may override via natural language (e.g. "only show tokens with 100+ holders").

## CLI

Run the complete workflow in one command:

```
onchainos workflow new-tokens [--chain <chain>] [--stage MIGRATED|MIGRATING]
```

## Steps

### Step 1 — Fetch [required] (sequential)

```
onchainos memepump tokens --chain <chain> --stage MIGRATED
```

> No `--limit` param. MIGRATED stage returns tokens within 72h of migration; MIGRATING within 24h of creation. Tokens outside these windows are not returned. Users may filter with `--min-holders`, `--min-market-cap`, `--max-top10-holdings-percent`.

Present: token list — name, symbol, mcap, holders, volume, SM count, creation time

### Step 2 — Safety + dev enrichment [recommended] (parallel per token, top 10)

For each top token:

```
onchainos security token-scan --tokens "<chainIndex>:<addr>"
onchainos token advanced-info --address <addr> --chain <chain>
onchainos memepump token-dev-info --address <addr> --chain <chain>
onchainos memepump token-bundle-info --address <addr> --chain <chain>
```

Present: per token — honeypot, tax flags, mint/freeze, dev rug count, dev holding %, bundle rate

## Output Template

```
NEW TOKENS — {chain} — {timestamp}

#1  {name} ({symbol})
    MCap: ${x}  |  Holders: {n}  |  SM: {n}  |  Age: {n}h
    Honeypot: {Y/N}  |  Tax: {x}/{x}%  |  Mint: {A/R}  |  Freeze: {A/R}
    Dev Rugs: {n}  |  Dev Holding: {x}%  |  Bundle: {x}%

#2  {name} ({symbol})
    ...
```

## Actions

- → "research [symbol]" — Token Research (`token-research.md`)
- → "show dev projects for [symbol]" — show dev project history
