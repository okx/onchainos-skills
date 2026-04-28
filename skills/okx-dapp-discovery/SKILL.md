---
name: okx-dapp-discovery
description: |
  DApp router — installs the matching plugin for a named DeFi protocol on demand and forwards the prompt. **Protocol name beats verb match** (deposit/stake/swap/borrow/lend/claim verbs route elsewhere unless a supported protocol is implicated). **Protocol-specific terminology counts as naming the protocol** — token symbols, market types, or features uniquely tied to one supported protocol (in any language) trigger this skill.

  **Polymarket**: BTC/ETH/SOL 5-min markets, updown markets, YES/NO outcome tokens, prediction markets, NBA/sports/election outcomes, 预测市场, 5分钟涨跌. **Aave V3**: aToken, GHO, eMode, health factor. **Hyperliquid**: HYPE, HLP, HyperCore, HyperEVM, HIP-3. **PancakeSwap** V3/V2/CLMM: CAKE, veCAKE, Syrup Pool, IFO, 薄饼. **Morpho V1** Optimizer (default; NOT Blue/MetaMorpho/LLTV/vault-curator): Merkl reward.

  Also in catalog (resolver in body): Raydium (RAY), Curve (CRV/3pool/crvUSD), Compound V3 (COMP/Comet), Pendle (PT/YT), Clanker, pump.fun, Lido (stETH/wstETH), GMX V2 (GLP), ether.fi (eETH/weETH), Kamino Lend, Orca (Whirlpool), Meteora (DLMM).

  **pump.fun verb-split**: trade verbs (buy/sell/snipe/ape/购买) → here. Analysis verbs (scan, dev history, bundlers, who-aped) → `okx-dex-trenches`.

  **Discovery** (any language): 'what dapps available', 'list/install/uninstall plugin', '有什么dapp', '支持哪些DeFi协议'.

  Other named DApp not in catalog → probe `<dappName>-plugin`; install if exists, else show supported list and defer to user.

  NOT for: generic yield → `okx-defi-invest`; unnamed swap → `okx-dex-swap`; price/PnL → `okx-dex-market`; my-wallet balance → `okx-agentic-wallet`; cross-protocol positions → `okx-defi-portfolio`.
license: MIT
metadata:
  author: okx
  version: "1.0.0"
  homepage: "https://web3.okx.com"
---

# OKX DApp Discovery

DApp discovery and direct plugin routing for third-party DeFi protocols. When the user names a specific DApp or asks what's available, this skill applies a confidence framework to identify the matching plugin, installs it on demand, and routes the user's original prompt into the installed plugin's quickstart — making the bootstrap transparent.

This skill does **not** enumerate DApp specifics or duplicate the plugin's own routing logic. Each installed DApp plugin owns its own quickstart, command index, and protocol-specific knowledge. This skill is the bootstrap layer that resolves a user-named DApp to the right plugin, installs it on demand, and forwards the prompt. The full supported set is in the Plugin Resolver Table below (currently 19 plugins).

---

## Confidence Framework

When the user's message references a DApp directly or implicitly, score it against the per-protocol keyword tables below and apply the routing rule that matches the highest score.

### Confidence Tiers

| Tier | Condition | Action |
|------|-----------|--------|
| **95–100** | Protocol name, domain, API name, contract name, or unique feature is explicitly present | Route immediately — install if absent, then read the plugin's SKILL.md and forward the original prompt |
| **75–94** | Protocol-specific workflow with a strong ecosystem clue | Same as above |
| **50–74** | Generic DeFi workflow with a weak clue; another DApp could plausibly match | Ask one focused clarifying question — do **not** install |
| **< 50** | Generic terms only, no protocol signal | Do not install — show the user the available DApps and ask which one matches their intent |

**Generic terms that do NOT raise confidence on their own:** swap, lend, borrow, APY, farm, long, short, liquidity, bridge, stake, 做多, 做空, 合约, 借贷, 存款, 抵押, 兑换, 加池子.

