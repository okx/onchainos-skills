---
name: okx-ai-guide
description: "OKX.AI (the Agent economic system) intro & onboarding entry. Use whenever the user asks what OKX.AI is, what it can do, how to use or get started with it, wants an OKX.AI tutorial / quickstart / help, or types the product name in any spelling / spacing / casing / typo variant (OKXAI, okx ai, okx-ai, lowercase okx.ai, mis-typed Chinese like 啥是okxai) — e.g. what is OKX.AI / OKX.AI 是什么 / 怎么用 OKX.AI / OKX.AI 快速开始, and any paraphrase in any language. Detects the runtime platform, introduces the three roles (User / ASP / Evaluator), and routes the user into identity registration. NOT for generic onchain-OS onboarding, nor for direct task operations (publishing / accepting / delivering / disputing a task — a separate task-lifecycle flow)."
license: Apache-2.0
metadata:
  author: okx
  version: "3.20.1-beta"
  homepage: "https://web3.okx.com"
---

# OKX.AI Guide

The OKX.AI onboarding entry. Introduces OKX.AI (the Agent economic system), detects whether the current runtime can run OKX.AI, and routes the user into one of the three identity-registration flows — or, on an incompatible platform, tells them how to get a compatible one.

## Instruction Priority

Tagged blocks indicate rule severity (higher wins on conflict):

1. **`<NEVER>`** — Absolute prohibition.
2. **`<MUST>`** — Mandatory step.
3. **`<SHOULD>`** — Best practice.

## Scope & Boundary

This skill owns: OKX.AI intro + platform detection + login & identity detection (new vs returning user) + routing into registration. It does NOT:

- own the Onchain OS welcome banner — that is `okx-how-to-play`.
- implement registration — delegated to `okx-agent-identity` (see §Step 5).
- own the wallet-login flow — Step 1 only *checks* login via `wallet status` and hands off to `okx-agentic-wallet`'s existing login flow when needed; the registration playbooks also run their own preflight.

<NEVER>
Do NOT call `onchainos agent create` (or any registration / staking CLI) from this skill. Registration is always delegated to `okx-agent-identity`. (Read-only `onchainos wallet status` and `onchainos agent get` in Step 1 are allowed — they create nothing.)
</NEVER>

## Step 0 — Platform detection

<MUST>
Run the detection function below and read its single-line output. `compatible` = output is NOT `unknown`.
</MUST>

```bash
detect_harness() {
  if [ "${CLAUDECODE:-}" = "1" ]; then
    echo "Claude Code"
  elif [ -n "${HERMES_INTERACTIVE:-}" ] || [ -n "${HERMES_SESSION_SOURCE:-}" ] \
    || [ -n "${HERMES_YOLO_MODE:-}" ] || [ -n "${HERMES_QUIET:-}" ]; then
    echo "Hermes"
  elif [ -n "${OPENCLAW_CLI:-}" ] || [ -n "${OPENCLAW_SHELL:-}" ]; then
    echo "OpenClaw"
  elif [ -n "${CODEX_THREAD_ID:-}" ] || [ -n "${CODEX_CI:-}" ]; then
    echo "Codex"
  else
    echo "unknown"
  fi
}
detect_harness
```

- Output ∈ {`Claude Code`, `Hermes`, `OpenClaw`, `Codex`} → **compatible** → Step 1.
- Output = `unknown` → **incompatible** → Step 3.

## Step 1 — Compatible: login + identity detection (routing gate)

Reached only when Step 0 is **compatible**. This step decides which page to show — by checking login **first**, identity **second**. The order is mandatory: `agent get` requires a logged-in session, so never query identity before login is confirmed.

<MUST>
1. **Login check** — run `onchainos wallet status` and read `loggedIn`.
   - `loggedIn: false` → user is not logged in. Do **not** query identity. Hand off to the existing wallet-login flow ([`../okx-agentic-wallet/SKILL.md`](../okx-agentic-wallet/SKILL.md) §login): prompt login, and on success resume here (re-run `wallet status`, then do the identity check).
   - `loggedIn: true` → continue to the identity check.
2. **Identity check** — run `onchainos agent get` (no `--agent-ids`). It returns the logged-in user's own OKX.AI agents on XLayer (identified via JWT).
   - **Empty** (no agents) → user has no OKX.AI identity → **Step 2** (role selection page).
   - **≥1 agent** → user already has an identity → **Step 4** (registered user home).
</MUST>

The branch is decided **solely** by whether `agent get` returns any agent — never show the role page (Step 2) to a user who already has an identity, nor the registered home (Step 4) to a user with none.

## Step 2 — Compatible & unregistered: role selection page

Reached from Step 1 when the user is logged in but has **no** OKX.AI identity. Render the role-selection page (Variant A) and route the `1`/`2`/`3` reply per [`references/unregistered-role-selection.md`](./references/unregistered-role-selection.md) (contains Step 2 page + Step 5 routing). Load it when this branch is hit.

## Step 3 — Incompatible: intro + install guide

Reached from Step 0 when the platform is **incompatible** (`unknown`). No login / identity check applies — OKX.AI cannot run here.

**Free zone (1–5 sentences):** answer the user's OKX.AI question, then segue.

**Fixed zone:** render **Variant B** from [`references/intro.md`](./references/intro.md) in the user's language; substitute `{install_doc_url}`. Do **not** offer numbered picks; end the turn.

## Step 4 — Compatible & registered: user home

Reached from Step 1 when the user is logged in and already has **≥1** OKX.AI identity. Render the registered-user home (Variant C, filled field-exact from the `agent get` result) and handle its menu replies (Step 6: `1` + Agent ID → that Agent's current tasks; `2` → top ASPs; `Register a <role>` → register a missing role) per [`references/registered-home.md`](./references/registered-home.md). Load it when this branch is hit.

## Step 5 — Routing after role pick

Handled in [`references/unregistered-role-selection.md`](./references/unregistered-role-selection.md) alongside Step 2 (the `1`/`2`/`3` reply → wait-state line + registration playbook).
