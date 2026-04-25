---
name: okx-dapp-discovery
description: "Routes to: Polymarket, Aave V3, Hyperliquid, PancakeSwap V3 AMM, Morpho V1 Optimizer. Use when user names a third-party DeFi DApp/protocol as the destination, or asks 'what dapps are available'. The skill applies a confidence framework to detect the matching protocol, installs the corresponding DApp plugin on demand (via `npx skills add okx/plugin-store --skill <plugin-name> --yes --global`), then routes the user's original prompt directly into that plugin's quickstart. Trigger keywords — DApp discovery: 'what dapps are available', 'any good dapps', 'show me dapps', 'recommend dapps', 'which protocols can I use', 'what protocols do you support', 'list installed dapps', 'show installed dapps', 'what DeFi tools are available', 'what plugins do you have', '有什么好的dapp', '推荐一些dapp', '有什么好的协议', '有什么DeFi协议', '推荐DeFi项目', '有什么链上应用', '支持哪些协议', '支持哪些DeFi协议', '装了哪些Plugin', '已安装的dapp'. Specific protocols (Polymarket): 'Polymarket', 'poly market', 'prediction market', 'YES shares', 'NO shares', 'outcome token', 'btc 5m', 'btc 五分钟', 'BTC 5分钟涨跌', '预测市场', '事件市场', '买涨跌', '5分钟涨跌', '五分钟涨跌'. Specific protocols (Aave V3): 'Aave', 'Aave V3', 'aToken', 'health factor', 'eMode', 'Efficiency Mode', 'Isolation Mode', 'GHO', 'Aave flash loan', 'liquidationCall'. Specific protocols (Hyperliquid): 'Hyperliquid', 'HyperLiquid', 'HyperCore', 'HyperEVM', 'HYPE', 'HLP', 'Hyperliquidity Provider', 'HIP-3'. Specific protocols (PancakeSwap): 'PancakeSwap', 'Pancake', 'PCS', 'CAKE', 'Syrup Pool', 'IFO', 'BNB Chain AMM', 'V3 LP NFT', '薄饼', 'veCAKE'. Specific protocols (Morpho V1 Optimizer): 'Morpho', 'Merkl reward'. Plugin management: 'install a plugin', 'uninstall a plugin', 'show installed plugins', '安装Plugin', '卸载Plugin'. Do NOT use for: generic yield/lending/staking verbs without a named DApp (route to okx-defi-invest); DEX swaps without a named DApp (okx-dex-swap); token prices/charts (okx-dex-market); wallet balances (okx-wallet-portfolio); viewing positions (okx-defi-portfolio)."
license: MIT
metadata:
  author: okx
  version: "1.0.0"
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
Morpho, Merkl reward, Morpho V1, AaveV2 Optimizer, AaveV3 Optimizer, CompoundV2 Optimizer.

**Do not install for:** Morpho Blue, MetaMorpho, vault curator, LLTV, market id, allocator, or isolated lending market requests — unless the user explicitly mentions V1, Optimizer, AaveV2/V3 Optimizer, or CompoundV2 Optimizer. (`MetaMorpho` is the Morpho Blue ERC-4626 vault standard, not a V1 Optimizer concept — it does not belong to `morpho-plugin`'s scope.)

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

> **Known limitations:**
> - The Read step further below uses `$HOME/.claude/skills/` paths, which is Claude-Code-specific. Codex / OpenCode / OpenClaw / Cursor users may need to substitute their agent's skills directory. Tracked as a follow-up against the `skills` CLI to add a `skills info <skill>` subcommand for cross-agent path resolution.
> - The `2>/dev/null` redirect on `npx skills list` silences stderr (intentional — avoids noise on agents where `npx` isn't available). If `npx` itself is broken or missing, the listing returns empty and every DApp will be treated as "not installed". The subsequent install path (`npx skills add … --yes --global`) is idempotent and surfaces the underlying error to the user via the Failure-mode note in Step 2 — do not retry the listing in a loop.

---

## Step 2 — Apply routing rules

**Rule 1 — Already installed, score ≥ 75:**
Do **not** re-install. Read the plugin's instructions and execute the user's request:

```
Read file: $HOME/.claude/skills/<plugin-name>/SKILL.md
```

> *(Path is Claude Code-specific — see Known Limitations in Step 1. On Codex / OpenCode / OpenClaw / Cursor, substitute the equivalent skills directory for your agent.)*

Then **immediately re-apply the user's original request** using the plugin's own routing — do not ask the user to repeat themselves. Do not show an install banner or onboarding table.

**Rule 2 — Not installed, exactly one DApp scores ≥ 75:**
Install silently, then load and execute:

```bash
npx skills add okx/plugin-store --skill <plugin-name> --yes --global
```
```
Read file: $HOME/.claude/skills/<plugin-name>/SKILL.md
```

> *(Path is Claude Code-specific — see Known Limitations in Step 1. On Codex / OpenCode / OpenClaw / Cursor, substitute the equivalent skills directory for your agent.)*

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

**Rule 5 — Highest score < 50 (no top-5 match):**

This skill's per-protocol tables cover 5 DApps with hard-coded routing. When none scores ≥ 50, decide between two sub-rules based on whether the user named *any* recognizable third-party DApp/protocol:

**Rule 5a — User named a recognizable third-party DApp NOT in the top 5** (e.g. Uniswap, Curve, GMX, Lido, Jupiter, Raydium, Compound, ether.fi, Pendle, Maker / Sky, Convex, Velodrome, Aerodrome, Camelot, SushiSwap, Balancer, Kamino, Orca, Meteora, dYdX, Across, LI.FI / Jumper, Mayan, deBridge, etc.):

The top-5 routing didn't match, but a plugin may exist in the broader registry. Install **plugin-store** as a catch-all discovery layer and delegate:

```bash
npx skills add okx/plugin-store --skill plugin-store --yes --global
```
```
Read file: $HOME/.claude/skills/plugin-store/SKILL.md
```

> *(Path is Claude Code-specific — see Known Limitations in Step 1.)*

Plugin-store has access to the broader plugin registry (`plugin-store list`) and can install a matching plugin if one exists. Forward the user's original request — the bootstrap is transparent. If plugin-store's registry has no matching plugin either, plugin-store will surface that and offer alternatives.

**Rule 5b — User did NOT name a specific DApp** (purely generic terms only):

Do not install anything. Show the user the supported DApps and ask which one matches their intent:

> The following third-party DApps are currently routable directly — let me know which one you'd like to use:
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
>
> If you want to use a different DApp not listed above (e.g., Uniswap, Curve, GMX, etc.), name it explicitly and I'll search the broader plugin registry via plugin-store.

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