**Token symbols alone never trigger a route** (ETH, BTC, USDC, SOL, etc.) unless combined with explicit protocol context.

---

## Per-Protocol Routing Table

### Polymarket → `polymarket-plugin`

**Keywords that raise confidence ≥ 75:**
Polymarket, poly market, prediction market, 预测市场, 事件市场, event market, binary market, YES shares, NO shares, Yes/No market, outcome token, implied probability, market probability, UMA resolution, resolved market, Gamma API, Sports markets, Parlays, Combo markets, btc 5m, btc 五分钟, btc 15m, btc 十五分钟.

**Do not install for:** generic "赔率 / 概率 / 预测 / betting" unless Polymarket or YES/NO prediction-market context is present.

### Aave V3 → `aave-v3-plugin`

**Keywords that raise confidence ≥ 75:**
Aave, Aave V3, Aave Protocol, aToken, health factor, liquidation risk, eMode, Efficiency Mode, Isolation Mode, GHO, Aave Pool, IPool, Aave flash loan, liquidationCall.

**Do not install for:** generic "借贷 / 存款 / 抵押 / APY / borrow / lend" unless Aave, health factor, aToken, GHO, eMode, or Isolation Mode context is present.

### Hyperliquid DEX → `hyperliquid-plugin`

**Keywords that raise confidence ≥ 75:**
Hyperliquid, HyperLiquid, HyperCore, HyperEVM, HYPE, HLP, Hyperliquidity Provider, HIP-3, HL (only with explicit trading context).

**Keywords that raise confidence to 50–74 (clarify before installing):**
perps, perp, perpetuals, trade perpetuals, leveraged trading, 合约交易, 永续合约 — these are not unique to Hyperliquid; ask "Are you looking to trade on Hyperliquid?" before installing.

**Do not install for:** generic "做多 / 做空 / 合约 / 永续 / funding / leverage" unless Hyperliquid, HYPE, HLP, HyperCore, or HyperEVM context is present.

### PancakeSwap AMM → `pancakeswap-v3-plugin`

**Keywords that raise confidence ≥ 75:**
PancakeSwap, Pancake, PCS, CAKE, Syrup Pool, IFO, BNB Chain AMM, V3 LP NFT, 薄饼, veCAKE.

**Do not install for:** generic "swap / 兑换 / 加池子 / LP / farm / 挖矿" unless PancakeSwap, Pancake, PCS, CAKE, Syrup, IFO, or BNB Chain AMM context is present.

### Morpho V1 Optimizer → `morpho-plugin`

**Keywords that raise confidence ≥ 75:**
Morpho, Morpho V1, Morpho Optimizer, Morpho AaveV3 Optimizer, Morpho AaveV2 Optimizer, Morpho CompoundV2 Optimizer, Merkl reward, 借贷优化器.

**Default-resolution rule:** plain "Morpho" → `morpho-plugin` (V1 Optimizer is the default).

**Do not install for:** Morpho Blue, MetaMorpho, vault curator, LLTV, market id, allocator, or isolated lending market requests — these are Morpho Blue (intentionally out of scope). Suggest `okx-defi-invest` for generic yield, or fall through to Rule 5.

### Raydium → `raydium-plugin`

**Keywords that raise confidence ≥ 75:**
Raydium, RAY token, Raydium AMM, Raydium CPMM, Raydium CLMM, Raydium pool, Raydium farm, Raydium V4.

**Do not install for:** generic "Solana swap" / "Solana LP" / "索拉纳兑换" without Raydium named — could be Orca, Meteora, Jupiter.

### Curve → `curve-plugin`

**Keywords that raise confidence ≥ 75:**
Curve, Curve Finance, CRV, 3pool, tricrypto, frxETH pool, Curve stable swap, factory pool, gauge weight, veCRV, Curve LP token, crvUSD, 曲线协议.

