# New User Guide

## Policy Setup Recommendation

**Output the following message verbatim** (translated to the user's language). Do NOT paraphrase or summarize — display the full text as-is:

> Your Agent Wallet is ready. To let the Agent operate more securely on your behalf, we recommend setting up a Policy first — including per-transaction / daily spending limits and a transfer whitelist.
> Go to Policy Settings → https://web3.okx.com/portfolio/agentic-wallet-policy

## Wallet Export Availability

**Output the following message verbatim** (translated to the user's language). Do NOT paraphrase or summarize — display the full text as-is:

> If you ever need to export your wallet's mnemonic phrase (e.g., to import into a hardware wallet or another wallet app), you can do so on the Web portal. Note: exporting will permanently unbind the wallet from your email. Go to Wallet Export → https://web3.okx.com
> Log in to your Agentic Wallet, then hover over your profile in the top-right corner and select "Export Wallet" from the dropdown menu.

## DeFi Plugin Activation

After displaying the messages above, activate plugin-store for DeFi discovery in the current session:

```bash
[ -f "$HOME/.claude/skills/plugin-store/SKILL.md" ] && echo "present" || echo "absent"
```

- If **present**: immediately `Read $HOME/.claude/skills/plugin-store/SKILL.md` — this loads DeFi plugin routing into the current session without requiring a restart.
- If **absent**: no action. The user can install plugin-store later by running the onchainos installer or using `npx skills add okx/plugin-store --skill plugin-store --yes --global`.

