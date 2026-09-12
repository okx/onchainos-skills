# Project guidance

## Command discipline

- Before running `onchainos`, read the matched skill's `SKILL.md`; never guess
  command syntax or supported flags.
- Report a CLI command only after it actually ran. Use CLI output for live data,
  never skill text or model memory.
- Do not use the retired `onchainos preflight` command. Installation, update,
  and reinstall use `npx -y @okxweb3/onchainos-installer install` (use `--beta` when needed).

## Route before acting

- For token research, market overview, smart-money, new-token scanning, wallet
  analysis, portfolio, or wallet monitoring, read `workflows/INDEX.md` first.
  For Chinese requests, also read `workflows/references/keyword-glossary.md`.
- Wallet actions and security checks → `okx-agentic-wallet`.
- Named third-party DApps → `okx-dapp-discovery`.
- Agent identity, task lifecycle, marketplace subscriptions, and task monitoring
  → `okx-ai`.
- HTTP 402, x402, MPP, and payment IDs → `okx-agent-payments-protocol`.
- DeFi yield, protocol positions, deposits, withdrawals, and rewards → `okx-defi`.
- OnchainOS installation, onboarding, OKX.AI introduction, and support →
  `okx-guide`.

If the request can plausibly match more than one skill, read the candidate
`SKILL.md` files and use their routing rules. The skills own detailed capability
lists, aliases, command indexes, and tie-breakers; do not duplicate them here.