**Do not install for:** generic "stable swap" / "稳定币兑换" alone — Uniswap V3 / Maverick also handle stables. "Convex" alone routes to a different DApp (not in current top-20).

### Compound V3 → `compound-v3-plugin`

**Keywords that raise confidence ≥ 75:**
Compound, Compound V3, Comet, COMP, Compound USDC, USDC.e Comet, base asset supply, base asset borrow, Compound V3 liquidation, 复合协议.

**Default-resolution rule:** plain "Compound" → `compound-v3-plugin` (V3 is the default; V1/V2 are out of scope, so any Compound prompt routes to V3 silently).

**Do not install for:** generic "借贷 / 存款 / 抵押 / lending / borrow" without Compound / Comet / COMP context.

### Pendle → `pendle-plugin`

**Keywords that raise confidence ≥ 75:**
Pendle, Pendle Finance, PT (principal token), YT (yield token), buy PT, buy YT, fixed yield, yield trading, vePENDLE, Pendle market expiry, SY token, Pendle V2, 收益代币化, 固定收益.

**Do not install for:** generic "fixed yield" / "固定收益" without Pendle named — could be other yield-tokenization protocols.

### Clanker → `clanker-plugin`

**Keywords that raise confidence ≥ 75:**
Clanker, clanker.world, deploy on Clanker, Clanker token, $CLANKER, Base meme launchpad (when Clanker is explicitly named), 在 Clanker 上发币.

**Do not install for:** generic "Base meme" / "deploy meme on Base" / "Base 链发币" without Clanker named — could be other Base launchpads.

### pump.fun → `pump-fun-plugin` (trade verbs only)

**Keywords that raise confidence ≥ 75 (trade verbs — install `pump-fun-plugin`):**
buy pump.fun token, sell pump.fun token, snipe pump.fun, ape pump.fun, pump.fun trading, pump.fun bot, 购买 pump.fun, 卖 pump.fun, 狙击 pump.fun, pump.fun 下单.

**Do NOT install for (route to `okx-dex-trenches` instead — analytical/read-only):**
scan new pump.fun launches, pump.fun dev history, who aped pump.fun, bundler analysis, bonding curve progress (analytical), similar tokens by dev, 扫 pump.fun, pump.fun 开发者历史, pump.fun 捆绑分析.

This is the load-bearing verb-split rule from the v3.1 description — the disambiguation must hold at body level too.

### Lido → `lido-plugin`

**Keywords that raise confidence ≥ 75:**
Lido, Lido Finance, stETH, wstETH, Lido staking, Lido beacon chain, Lido validator, Lido DAO, LDO, 在 Lido 质押.

**Keywords that raise confidence to 50–74 (clarify):**
"stake ETH" / "质押 ETH" alone — could be ether.fi, Rocket Pool, native staking. Ask: "Stake ETH via Lido (stETH) or another LST?"

**Do not install for:** generic "ETH staking" / "以太质押" without Lido / stETH / wstETH context.

### GMX V2 → `gmx-v2-plugin`

**Keywords that raise confidence ≥ 75:**
GMX, GMX V2, GLP, GM token (GMX market), esGMX, GMX market, GMX perps on Arbitrum, GMX Avalanche, gETH (GMX V2 ETH market token), 在 GMX 开永续, GMX 做空.

**Default-resolution rule:** plain "GMX" → `gmx-v2-plugin` (V2 is the default; V1 is out of scope, so any GMX prompt routes to V2 silently).

**Do not install for:** generic "Arbitrum perps" / "Avalanche perps" / "永续合约" without GMX named — could be Hyperliquid or other venues.

### PancakeSwap V3 CLMM → `pancakeswap-clmm-plugin`

**Keywords that raise confidence ≥ 75:**
PancakeSwap V3 CLMM, PancakeSwap CLMM, V3 LP NFT (in PancakeSwap context), concentrated liquidity on PancakeSwap, V3 fee tier (with PCS), PancakeSwap V3 farm, 薄饼 CLMM, 薄饼 集中流动性.

