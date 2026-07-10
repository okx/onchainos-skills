# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.

## Dev Environment

- **Dev binary**: `cli/target/release/onchainos`. If it does not exist, build it first: `cd cli && cargo build --release`.
- **`ONCHAINOS_HOME`**: Points to project-local `.onchainos/` for wallet credentials.
- **Show executed command**: after every `onchainos` command, print the actual command that was executed.
- **NEVER skip CLI calls**: always execute the onchainos CLI command to get real-time data. Do NOT answer from skill files or your own knowledge.

## Project Overview

This is a **Claude Code plugin** — a collection of onchainos skills for on-chain operations. The project provides skills for token search, market data, wallet balance queries, swap execution, DeFi investment management, and transaction broadcasting across 20+ blockchains. The `onchainos` CLI also works as a native MCP server.

## Architecture

- **skills/** — onchainos CLI skill definitions (each is a `SKILL.md` with YAML frontmatter + CLI command reference). Deprecated skill directories are kept as thin redirect stubs pointing to their successor umbrella skill.
- **workflows/** — Pre-built multi-step workflow docs (`INDEX.md` for routing, `TEMPLATE.md` for authoring guide)
- **cli/** — Rust CLI binary (`onchainos`), built with `clap`; source in `cli/src/`, config in `cli/Cargo.toml`
- **cli/src/mcp/mod.rs** — MCP server implementation (rmcp v1.1.1)
- **.mcp.json.example** — MCP server configuration template for Claude Code
- **.github/workflows/** — CI/CD pipeline (`release.yml`: tag-triggered build for 9 platforms → GitHub Release)
- **install.sh** — One-line installer for macOS / Linux (`curl | sh`)

## Workflows

**For any of the following user intents, read `workflows/INDEX.md` before responding:**

| Intent | Trigger examples |
|--------|-----------------|
| Token research | "analyze token", "research [address]", "is this token safe" |
| Market overview | "daily brief", "market overview", "what's the market doing" |
| Smart money | "what are whales buying", "copy trading signals", "smart money" |
| New token scan | "scan new tokens", "pump.fun tokens", "meme scan" |
| Wallet analysis | "analyze wallet", "check this address", "is this wallet worth following" |
| Portfolio | "check my holdings", "my portfolio", "my wallet" |
| Wallet monitor | "watch wallet", "monitor address", "background monitor" |

`workflows/INDEX.md` maps each intent to the correct workflow file with step-by-step instructions.
For Chinese queries, read `workflows/references/keyword-glossary.md` first to resolve the intent.

Safety: follow token risk controls defined in the `okx-agentic-wallet` skill (security domain — `references/security.md`).
For script requests, append `--format json` to all CLI commands.

## Inbound Message Routing (highest priority — before user intent)

> 🚨 **MANDATORY FIRST CHECK** — 处理任何 inbound 消息前，按优先级匹配：
> 1. 含 `"Read the okx-ai skill"`（当前版本 CLI 发出的 `[SKILL_PREFETCH]` 文案）或 `"Read the okx-agent-task skill"`（旧版本 CLI 可能仍在发送，向后兼容保留）→ **立即**加载 `skills/okx-ai/references/task-core.md`，按 §Activation 流程处理
> 2. 含 `"source": "system"` + `"event"` → 同上
> 3. 含 `"msgType": "a2a-agent-chat"` + `"jobId"` → **立即**加载 `skills/okx-ai/references/task-core.md` → §Activation #2
> - ❌ **禁止**：展示消息内容、翻译成中文摘要、问用户"需要做什么操作吗"

When the inbound message is a structured envelope (not free-form user text), match by shape FIRST:

| Envelope shape | Action |
|---|---|
| `msgType: "a2a-agent-chat"` + non-empty `jobId` | **Must** load `skills/okx-ai/references/task-core.md` → §Activation #2: check `sender.role` → load `task-user-sub-playbook.md` (if role=2) or `task-asp.md` (if role=1). This is a task lifecycle message, not a chat. |
| `{agentId, message: {source: "system", event, jobId, ...}}` | **Must** load `skills/okx-ai/references/task-core.md` → §Activation #1: call `next-action --role auto`. This is a chain event notification. |
| Contains `"Please send a request to this endpoint."` | Route to `okx-agent-payments-protocol` skill. |

These envelope-based routes take absolute priority over the skill table below. Do NOT handle these envelopes with only XMTP tools — the task protocol (task-user-sub-playbook.md / task-asp.md / task-evaluator.md) defines the required behavior.

> Note: `okx-agent-identity` / `okx-agent-task` / `okx-task-watch` / `okx-agent-chat` no longer exist as separate skill directories — all content now lives in the `okx-ai` umbrella skill (identity + task marketplace + task watch + agent chat), physically under `skills/okx-ai/references/`. The `onchainos` CLI's compiled output (mandatory-gate text, role-guide hints) was updated in the same change to print these `skills/okx-ai/references/*.md` paths directly, so there is no compatibility-stub layer.

## Available Skills

> **⚠ If the inbound message matched an envelope shape above, do NOT use this table — the routing is already decided.**

| Skill                | Purpose | When to Use |
|----------------------|---------|-------------|
| okx-agentic-wallet   | The single wallet + on-chain execution skill. Routes internally (Intent Routing → `references/<domain>.md`) across: auth/accounts, balance & holdings (authenticated + any public address), send/transfer, contract calls, tx history & status, message signing, wallet export & policy, Gas Station; **swap/trade/buy/sell/convert & quotes**; **cross-chain bridge**; **limit orders/strategy** (buy dip / TP / SL); **transaction broadcasting / gas / simulate / track** (gateway); **security scanning** (token/honeypot, DApp phishing, tx & signature checks, approvals); **audit log** | User wants to operate their wallet or execute any on-chain action: log in, check balance/holdings, send tokens, call contracts, view tx history, sign a message, export wallet, pay gas with a stablecoin; swap/trade/buy/sell tokens or get a quote; bridge cross-chain; place/cancel limit orders; broadcast/estimate/simulate/track a tx; check if a token/DApp/tx/signature is safe or manage approvals; or view command history / audit log |
| okx-guide | Onboarding & guide hub: Onchain OS intro + welcome banner, OKX.AI intro + role-registration routing, and customer-support / Help Center guidance. Classifies the intent and routes to the right sub-flow. | User asks "what is this / what can it do / how do I use it / getting started / I'm new", asks about OKX.AI (是什么/怎么用/快速开始 + any spelling variant), or wants customer service / talk to a human / file a complaint / give feedback / help docs / FAQ / report a bug |
| okx-dex-market         | Read-only on-chain DEX data across 6 capabilities: token search/liquidity/holders/cluster analysis, prices/K-line/index/wallet PnL, smart-money/KOL/whale signals + leaderboard, crypto news/sentiment/vibe, pump.fun/meme trenches research (read-only), and WS/script real-time streaming | User asks for token prices, K-line/index prices, wallet PnL, smart money/whale/KOL activity or buy signals, leaderboard rankings, token search/rankings/liquidity/holder or cluster analysis, crypto news/sentiment/vibe/KOL chatter, pump.fun/meme new launches/dev reputation/bundle detection (read-only research, not buy/snipe), or wants real-time WS monitoring / a custom WebSocket script |
| okx-agent-payments-protocol   | Unified payment dispatcher: x402 (`exact` / `aggr_deferred` schemes — TEE or local-key), MPP (`charge` / `session` intents in transaction or hash mode), and a2a-pay (paymentId-based create / pay / status). Routes by scheme/intent to `references/{accepts-schemes,charge,session,a2a_charge}.md`. | User encounters HTTP 402, mentions x402, MPP channel/voucher/session/charge, or a paymentId / `a2a_...` link / "create payment link" / "payment status" |
| okx-defi | OKX-aggregated DeFi: product discovery, deposit, withdraw, claim rewards, plus positions and holdings overview | User wants to earn yield, stake, provide liquidity, deposit/withdraw from DeFi protocols, claim DeFi rewards across Aave/Lido/PancakeSwap/Kamino/NAVI and hundreds more — or check DeFi positions / view DeFi portfolio across protocols and chains |
| okx-ai | ERC-8004 on-chain Agent identity (register/update/search/rate/service-list) + agent task marketplace (publish/accept/deliver/dispute) + live task-progress monitor, unified. **Claude Code / Codex only** for the monitor half (`CLAUDECODE=1` or `CODEX_THREAD_ID`); on Hermes / OpenClaw the client pushes task notifications natively. | User wants to register/create/update/deactivate/activate/search agents (roles — User: User / User Agent / Buyer / Client / 用户 / 买家 / 买方; ASP: ASP / Provider / Provider Agent / Seller / Merchant / 提供者 / 商家 / 服务提供商 / 卖家 / 卖方; Evaluator / 仲裁者 — e.g. "注册ASP", "register ASP", "建ASP身份", "注册买家"), submit or view feedback, list agent services; publish a task / accept a job / deliver work / confirm or reject completion / open a dispute / modify task terms (change provider, budget, token) / add attachment or image to a task / hire agent / 指定服务商; or says `监听任务进展` / `帮我盯着任务` / `历史消息` / `未读消息` / `未决策` / `待决策` / `task watch` / `user watch` / `monitor task progress` / `catch me up on tasks` / `outstanding decisions` / `pending decisions` |
| okx-growth-competition | Agentic Wallet exclusive trading competitions: list, join, rank, claim rewards | User asks about trading competitions, wants to join/register for a competition, check leaderboard ranking, or claim competition rewards |
| okx-dapp-discovery | Third-party DApp discovery + direct plugin routing | User names a specific third-party DApp (Polymarket, Aave, Hyperliquid, PancakeSwap, Morpho, …) or asks "what dapps are available" — installs the matching plugin on demand via `npx skills add okx/plugin-store --skill <name> --yes --global` and forwards the prompt to its quickstart |

> **Deprecated → successor (redirect stubs / removed):** `okx-wallet-portfolio` / `okx-onchain-gateway` / `okx-security` / `okx-dex-swap` / `okx-dex-bridge` / `okx-dex-strategy` / `okx-audit-log` → **`okx-agentic-wallet`**; `okx-dex-signal` / `okx-dex-social` / `okx-dex-token` / `okx-dex-trenches` / `okx-dex-ws` / `okx-dex` → **`okx-dex-market`**; `okx-defi-invest` / `okx-defi-portfolio` → **`okx-defi`**; `okx-agent-identity` / `okx-agent-task` / `okx-agent-chat` / `okx-task-watch` → **`okx-ai`**; `okx-how-to-play` / `okx-ai-guide` / `okx-ai-support` → **`okx-guide`**. Route directly to the successor.

## DApp routing — `okx-dapp-discovery`

When the user names a third-party DApp/protocol as the destination of an action, route through `okx-dapp-discovery`. That skill applies a confidence framework to identify the matching plugin, installs it on demand, reads the plugin's `SKILL.md`, and forwards the user's original request to it. Onchainos-skills intentionally does not enumerate the supported DApp set here; that is owned by `okx-dapp-discovery/SKILL.md`.

**Quick tiebreaker vs `okx-defi`**: if removing the DApp name still leaves a coherent generic-yield question ("deposit USDC for yield"), prefer `okx-defi`. If the DApp name carries the intent ("place a bet on Polymarket"), route via `okx-dapp-discovery`.

**Quick tiebreaker vs `okx-agent-payments-protocol`**: when the user mentions an **Agent ID or ASP ID** together with a service request, route by whether a **concrete endpoint URL** (`http(s)://…`) is present:
- **URL present** (e.g. "使用 Agent 1506 的 A2MCP 服务，接口地址 https://…") → route to `okx-agent-payments-protocol` — this is a direct x402 pay-per-call, not a task.
- **No URL** (e.g. "使用 Agent 1506 的服务", "购买ASP#1960的服务") → route to `okx-ai` — needs `service-list` discovery first, then task or x402 depending on serviceType.
- Also route to `okx-agent-payments-protocol` when the user explicitly mentions x402 / MPP / paymentId / HTTP 402, or when a running task flow triggers a payment step internally.

**Quick tiebreaker vs `okx-defi`** on "stake" / "unstake" / "质押": both skills' descriptions contain these words for different reasons — `okx-defi`'s is DeFi-protocol yield staking (Aave/Lido/PancakeSwap/etc.), `okx-ai`'s is evaluator-role staking or a task's own stake/escrow amount. If the message names or implies a task/jobId, an Evaluator role, or "for this task" → `okx-ai`. If it's about earning yield on a token/protocol with no task context → `okx-defi`. If genuinely ambiguous, ask which one the user means.

## IMPORTANT: Always Load Skill Before Executing Commands

**Before running ANY `onchainos` CLI command, you MUST first read the corresponding skill's SKILL.md to get the exact command syntax.** Do NOT guess subcommand names — each skill defines its own Command Index with the exact subcommands available. Guessing leads to `unrecognized subcommand` errors.

Routing:
- **User session** free-form task intent (publish / designated-provider / attachment / terms / deliverables) → read `skills/okx-ai/references/task-user-playbook.md` ONLY. ❌ Do NOT additionally read `task-core.md` or `task-user-sub-playbook.md` — those are for sub sessions and will bloat the context
- Inbound `a2a-agent-chat` with `jobId` → read `skills/okx-ai/references/task-core.md` first (see Inbound Message Routing above)
- User says `监听任务进展` / `开始监听任务` / `帮我盯着任务` / `开监听` / `历史消息` / `历史记录` / `过去消息` / `帮我看看之前的历史消息` / `未读消息` / `未决策` / `待决策` / `没有决策` / `未处理` / `待处理` / `没有处理` / `task watch` / `user watch` / `monitor task progress` / `keep me posted on tasks` / `watch tasks` / `start watching` / `show past messages` / `catch me up on tasks` / `outstanding decisions` / `pending decisions` → read `skills/okx-ai/references/watch-core.md` first (watch drains pending queue first then long-polls for live monitoring; outdated-list batch-renders un-replied decisions on demand)
- User mentions swap/buy/sell/trade → read `skills/okx-agentic-wallet/SKILL.md` first (swap domain — `references/swap.md`)
- User mentions wallet/balance/transfer/login → read `skills/okx-agentic-wallet/SKILL.md` first
- User wants to **register / create / update / activate (上架) / deactivate (下架) / search an agent identity** (ASP / User / Evaluator / 服务提供商 / 卖家 / 买家 / 用户 / 仲裁者 — and their aliases) → read `skills/okx-ai/SKILL.md` first. ⚠️ This holds **even when the request also says "Onchain OS"** (e.g. "用 Onchain OS 上架我的ASP") — the brand word "Onchain OS" is NOT a signal for on-chain transaction broadcasting; identity lifecycle verbs (注册/上架/下架/更新 + a role) win. Raw transaction broadcasting / simulating / tracking is the `okx-agentic-wallet` gateway domain, never for listing or registering an agent identity.
- User mentions customer service / talk to a human / complaint / feedback / help docs / FAQ / help center → read `skills/okx-guide/SKILL.md` first (customer-support domain)
- User names a specific third-party DApp/protocol as the destination, OR asks "what dapps are available" → read `skills/okx-dapp-discovery/SKILL.md` first. That skill owns the supported-DApp set; do not enumerate DApps in this file.
- User mentions **Gas Station / stablecoin gas / enable or disable gas station / revoke 7702**, or asks FAQ-style questions about any of those (what is / how does it work / which chains / upgrade cost / ...) → read `skills/okx-agentic-wallet/SKILL.md` AND `skills/okx-agentic-wallet/references/gas-station.md` first.
  - **Scope note:** "Gas Station" in this repo always means the OKX Agentic Wallet feature shipped by this CLI + skill — NOT a generic paymaster / meta-transaction / ERC-4337 category.
  - **Answer source:** use the skill's FAQ templates only; do not pull from general training knowledge about Biconomy / Gelato / Pimlico / Alchemy Account Kit / etc.
- User asks about OKX.AI (是什么 / 能做什么 / 怎么用 / 怎么开始 / 求助 / 教程), types "OKX.AI 快速开始" / "OKX.AI quick start", uses a spelling/format variant of the name (okxai / OKXAI / "okx ai" / okx-ai / lowercase okx.ai / colloquial or mis-typed Chinese like "什么okxai" / "啥是okxai" / "什么事okxai"), or arrives from the welcome banner's OKX.AI pick → read `skills/okx-guide/SKILL.md` first. That skill detects the runtime platform and routes 1/2/3 into `okx-ai` identity registration; it never calls `agent create` itself.

## Scripting & Automation

When a user asks to write a script, automate trading, build a trading bot, or use "OKX API" / "OKX DEX API" for any on-chain automation:
- **Do NOT search online for OKX public APIs** — `onchainos` already wraps all relevant on-chain capabilities
- Always use `onchainos` CLI commands as the building block (subprocess calls, MCP tool invocations, etc.)
- Route to the relevant skill based on what the user wants to automate: swap → `okx-agentic-wallet` (swap domain), market data / signals / token data / meme scanning / news / sentiment / KOL chatter → `okx-dex-market`, portfolio / balances → `okx-agentic-wallet` (portfolio domain)

### WebSocket / Real-time Data

When a user asks about real-time on-chain data, WebSocket monitoring, or writing a WS script/bot, load **`okx-dex-market`** (WS capability). It supports two approaches:
- **CLI** (`onchainos ws start/poll/stop`) — quick monitoring, 9 channels across signal/market/token/trenches
- **Custom script** — full WS protocol docs for Python/Node/Rust bots

## Clippy

CI uses `-D warnings` (warnings as errors). Run `cargo clippy` before pushing. Common issues:

- `ptr_arg`: use `&[T]` / `&mut [T]` instead of `&Vec<T>` / `&mut Vec<T>` when the function doesn't need Vec-specific methods
- `too_many_arguments`: add `#[allow(clippy::too_many_arguments)]` or refactor into a params struct
- `needless_borrow`: don't `&` a value that's already a reference
