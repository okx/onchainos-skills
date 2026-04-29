# Welcome — Zero Balance (Free-zone preamble + Verbatim Welcome)

Shown to any user whose total wallet balance = $0. Reached from:

- **Path A**, `isNew=true` (brand-new account, balance always 0)
- **Path A**, `isNew=false` AND `wallet balance` total USD = 0
- **Path B** (already logged in at entry), `wallet balance` total USD = 0

All three reach the same onboarding state, so the same copy applies.

## Step 1 — Free zone (1–5 sentences)

Answer the user's actual opener question or acknowledgement first, in their language. If the opener was a "what is / how to use" question, ground the answer in the URL fetched per **SKILL.md → Free-Zone Source of Truth**. Then segue naturally into the welcome below.

## Step 2 — Prepare placeholders (run BEFORE rendering the banner)

The banner below contains four `{...}` placeholders — `{evm_address}`, `{evm_qrcode}`, `{solana_address}`, `{solana_qrcode}` — and one boolean flag `polymarket_available` that picks the menu variant. Resolve all of them **before** emitting the banner.

1. **Addresses.** Take `evmAddress` and `solAddress` from the `wallet balance` response (already returned). Never fabricate addresses.
2. **QR codes.** For each of the two addresses, run:

   ```bash
   onchainos wallet qrcode --address <evm_address>
   onchainos wallet qrcode --address <solana_address>
   ```

   Capture each command's raw stdout (Unicode-block art). The two outputs become `{evm_qrcode}` and `{solana_qrcode}`. Do not modify, re-wrap, or re-format. Never fabricate QR output.

3. **Polymarket geoblock check (fail-closed).** Polymarket is restricted in some jurisdictions (e.g. United States), so we probe the user's geo before showing it as a menu option. **Use Bash `curl` (the check must run from the user's local IP — do NOT use `WebFetch`, which runs from a different network).**

   ```bash
   curl -sS -m 5 -o /tmp/onchainos_geoblock.json -w "%{http_code}" https://polymarket.com/api/geoblock
   ```

   Expected response shape (HTTP 200):

   ```json
   { "blocked": false, "ip": "...", "country": "HK", "region": "" }
   ```

   Set `polymarket_available` strictly from the boolean `blocked` field:

   - HTTP `200` AND body parses to JSON AND `blocked === false` → `polymarket_available = true`.
   - **Any other outcome** — non-2xx response, timeout, network error, JSON parse error, missing `blocked` field, non-boolean `blocked`, or `blocked === true` — → `polymarket_available = false`.

   This is **fail-closed**: if we can't confidently determine the user is allowed, we hide Polymarket from the menu. Do not warn or surface the geoblock decision to the user — silently switch to the geoblocked menu variant.

   **PII handling**: the response includes the user's `ip` and `country`. Do **not** log these, do **not** echo them to the user, do **not** persist beyond the in-memory check. Discard `/tmp/onchainos_geoblock.json` after reading.

## Step 3 — Render the banner (output verbatim)

Translate the banner copy and the menu items to the user's language at runtime — match whatever language the user used. Substitute the placeholders prepared in Step 2.

**Output as plain text** (no `>` blockquote prefix, no surrounding fence). The two `{evm_qrcode}` / `{solana_qrcode}` placeholders **must each be wrapped in their own fenced monospace code block** at emission time so the block art scans cleanly. The address lines and menu lines stay as plain text.

The banner has a fixed top half (welcome line + addresses + QRs + balance) and a **conditional Quick-start menu** that depends on `polymarket_available`.

### Banner top — same in both variants

```
✅ Welcome to the Onchain OS ecosystem
Just chat with the Agent to run strategies, make payments, and play the hottest DApps — 24/7.

EVM: {evm_address}

{evm_qrcode}

Solana: {solana_address}

{solana_qrcode}

Balance: $0
```

### Quick-start menu — Variant A: `polymarket_available = true` (5 items)

```
Quick start:
1. Polymarket — pick today's Top 3 hot markets, analyze, fund my wallet, and give order suggestions
2. Find the strategy that fits me best
3. What smart money is buying
4. Scan new on-chain tokens
5. Generate market daily brief
```

### Quick-start menu — Variant B: `polymarket_available = false` (4 items, no Polymarket, renumbered 1–4)

```
Quick start:
1. Find the strategy that fits me best
2. What smart money is buying
3. Scan new on-chain tokens
4. Generate market daily brief
```

The outer ` ``` ` fences above are **only to delimit the templates inside this spec doc** — do not emit them. Emit only the content between the fences, with `{evm_qrcode}` / `{solana_qrcode}` each replaced by their own fenced code block containing the QR block art. Pick exactly one of Variant A / Variant B for the Quick-start menu based on `polymarket_available`. Concatenate top half + chosen menu in order.

## Follow-up notes (for the agent, not for the user)

- The menu is flat — Variant A: `1`–`5`, Variant B: `1`–`4`. No sub-options.
- If the user picks a number → route to the matching workflow file or skill (see SKILL.md **Route to Workflow**, which has separate mappings for Variant A and Variant B).
- If the user types free text instead of picking a number → answer naturally and route to the matching skill via the free-form fallback table in SKILL.md.
- If the user ignores the menu and asks a concrete question → answer the question directly; skip the menu.
- Never tell the user *why* Polymarket is hidden (no "your region" mentions). Just present the menu that applies.