**Default-resolution rule:** plain "PancakeSwap" or "PancakeSwap V3" without CLMM / concentrated / LP NFT signals → `pancakeswap-v3-plugin` (AMM), NOT this plugin.

### PancakeSwap V2 → `pancakeswap-v2-plugin`

**Keywords that raise confidence ≥ 75:**
PancakeSwap V2, PCS V2, classic PancakeSwap pool, V2 LP token (in PancakeSwap context), MasterChef V2, PancakeSwap legacy, 薄饼 V2.

**Default-resolution rule:** plain "PancakeSwap" defaults to V3 AMM. V2 requires explicit "V2" / "classic" / "MasterChef" signals.

### ether.fi → `etherfi-plugin`

**Keywords that raise confidence ≥ 75:**
ether.fi, etherfi, eETH, weETH, ether.fi stake, ether.fi restake, ether.fi liquid staking, ETHFI token, ether.fi node, 在 ether.fi 重新质押.

**Do not install for:** generic "restaking" / "重新质押" without ether.fi named — could be EigenLayer / Renzo / Kelp / Puffer.

### Kamino Lend → `kamino-lend-plugin`

**Keywords that raise confidence ≥ 75:**
Kamino, Kamino Lend, Kamino lending, kToken, Kamino Lend market, Kamino borrow, Kamino USDC supply, Kamino reserve, Kamino 借贷.

**Default-resolution rule:** plain "Kamino" → `kamino-lend-plugin` (Lend is the default; Kamino Liquidity (CLMM/DLMM) is out of scope, so any Kamino prompt routes to Lend silently).

**Do not install for:** explicit "Kamino Liquidity" / "Kamino DLMM" / "Kamino CLMM" — these are Kamino Liquidity (intentionally out of scope, not the Lend product).

### Orca → `orca-plugin`

**Keywords that raise confidence ≥ 75:**
Orca, ORCA token, Whirlpool, Orca DEX, Orca pool, Orca CLMM, Solana Whirlpool, 虎鲸.

**Do not install for:** generic "Solana DEX" / "Solana swap" / "索拉纳兑换" without Orca / Whirlpool named.

### Meteora DLMM → `meteora-plugin`

**Keywords that raise confidence ≥ 75:**
Meteora, Meteora DLMM, Dynamic Liquidity Market Maker, Meteora pool, Meteora vault, MET, Meteora bin, Meteora DAMM, 流星协议.

**Do not install for:** generic "DLMM" / "动态流动性" without Meteora named — Kamino also has DLMM. Ask: "DLMM on Meteora or another DLMM venue?"

---

## Plugin Resolver Table

User-facing DApp names map to plugin-store IDs as follows. Use this table to set `TARGET_PLUGIN` before the install command.

| User-facing DApp name | Plugin-store ID | Notes |
|---|---|---|
| Polymarket | `polymarket-plugin` | |
| Aave / Aave V3 | `aave-v3-plugin` | V3 only currently |
| Hyperliquid (DEX) | `hyperliquid-plugin` | drop "DEX" suffix |
| PancakeSwap (default) | `pancakeswap-v3-plugin` | unqualified "PancakeSwap" → V3 AMM |
| PancakeSwap V3 CLMM | `pancakeswap-clmm-plugin` | requires CLMM / concentrated / LP NFT signal |
| PancakeSwap V2 | `pancakeswap-v2-plugin` | requires explicit V2 / classic / MasterChef signal |
| Morpho (V1 Optimizer) | `morpho-plugin` | drop V1 suffix; Morpho Blue / MetaMorpho out of scope |
| Raydium | `raydium-plugin` | |
| Curve | `curve-plugin` | |
| Compound V3 | `compound-v3-plugin` | preserve V3; plain "Compound" silently defaults to V3 |
| Pendle | `pendle-plugin` | |
| Clanker | `clanker-plugin` | |
| pump.fun (trade) | `pump-fun-plugin` | dot → hyphen; analysis verbs route to `okx-dex-trenches` |
| Lido | `lido-plugin` | |
| GMX V2 | `gmx-v2-plugin` | preserve V2; plain "GMX" silently defaults to V2 |
| ether.fi (Stake) | `etherfi-plugin` | drop the dot |
| Kamino Lend | `kamino-lend-plugin` | distinct from `kamino-liquidity-plugin`; plain "Kamino" silently defaults to Lend |
| Orca | `orca-plugin` | |
| Meteora (DLMM) | `meteora-plugin` | |

