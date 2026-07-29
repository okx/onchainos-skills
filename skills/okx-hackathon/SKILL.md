---
name: okx-hackathon
description: "Register a Trading ASP agent for the OKX.AI trading hackathon (交易黑客松 / OKX.AI 交易黑客松). Use when the user wants to sign up / 报名 their Trading ASP agent for the hackathon, asks about hackathon registration requirements, or mentions '黑客松' / 'hackathon' together with an agent/ASP. Do NOT use for joining a normal trading competition or cup (that is `okx-growth-competition`) — see the disambiguation table below."
license: MIT
metadata:
  author: okx
  version: "1.1.0"
  homepage: "https://web3.okx.com"
---

# OKX.AI Trading Hackathon Registration

Register the user's **Trading ASP agent** for the OKX.AI trading hackathon. Wraps the standalone `onchainos hackathon` CLI command group / `hackathon_register` MCP tool.

This SKILL.md holds the **global rules** (disambiguation, pre-flight, output rules, security) that the reference files depend on. Always read this file first; then jump into the matching reference for the user's intent.

## `hackathon register` vs `competition join` — do not conflate

They register **different subjects** against **different endpoints**:

| Signal in the request | Skill | Command | What is registered |
|---|---|---|---|
| "hackathon" / "黑客松" / "OKX.AI 交易黑客松" / "报名黑客松" / "register my ASP / agent" | **this skill** | `hackathon register` | the user's **Trading ASP agent** for the OKX.AI hackathon |
| "join / register for `<competition name>`" / a normal trading contest or cup | `okx-growth-competition` | `competition join` | the active **wallet account** for a standard trading competition |

**NEVER** run `competition join` for a hackathon request, or `hackathon register` for a plain "join this competition" request — they hit different backends and the wrong one fails or mis-registers.

## Pre-flight

> Read `../okx-agentic-wallet/_shared/preflight.md`. If missing, read `_shared/preflight.md`.

Wallet login is required. If not logged in, walk the user through the `okx-agentic-wallet` login flow (run `onchainos wallet login`), then resume at the step that failed.

## Mandatory reading order

**Before producing ANY user-facing message about hackathon registration, you MUST first locate the matching reference file below.** Do NOT improvise the format. Do NOT shorten or reorder the fixed templates in `references/registration.md` — they are product-mandated copy.

| User intent | Reference file |
|---|---|
| "register my agent / ASP for the hackathon", actively walking through registration | `references/registration.md` — full Step 1-4 flow + CLI/MCP reference |
| Standalone question about eligibility, funding, account types, errors, or "can I do X" — not mid-registration | `references/faq.md` |

If the user's intent does not clearly map to either file, ask which they meant before responding — do **not** invent a freeform format.

## Output Rules

**Never include any internal id (agent id, activity id) in a message produced for the user — under ANY circumstance, in ANY format.** Identify the agent and hackathon EXCLUSIVELY by name.

## Security

- **Non-spending**: registration receives value, so there is no confirm-to-spend (`CliConfirming`) gate.
- The JWT `access_token` is injected by the client layer from the keychain; it is **NEVER** logged, printed, or placed in a flag — leaking it would let an attacker act as the user.
- No new secrets are created or stored; the flow reads the existing wallet session.
