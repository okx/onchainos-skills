---
name: okx-guide
description: "Guide users through installing or updating Onchain OS, getting started, OKX.AI introduction and role-registration routing, and customer support. Use when someone wants to install or update the Onchain OS CLI or skills, is new or asks what Onchain OS or OKX.AI is, wants a quick start, needs to register a User, ASP, or Evaluator role, or asks for Help Center, support, feedback, or FAQs."
license: MIT
metadata:
  author: okx
  version: "4.5.2"
  homepage: "https://web3.okx.com"
---

# Onchain OS Guide Hub

Route install/update, onboarding, OKX.AI introduction, and customer-support requests to one reference flow.

## Pre-flight Checks

At the start of each thread, complete the checks in `../okx-agentic-wallet/_shared/preflight.md`. If missing, read `_shared/preflight.md`. That is the routine (throttled) session-start check; an explicit install/update intent uses the **forced** flow in row 1 below instead.

## Intent Routing

Apply the first matching row, read that reference before responding, and follow it to completion.

| Priority | User signal | Reference |
|---|---|---|
| 1 | Intent to **install / update / upgrade / reinstall** the Onchain OS CLI or skills — a request to perform the maintenance, not "I just installed, now what?" | [install-update.md](references/install-update.md) |
| 2 | Human support, customer service, complaint, feedback, bug/system error, Help Center, FAQ, or user guide | [ai-support.md](references/ai-support.md) |
| 3 | Explicit OKX.AI subject or spelling variant, quick start, platform compatibility, or User/ASP/Evaluator registration | [ai-guide.md](references/ai-guide.md) |
| 4 | Generic Onchain OS introduction, first use, tutorial, getting started, “what can it do?”, or “where do I start?” | [how-to-play.md](references/how-to-play.md) |
