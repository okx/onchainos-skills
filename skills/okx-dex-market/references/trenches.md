# Capability: Trenches

7 commands for meme token discovery, developer analysis, bundle detection, and co-investor tracking.

## Step 0 — Read vs Write Re-Route (run before every other step)

This capability is **READ-ONLY research**. Before running any `onchainos memepump` command, re-classify the user's intent as read or write:

- **WRITE intent → STOP and invoke `okx-dapp-discovery`** (which installs `pump-fun-plugin`):
  - English action verbs: `buy`, `sell`, `swap`, `snipe`, `ape`, `purchase`, `trade` + a pump.fun token / address
  - Apply direct translations like their English equivalents; normalize non-literal Chinese write slang through the Trenches keyword glossary.
  - Examples that MUST re-route: "snipe this pump.fun token 0xabc", "buy this pump.fun token", "buy the hottest pump.fun token".
  - **Snipe disambiguation**: a bare "snipe + token/address" request is a write operation and must re-route. It remains here only when paired with an analytical noun such as "bundle/sniper detection" or "who sniped this token".

- **READ intent → stay in this capability** (default for all `memepump` commands):
  - Developer reputation, launch history, or rug history
  - Bundle/sniper detection or analysis of who sniped a token
  - Bonding-curve progress, similar tokens by the same developer, or co-investor wallets
  - Token launch scans through `memepump tokens`

If you have already started running commands and only then realise the user's intent is a write op, halt mid-flow and invoke `okx-dapp-discovery` — do not run any `swap`/`execute` from inside this capability.

## Commands

| # | Command | Use When |
|---|---|---|
| 1 | `onchainos memepump chains` | Discover supported chains and protocols |
| 2 | `onchainos memepump tokens --chain <chain> [--stage <stage>]` | Browse/filter meme tokens by stage (default: NEW) |
| 3 | `onchainos memepump token-details --address <address>` | Deep-dive into a specific meme token |
| 4 | `onchainos memepump token-dev-info --address <address>` | Developer reputation and holding info |
| 5 | `onchainos memepump similar-tokens --address <address>` | Find similar tokens by same creator |
| 6 | `onchainos memepump token-bundle-info --address <address>` | Bundle/sniper analysis |
| 7 | `onchainos memepump aped-wallet --address <address>` | Aped/co-investor wallet list |

### Step 1: Collect Parameters

- Missing chain → default to Solana (`--chain solana`); verify support with `onchainos memepump chains` first
- Missing `--stage` for memepump-tokens → default to `NEW`; only ask if the user's intent clearly points to a different stage
- Stage coverage: `NEW` and `MIGRATING` include tokens created within the last **24 h**; `MIGRATED` includes tokens whose migration completed within the last **3 days**
- User mentions a protocol name → first call `onchainos memepump chains` to get the protocol ID, then pass `--protocol-id-list <id>` to `memepump-tokens`. Do NOT use the **Token** capability to search for protocol names as tokens.

### Step 2: Call and Display

- Translate field names as specified below — never dump raw JSON keys
- For `memepump-token-dev-info`, present as a developer reputation report
- For `memepump-token-details`, present as a token safety summary highlighting red/green flags
- When listing tokens from `memepump-tokens`, never merge or deduplicate entries that share the same symbol. Different tokens can have identical symbols but different contract addresses — each is a distinct token and must be shown separately. Always include the contract address to distinguish them.
- Translate field names: `top10HoldingsPercent` → "top-10 holder concentration", `rugPullCount` → "rug pull count", `bondingPercent` → "bonding curve progress"

## Data Freshness

Render `requestTime` (Unix ms) as-is — the upstream snapshot time. Do NOT chain off a previous response's `requestTime`.
