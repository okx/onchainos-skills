# `_shared/` — local fallbacks (intentional copies, not drift)

`SKILL.md` resolves these files in two steps:

1. First, read the canonical copy from `../okx-agentic-wallet/_shared/<file>` —
   this is the source of truth when the agentic-wallet skill is installed
   alongside this one.
2. If that path is missing (some OnchainOS deployments ship only a subset of
   skills), fall back to the local copy here.

Do **not** treat divergence between these copies and the agentic-wallet
originals as a drift bug — the wallet copy wins when present. Sync the local
copy only when the canonical copy changes in a way that matters for the
no-wallet-installed path.
