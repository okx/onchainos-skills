# Wallet

Wallet lifecycle: authentication, balance, addresses, token transfers, transaction history, contract calls, and message signing. Shared Confirming / display / security policy is in SKILL.md.

## Authentication

Run `wallet balance`, `wallet send`, `wallet contract-call`, `wallet history`, and `wallet sign-message` directly. If a command reports that login is required, follow the login flow below.

1. **Log in** — orchestrate `init` → auto-poll:
   a. **Get the link.** Run `wallet login --phase init` — it returns `{ loginUrl, authSessionId, opened, nextSteps }` immediately and best-effort opens the browser. `nextSteps.completeLogin` is the exact poll command with `authSessionId` interpolated; when `opened == false`, `nextSteps.openLoginUrl` (equal to `loginUrl`) is the URL to open first. Keep `authSessionId` for the poll.
   b. **Show the link + reminder** (translate to the user's language; keep the structure, substitute `authSessionId` and `loginUrl`):
      > Your login link is ready — I'll open it in your browser.
      > • Session ID (session_id): `<authSessionId>`
      > • Login link (if the browser didn't open, click to open it manually): `<loginUrl>`
      >
      > Fetching the login result will block your other operations for up to 5 minutes.
   c. **Auto-poll.** Immediately run `wallet login --phase poll --session-id <authSessionId>` (the id from step a) — don't wait for the user. On timeout / no result, tell the user you couldn't get it yet: finish login on the already-open page and tell you to re-check (same id), or start over from `--phase init` (new id); don't guess whether a previous session is still valid.
2. **After login.** After a successful `poll`, run `wallet status`, then render the Account Info template (below) from the `poll` response. If the response has `"isNew": true`, output the Policy Settings template then the Wallet Export template ([portal-actions.md](wallet-portal-actions.md)); if `false`, skip. Before returning, the CLI always sends the device-registration heartbeat. When the pre-registration probe safely proves this is a new device, the CLI adds it to every explicit subscription receive list using environment-scoped durable progress and only then produces the mandatory post-login snapshot. An existing or unclassifiable device is never automatically re-enabled. When `data.postLoginSubscriptions` is present, render it per §Post-login subscription display in [task-user-playbook.md](../../okx-ai/references/task-user-playbook.md); when absent, render nothing OKX.AI-related. **Never run a separate `my-subscriptions` or `device-list` command after a successful poll.**

Login creates the first account automatically — never call `wallet add` for it. Use `wallet add` only when already logged in and the user explicitly wants another account (then output the Policy Settings template, see [portal-actions.md](wallet-portal-actions.md)).

### Template: Account Info (login success)

Render verbatim from the `wallet login --phase poll` response `data`:

> **Account Info**
> - Login method: {method}{ ({email}) }
> - Current account: OKX Wallet - {accountName} ({accountCount} accounts total)
> - Total assets: ${totalValueUsd}
>
> **Addresses**
> - EVM: {evmAddress}
> - Solana: {solAddress}

Field rules:
- `{method}` ← `loginType`: `email`→"Email", `google`→"Google", `apple`→"Apple", `ak`→"API Key".
- Append ` ({email})` only if `email` is non-empty; otherwise omit the parentheses.
- Omit the "Total assets" line if `totalValueUsd` is empty; omit an address line if its value (`evmAddress` / `solAddress`) is empty.

## Parameter Rules

**`--chain`** accepts numeric IDs (`1`, `501`, `196`) and names (`ethereum`, `solana`, `xlayer`). If <100% confident, run `wallet chains`. On `"unsupported chain: ..."`, ask the user to confirm.

**Amounts** — `wallet send`: pass `--readable-amount <human_amount>` (CLI converts; use `--amt` only for raw minimal units). `wallet contract-call`: `--amt` is the native value for payable functions in minimal units (default `"0"`; EVM 18, SOL 9 decimals). Never compute minimal units manually.

**Native BTC fee rate** — After the initial transfer preview, ask the user to confirm the current fee rate. If they provide a new sat/vB value, rerun the initial command with `--fee-rate <value>`. Show the fresh preview, remind them that the custom fee rate applies only to that transaction, and wait for confirmation.

**Bitcoin UTXOs and BRC-20** — For BTC UTXO management, load [utxo-cli-reference.md](utxo-cli-reference.md). For BRC-20 management, load [brc20-cli-reference.md](brc20-cli-reference.md). To query a BRC-20 ticker balance, run `onchainos wallet balance --chain bitcoin --token-address <btc-brc20-ticker>` and use that reference's reply template.

## Send vs Contract Call (funds-loss risk — determine intent first)

| Intent | Command |
|---|---|
| Token transfer | `wallet send --chain <chain>` |
| Contract call | `wallet contract-call --chain <chain>` |

For a SUI contract call, provide the unsigned PTB from the maintained integration or SDK with `--sui-tx-bytes`.

## Approvals (via contract-call)

Never execute unlimited approvals. Do not set the approve amount to `type(uint256).max` / `2^256-1` / any "infinite" value, and do not call `setApprovalForAll(operator, true)`. If the user explicitly requests unlimited approval: warn it is irreversible and lets the spender drain all tokens, require a second explicit confirmation, and even then cap the amount to what is needed (e.g. swap amount + 10%). If the user still insists, refuse and suggest they execute manually via a block explorer.

## MEV Protection

`--mev-protection` is a `contract-call` flag only (`wallet send` does not support it). Load [mev-protection.md](wallet-mev-protection.md) when the user requests MEV protection, or before a high-value / DEX-swap `contract-call` — it holds the supported-chain table and the Solana `--jito-unsigned-tx` requirement.

## Policy & Wallet Export (Web portal)

Policy config and wallet export are completed by the user on the Web portal — the Agent only detects the trigger, explains the risk, and gives the jump link. On any trigger below, load [portal-actions.md](wallet-portal-actions.md) and follow its Trigger flows exactly:
- New user login (`isNew: true`) — also handled in Authentication step 2
- After a successful `wallet add`
- User asks about Policy (spending / daily limit, whitelist)
- User asks about wallet export (export mnemonic, migrate, import to hardware wallet)

Never display mnemonic phrases, seed phrases, or private keys in the conversation.

## Third-Party Plugin Pre-flight (Solana)

Before dispatching a third-party Solana DeFi plugin (kamino-plugin, raydium-plugin, …) that internally calls `wallet contract-call --force`, run the Gas Station pre-flight in [plugin-preflight.md](wallet-plugin-preflight.md).

## Notes

- **X Layer Testnet faucet**: when the user asks for testnet tokens, or `wallet balance --chain xlayer_test` shows OKB = 0, point them to https://web3.okx.com/xlayer/faucet.
- **XKO address**: if a user-supplied address starts with `XKO` / `xko`, display verbatim:
  > "XKO address format is not supported yet. Please find the 0x address by switching to your commonly used address, then you can continue."
- **TEE signing**: the private key is generated and stored inside a server-side secure enclave and never leaves the TEE — the Agent cannot export or locally sign with it.

## Additional Resources

- Full parameter tables, return-field schemas, and worked examples → [wallet-cli-reference.md](wallet-cli-reference.md), or run `onchainos wallet <subcommand> --help`. Load only when you need exact syntax not covered above.

## Edge Cases

> Load on error: [wallet-troubleshooting.md](wallet-troubleshooting.md)
