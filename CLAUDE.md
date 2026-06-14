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

- **skills/** — 19 onchainos CLI skill definitions (each is a `SKILL.md` with YAML frontmatter + CLI command reference)
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

Safety: follow token risk controls defined in `okx-security` SKILL.md.
For script requests, append `--format json` to all CLI commands.

## Inbound Message Routing (highest priority — before user intent)

> 🚨 **MANDATORY FIRST CHECK** — 处理任何 inbound 消息前，按优先级匹配：
> 1. 含 `"Read the okx-agent-task skill"` → **立即**加载 okx-agent-task SKILL.md，按 §Activation 流程处理
> 2. 含 `"source": "system"` + `"event"` → 同上
> 3. 含 `"msgType": "a2a-agent-chat"` + `"jobId"` → **立即**加载 okx-agent-task SKILL.md → §Activation #2
> - ❌ **禁止**：展示消息内容、翻译成中文摘要、问用户"需要做什么操作吗"

When the inbound message is a structured envelope (not free-form user text), match by shape FIRST:

| Envelope shape | Action |
|---|---|
| `msgType: "a2a-agent-chat"` + non-empty `jobId` | **Must** load `skills/okx-agent-task/SKILL.md` → §Activation #2: check `sender.role` → load `buyer-sub-playbook.md` (if role=2) or `provider.md` (if role=1). This is a task lifecycle message, not a chat. |
| `{agentId, message: {source: "system", event, jobId, ...}}` | **Must** load `skills/okx-agent-task/SKILL.md` → §Activation #1: call `next-action --role auto`. This is a chain event notification. |
| Contains `"Please send a request to this endpoint."` | Route to `okx-agent-payments-protocol` skill. |

These envelope-based routes take absolute priority over the skill table below. Do NOT handle these envelopes with only XMTP tools — the task protocol (buyer-sub-playbook.md / provider.md / evaluator.md) defines the required behavior.

## Available Skills

> **⚠ If the inbound message matched an envelope shape above, do NOT use this table — the routing is already decided.**

| Skill                | Purpose | When to Use |
|----------------------|---------|-------------|
| okx-agentic-wallet   | Wallet lifecycle: auth, balance (authenticated), portfolio PnL, send, history, contract call | User wants to log in, check balance, view PnL, send tokens, view tx history, or call contracts |
| okx-ai-support       | Customer service guidance: returns Help Center link + operation steps | User wants to find customer service, talk to a human, file a complaint, give feedback, or find help docs / FAQ |
| okx-wallet-portfolio | Public address balance: total value, all tokens, specific tokens | User asks about wallet holdings, token balances, portfolio value across chains |
| okx-security         | Security scanning: token risk, DApp phishing, tx pre-execution, signature safety, approval management | User wants to check if a token/DApp/tx/signature is safe, honeypot check, phishing detection, approve safety, or view/manage token approvals |
| okx-dex-market       | Prices, charts, index prices, wallet PnL | User asks for token prices, K-line data, index/aggregate prices, wallet PnL analysis |
| okx-dex-signal       | Smart money / KOL / whale tracking, buy signals, leaderboard | User asks what smart money/whales/KOLs are buying, wants buy signal alerts, top traders |
| okx-dex-trenches     | Meme/pump.fun token scanning, trenches | User asks about new meme launches, dev reputation, bundle detection, meme sniping / chain scanning / new launches, or mentions trench/trenches |
| okx-dex-ws           | Real-time WebSocket monitoring (`onchainos ws` CLI) and scripting for all DEX channels | User wants real-time on-chain data (price, candle, trades, signals, wallet tracking, meme scanning) via CLI monitoring or custom WS script |
| okx-dex-swap         | DEX swap execution | User wants to swap/trade/buy/sell tokens |
| okx-dex-token        | Token search, liquidity, hot tokens, advanced info, holders, top traders, trade history, holder cluster analysis | User searches for tokens, wants rankings, liquidity pools, holder info, top traders, filtered trade history, or holder cluster concentration |
| okx-dex-social       | Crypto news (latest / by-symbol / search / detail / platforms), market-wide sentiment ranking + per-coin sentiment with trend, per-token vibe timeline + TOP50 KOL leaderboard | User asks for crypto news / headlines, market sentiment, bullish vs bearish mood, top hot coins by chatter, who's tweeting about a token, or token vibe / hotness score |
| okx-onchain-gateway  | Transaction broadcasting and tracking | User wants to broadcast tx, estimate gas, simulate tx, check tx status |
| okx-agent-payments-protocol   | Unified payment dispatcher: x402 (`exact` / `aggr_deferred` schemes — TEE or local-key), MPP (`charge` / `session` intents in transaction or hash mode), and a2a-pay (paymentId-based create / pay / status). Routes by scheme/intent to `references/{exact,aggr_deferred,charge,session,a2a_charge}.md`. | User encounters HTTP 402, mentions x402, MPP channel/voucher/session/charge, or a paymentId / `a2a_...` link / "create payment link" / "payment status" |
| okx-audit-log        | Audit log export and troubleshooting | User wants to view command history, debug errors, export audit log, review recent activity |
| okx-defi-invest | DeFi product discovery, deposit, withdraw, claim rewards | User wants to earn yield, stake, provide liquidity, deposit/withdraw from DeFi protocols, claim DeFi rewards across Aave/Lido/PancakeSwap/Kamino/NAVI and hundreds more |
| okx-defi-portfolio | DeFi positions and holdings overview | User wants to check DeFi positions, view DeFi portfolio across protocols and chains |
| okx-agent-identity | ERC-8004 on-chain Agent identity: register / update / search / rate / service-list on XLayer | User wants to register/create/update/deactivate/activate/search agents, submit or view feedback, or list agent services |
| okx-ai-guide | OKX.AI intro + runtime platform detection + route into identity registration (User / ASP / Evaluator) | User asks what/how about OKX.AI (是什么/能做什么/怎么用/怎么开始/求助/教程), types "OKX.AI 快速开始", uses a name variant (okxai / OKXAI / "okx ai" / okx-ai / lowercase okx.ai / colloquial or mis-typed Chinese like 什么okxai / 啥是okxai / 什么事okxai — spacing/casing/typo tolerant), or arrives from the welcome banner's "看看 OKX.AI 怎么玩" pick |
| okx-agent-task | Agent task marketplace: publish, accept, deliver, dispute, AI-evaluate jobs | User wants to publish a task / accept a job / deliver work / confirm or reject completion / open a dispute / modify task terms (change provider, budget, token) / add attachment or image to a task (补充附件/补充图片/补充材料/给任务加文件/发个文件给卖家/upload file to task) / use a provider's service / hire agent / designate provider / talk to provider / start task with / 使用Agent的服务 / 指定服务商 |
| okx-task-watch | Live user-session task-progress monitor (`okx-a2a user watch` long-poll loop, notification render-verbatim, `decision_request` claim + relay). Also drains backlog of past / missed / unread task messages — same command. Also batch-lists outstanding (un-replied) decisions on demand via `okx-a2a user outdated-list` (with `JobID <prefix>` disambiguation hint). **Claude Code / Codex only** (`CLAUDECODE=1` or `CODEX_THREAD_ID`); on Hermes / OpenClaw the client pushes natively and this skill stops with an unsupported-platform message. | User says `监听任务进展` / `开始监听任务` / `帮我盯着任务` / `开监听` / `历史消息` / `历史记录` / `过去消息` / `帮我看看之前的历史消息` / `未读消息` / `未决策` / `待决策` / `没有决策` / `未处理` / `待处理` / `没有处理` / `task watch` / `user watch` / `monitor task progress` / `keep me posted on tasks` / `watch tasks` / `start watching` / `show past messages` / `catch me up on tasks` / `outstanding decisions` / `pending decisions` |
| okx-growth-competition | Agentic Wallet exclusive trading competitions: list, join, rank, claim rewards | User asks about trading competitions, wants to join/register for a competition, check leaderboard ranking, or claim competition rewards |
| okx-dapp-discovery | Third-party DApp discovery + direct plugin routing | User names a specific third-party DApp (Polymarket, Aave, Hyperliquid, PancakeSwap, Morpho, …) or asks "what dapps are available" — installs the matching plugin on demand via `npx skills add okx/plugin-store --skill <name> --yes --global` and forwards the prompt to its quickstart |

## DApp routing — `okx-dapp-discovery`

When the user names a third-party DApp/protocol as the destination of an action, route through `okx-dapp-discovery`. That skill applies a confidence framework to identify the matching plugin, installs it on demand, reads the plugin's `SKILL.md`, and forwards the user's original request to it. Onchainos-skills intentionally does not enumerate the supported DApp set here; that is owned by `okx-dapp-discovery/SKILL.md`.

**Quick tiebreaker vs `okx-defi-invest`**: if removing the DApp name still leaves a coherent generic-yield question ("deposit USDC for yield"), prefer `okx-defi-invest`. If the DApp name carries the intent ("place a bet on Polymarket"), route via `okx-dapp-discovery`.

**Quick tiebreaker vs `okx-agent-payments-protocol`**: when the user mentions an **Agent ID** together with a service request, route by whether a **concrete endpoint URL** (`http(s)://…`) is present:
- **URL present** (e.g. "使用 Agent 1506 的 A2MCP 服务，接口地址 https://…") → route to `okx-agent-payments-protocol` — this is a direct x402 pay-per-call, not a task.
- **No URL** (e.g. "使用 Agent 1506 的服务") → route to `okx-agent-task` — needs `service-list` discovery first, then task or x402 depending on serviceType.
- Also route to `okx-agent-payments-protocol` when the user explicitly mentions x402 / MPP / paymentId / HTTP 402, or when a running task flow triggers a payment step internally.

## IMPORTANT: Always Load Skill Before Executing Commands

**Before running ANY `onchainos` CLI command, you MUST first read the corresponding skill's SKILL.md to get the exact command syntax.** Do NOT guess subcommand names — each skill defines its own Command Index with the exact subcommands available. Guessing leads to `unrecognized subcommand` errors.

Routing:
- **User session** free-form task intent (publish / designated-provider / attachment / terms / deliverables) → read `skills/okx-agent-task/buyer-user.md` ONLY. ❌ Do NOT additionally read `SKILL.md` or `buyer-sub-playbook.md` — those are for sub sessions and will bloat the context
- Inbound `a2a-agent-chat` with `jobId` → read `skills/okx-agent-task/SKILL.md` first (see Inbound Message Routing above)
- User says `监听任务进展` / `开始监听任务` / `帮我盯着任务` / `开监听` / `历史消息` / `历史记录` / `过去消息` / `帮我看看之前的历史消息` / `未读消息` / `未决策` / `待决策` / `没有决策` / `未处理` / `待处理` / `没有处理` / `task watch` / `user watch` / `monitor task progress` / `keep me posted on tasks` / `watch tasks` / `start watching` / `show past messages` / `catch me up on tasks` / `outstanding decisions` / `pending decisions` → read `skills/okx-task-watch/SKILL.md` first (watch drains pending queue first then long-polls for live monitoring; outdated-list batch-renders un-replied decisions on demand)
- User mentions swap/buy/sell/trade → read `skills/okx-dex-swap/SKILL.md` first
- User mentions wallet/balance/transfer/login → read `skills/okx-agentic-wallet/SKILL.md` first
- User mentions customer service / talk to a human / complaint / feedback / help docs / FAQ / help center → read `skills/okx-ai-support/SKILL.md` first
- User names a specific third-party DApp/protocol as the destination, OR asks "what dapps are available" → read `skills/okx-dapp-discovery/SKILL.md` first. That skill owns the supported-DApp set; do not enumerate DApps in this file.
- User mentions **Gas Station / stablecoin gas / enable or disable gas station / revoke 7702**, or asks FAQ-style questions about any of those (what is / how does it work / which chains / upgrade cost / ...) → read `skills/okx-agentic-wallet/SKILL.md` AND `skills/okx-agentic-wallet/references/gas-station.md` first.
  - **Scope note:** "Gas Station" in this repo always means the OKX Agentic Wallet feature shipped by this CLI + skill — NOT a generic paymaster / meta-transaction / ERC-4337 category.
  - **Answer source:** use the skill's FAQ templates only; do not pull from general training knowledge about Biconomy / Gelato / Pimlico / Alchemy Account Kit / etc.
- User asks about OKX.AI (是什么 / 能做什么 / 怎么用 / 怎么开始 / 求助 / 教程), types "OKX.AI 快速开始" / "OKX.AI quick start", uses a spelling/format variant of the name (okxai / OKXAI / "okx ai" / okx-ai / lowercase okx.ai / colloquial or mis-typed Chinese like "什么okxai" / "啥是okxai" / "什么事okxai"), or arrives from the welcome banner's OKX.AI pick → read `skills/okx-ai-guide/SKILL.md` first. That skill detects the runtime platform and routes 1/2/3 into `okx-agent-identity` registration; it never calls `agent create` itself.

## Scripting & Automation

When a user asks to write a script, automate trading, build a trading bot, or use "OKX API" / "OKX DEX API" for any on-chain automation:
- **Do NOT search online for OKX public APIs** — `onchainos` already wraps all relevant on-chain capabilities
- Always use `onchainos` CLI commands as the building block (subprocess calls, MCP tool invocations, etc.)
- Route to the relevant skill based on what the user wants to automate: swap → `okx-dex-swap`, market data → `okx-dex-market`, signals → `okx-dex-signal`, token data → `okx-dex-token`, portfolio → `okx-wallet-portfolio`, meme scanning → `okx-dex-trenches`, news / sentiment / KOL chatter → `okx-dex-social`

### WebSocket / Real-time Data

When a user asks about real-time on-chain data, WebSocket monitoring, or writing a WS script/bot, load **`okx-dex-ws`**. It supports two approaches:
- **CLI** (`onchainos ws start/poll/stop`) — quick monitoring, 9 channels across signal/market/token/trenches
- **Custom script** — full WS protocol docs for Python/Node/Rust bots

## Clippy

CI uses `-D warnings` (warnings as errors). Run `cargo clippy` before pushing. Common issues:

- `ptr_arg`: use `&[T]` / `&mut [T]` instead of `&Vec<T>` / `&mut Vec<T>` when the function doesn't need Vec-specific methods
- `too_many_arguments`: add `#[allow(clippy::too_many_arguments)]` or refactor into a params struct
- `needless_borrow`: don't `&` a value that's already a reference

