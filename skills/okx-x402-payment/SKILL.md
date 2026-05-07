---
name: okx-x402-payment
description: "HTTP 402 Payment Required dispatcher for x402 + MPP. Detects protocol from response headers and routes to the matching protocol playbook: 'WWW-Authenticate: Payment' header → MPP (`protocols/mpp.md`); 'PAYMENT-REQUIRED' header or `x402Version` body field → x402 (`protocols/x402.md`). Returns a ready-to-paste authorization header. MPP covers charge (one-shot) and session (open / voucher / topUp / close) in transaction (TEE-signed EIP-3009) and hash (client-broadcast) modes, with splits, optional initial-voucher prepay, and channel state tracking. x402 covers v1 (`X-PAYMENT` header) and v2 (`PAYMENT-SIGNATURE` header) with TEE or local-key signing. Trigger words (English): '402', 'payment required', 'mpp', 'machine payment', 'pay for access', 'payment-gated', 'WWW-Authenticate: Payment', 'x402', 'x402Version', 'PAYMENT-REQUIRED', 'PAYMENT-SIGNATURE', 'X-PAYMENT', 'open channel', 'voucher', 'session payment', 'close channel', 'topup channel', 'top up channel', 'settle channel', 'settle session', 'refund channel', 'channelId', 'channel_id'. Trigger words (Chinese): '支付通道', '关闭通道', '关闭会话', '关闭支付通道', '充值通道', '续费通道', '结算通道', '结算会话', '关单', '凭证', '会话支付'. Critical sensitivity rule: any user mention of close / topup / settle / voucher / refund near a `channel_id`, `0x...` channel hash, or 'session' / 'channel' context = MPP mid-session operation — load this skill, do NOT search for a separate close/topup tool."
license: MIT
metadata:
  author: okx
  version: "3.1.2"
  homepage: "https://web3.okx.com"
---

# Onchain OS HTTP 402 Payment (Dispatcher)

Detects whether a 402 is **MPP** or **x402** and loads the matching protocol playbook end-to-end.

> Read `../okx-agentic-wallet/_shared/preflight.md` before any `onchainos` command. EVM only — CAIP-2 `eip155:<chainId>` (run `onchainos wallet chains` for the list).

## Skill Routing

| Intent                                                   | Use skill              |
|----------------------------------------------------------|------------------------|
| Token prices / charts / wallet PnL / tracker activities  | `okx-dex-market`       |
| Token search / metadata / holders / cluster analysis     | `okx-dex-token`        |
| Smart money / whale / KOL signals                        | `okx-dex-signal`       |
| Meme / pump.fun token scanning                           | `okx-dex-trenches`     |
| Token swaps / trades / buy / sell                        | `okx-dex-swap`         |
| Authenticated wallet (balance / send / tx history)       | `okx-agentic-wallet`   |
| Public address holdings                                  | `okx-wallet-portfolio` |
| Tx broadcasting (MPP `feePayer=false` hash mode)         | `okx-onchain-gateway`  |
| Security scanning (token / DApp / tx / signature)        | `okx-security`         |

**MPP mid-session ops** (close / topup / settle / voucher / refund mentioned with an active `channel_id`, regardless of fresh 402) → stay here, load `protocols/mpp.md`, jump to the matching phase. **Do NOT** search for a separate `close-channel` / `topup-channel` / `settle-channel` tool — they're all `onchainos payment mpp-session-*` variants.

## Step 1: Send the Original Request

Make the HTTP request the user asked for. If status is **not 402**, return the body directly — no payment, no wallet check, no other tool calls.

## Step 2: Detect the Protocol

```
Priority 1: response.headers['WWW-Authenticate']
  starts with "Payment "        → MPP      → protocols/mpp.md
Priority 2: response.headers['PAYMENT-REQUIRED']
  base64-encoded JSON           → x402 v2  → protocols/x402.md
Priority 3: response body JSON has "x402Version"
                                → x402 v1  → protocols/x402.md
Otherwise                       → not a supported payment protocol, stop
```

**Both headers present** — STOP and ask:

> The server offers both MPP and x402 payment protocols. Which would you like to use?
> 1. **MPP** (newer, supports sessions and streaming, recommended)
> 2. **x402** (simpler, single-shot)

## Step 3: Dispatch

Load the matching playbook and follow it from decode → confirm → wallet check → sign → assemble header → replay → suggest next steps:

- **MPP** → `protocols/mpp.md` (charge + session, transaction + hash, splits, state tracking, seller error handling).
- **x402** → `protocols/x402.md` (v1 + v2, TEE signing, local-key fallback).
