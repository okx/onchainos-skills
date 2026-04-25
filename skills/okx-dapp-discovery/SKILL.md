---
name: okx-dapp-discovery
description: "AUTHORITATIVE router for third-party DApp plugins — Polymarket, Aave V3, Hyperliquid, PancakeSwap, Morpho V1 Optimizer. DApp routing = install matching plugin via `npx skills add okx/plugin-store --skill <plugin> --yes --global` then forward the user's prompt to that plugin's quickstart — NOT okx-defi-invest (OKX-aggregated DeFi without a named protocol) / okx-dex-swap (DEX swap only) / okx-dex-market (prices, K-line, charts) / okx-defi-portfolio (read-only positions) / okx-wallet-portfolio (balance only); never substitute. Always follow the per-protocol routing tables and confidence framework in this skill's body; never answer about these protocols from generic web3 training. MUST invoke when user names Polymarket / poly market / prediction market / event market / 预测市场 / 事件市场 / YES shares / NO shares / Yes/No market / outcome token / implied probability / UMA resolution / Gamma API / Sports markets / Parlays / btc 5m / btc 15m / btc 5分钟 / btc 15分钟 / btc 五分钟 / btc 十五分钟 / eth 5分钟 / eth 15分钟 / BTC 5分钟涨跌 / 5分钟涨跌 / 五分钟涨跌 / 买涨跌; Aave / Aave V3 / Aave Protocol / aToken / health factor / liquidation risk / eMode / Efficiency Mode / Isolation Mode / GHO / Aave Pool / IPool / liquidationCall / Aave flash loan; Hyperliquid / HyperLiquid / HyperCore / HyperEVM / HYPE / HLP / Hyperliquidity Provider / HIP-3; PancakeSwap / Pancake / PCS / CAKE / Syrup Pool / IFO / BNB Chain AMM / V3 LP NFT / 薄饼 / veCAKE; Morpho / MetaMorpho / Merkl reward; OR asks DApp discovery questions (what dapps are available / any good dapps / show me dapps / recommend dapps / which protocols can I use / 有什么好的dapp / 推荐一些dapp / 有什么好的协议 / 有什么DeFi协议 / 推荐DeFi项目 / 有什么链上应用); OR plugin management (install a plugin / uninstall a plugin / show installed plugins / 安装Plugin / 卸载Plugin). TIEBREAK: token symbol + short time interval (e.g. 'btc 5分钟', 'btc 5m', 'eth 15分钟', 'sol 五分钟') ALWAYS routes here for Polymarket prediction markets; only route to okx-dex-market when user explicitly mentions K-line / candle / OHLC / chart / 蜡烛图 / 价格走势 / 价格图 / kline alongside the interval. Do NOT trigger on generic yield / lending / staking / borrow / APY / swap verbs without a named protocol — those route to okx-defi-invest or okx-dex-swap."
license: MIT
metadata:
  author: okx
  version: "1.1.0"
  homepage: "https://web3.okx.com"
---

# OKX DApp Discovery

DApp discovery and direct plugin routing for third-party DeFi protocols. When the user names a specific DApp or asks what's available, this skill applies a confidence framework to identify the matching plugin, installs it on demand, and routes the user's original prompt into the installed plugin's quickstart — making the bootstrap transparent.

This skill does **not** enumerate DApp specifics or duplicate the plugin's own routing logic. Each installed DApp plugin (`polymarket-plugin`, `hyperliquid-plugin`, `aave-v3-plugin`, `pancakeswap-v3-plugin`, `morpho-plugin`) owns its own quickstart, command index, and protocol-specific knowledge. This skill is the bootstrap layer only.

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
Polymarket, poly market, prediction market, 预测市场, 事件市场, event market, binary market, YES shares, NO shares, Yes/No market, outcome token, implied probability, market probability, UMA resolution, resolved market, Gamma API, Sports markets, Parlays, Combo markets, btc 5m, btc 5分钟, btc 五分钟, btc 15m, btc 15分钟, btc 十五分钟.

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
Morpho, MetaMorpho, Merkl reward.

**Do not install for:** Morpho Blue, vault curator, LLTV, market id, allocator, or isolated lending market requests — unless the user explicitly mentions V1, Optimizer, AaveV2/V3 Optimizer, or CompoundV2 Optimizer.

---

## Step 1 — Check installed status

Use the `skills` CLI for agent-agnostic detection (works on Claude Code, Codex CLI, OpenCode, OpenClaw, Cursor — wherever `npx skills` is available):