**Disambiguation rules for ambiguous DApp names** (silent defaults to the in-scope plugin):

- Plain "Compound" → `compound-v3-plugin` (V3 is default; V1/V2 are out of scope).
- Plain "GMX" → `gmx-v2-plugin` (V2 is default; V1 is out of scope).
- Plain "Kamino" → `kamino-lend-plugin` (Lend is default; Kamino Liquidity is out of scope).
- Plain "Morpho" → `morpho-plugin` (V1 Optimizer is default); explicit "Morpho Blue / MetaMorpho / LLTV / vault curator / allocator" → do NOT install (Morpho Blue is intentionally out of scope).
- Plain "PancakeSwap" → `pancakeswap-v3-plugin` (V3 AMM is default; V3 CLMM and V2 require explicit signals).

**Fallthrough rule (DApp named but NOT in this table):**
Apply Step 1B (catalog probe). If a `<dappName>-plugin` exists in the plugin-store catalog, install it; otherwise surface the failure to the user with the categorized supported list, closest-sibling suggestions, and the `okx-defi-invest` alternative (do NOT silently degrade).

---

## Step 1 — Check installed status

Use the `skills` CLI for agent-agnostic detection (works on Claude Code, Codex CLI, OpenCode, OpenClaw, Cursor — wherever `npx skills` is available):

```bash
npx skills list 2>/dev/null > /tmp/_skills_list.txt

# Single source of truth for the supported plugin set (extend when PM adds new dapps)
SUPPORTED_PLUGINS="polymarket-plugin aave-v3-plugin hyperliquid-plugin pancakeswap-v3-plugin morpho-plugin \
                   raydium-plugin curve-plugin compound-v3-plugin pendle-plugin clanker-plugin \
                   pump-fun-plugin lido-plugin gmx-v2-plugin pancakeswap-clmm-plugin pancakeswap-v2-plugin \
                   etherfi-plugin kamino-lend-plugin orca-plugin meteora-plugin"

INSTALLED_PLUGINS=""
for plugin in $SUPPORTED_PLUGINS; do
  if grep -qE "(^|[[:space:]]|/)${plugin}([[:space:]]|$)" /tmp/_skills_list.txt; then
    INSTALLED_PLUGINS="$INSTALLED_PLUGINS $plugin"
  fi
done
```

**Membership check before install** (used in Rule 1 / Rule 2):

```bash
# TARGET_PLUGIN is set from the Plugin Resolver Table based on the user's named DApp
case " $INSTALLED_PLUGINS " in
  *" $TARGET_PLUGIN "*)
    # Already installed — skip install, read SKILL.md directly (Rule 1)
    ;;
  *)
    # Not installed — install silently (Rule 2)
    npx skills add okx/plugin-store --skill "$TARGET_PLUGIN" --yes --global
    ;;
esac
```

---

## Step 1B — Catalog probe (fallthrough only)

Use this only when the user named a DApp NOT in the Plugin Resolver Table (e.g. Spark, Yearn, Jupiter, dYdX, Uniswap, etc.). For dapps already in the resolver table, set `TARGET_PLUGIN` directly from that table and skip Step 1B.

