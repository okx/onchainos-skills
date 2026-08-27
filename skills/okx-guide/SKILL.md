---
name: okx-guide
description: "Guide users through Onchain OS onboarding, OKX.AI introduction and role-registration routing, and customer support. Use when someone is new, asks what Onchain OS or OKX.AI is, wants a quick start or tutorial, needs to register a User, ASP, or Evaluator role, or asks for Help Center, human support, complaints, feedback, FAQs, or bug-report guidance."
license: MIT
metadata:
  author: okx
  version: "4.8.1-beta"
  homepage: "https://web3.okx.com"
---

# Onchain OS Guide Hub

Route onboarding, OKX.AI introduction, and customer-support requests to one existing reference flow.

## Pre-flight Checks

At the start of each thread, complete the checks in `../okx-agentic-wallet/_shared/preflight.md`. If missing, read `_shared/preflight.md`.

## Intent Routing

Apply the first matching row, read that reference before responding, and follow it to completion.

| Priority | User signal | Reference |
|---|---|---|
| 1 | Human support, customer service, complaint, feedback, bug/system error, Help Center, FAQ, or user guide | [ai-support.md](references/ai-support.md) |
| 2 | Explicit OKX.AI subject or spelling variant, quick start, platform compatibility, or User/ASP/Evaluator registration | [ai-guide.md](references/ai-guide.md) |
| 3 | Generic Onchain OS introduction, first use, tutorial, getting started, “what can it do?”, or “where do I start?” | [how-to-play.md](references/how-to-play.md) |
