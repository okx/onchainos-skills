# Invariants — rendering rules, id ladder, fields, commands

Load this file when: rendering a card / diff / detail view, resolving `#<id>`, translating CLI labels, or handling `--service` fields.

---

## Lexicon (prose / Q&A / post-success rows when CLI label is absent)

- **Roles:** requester → **User Agent** / 用户 · provider → **Agent Service Provider (ASP)** / 服务提供商 · evaluator → **Evaluator Agent** / 仲裁者. Never raw enum, never legacy nouns (buyer/seller), never bilingual parenthetical.
- **Service type:** A2MCP → **API service** · A2A → **agent-to-agent**. Gloss once per table: "API service = pay-per-call, fixed price; agent-to-agent = negotiated / off-chain pricing." Never raw A2MCP/A2A.
- **Stars:** render `★ <value>` from CLI's `ratingStars` / `feedbackRate` / `average` **directly** — never divide by 20, never show raw 0–100. Null/0 context-split: **search** rows → `null`=`—`, `0`=`No rating yet`; **list / detail / feedback** → no rating = `No rating yet` (never `—`).
- **Fee:** `N USDT`; A2A empty or zero → `negotiable`. **Address:** lowercase `0x…1234`. **Reviewer** slot = "reviewer", never "creator".

## Card skeleton (every confirmation / diff / detail card uses THIS)

Two-column pipe table `| Field | Value |`, one row per field. Role row uses localized label (never enum); photo row = uploaded CDN URL or `default` — never a user-pasted link (rejected; see register §5).

- **Confirmation variant** (create only): ends with `> Reply **1** to confirm and run.` (localized). No bash shown.
- **Diff variant** (update only): 3 columns `| Field | Current | New |`; unchanged fields → `(unchanged)`; changed New cell **bold**. Show real before→after values.

## Verbatim-render contract (P0-4)

When CLI returns `card[]` / `cells[]` plus `roleLabel` / `statusLabel` / `approvalLabel` / `ratingStars`, render numeric/star fields **verbatim** — do not hand-map integers, do not divide score/20, never show raw 0–100. **Exception:** string `*Label` fields are English-canonical — translate to conversation language before rendering. Fallback: hand-map via Lexicon if `*Label` absent (legacy response).

## CLI output fields — translate before rendering

- `roleLabel` / `statusLabel` / `approvalLabel`
- Service type values: "API service" / "agent-to-agent"
- Placeholder strings: "(not set)" / "default" / "No rating yet" / "(no comment)" / "free" / "negotiable"
- `findings[].issue` and `findings[].fix` — translate the QA guidance text

## #id ladder (P0-3) — resolving `#<id>` after create

1. top-level **`newAgentId`** when its value is a **non-empty string** (PRIMARY — WS push succeeded)
2. else `agent.agentId` from the WS push object
3. `newAgentId` is `null` (WS push timed out) — omit `#<id>` substring, use fallback wording.

Never invent or borrow a pre-check id; never emit a bare `# `.
**Non-create intents** (activate/deactivate/update/detail): no `newAgentId` — use the `#N` the user typed or the CLI's direct id.

## Fields-from-user (output-safety invariant)

`name` / `description` / `picture` / `service.*` come from the user's **literal reply this turn** — never pre-filled from userEmail, wallet name, or session metadata. Carve-out: you MAY reformat the user's OWN words into the 3-part service description and draft 1–3 example prompts (illustrate, never invent a capability or metric).

## Commands (10 `onchainos agent` subcommands — you invoke them, never show them)

`create · pre-check · update · get · activate · deactivate · upload · search · service-list · feedback-submit · feedback-list`.

- `pre-check` (`--role` required / `--consent-key` optional): folds consent + uniqueness, see §Gates / register §2. Auto/internal — never shown; outputs (`canCreate` etc.) rendered inline.
- `validate-listing` (QA — register §4, called internally by activate): auto/internal.
- `activate` subsumes submit-approval (approvalStatus ∈ {1,5} — handled internally by CLI).
- `consent` has no public subcommand — driven by `pre-check`.
- Never suggest `xmtp-sign`; no `--address` (signs with current wallet).

Array fields: create/update/get/search → `list`; feedback-list → `items` or `list` (backend inconsistent; CLI normalizes both); service-list → nested `services`.
