---
name: okx-x402-payment
description: "Use this skill when the user encounters an HTTP 402 Payment Required response, OR mentions any operation against an existing MPP payment channel / voucher / session, OR explicitly mentions x402. Detects the protocol from response headers and dispatches to the matching protocol playbook: 'WWW-Authenticate: Payment' → MPP (`protocols/mpp.md`); 'PAYMENT-REQUIRED' header / `x402Version` body → x402 (`protocols/x402.md`). For MPP: charge (one-shot) and session (open / voucher / topUp / close) with both transaction (TEE-signed EIP-3009) and hash (client-broadcast) modes, splits, optional initial-voucher prepay, and channel state tracking. For x402: TEE signing (via wallet session) or local signing (with user's own private key) for v1 (`X-PAYMENT` header, body payload) and v2 (`PAYMENT-SIGNATURE` header, base64 in `PAYMENT-REQUIRED`). Returns a ready-to-paste authorization header that the agent attaches to the original request to retry. Trigger words (English): '402', 'payment required', 'mpp', 'machine payment', 'pay for access', 'payment-gated', 'WWW-Authenticate: Payment', 'x402', 'x402Version', 'PAYMENT-REQUIRED', 'PAYMENT-SIGNATURE', 'X-PAYMENT', 'open channel', 'open session', 'voucher', 'session payment', 'close channel', 'close session', 'close payment channel', 'topup channel', 'top up channel', 'top up session', 'settle channel', 'settle session', 'refund channel', 'channelId', 'channel_id'. Trigger words (Chinese): '支付通道', '关闭通道', '关闭会话', '关闭支付通道', '充值通道', '续费通道', '结算通道', '结算会话', '关单', '凭证', '会话支付'. Critical sensitivity rule: any time the user mentions close / topup / settle / voucher / refund in proximity to a `channel_id`, `0x...` channel hash, or 'session' / 'channel' context, this is an MPP mid-session operation — load this skill (then `protocols/mpp.md`), do NOT search for a separate close/topup tool. Do NOT use for token swaps — use okx-dex-swap. Do NOT use for wallet balance / transfers — use okx-agentic-wallet / okx-wallet-portfolio. Do NOT use for arbitrary on-chain broadcasting — use okx-onchain-gateway (this skill delegates to it for hash-mode broadcasting). Do NOT use for security scans — use okx-security."
license: MIT
metadata:
  author: okx
  version: "1.0.4"
  homepage: "https://web3.okx.com"
---

# Onchain OS HTTP 402 Payment (Dispatcher)

Step-by-step dispatcher for HTTP 402 Payment Required. Detects whether the protocol is **MPP** or **x402**, then loads the matching protocol playbook and follows it end-to-end.

## Pre-flight Checks

> Read `../okx-agentic-wallet/_shared/preflight.md` before running any `onchainos` command.

## Skill Routing

This skill handles 402 payment authorization signing. For other operations, route to the matching skill instead — those own their flow end-to-end:

| Intent                                                                    | Use skill              |
|---------------------------------------------------------------------------|------------------------|
| Token prices / K-line charts / wallet PnL / address tracker activities    | `okx-dex-market`       |
| Token search / metadata / rankings / holder info / cluster analysis       | `okx-dex-token`        |
| Smart money / whale / KOL signals / leaderboard                           | `okx-dex-signal`       |
| Meme / pump.fun token scanning                                            | `okx-dex-trenches`     |
| Token swaps / trades / buy / sell                                         | `okx-dex-swap`         |
| Authenticated wallet balance / send tokens / tx history                   | `okx-agentic-wallet`   |
| Public wallet balance (by address)                                        | `okx-wallet-portfolio` |
| Hash-mode tx broadcasting (MPP `feePayer=false`)                          | `okx-onchain-gateway`  |
| Security scanning (token / DApp / tx / signature)                         | `okx-security`         |