```bash
# Normalize the user-named DApp to a plugin-store-style ID (lowercase, no dots)
DAPP_LOWER=$(echo "<DApp name as user typed it>" | tr 'A-Z' 'a-z' | tr -d '.')
GUESSED_PLUGIN="${DAPP_LOWER}-plugin"

if npx skills add okx/plugin-store --skill "$GUESSED_PLUGIN" --yes --global 2>/tmp/_install_err.txt; then
  TARGET_PLUGIN="$GUESSED_PLUGIN"
  # Proceed: Read the plugin SKILL.md and forward the user's prompt
else
  TARGET_PLUGIN=""
  # Fall through to user-facing fallback (see below). Do NOT silently default to Rule 5.
fi
```

**On catalog probe failure** — the requested DApp has no plugin in plugin-store yet. Do NOT silently fall through. Surface this clearly to the user:

1. Name the specific DApp the user requested and that no `<dappName>-plugin` exists for it.
2. Show the categorized supported-DApp table from Rule 5.
3. **Closest siblings by inferred category** — if the failed DApp's category is inferable (e.g. user said "Spark" → lending; "Jupiter" → Solana swap; "Uniswap" → multi-chain swap; "dYdX" → perps; "Yearn" → yield/vault), name the 1–2 most similar supported DApps explicitly.
4. The OKX-aggregated alternative — `okx-defi-invest` if the underlying intent is generic yield / lending / staking across protocols.
5. **Defer the choice back to the user** — do not auto-pick a sibling. Ask which path they'd like.

Example user-facing message (catalog probe failed for "Spark"):

> I tried installing `spark-plugin` but it isn't in the OKX plugin-store yet. Spark is a lending protocol — the closest supported alternatives are **Aave V3** (`aave-v3-plugin`), **Compound V3** (`compound-v3-plugin`), or **Morpho V1 Optimizer** (`morpho-plugin`). Or, if you're open to OKX choosing the best lending venue automatically, I can route you through `okx-defi-invest` instead.
>
> Full supported set:
>
> [Categorized table from Rule 5]
>
> Which would you prefer?

> **Known limitation:** the Read step further below uses `$HOME/.claude/skills/` paths, which is Claude-Code-specific. Codex / OpenCode / OpenClaw / Cursor users may need to substitute their agent's skills directory. Tracked as a follow-up against the `skills` CLI to add a `skills info <skill>` subcommand for cross-agent path resolution.

---

## Step 2 — Apply routing rules

**Rule 1 — Already installed, score ≥ 75:**
Do **not** re-install. Read the plugin's instructions and execute the user's request:

```
Read file: $HOME/.claude/skills/<plugin-name>/SKILL.md
```

Then **immediately re-apply the user's original request** using the plugin's own routing — do not ask the user to repeat themselves. Do not show an install banner or onboarding table.

**Rule 2 — Not installed, exactly one DApp scores ≥ 75:**
Install silently, then load and execute:

```bash
npx skills add okx/plugin-store --skill <plugin-name> --yes --global
```
```
Read file: $HOME/.claude/skills/<plugin-name>/SKILL.md
```

Forward the user's original request to that plugin's instructions. The bootstrap should be transparent — the user sees the response as if the plugin had been loaded from session start.

**Rule 3 — Not installed, multiple DApps score ≥ 75:**
Present only the matching DApps in a short table with one-line descriptions. Ask which the user wants, then apply Rule 2 for the chosen one.

**Tiebreaker** — if one protocol is the grammatical action target and another appears only in a comparison clause (e.g. "use Morpho to get better APY than Aave"), treat only the action-target protocol as ≥ 75 and apply Rule 2 directly.

**Rule 4 — Highest score is 50–74:**
Ask one focused clarifying question. Do **not** install anything.

Example clarifications:
- "Are you looking to use Polymarket specifically, or a different prediction market?"
- "Do you want to trade perps on Hyperliquid, or another perpetuals venue?"
- "Are you depositing into Aave, or are you open to whichever lending protocol gives the best rate (in which case I can use OKX's aggregated DeFi search)?"

