---
name: okx-ai-guide
description: "OKX.AI (the Agent economic system) intro & onboarding entry. Trigger when the user asks about OKX.AI — 'what is OKX.AI' / 'OKX.AI 是什么' / '什么是 OKX.AI' / '介绍一下 OKX.AI'; 'what can OKX.AI do' / 'OKX.AI 能做什么' / '有什么用' / 'OKX.AI 功能'; 'how to use OKX.AI' / 'OKX.AI 怎么用' / '如何使用 OKX.AI' / '操作方法'; 'how to start' / '怎么开始用 OKX.AI' / '如何入门' / 'OKX.AI 新手' / '怎么注册 OKX.AI'; 'OKX.AI help' / 'OKX.AI 不会用' / 'OKX.AI 帮助' / '求助'; 'OKX.AI tutorial' / 'OKX.AI 教程' / '新手教程' / '入门指南' / '教我用 OKX.AI'; the literal phrase 'OKX.AI 快速开始' / 'OKX.AI quick start' / 'OKX.AI quickstart'; or a handoff from the Onchain OS welcome banner pick 'see how OKX.AI works' / '看看 OKX.AI 怎么玩'. Also matches spelling / spacing / casing / typo variants of the name itself — 'OKXAI' / 'okxai' / 'OKX AI' / 'okx ai' / 'okx-ai' / lowercase 'okx.ai', and colloquial or mis-typed Chinese forms such as '什么okxai' / '什么是okxai' / '啥是okxai' / '什么事okxai' / 'okxai是什么'. Detects the runtime platform, shows the OKX.AI intro + three roles (User / ASP / Evaluator), and routes the user into identity registration. NOT for: generic onchainOS onboarding (use okx-how-to-play), or direct task ops like publish/accept/deliver/dispute (use okx-agent-task)."
license: Apache-2.0
metadata:
  author: okx
  version: "3.4.3-beta"
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

This skill owns ONLY: OKX.AI intro + platform detection + routing into registration. It does NOT:

- own the Onchain OS welcome banner — that is `okx-how-to-play`.
- implement registration — delegated to `okx-agent-identity` (see §Step 2).
- check wallet login — the registration playbooks run their own login preflight.

<NEVER>
Do NOT call `onchainos agent create` (or any registration / staking CLI) from this skill. Registration is always delegated to `okx-agent-identity`.
</NEVER>

## Step 0 — Platform detection

<MUST>
Run the detection function below and read its single-line output. `compatible` = output is NOT `unknown`.
</MUST>

```bash
detect_harness() {
  if [ -n "${HERMES_INTERACTIVE:-}" ] || [ -n "${HERMES_SESSION_SOURCE:-}" ] \
    || [ -n "${HERMES_YOLO_MODE:-}" ] || [ -n "${HERMES_QUIET:-}" ]; then
    echo "Hermes"
  elif [ -n "${OPENCLAW_CLI:-}" ] || [ -n "${OPENCLAW_SHELL:-}" ]; then
    echo "OpenClaw"
  else
    echo "unknown"
  fi
}
detect_harness
```

- Output ∈ {`OpenClaw`, `Hermes`} → **compatible** → Step 1A.
- Output = `unknown` → **incompatible** → Step 1B.

## Step 1A — Compatible: role selection page

**Free zone (1–5 sentences, agent's own words):** answer whatever the user actually asked about OKX.AI, then segue naturally into the menu.

**Fixed zone:** render **Variant A** from [`references/intro.md`](./references/intro.md) in the user's language; substitute `{okx_ai_site}`. Then **stop and wait** for the user to reply `1` / `2` / `3`.

## Step 1B — Incompatible: intro + install guide

**Free zone (1–5 sentences):** answer the user's OKX.AI question, then segue.

**Fixed zone:** render **Variant B** from [`references/intro.md`](./references/intro.md) in the user's language; substitute `{install_doc_url}` per its locale rule. Do **not** offer numbered picks; end the turn.

## Step 2 — Routing (compatible branch only)

When the user replies `1` / `2` / `3`:

<MUST>
Render the matching wait-state line from [`references/intro.md`](./references/intro.md), then load the registration playbook below and follow it to completion.
</MUST>

The wait-state lines live in [`references/intro.md`](./references/intro.md) (authoritative — render that exact text, do not retype a variant here):

| Pick | Wait-state line (from `intro.md`) | Then load |
|---|---|---|
| `1` (User) | `Registering your User identity, hang tight... ⏳` | [`../okx-agent-identity/references/role-requester.md`](../okx-agent-identity/references/role-requester.md) |
| `2` (ASP) | `Registering your ASP identity, hang tight... ⏳` | [`../okx-agent-identity/references/role-provider.md`](../okx-agent-identity/references/role-provider.md) |
| `3` (Evaluator) | `Registering your Evaluator identity, hang tight... ⏳` | [`../okx-agent-identity/references/role-evaluator.md`](../okx-agent-identity/references/role-evaluator.md) (→ then evaluator staking, owned by that flow) |

<MUST>
If the user's reply is NOT exactly `1` / `2` / `3`: map an unambiguous role word to its number (`user` / `用户` → 1; `ASP` / `服务商` → 2; `evaluator` / `仲裁者` / `arbiter` → 3). If it is still ambiguous, empty, multiple roles, or unrelated, re-render the three options from Variant A and ask the user to reply `1` / `2` / `3`. NEVER guess a role or invent a fourth path.
</MUST>

Login + consent + post-success comm-init are handled inside the registration playbook. This skill does not duplicate them.

## Acceptance Criteria

1. `detect_harness` returns the right platform for each marker set; everything else → `unknown` → incompatible branch.
2. Compatible branch shows the role page; replying 1/2/3 renders the right wait-state and loads the right registration playbook.
3. Incompatible branch shows the three-role intro (no picks) + install heads-up + `{install_doc_url}`.
4. `OKX.AI 快速开始` / `OKX.AI quick start` triggers this skill.
5. Fixed-zone copy renders in the user's language; emojis / numbers / URLs / placeholders stay literal.
6. Zero `onchainos agent create` calls in this skill; zero Rust changes.
