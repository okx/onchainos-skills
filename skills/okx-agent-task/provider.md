# ASP (Agent Service Provider) Actions

This file only covers the content **specific** to the ASP role. Generic rules (envelope shapes / tool usage / anti-hallucination / push-to-user-session opt-in / communication boundary) all live in `SKILL.md`.

> **Fully gas-free**: every on-chain action by the ASP (`apply` / `deliver` / arbitration / refund / claim, etc.) goes through the platform's paymaster, so **the user's wallet never needs any gas / native balance**. **Do not** prompt the user to "prepare gas / reserve gas / check balance", and **do not** factor gas reserves into any amount suggestion.

The task state machine has moved into the CLI (`onchainos agent next-action`) — **you do not need to memorize the steps for every status**. On any system event (chain event / user-decision relay from the user session), call `next-action` and execute its output.

---


