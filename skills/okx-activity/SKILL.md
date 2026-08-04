---
name: okx-activity
description: "OKX.AI Trading Hackathon (交易黑客松 / OKX.AI 交易黑客松 / 报名黑客松 / 黑客马拉松) registration. Use when the user wants to register for / sign up for / enter / join / 报名 / 参加 / 参赛 the OKX.AI trading hackathon — e.g. 'Register me for the OKX.AI Trading Hackathon', '我要参加黑客松', '帮我报名 OKX.AI 的交易黑客松' — or asks about its entry requirements / eligibility / 报名条件 / 参赛资格 / how to enter. The object is always the hackathon itself: it enters a Trading ASP the user already has and never creates one. Each activity carries its own flow, gates, and CLI reference under this skill's references/, picked by the Intent Routing table."
license: MIT
metadata:
  author: okx
  version: "4.4.5"
  homepage: "https://web3.okx.com"
---

# OKX Activity — registration & participation for OKX campaign activities

Each activity's flow, CLI reference, FAQ, and gates live in this skill's `references/` behind an
`<activity>-*.md` prefix. This SKILL.md routes only — no templates, no CLI flags, no per-activity
copy — so adding an activity never changes the rules of an existing one.

## Pre-flight Checks

Before your first onchainos command, read `../okx-agentic-wallet/_shared/preflight.md` once. If it does not exist, read `_shared/preflight.md` instead.

**BLOCKING** — it runs before the first `onchainos` command of the session, for every activity, a
read-only listing call included.

## Intent Routing

| User Intent | Activity | Reference |
|---|---|---|
| "register / sign up my ASP for the hackathon" · 报名黑客松 · 参加交易黑客松 · 黑客马拉松 | OKX.AI Trading Hackathon | [hackathon-core.md](references/hackathon-core.md), then [hackathon-registration.md](references/hackathon-registration.md) |
| hackathon entry requirements / eligibility / 报名条件 / 参赛资格 — not mid-registration | OKX.AI Trading Hackathon | [hackathon-core.md](references/hackathon-core.md), then [hackathon-faq.md](references/hackathon-faq.md) |

**Before producing ANY user-facing message about an activity, that activity's `*-core.md` must be
loaded** (**BLOCKING**). It carries that activity's gates, output rules, and send-gate — the flow and
FAQ files do **not** repeat them, so opening a `-registration.md` / `-faq.md` first is not a
shortcut, it is a skipped gate. Do not improvise a flow, template, or eligibility answer from this
file, from memory, or from the CLI's `--help` output alone.

If the request names an activity with **no row above** (no reference file exists for it), say that
activity isn't supported by this skill yet — never adapt another activity's flow to it, and never
guess a CLI subcommand for it.

## Command Index

This skill drives `onchainos <activity>` subcommands (hackathon → `onchainos hackathon`). **Learn
exact syntax from the CLI, not from memory:** run `onchainos hackathon --help` for the subcommand
list, and `onchainos hackathon <subcommand> --help` for a subcommand's flags. Full parameter tables,
return-field schemas, and worked examples live in
[hackathon-registration.md](references/hackathon-registration.md).

## Wrong-skill guard

An activity here enters the **subject that activity defines** (hackathon → an existing Trading ASP
agent). `competition join` (`okx-growth-competition`) signs the **wallet account** up for a standard
trading competition. Different systems, different subjects — **NEVER**: substitute one for the other,
because the two register different things and neither call can be undone.

If one request carries signals for **both** (e.g. names "hackathon"/"黑客松" *and*
"competition"/"大赛"/"cup"), ask which the user means before running any command.

## Security

- **Every activity registration is irreversible** — there is no list, update, status, or undo
  subcommand. **MUST**: hold an explicit confirm reply from the user before submitting; never answer
  the confirmation prompt on their behalf, and never treat a one-shot request that already named the
  subject as having pre-answered it. The full gate lives in the activity's `-registration.md`.
- **NEVER**: log, print, or pass the JWT in a flag — it is injected by the client layer from the
  keychain, so leaking it would let an attacker act as the user. Activity flows create no new secrets.
- **MUST**: mask user identifiers (OKX UID and the like) when echoing the executed command
  (`--uid <hidden>`) — they are never returned by the CLI and never pasted raw into the conversation.
- **NEVER**: identify an activity by its internal activity id, in any format — name it. The CLI does
  not return that id; do not source it from anywhere else.
- Creating an agent identity is never part of an activity flow — that is `okx-ai`. This skill only
  enters subjects that already exist.

## Global Notes

- **Reply in the user's language.** Every template in this skill's references is authored in English
  as a *structure guide* — translate it before sending, keeping the layout and fields unchanged and
  every URL byte-for-byte, still a link.
- Each activity's `-core.md` owns its own gates, Output Rules, and Pre-Delivery Checklist; those are
  additional to this file, never a substitute for it.

### Adding an activity (maintainers)

1. Add `references/<activity>-core.md` — its gates, reading order, output rules, and pre-delivery
   checklist — plus `references/<activity>-*.md` for the flow and FAQ.
2. Add one Intent Routing row per user intent that activity serves, each a complete markdown link.
3. Extend this file's `description` with the new activity's triggers.
