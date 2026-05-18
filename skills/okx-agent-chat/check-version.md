# Update OKX A2A Plugin

(Re)install the OKX A2A OpenClaw plugin from the npm `beta` dist-tag of `test-okx-openclaw-a2a`. If the legacy plugin (`openclaw-okx-a2a-extension`) is already installed, uninstall it first; then install the new package. `openclaw plugins install` auto-restarts the gateway, so no manual gateway control is required.

This flow can be run independently or is automatically invoked after `after-agent-list-changed.md`.

## Command Index

| # | Command | Description |
|---|---|---|
| 0 | `openclaw --version` | Verify OpenClaw >= 2026.4.1 |
| 1 | `npm view test-okx-openclaw-a2a@beta version` | Show the latest beta version (informational, for the user prompt) |
| 2 | `openclaw plugins list` | Detect whether the legacy `openclaw-okx-a2a-extension` plugin is installed |
| 3 | `openclaw plugins uninstall openclaw-okx-a2a-extension` | Remove the legacy plugin (only if Step 2 found it) |
| 4 | `openclaw plugins install test-okx-openclaw-a2a@beta` | Install the new npm package; gateway auto-restarts on success |

## Why Use `openclaw plugins install`

`openclaw plugins install` is the only correct entry point for installing/updating OpenClaw plugins. It:
- Pulls the npm tarball from the given package name + dist-tag,
- Materializes the plugin into `~/.openclaw/extensions/<plugin-id>/`,
- Automatically restarts the gateway so `register()` reloads with the new build.

Do **not** use `npm install -g` — the OKX A2A plugin is not a globally installed npm CLI, it is an OpenClaw extension and lives under `~/.openclaw/extensions/`.

## Execution Flow

### Pre-flight: Environment check

Run:
```bash
openclaw --version 2>&1
```

Requirements:
- OpenClaw **>= 2026.4.1**

If OpenClaw is below the minimum, inform the user it needs upgrading and stop.

### Step 1: Show the user what will be installed

Run:
```bash
npm view test-okx-openclaw-a2a@beta version
```

Display to the user (translate to their language as needed):
> 即将（重新）安装 OKX A2A 插件 `test-okx-openclaw-a2a@beta`（当前 beta 版本：`A.B.C`）。
> 安装过程中 openclaw gateway 会自动重启，请稍候即可。
> 是否继续？

- User says **no** → stop.
- User says **yes** → proceed to Step 2.

### Step 2: Detect the legacy plugin

```bash
openclaw plugins list 2>&1
```

Scan the output for an entry whose plugin id equals `openclaw-okx-a2a-extension`.

- Found → proceed to Step 3.
- Not found → skip Step 3, jump directly to Step 4.

### Step 3: Uninstall the legacy plugin (only when present)

<MUST>
Do **not** delete `~/.openclaw/extensions/openclaw-okx-a2a-extension` with `rm -rf`. Always go through the OpenClaw CLI so plugin hooks, daemons, and config bindings are cleaned up properly.
</MUST>

```bash
openclaw plugins uninstall openclaw-okx-a2a-extension
```

If uninstall fails, surface the error and stop.

### Step 4: Install the new package

```bash
openclaw plugins install test-okx-openclaw-a2a@beta
```

`openclaw plugins install` auto-restarts the gateway on success — the new plugin loads in that single restart. Flow ends here; no manual `gateway stop` / `gateway start` is needed.

If install fails, surface the error verbatim and stop. Suggest checking npm registry connectivity or OpenClaw logs.

## Edge Cases

| Scenario | Behavior |
|---|---|
| OpenClaw < 2026.4.1 | Inform the user OpenClaw is too old, stop. Do not attempt install. |
| `npm view test-okx-openclaw-a2a@beta version` fails (network) | Surface the error and stop. Suggest checking network connectivity. |
| `openclaw plugins list` fails | Surface the error and stop — cannot determine plugin state. |
| Legacy plugin `openclaw-okx-a2a-extension` not present in `openclaw plugins list` | Skip Step 3, proceed straight to Step 4. |
| `openclaw plugins uninstall` fails | Surface the error and stop. Do not run install while the legacy plugin is still half-removed. |
| `openclaw plugins install` fails | Surface the error verbatim. Suggest checking npm registry / OpenClaw logs. |
| `openclaw` command not found | Inform the user the OpenClaw CLI is required. |
