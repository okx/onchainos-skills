---
name: okx-hackathon
description: "Register a Trading ASP agent for the OKX.AI trading hackathon (交易黑客松 / OKX.AI 交易黑客松). Use when the user wants to sign up / 报名 their Trading ASP agent for the hackathon, asks about hackathon registration requirements, or mentions '黑客松' / 'hackathon' together with an agent/ASP. Do NOT use for joining a normal trading competition or cup (that is `okx-growth-competition`)."
license: MIT
metadata:
  author: okx
  version: "4.4.1"
  homepage: "https://web3.okx.com"
---

# OKX.AI Trading Hackathon Registration

Register the user's **Trading ASP agent** for the OKX.AI trading hackathon. Wraps the `onchainos hackathon` CLI command group / `hackathon_register` MCP tool.

## Mandatory reading order

**Before producing ANY user-facing message about hackathon registration, you MUST first read the matching reference file below.** Do NOT improvise the format. Do NOT shorten or reorder the fixed templates in `references/registration.md` — they are product-mandated copy.

| User intent | Reference file |
|---|---|
| "register my agent / ASP for the hackathon", actively walking through registration | `references/registration.md` — Step 1-4 flow + CLI/MCP reference |
| Standalone question about eligibility, funding, account types, errors, or "can I do X" — not mid-registration | `references/faq.md` |

If the intent maps to neither, ask which they meant — do **not** invent a freeform format.

## Wrong-skill guard

`hackathon register` registers a **Trading ASP agent** for the OKX.AI hackathon. `competition join` (`okx-growth-competition`) registers the **wallet account** for a standard trading competition. Different backends, different subjects — **NEVER** substitute one for the other.

If one request carries signals for **both** (e.g. names "hackathon"/"黑客松" *and* "competition"/"大赛"/"cup"), ask which the user means before running either command.

## Pre-flight

> Read `../okx-agentic-wallet/_shared/preflight.md`. If missing, read `_shared/preflight.md`.

Wallet login is required. If not logged in, walk the user through the `okx-agentic-wallet` login flow (`onchainos wallet login`), then resume at the step that failed.

## Output Rules

- Identify the hackathon EXCLUSIVELY by name ("OKX.AI Trading Hackathon") — **never** by its internal activity id, in any format. The CLI does not return that id; do not source it from anywhere else.
- The agent id MAY appear exactly once: in the numbered ASP-selection list (`references/registration.md` Step 1), so ASPs sharing a name can be told apart. Every later message — confirmation, success, failure — identifies the chosen agent **by name only**.
- Registration receives value, so there is no confirm-to-spend (`CliConfirming`) gate.
- The JWT is injected by the client layer from the keychain — never log it, print it, or pass it in a flag. The flow creates no new secrets.