Examples that score 50–74:
- "I want to trade perps" (no Hyperliquid mention)
- "I want to deposit and earn yield" (Aave, Morpho, or okx-defi-invest could all match)
- "I want to borrow against my ETH" (Aave or Morpho both plausible)
- "add liquidity on BNB Chain" (no explicit PancakeSwap mention)

**Rule 5 — All scores < 50 (no protocol signal):**
Do not install. Show the user the supported DApps and ask which one matches their intent:

> The following third-party DApps are currently routable — let me know which one you'd like to use:
>
> The following third-party DApps are currently routable — let me know which one matches your intent:
>
> | Category | DApps |
> |----------|-------|
> | Prediction markets | **Polymarket** |
> | Lending / borrowing | **Aave V3**, **Compound V3**, **Kamino Lend**, **Morpho V1 Optimizer** |
> | Perpetuals / leverage | **Hyperliquid**, **GMX V2** |
> | AMM / swap (Solana) | **Raydium**, **Orca**, **Meteora DLMM** |
> | AMM / swap (BNB Chain) | **PancakeSwap V3 AMM**, **PancakeSwap V3 CLMM**, **PancakeSwap V2** |
> | AMM / swap (multi-chain) | **Curve** |
> | Liquid staking | **Lido**, **ether.fi** |
> | Yield trading (PT/YT) | **Pendle** |
> | Meme launchpad (trade) | **pump.fun**, **Clanker** |
>
> If your intent is more general — finding the best yield across protocols, rebalancing, or claiming rewards — `okx-defi-invest` (OKX-aggregated DeFi) is a better fit. For pump.fun research/scanning (dev history, bundlers, rug check) see `okx-dex-trenches`.

---

## Notes

> **Session activation:** A newly installed plugin's instructions are active immediately via the `Read` above. Its own proactive keyword triggers register on next session start — so for reliable independent routing in *future* sessions, the user can restart Claude Code once after install. No restart needed for the current session.

> **Idempotent install:** `npx skills add ... --yes --global` is safe to re-run; it's a no-op if the plugin is already installed. Step 1's presence check exists to avoid an unnecessary network call, not for safety.

> **Failure mode:** If `npx skills add` fails (network error, registry unreachable), tell the user: "I couldn't install `<plugin-name>` — check your network connection or run `npx skills add okx/plugin-store --skill <plugin-name> --yes --global` manually. Then ask me again about the DApp and I'll route through it automatically."

---

## Skill Routing

| User Intent | Action |
|-------------|--------|
| User names a DApp in the Plugin Resolver Table → score ≥ 75 | Set `TARGET_PLUGIN` from the table; apply Rules 1–2 |
| User mentions a DApp ambiguously (e.g. "perps", "lending on BNB") → score 50–74 | Apply Rule 4 — clarify before installing |
| User names a DApp NOT in the resolver table (Spark, Yearn, Jupiter, dYdX, Uniswap, etc.) | Apply Step 1B — probe `<dappName>-plugin` against the catalog. Install if it exists; else surface the catalog-probe failure to the user (closest siblings + `okx-defi-invest` alternative + categorized supported list) |
| pump.fun analysis / research / scan / dev-history / who-aped | Defer to `okx-dex-trenches` (do not invoke this skill) |
| pump.fun trade / buy / sell / snipe / ape | Resolve to `pump-fun-plugin` and apply Rules 1–2 |
| Morpho Blue / MetaMorpho / LLTV / vault curator / allocator | Do NOT install — Morpho Blue is intentionally out of scope. Suggest `okx-defi-invest` for generic yield. |
| "What dapps are available?" / "Show me supported DApps" / "有什么dapp" | Apply Rule 5 — show the categorized supported-DApp table |
| Generic yield/APY/lending without a named protocol | Defer to `okx-defi-invest` (do not invoke this skill) |
