# Claude Code Guidance

## Command discipline

- Before running `onchainos`, read the matching skill's `SKILL.md`; do not
  guess command syntax.
- After a CLI command actually runs, show the command that ran. Do not emit a
  placeholder for conversational-only steps.
- Use CLI results for live data; do not substitute skill text or model memory.

## Workflow routing

Read `workflows/INDEX.md` before responding to token research, market overview,
smart-money, new-token scanning, wallet analysis, portfolio, or wallet-monitor
requests. For Chinese requests, also read `workflows/references/keyword-glossary.md`.

Follow the risk controls in `okx-agentic-wallet`. Use `--format` only when the
command documents it.

## Intent boundaries

- OnchainOS installation, update, or reinstall → `okx-guide`, which uses
  `npx -y oc-onchainos install` (`--beta` for beta).
- Named third-party DApp → `okx-dapp-discovery`.
- Agent identity, task lifecycle, task monitoring, or agent-marketplace
  subscriptions → `okx-ai`.
- HTTP 402, x402, MPP, or paymentId → `okx-agent-payments-protocol`.
- Wallet operations, swaps, bridges, Gateway, Gas Station, and safety checks →
  `okx-agentic-wallet`.

For automation, build on documented `onchainos` commands and route to the
matching skill; do not invent or search for separate OKX API command paths.