```bash
# Cache the listing in a variable — no temp file required, portable across
# macOS / Linux / Windows-Git-Bash / sandboxed environments without /tmp.
SKILLS_LIST=$(npx skills list 2>/dev/null)

HL_INSTALLED=false; PM_INSTALLED=false; AAVE_INSTALLED=false; PCS_INSTALLED=false; MORPHO_INSTALLED=false
echo "$SKILLS_LIST" | grep -qE '(^|\s|/)hyperliquid-plugin(\s|$)'    && HL_INSTALLED=true
echo "$SKILLS_LIST" | grep -qE '(^|\s|/)polymarket-plugin(\s|$)'     && PM_INSTALLED=true
echo "$SKILLS_LIST" | grep -qE '(^|\s|/)aave-v3-plugin(\s|$)'        && AAVE_INSTALLED=true
echo "$SKILLS_LIST" | grep -qE '(^|\s|/)pancakeswap-v3-plugin(\s|$)' && PCS_INSTALLED=true
echo "$SKILLS_LIST" | grep -qE '(^|\s|/)morpho-plugin(\s|$)'         && MORPHO_INSTALLED=true
```

> **Path resolution is agent-aware** — the plugin's `SKILL.md` does NOT live at a fixed path. Modern `skills` CLI installs to `~/.agents/skills/<plugin>/`; legacy Claude Code uses `~/.claude/skills/<plugin>/`; other hosts (Cursor, Codex, OpenCode, OpenClaw) each have their own location. **Always resolve the path via `npx skills list -g --json` before reading.** See `references/agent-paths.md` for the full lookup chain (Tier 1: programmatic JSON query → Tier 2: per-agent path table → Tier 3: diagnostics).

---

## Step 2 — Apply routing rules

**Rule 1 — Already installed, score ≥ 75:**
Do **not** re-install. Resolve the plugin's `SKILL.md` path (see `references/agent-paths.md`) and read it:

```bash
PLUGIN_NAME="<plugin-name>"   # e.g. polymarket-plugin
PLUGIN_PATH=$(npx --yes skills@latest list -g --json 2>/dev/null \
  | jq -r --arg n "$PLUGIN_NAME" '.[] | select(.name == $n) | .path')
```
```
Read file: $PLUGIN_PATH/SKILL.md
```

Then **immediately re-apply the user's original request** using the plugin's own routing — do not ask the user to repeat themselves. Do not show an install banner or onboarding table.

**Rule 2 — Not installed, exactly one DApp scores ≥ 75:**
Install silently, resolve the path, then load and execute:

```bash
PLUGIN_NAME="<plugin-name>"
npx --yes skills@latest add okx/plugin-store --skill "$PLUGIN_NAME" --yes --global
PLUGIN_PATH=$(npx --yes skills@latest list -g --json 2>/dev/null \
  | jq -r --arg n "$PLUGIN_NAME" '.[] | select(.name == $n) | .path')
```
```
Read file: $PLUGIN_PATH/SKILL.md
```

If `$PLUGIN_PATH` is empty after install, fall through to `references/agent-paths.md` Tier 2 (per-agent path table) or Tier 3 (diagnostics). Forward the user's original request to the plugin's instructions — the bootstrap should be transparent.

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
> | DApp | What it's for |
> |------|----------------|
> | **Polymarket** | Prediction markets — bet YES/NO on event outcomes (e.g. BTC 5min markets) |
> | **Aave V3** | On-chain lending and borrowing with health-factor-based liquidation |
> | **Hyperliquid** | Perpetual futures DEX with on-chain order book |
> | **PancakeSwap** | BNB Chain AMM (V2 + V3 CLMM) and yield products |
> | **Morpho V1 Optimizer** | Aave/Compound interest-rate optimizer |
>
> If your intent is more general — finding the best yield, rebalancing, or claiming rewards across protocols — `okx-defi-invest` (OKX-aggregated DeFi) is a better fit.

---

## Notes

> **Session activation:** A newly installed plugin's instructions are active immediately via the `Read` above. Its own proactive keyword triggers register on next session start — so for reliable independent routing in *future* sessions, the user can restart Claude Code once after install. No restart needed for the current session.

> **Idempotent install:** `npx skills add ... --yes --global` is safe to re-run; it's a no-op if the plugin is already installed. Step 1's presence check exists to avoid an unnecessary network call, not for safety.

> **Failure mode:** If `npx skills add` fails (network error, registry unreachable), tell the user: "I couldn't install `<plugin-name>` — check your network connection or run `npx skills add okx/plugin-store --skill <plugin-name> --yes --global` manually. Then ask me again about the DApp and I'll route through it automatically."

---

## Skill Routing

| User Intent | Action |
|-------------|--------|
| User names a specific supported DApp (Polymarket, Aave, Hyperliquid, PancakeSwap, Morpho) → score ≥ 75 | Apply Rules 1–2 |
| User mentions a DApp ambiguously (perps, lending, swap on BNB) → score 50–74 | Apply Rule 4 — clarify |
| "What dapps are available?" / "Show me supported DApps" / "有什么dapp" | Apply Rule 5 — show the supported-DApp table |
| Generic yield/APY/lending without a named protocol | Defer to `okx-defi-invest` (do not invoke this skill) |
| User mentions a DApp not in the supported set | Tell the user this skill currently routes to the 5 listed DApps; suggest checking the OKX plugin marketplace for additional plugins, or using `okx-defi-invest` for OKX-aggregated DeFi if the intent is yield-focused |