For **MPP mid-session operations** (any user mention of close / topup / settle / voucher / refund in the context of an existing `channel_id`, regardless of whether a fresh 402 was received) — stay here, load `protocols/mpp.md`, and jump directly to the matching phase. **Do NOT** search for a separate `close-channel` / `topup-channel` / `settle-channel` tool; those operations are all variants of `onchainos payment mpp-session-*` commands.

## Supported Chains

EVM only. Use CAIP-2 `eip155:<chainId>` (e.g. `eip155:1` Ethereum, `eip155:196` X Layer, `eip155:8453` Base, `eip155:42161` Arbitrum). Non-EVM chains (Solana, Tron, Ton, Sui) are **not** supported by either protocol.

For the full EVM chain list:
```bash
onchainos wallet chains
```

The `realChainIndex` field in the chain list corresponds to the `<chainId>` portion of the CAIP-2 identifier.

## Step 1: Send the Original Request

Make the HTTP request the user asked for. If the response status is **not 402**, return the body directly — **no payment needed, do not check wallet, do not log in, do not call any other tool**.

> **IMPORTANT**: Only proceed to payment steps if the response is HTTP 402. Don't pre-emptively check wallet status.

## Step 2: Detect the Protocol

When the response status is 402, inspect headers in this priority order:

```
Priority 1: response.headers['WWW-Authenticate']
  starts with "Payment "                          → MPP        → load protocols/mpp.md
Priority 2: response.headers['PAYMENT-REQUIRED']
  base64-encoded JSON                             → x402 v2    → load protocols/x402.md
Priority 3: parse response body
  JSON with "x402Version" field                   → x402 v1    → load protocols/x402.md
Otherwise: not a supported payment protocol       → stop
```

**Both headers present** (server offers both protocols) — STOP and ask the user:

> The server offers both MPP and x402 payment protocols. Which would you like to use?
> 1. **MPP** (newer, supports sessions and streaming, recommended)
> 2. **x402** (simpler, single-shot)

If user chooses x402 → load `protocols/x402.md`. If MPP → load `protocols/mpp.md`.

## Step 3: Dispatch

Based on the detection result, load the matching protocol playbook and follow it from start to finish:

- **MPP** → read `protocols/mpp.md`. Covers charge (one-shot) + session (open / voucher / topUp / close), both transaction and hash modes, splits, channel state tracking, and seller error handling.
- **x402** → read `protocols/x402.md`. Covers v1 (`X-PAYMENT` header, body payload) and v2 (`PAYMENT-SIGNATURE` header, base64 in `PAYMENT-REQUIRED`), TEE signing, and local-key fallback.

Each protocol playbook is self-contained: decode → confirm → wallet check → sign → assemble header → replay original request → suggest next steps.

## Cross-Skill Workflows (both protocols)

| Need                                               | Skill                  |
|----------------------------------------------------|------------------------|
| Broadcast a transferWithAuth / open / topUp tx     | `okx-onchain-gateway`  |
| Check wallet balance before paying                 | `okx-agentic-wallet`   |
| Check public address holdings                      | `okx-wallet-portfolio` |
| Scan the tx / token / DApp for safety first        | `okx-security`         |

## Trigger Words Quick Reference

If any of these appear in the user's message, this dispatcher is the right entry point.

**English** — `402`, `payment required`, `mpp`, `machine payment`, `pay for access`, `payment-gated`, `WWW-Authenticate: Payment`, `x402`, `x402Version`, `PAYMENT-REQUIRED`, `PAYMENT-SIGNATURE`, `X-PAYMENT`, `open channel`, `voucher`, `session payment`, `close channel`, `topup channel`, `top up channel`, `settle channel`, `settle session`, `refund channel`, `channelId`, `channel_id`

**Chinese** — `支付通道`, `关闭通道`, `关闭会话`, `关闭支付通道`, `充值通道`, `续费通道`, `结算通道`, `结算会话`, `关单`, `凭证`, `会话支付`
