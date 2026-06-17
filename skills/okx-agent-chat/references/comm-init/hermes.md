# Step 3: Hermes Flow

> **Precondition**: [ensure-okx-a2a-communication-ready.md](../../ensure-okx-a2a-communication-ready.md) already confirmed `xmtp_refresh_agents` is absent from the toolset, and the runtime detector returned `runtime: "hermes"`. If the tool is in fact present in the current toolset, go back and call it directly instead — do not run this install flow.

Use this branch when a Hermes user needs to install the `okx-a2a` plugin from the npm package and restart Hermes Gateway.

## Step 3.2: Check Node.js Version

Run:

```bash
node --version
```

Requirements:

- Node.js **>= 22.14.0**

If Node.js is below the minimum, inform the user it needs upgrading and stop. Do not proceed.

## Step 3.3: Download The Hermes Plugin Package

Create a temporary working directory and download the latest npm package:

```bash
mkdir -p /tmp/okx-a2a-hermes-install
cd /tmp/okx-a2a-hermes-install
npm pack @okxweb3/a2a-hermes@latest
```

If `npm pack` fails, surface the error verbatim and stop.

Extract the npm package:

```bash
tar -xzf okxweb3-a2a-hermes-*.tgz
```

Extract the Hermes plugin package inside the npm package:

```bash
tar -xzf package/dist/okx-a2a-hermes-plugin-*.tar.gz
cd okx-a2a-hermes-plugin
```

Verify the plugin package contents:

```bash
ls __init__.py plugin.yaml package.json scripts/install-or-upgrade.sh src dist/server.js
```

If any required file or directory is missing, surface the error and stop.

On macOS, remove quarantine attributes from native files bundled in the plugin package before installing:

```bash
xattr -dr com.apple.quarantine .
```

## Step 3.4: Install Or Upgrade The Plugin

Before running the install command, tell the user in English:

> Installing or upgrading the OKX A2A Hermes plugin from npm package `@okxweb3/a2a-hermes`. Hermes Gateway will restart automatically after installation; this is expected, and no manual action is required.

Install or upgrade the plugin into the Hermes **user plugins directory** and automatically restart Hermes Gateway:

```bash
bash scripts/install-or-upgrade.sh --restart --target "$HOME/.hermes/plugins/platforms/okx-a2a"
```

If the install fails, surface the error verbatim and stop.

Always pass `--target "$HOME/.hermes/plugins/platforms/okx-a2a"` so the plugin lands in the Hermes user plugins directory (`~/.hermes/plugins/`), not inside the Hermes git clone.

User-installed plugins are gated by the `plugins.enabled` allow-list in `~/.hermes/config.yaml`. The install script adds the entry automatically; if it prints `ACTION REQUIRED` (an existing `plugins:` section it will not modify), ask the user to add this to `~/.hermes/config.yaml` and restart the gateway:

```yaml
plugins:
  enabled:
    - okx-a2a
```

On success, Hermes communication initialization is complete. Flow ends here.

## Step 3.5: Manual Gateway Restart Fallback

If the install script should not restart Hermes Gateway automatically, install only (same `--target` requirement as Step 3.4):

```bash
bash scripts/install-or-upgrade.sh --target "$HOME/.hermes/plugins/platforms/okx-a2a"
```

Then restart Hermes Gateway manually:

```bash
hermes gateway restart
```

If the current Hermes CLI does not provide a `restart` command, run:

```bash
hermes gateway stop
hermes gateway run --replace
```

If Gateway still reports that a `.node` native file cannot be opened after restart, remove quarantine attributes from the installed plugin directory and restart Gateway again:

```bash
cd ~/.hermes/plugins/platforms/okx-a2a
xattr -dr com.apple.quarantine .
hermes gateway restart
```

If a legacy copy exists at `~/.hermes/hermes-agent/plugins/platforms/okx-a2a`, remove it so only the user-plugins copy is loaded.

## Edge Cases (Hermes)

| Scenario | Behavior |
|---|---|
| Hermes runtime and tool `xmtp_refresh_agents` is missing | Continue with this install flow. |
| Gateway reports a `.node` native file cannot be opened after restart | Remove quarantine attributes from the installed plugin directory (Step 3.5) and restart Gateway again. |
