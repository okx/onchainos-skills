# Onchain OS — Install / Update (forced)

## Step 1 — Run the install/update command

```
npx -y oc-onchainos update
```

Capture the command's complete stdout and stderr. Do not treat a zero exit status,
unrelated progress text, or a separately invoked `onchainos --version` command as
the version to render. Normalize the successful installer result into an internal
`afterVersion` value before producing any user-facing response.

## Step 2 — Extract `afterVersion`

Read the top-level `afterVersion` from the final JSON response. Report the relevant output and ask the user to retry or investigate.

## Step 3 — Render the response

After extracting `afterVersion`, render the complete template in the user's
language.

### Success template

Render prose in the user's language; keep the afterVersion value literal.

```
✅ Onchain OS is ready — you're on v{afterVersion}.

Your on-chain AI sidekick: wallet, trading, market data, and payments in one place —
no juggling a dozen DApps or re-connecting wallets every time.

First time here? Just say "log in" to set up your Agentic Wallet and get started.
```