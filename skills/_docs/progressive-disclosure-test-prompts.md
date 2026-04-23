# Progressive Disclosure — Test Prompts

Test prompts to verify that skills load content progressively. For each prompt, track which files the agent reads and compare against the expected load pattern.

## How to Test

1. Start a fresh Claude Code session (no prior skill context)
2. Run each prompt
3. Export transcript (`/export-transcript`) or check audit log
4. Verify which files were read against the "Expected" column

---

## Market Skill Tests

| # | Prompt | Expected Files Read | Stage | Validates |
|---|---|---|---|---|
| M1 | "What's the price of ETH?" | SKILL.md, preflight.md, chain-support.md, grep `market price` from cli-reference | 0→1→2→3b | Basic single command, no glossary |
| M2 | "Show me the K-line chart for SOL" | SKILL.md, preflight.md, chain-support.md, grep `market kline` from cli-reference | 0→1→2→3b | Should use wSOL address (edge case in SKILL.md) |
| M3 | "ETH 行情多少钱" | SKILL.md, preflight.md, chain-support.md, keyword-glossary.md, grep `market price` from cli-reference | 0→1→2→3a→3b | Chinese query triggers glossary load |
| M4 | "Show my PnL for this wallet on Solana" | SKILL.md, preflight.md, chain-support.md, grep `portfolio-overview` from cli-reference | 0→1→2→3b | Portfolio command, no glossary |
| M5 | "盈亏分析" | SKILL.md, preflight.md, chain-support.md, keyword-glossary.md | 0→1→2→3a | Chinese-only, glossary maps to portfolio commands |

## Signal Skill Tests

| # | Prompt | Expected Files Read | Stage | Validates |
|---|---|---|---|---|
| S1 | "What are smart money buying on Solana?" | SKILL.md, preflight.md, chain-support.md, grep `tracker activities` from cli-reference | 0→1→2→3b | Routes to tracker (transaction-level), not signal list |
| S2 | "Show me whale buy signals on Ethereum" | SKILL.md, preflight.md, chain-support.md, grep `signal list` from cli-reference | 0→1→2→3b | Routes to signal list (aggregated alerts) |
| S3 | "Top traders on Solana by PnL this week" | SKILL.md, preflight.md, chain-support.md, grep `leaderboard list` from cli-reference | 0→1→2→3b | Leaderboard with inferred --time-frame 3, --sort-by 1 |
| S4 | "聪明钱最新交易" | SKILL.md, preflight.md, chain-support.md, keyword-glossary.md, grep `tracker activities` from cli-reference | 0→1→2→3a→3b | Chinese query, glossary maps to tracker |
| S5 | "牛人榜" | SKILL.md, preflight.md, chain-support.md, keyword-glossary.md, grep `leaderboard list` from cli-reference | 0→1→2→3a→3b | Chinese slang for leaderboard |

## Token Skill Tests

| # | Prompt | Expected Files Read | Stage | Validates |
|---|---|---|---|---|
| T1 | "Search for BONK on Solana" | SKILL.md, preflight.md, chain-support.md, grep `token search` from cli-reference | 0→1→2→3b | Basic search, no glossary |
| T2 | "Show me hot tokens" | SKILL.md, preflight.md, chain-support.md, grep `token hot-tokens` from cli-reference | 0→1→2→3b | Hot tokens with default ranking-type 4 |
| T3 | "Is this token safe? 0x1234..." | Should redirect to okx-security skill | 0→1 (redirect) | IMPORTANT block redirects safety queries |
| T4 | "Show me holder cluster analysis for this token" | SKILL.md, preflight.md, chain-support.md, grep `token cluster-overview` from cli-reference | 0→1→2→3b | Cluster command |
| T5 | "持仓集中度分析" | SKILL.md, preflight.md, chain-support.md, keyword-glossary.md, grep `token cluster-overview` from cli-reference | 0→1→2→3a→3b | Chinese cluster query |
| T6 | "What's the risk level of this token?" | SKILL.md, preflight.md, chain-support.md, grep `token advanced-info` from cli-reference | 0→1→2→3b | Advanced info — display rules now in cli-reference |

## Trenches Skill Tests

| # | Prompt | Expected Files Read | Stage | Validates |
|---|---|---|---|---|
| R1 | "Show me new meme tokens on Solana" | SKILL.md, preflight.md, chain-support.md, grep `memepump tokens` from cli-reference | 0→1→2→3b | Basic meme scan, default stage NEW |
| R2 | "Check if this dev has rugged before" | SKILL.md, preflight.md, chain-support.md, grep `memepump token-dev-info` from cli-reference | 0→1→2→3b | Dev reputation check |
| R3 | "Show me pumpfun tokens" | SKILL.md, preflight.md, chain-support.md, keyword-glossary.md, grep `memepump tokens` from cli-reference | 0→1→2→3a→3b | Protocol name triggers glossary (for protocol ID lookup) |
| R4 | "扫链" | SKILL.md, preflight.md, chain-support.md, keyword-glossary.md | 0→1→2→3a | Chinese meme slang |
| R5 | "打狗 新盘" | SKILL.md, preflight.md, chain-support.md, keyword-glossary.md, grep `memepump tokens` from cli-reference | 0→1→2→3a→3b | Multiple Chinese triggers |
| R6 | "Who aped into this token?" | SKILL.md, preflight.md, chain-support.md, grep `memepump aped-wallet` from cli-reference | 0→1→2→3b | Aped wallet query |

## Cross-Skill Tests (Routing)

These test whether the agent picks the right skill from the description alone.

| # | Prompt | Expected Skill | Validates |
|---|---|---|---|
| X1 | "What's the price of BONK?" | okx-dex-market (not token) | "price" routes to market, not token price-info |
| X2 | "Tell me about BONK — holders, liquidity, risk" | okx-dex-token | Multi-faceted token research |
| X3 | "What are KOLs buying?" | okx-dex-signal | Smart money tracking |
| X4 | "New pump.fun launches" | okx-dex-trenches | Meme launchpad |
| X5 | "Show me trending tokens" | okx-dex-token | Hot tokens ranking |
| X6 | "Check my wallet PnL" | okx-dex-market | Portfolio PnL |
| X7 | "Track this wallet address" | okx-dex-signal | Address tracker |
| X8 | "Is this token a honeypot?" | okx-security (not token) | Safety redirects to security skill |

## Negative Tests (Should NOT Trigger)

| # | Prompt | Should NOT Load | Why |
|---|---|---|---|
| N1 | "What's the price of ETH?" (English) | keyword-glossary.md | English query, no Chinese text |
| N2 | "Show me hot tokens" | ws-protocol.md | Not a WebSocket request |
| N3 | "Search for BONK" | Full cli-reference.md (all 517 lines) | Should grep only the search section |
| N4 | "新盘" (to trenches) | okx-dex-signal SKILL.md | Should not cross-load another skill |

## Pass Criteria

A test passes if:
1. The correct skill is loaded (routing)
2. Only the expected files are read (progressive disclosure)
3. The agent produces the correct CLI command
4. No unnecessary files are loaded (negative tests)
