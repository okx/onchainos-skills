---
name: okx-activity
description: "OKX activity hub — registration & participation for OKX.AI / Onchain OS campaign activities, routed internally to the matching activity file under `references/`. Currently covers the OKX.AI Trading Hackathon (交易黑客松 / OKX.AI 交易黑客松 / 报名黑客松 / 黑客马拉松): use when the user wants to register for / sign up for / enter / join / 报名 / 参加 / 参赛 the OKX.AI trading hackathon — e.g. 'Register me for the OKX.AI Trading Hackathon', '我要参加黑客松', '帮我报名 OKX.AI 的交易黑客松' — or asks about its entry requirements / eligibility / 报名条件 / 参赛资格 / how to enter. The hackathon enters a Trading ASP the user already has; it never creates one. NOT for standard trading competitions / cups / 交易大赛 / leaderboards / prize claiming — those are okx-growth-competition."
license: MIT
metadata:
  author: okx
  version: "4.4.4"
  homepage: "https://web3.okx.com"
---

# OKX Activity

Single entry point for OKX activities (campaign events the user can enter). Each activity's own
content — flow, CLI/MCP reference, FAQ, gates — lives in this skill's `references/` behind an
`<activity>-*.md` prefix. This SKILL.md **routes only**: it holds no templates, no CLI flags, and no
per-activity copy, so adding an activity never changes the rules of an existing one.

## Activity Routing (do this FIRST, before loading any reference)

| Activity | Triggers | Load |
|---|---|---|
| **OKX.AI Trading Hackathon** — enters an existing Trading ASP | hackathon · 黑客松 · 交易黑客松 · 黑客马拉松 · 报名黑客松 · its entry requirements / 报名条件 / 参赛资格 | [`references/hackathon-core.md`](references/hackathon-core.md) — read it FIRST; it owns the hackathon's gates and routes on to `references/hackathon-registration.md` / `references/hackathon-faq.md` |

**Before producing ANY user-facing message about an activity, that activity's `*-core.md` must be
loaded** (**BLOCKING**). It carries that activity's gates, output rules, and send-gate — the flow and
FAQ files do **not** repeat them, so reading a `-registration.md` / `-faq.md` first is not a
shortcut, it is a skipped gate. Do not improvise a flow, template, or eligibility answer from this
file, from memory, or from the CLI's `--help` output alone.

If the request names an activity with **no row above** (no reference file exists for it), say that
activity isn't supported by this skill yet — never adapt another activity's flow to it, and never
guess a CLI subcommand for it.

## Wrong-skill guard (applies to every activity)

An activity here enters the **subject that activity defines** (hackathon → an existing Trading ASP
agent). `competition join` (`okx-growth-competition`) signs the **wallet account** up for a standard
trading competition. Different systems, different subjects — **NEVER** substitute one for the other.

If one request carries signals for **both** (e.g. names "hackathon"/"黑客松" *and*
"competition"/"大赛"/"cup"), ask which the user means before running any command.

Creating an agent identity is never part of an activity flow — that is `okx-ai`. This skill only
enters subjects that already exist.

## Pre-flight

> Read `../okx-agentic-wallet/_shared/preflight.md`. If that file does not exist, read
> `_shared/preflight.md` instead.

**BLOCKING** — runs before the first `onchainos` command of the session, for every activity, a
read-only listing call included.

## Learn the CLI from the CLI

Each activity maps to its own `onchainos` subcommand group (hackathon → `onchainos hackathon`).
**Learn exact syntax from the CLI, not from memory:** run `onchainos <group> --help` for the
subcommand list and `onchainos <group> <sub> --help` for its flags. The authoritative flag table,
return fields, and CLI-side error list for an activity live in that activity's reference files.

## Cross-activity rules (hold in addition to each activity's own Output Rules)

- **Identify an activity by name, never by its internal activity id**, in any format. The CLI does
  not return that id; do not source it from anywhere else.
- **Reply in the user's language.** Every template in this skill's references is authored in English
  as a *structure guide* — translate it before sending, keeping the layout and fields unchanged and
  every URL byte-for-byte, still a link.
- **The JWT is injected by the client layer from the keychain.** NEVER log it, print it, or pass it
  in a flag. Activity flows create no new secrets.
- **User identifiers** (OKX UID and the like) are masked when echoing the executed command, and are
  never pasted raw into the conversation.

## Adding an activity (maintainers)

1. Add `references/<activity>-core.md` — its gates, reading order, output rules, and pre-delivery
   checklist — plus `references/<activity>-*.md` for the flow and FAQ.
2. Add one row to §Activity Routing pointing at that `-core.md`.
3. Extend this file's `description` with the new activity's triggers.

Nothing else in this SKILL.md is per-activity; leave the other sections untouched.
