# OnchainOS local development

This checkout supports a project-local Codex, Rust CLI, Skill, and A2A runtime.
It does not replace globally installed Skills or binaries.

## Prerequisites

- Node.js 22.14 or newer (Node 24 is recommended for `node:sqlite`)
- Rust/Cargo
- A globally available `okx-a2a` executable
- A trusted Codex workspace (project `.codex/config.toml` is loaded only for trusted workspaces)

## Initialize

```bash
npm run dev:init
```

The command creates ignored, machine-specific files under `.codex/` and
project Skill links under `.agents/skills/`. Reload Codex and start a new task
after the first initialization.

Verify from the new task:

```bash
command -v onchainos
command -v okx-a2a
npm run dev:doctor
```

Both commands must resolve under this checkout's `.codex/bin/`.

## Select an environment

`.codex/dev.json` has exactly one field:

```json
{
  "environment": "beta"
}
```

Supported values are `beta`, `production`, or a custom HTTP(S) base URL.

```bash
npm run dev:env -- beta
npm run dev:env -- production
npm run dev:env -- http://127.0.0.1:8080
```

The wrappers derive the base URL and all project-local state paths from that
selection. OnchainOS credentials use the encrypted file store under
`.codex/runtime/onchainos`; A2A SQLite/XMTP/log state uses
`.codex/runtime/a2a`.

## Daily workflow

Skills keep their production commands (`onchainos ...` and `okx-a2a ...`).
Codex resolves those names through `.codex/bin` in this trusted project.

- Existing Skill file edits are visible through symlinks. Start a new task when
  changing `SKILL.md` metadata or routing instructions.
- Run `npm run dev:skills` after adding, deleting, or renaming a Skill.
- Every direct `onchainos` invocation runs Cargo's incremental build before
  executing the current debug binary. The `okx-a2a` wrapper pins daemon child
  commands to that prebuilt checkout-local binary so concurrent probes do not
  spend their bounded runtime waiting on Cargo locks. Run `npm run dev:cli`
  after CLI source changes and before starting A2A.
- Daemon-managed Codex sessions run through the checkout-local Codex adapter.
  It explicitly enables this checkout's Skill files, disables same-name stale
  global copies, and disables legacy `okx-agent-task*`, `okx-agent-chat`,
  and `okx-task-watch` Skills that compete for structured task envelopes.
  User and ASP task sub-sessions therefore use the same routing tree as the
  User main session. This adapter is installed only when the Codex CLI is
  available; Claude and other development environments keep using their native
  A2A launch path and do not require Codex.
- `okx-a2a` remains the globally installed program; the project wrapper only
  injects isolated state and the project PATH.

## Stop and clean

```bash
npm run dev:stop
npm run dev:clean
```

The default clean preserves credentials and A2A identity. To remove them too:

```bash
npm run dev:clean -- --all
```

Add `--config` only when the generated Codex and environment configuration
should also be removed.
