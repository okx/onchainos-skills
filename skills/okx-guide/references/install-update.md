# Onchain OS — Install / Update (forced preflight)

Reached when the user has an explicit **install / update / upgrade** intent for the
Onchain OS CLI or its skills ("install onchain os", "update onchainos", "upgrade to
the latest version", "reinstall the CLI").

> This is the **forced** path. The routine session-start check (daily silent precheck)
> stays in [_shared/preflight.md](../_shared/preflight.md) and uses the **un-forced**
> `onchainos preflight` form. Do not confuse the two: only an explicit user intent runs
> `--force`.

## Step 1 — Run the forced preflight

```
onchainos preflight --force --skill-version <this skill's frontmatter version>
```

- `--force` bypasses the 12-hour throttle, always performs a fresh online check, and
  **applies the update when one exists** (channel resolution → download / install → integrity
  verification → drift check → deprecated-skill cleanup).
- `--skill-version` is optional but **SHOULD** be passed (it drives the drift check + beta
  routing); substitute this skill's own frontmatter `version`.
- **`command not found` / `unrecognized subcommand 'preflight'`** → the CLI is missing; install it,
  then re-run the command above:
  - macOS/Linux: `curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh`
  - Windows: `irm https://raw.githubusercontent.com/okx/onchainos-skills/main/install.ps1 | iex`
  - Stop only if installation itself fails.

The CLI keeps its standard `{ ok, data }` envelope. Read `data.status` to decide what to render.

## Step 2 — Branch on `data.status`

| `data.status` | Success? | Render |
|---|---|---|
| `ok` | Yes (already latest) | the unified install-success template (Step 3) |
| `updated` | Yes (newer version installed this run) | the unified install-success template (Step 3) |
| `offline` | No | `data.action` verbatim (Step 4) |
| `update_failed` | No | `data.action` verbatim (Step 4) |
| `update_skipped` | No | `data.action` verbatim (Step 4) |

`fresh` is **not** returned on the `--force` path (the throttle is bypassed); if you ever see it,
`--force` was dropped from the command — re-run with `--force`.

**Version fields** (used only in Step 3):

- `updated == true` → display `data.latestVersion` as the current version — the on-disk binary is
  now that version.
- `updated == false` → display `data.currentVersion`.
- **NEVER**: guess, fabricate, or round a version — a success status always carries a populated
  version field, and an unresolvable latest surfaces as `status: "offline"` (a failure), so there is
  no "success with an empty version" case to paper over.

## Step 3 — Unified install-success template (`ok` / `updated`)

Render this single template for **both** `ok` and `updated`. Render prose in the user's language;
keep the version value and any machine strings literal. `{version}` is the value chosen by the
version-field rule above.

```
✅ Onchain OS is ready — you're on v{version}.

Your on-chain AI sidekick: wallet, trading, market data, and payments in one place —
no juggling a dozen DApps or re-connecting wallets every time.

First time here? Just say "log in" to set up your Agentic Wallet and get started.
```

- This install-screen blurb is a **brief overview + first-login CTA** and is intentionally distinct
  from the post-login capability blurb in `okx-agentic-wallet` (that one lists concrete entry
  points). Do not merge them.
- Write any product name (e.g. **OKX.AI**, with the dot) exactly as the repo's canonical usage.
- **SHOULD**: if `data.action` is non-null on a success status (e.g. a skill-drift or
  deprecated-skill-cleanup hint), relay it verbatim **after** the template — it is actionable
  guidance the CLI chose to surface. On a null `action`, render the template alone.

## Step 4 — Failure statuses (`offline` / `update_failed` / `update_skipped`)

- **MUST**: render the string(s) in `data.action` **verbatim** as the failure message — the CLI
  single-sources the per-status guidance (e.g. `update_failed` → "CLI update to v… failed; kept
  v…. Retry: onchainos upgrade --force"; `offline` → the GitHub-unreachable action). Surfacing it
  verbatim keeps the failure contract single-sourced in the CLI and prevents drift.
- **NEVER**: author your own per-status failure copy, and **NEVER**: render the install-success
  template on a failure status — doing either would tell the user they succeeded when they did not.
- Exit code stays `0` on every status (preflight is a session-start advisory); the outcome lives in
  `data.status` / `data.action`, not the exit code — do not treat a `0` exit as "install succeeded".

## Legacy `upgrade` entry (compatibility)

`onchainos upgrade [--force]` still works and reaches the same underlying check-and-install flow;
it is kept for one release cycle. Prefer `onchainos preflight --force` for a new install/update
intent, but a user or older skill calling `upgrade` is not an error.
