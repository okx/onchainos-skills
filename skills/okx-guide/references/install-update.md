# Onchain OS — Install / Update (forced)

## Step 1 — Run the install/update command

```
npx -y @okxweb3/onchainos-installer install
```

## Step 2 — Extract `afterVersion`

Read the top-level `cliUpdate.afterVersion` from the final JSON response. Report the relevant output and ask the user to retry or investigate.

## Step 3 — Render the response

Render the complete template in the user's language.

### Success template

```
✅ Onchain OS is ready — you're on v{cliUpdate.afterVersion}.

Your on-chain AI sidekick: wallet, trading, market data, and payments in one place —
no juggling a dozen DApps or re-connecting wallets every time.

First time here? Just say "log in" to set up your Agentic Wallet and get started.
```