# Welcome — Zero Balance (Free-zone preamble + Verbatim Welcome)

Shown to any user whose total wallet balance = $0. Reached from:

- **Path A**, `isNew=true` (brand-new account, balance always 0)
- **Path A**, `isNew=false` AND `wallet balance` total USD = 0
- **Path B** (already logged in at entry), `wallet balance` total USD = 0

All three reach the same onboarding state, so the same copy applies.

## Step 1 — Free zone (1–5 sentences)

Answer the user's actual opener question or acknowledgement first, in their language. If the opener was a "what is / how to use" question, ground the answer in the URL fetched per **SKILL.md → Free-Zone Source of Truth**. Then segue naturally into the welcome below.

## Step 2 — Prepare placeholders (run BEFORE rendering the banner)

The banner below contains four `{...}` placeholders — `{evm_address}`, `{evm_qrcode}`, `{solana_address}`, `{solana_qrcode}`. Resolve all four **before** emitting the banner so they substitute cleanly inline.

1. **Addresses.** Take `evmAddress` and `solAddress` from the `wallet balance` response (already returned). Never fabricate addresses.
2. **QR codes.** For each of the two addresses, run:

   ```bash
   onchainos wallet qrcode --address <evm_address>
   onchainos wallet qrcode --address <solana_address>
   ```

   Capture each command's raw stdout (Unicode-block art). The two outputs become `{evm_qrcode}` and `{solana_qrcode}`. Do not modify, re-wrap, or re-format. Never fabricate QR output.

## Step 3 — Render the banner (output verbatim)

Translate the banner copy and the menu items to the user's language at runtime — match whatever language the user used. Substitute the four placeholders prepared in Step 2.

**Output as plain text** (no `>` blockquote prefix, no surrounding fence). The two `{evm_qrcode}` / `{solana_qrcode}` placeholders **must each be wrapped in their own fenced monospace code block** at emission time so the block art scans cleanly. The address lines and menu lines stay as plain text.

Banner template (canonical English; translate at runtime):

```
✅ Welcome to the Onchain OS ecosystem
Just chat with the Agent to run strategies, make payments, and play the hottest DApps — 24/7.

EVM: {evm_address}

{evm_qrcode}

Solana: {solana_address}

{solana_qrcode}

Balance: $0

Quick start:
1. Polymarket — pick today's Top 3 hot markets, analyze, fund my wallet, and give order suggestions
2. Find the strategy that fits me best
3. What smart money is buying
4. Scan new on-chain tokens
5. Generate market daily brief
```

The outer ` ``` ` fence above is **only to delimit the template inside this spec doc** — do not emit it. Emit only the content between the fences, with `{evm_qrcode}` / `{solana_qrcode}` each replaced by their own fenced code block containing the QR block art.

## Follow-up notes (for the agent, not for the user)

- The menu is flat — `1`–`5`. No sub-options.
- If the user picks a number → route to the matching workflow file or skill (see SKILL.md **Route to Workflow**).
- If the user types free text instead of picking a number → answer naturally and route to the matching skill via the free-form fallback table in SKILL.md.
- If the user ignores the menu and asks a concrete question → answer the question directly; skip the menu.
