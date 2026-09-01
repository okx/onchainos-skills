# Onchain OS — Install / Update (forced preflight)

Reached when the user has an explicit **install / update / upgrade** intent for the
Onchain OS CLI or its skills ("install onchain os", "update onchainos", "upgrade to
the latest version", "reinstall the CLI").

## Step 1 — Run the forced preflight

Read [`../../okx-agentic-wallet/_shared/preflight.md`](../../okx-agentic-wallet/_shared/preflight.md)
first — it owns the preflight contract: the install fallback when the CLI is missing, and the
`data.action` handling for every non-success outcome. This install/update path is that same
check with `--force`, which bypasses the throttle and applies an available update:

```
onchainos preflight --force --skill-version <this skill's frontmatter version>
```

## Step 2 — Render the outcome

From the `{ ok, data }` envelope, read `data.status`:

- **`ok` / `updated`** → success: render the template below, using `data.versionAfter`
  as `{version}` (the on-disk version after preflight — the single version field to display).
- **anything else** → not a success: follow `_shared/preflight.md` — show `data.action`
  verbatim. Never render the success template on a non-success status.

### Success template

Render prose in the user's language; keep the version value literal.

```
✅ Onchain OS is ready — you're on v{version}.

Your on-chain AI sidekick: wallet, trading, market data, and payments in one place —
no juggling a dozen DApps or re-connecting wallets every time.

First time here? Just say "log in" to set up your Agentic Wallet and get started.
```

If `data.action` is non-null on a success status (e.g. a skill-drift hint), relay it verbatim
after the template.
