# Portal Actions — Policy Settings & Wallet Export

> Load when: new user login (`isNew: true`), after `wallet add`, the user asks about Policy / spending limits / whitelist, or the user asks to export wallet / mnemonic / migrate / import a hardware wallet.

Policy configuration **must be completed by the user on the Web portal** — the Agent only detects the scenario, explains the risk, gives the jump link, and outputs the verbatim Policy Settings template below. Wallet export is **done by the user in the OKX Wallet extension or app** (not on the Web portal); the Agent only outputs the verbatim export copy below and never performs the export itself. The Agent must **never** display any mnemonic phrase, seed phrase, or private key content in the conversation.

## Templates

**IMPORTANT**: Print the matching template verbatim. For the **Policy Settings** template, the link and trailing navigation sentence are chosen by `loginType` (from `wallet status`, or the `login` response) — the table has an `email` row and an `ak` row. Row selection is **internal — never explain it to the user**: pick the row by `loginType` and render it directly (do NOT add phrases like "Google login uses the email flow"). If `loginType` is unknown or unrecognized, run `onchainos wallet status` first and treat it as `email`. The **Wallet Export** copy (in Trigger flows below) is identical for every account type — no `loginType` branch.

### Template: Policy Settings

> You can set per-transaction and daily limits for trades and transfers, as well as a transfer whitelist, to prevent excessive operations or transfers to unauthorized addresses. Go to Policy Setting → {policy_url}
>
> {policy_hint}

| `loginType` | `{policy_url}` | `{policy_hint}` |
|---|---|---|
| `email` | `https://web3.okx.com/portfolio/agentic-wallet-policy` | Log in to your Agentic Wallet, then hover over your profile in the top-right corner and select "Policy Setting" from the dropdown menu. |
| `ak` | `https://web3.okx.com/onchainos/dev-portal` | Log in with the EOA wallet that created the Agentic Wallet and open the OKX Web3 Dev platform, and click on the Agentic Wallet - Policy Setting in the upper right corner to set security rules. |

## Available Policy Rules

Policy **only** includes the following rules. Do NOT invent or mention any rules beyond this list (e.g., no "transaction count limit", no "gas limit", no "token blacklist"):

| Rule | Description | Field (from `wallet status`) |
|---|---|---|
| Per-transaction limit | Max USD amount per single transaction or transfer | `singleTxLimit` / `singleTxFlag` |
| Daily transfer limit | Max USD amount for transfers per day (resets at UTC 0:00) | `dailyTransferTxLimit` / `dailyTransferTxFlag` / `dailyTransferTxUsed` |
| Daily trade limit | Max USD amount for trades (swaps) per day (resets at UTC 0:00) | `dailyTradeTxLimit` / `dailyTradeTxFlag` / `dailyTradeTxUsed` |
| Transfer whitelist | Only allow transfers to pre-approved addresses | Configured on Web portal only |

## Trigger flows

The following are **trigger conditions** — when any is met, the Agent **MUST** output the corresponding guidance. Do not skip or omit.

### New user login (`isNew: true`)

Handled in [Wallet Authentication step 2](wallet.md) — when `isNew: true`, output the **Policy Settings template** (above), regardless of `loginType`.

### New account via `wallet add`

After a successful `wallet add`, **MUST** output the **Policy Settings template** (above), prefixed with a short line such as "New account created.".

### User asks about Policy

e.g., "How do I set a spending limit?", "What's my daily limit?", "How to configure whitelist?"
- Run `onchainos wallet status` and check the `policy` field.
- If any flag is true, first display the current settings (limits, used amounts).
- Then output the **Policy Settings template** (above).

### User asks about wallet export

e.g., "How do I export my mnemonic?", "I want to migrate my wallet", "How do I import my wallet into a hardware wallet?"

Output the following copy **verbatim** as the complete response, **identical for every account type** (`email`, `ak`, `google`, `apple`, or empty/unknown) — there is no account-type branch and no "not supported" case, so do NOT run `wallet status` to route on `loginType` here:

> Export your seed phrase in the OKX Wallet extension or app.
>
> Please note: After export, your wallet will be permanently unlinked from your social account, and the Agent will no longer be able to operate it.
>
> Before exporting, move your assets to a secure address and stop any active tasks. After exporting, back up your seed phrase securely and never share it with anyone.
