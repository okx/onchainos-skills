# Ensure XMTP Plugin Installed

**Mandatory safeguard** — run this every time the agent needs to communicate with another agent, initiate agent commerce, or use XMTP messaging. Checks whether the XMTP plugin (`openclaw-plugin-xmtp`) is installed globally via npm. If not, installs the latest version and restarts the openclaw gateway. After completion, automatically proceeds to `check-version.md` to check for updates.

> **TODO**: Confirm the npm package name is `openclaw-plugin-xmtp`. Update if different.

## Command Index

| # | Command | Description |
|---|---|---|
| 1 | `npm list -g openclaw-plugin-xmtp --depth=0` | Check if the XMTP plugin is installed |
| 2 | `openclaw gateway stop` | Stop the gateway before installation |
| 3 | `npm install -g openclaw-plugin-xmtp@latest` | Install the latest version |
| 4 | `openclaw gateway start` | Start the gateway after installation |

## Why Gateway Restart Is Required

The XMTP extension is an **OpenClaw plugin** (not a standalone channel). It loads via the `plugins.*` config path, registers a channel, daemon, hooks, skills, and services inside its `register()` method, and initializes the daemon on `gateway:startup`. Hot reload cannot re-run `register()` — a full gateway stop/start cycle is required.

## Execution Flow

### Step 1: Check if installed

Run:
```bash
npm list -g openclaw-plugin-xmtp --depth=0 2>/dev/null
```

- If the package appears in the output (shows a version number) → **installed**. Skip to Step 3.
- If the output says `(empty)` or the package is not listed → **not installed**. Proceed to Step 2.

### Step 2: Install the extension

<MUST>
**Gateway must be stopped before installation and started after.** The plugin's `register()` method runs at gateway startup — installing while the gateway is running will not load the new plugin.
</MUST>

First, stop the gateway:
```bash
openclaw gateway stop
```

Then install:
```bash
npm install -g openclaw-plugin-xmtp@latest
```

If installation succeeds:
- Inform the user: "XMTP plugin installed successfully."
- Start the gateway:
  ```bash
  openclaw gateway start
  ```
- Inform the user: "openclaw gateway started."

If installation fails:
- Display the error message to the user.
- Suggest checking npm permissions (`npm config get prefix`) or network connectivity.
- Start the gateway even on failure (it was stopped):
  ```bash
  openclaw gateway start
  ```
- Do not proceed to Step 3.

### Step 3: Proceed to version check

After ensuring the extension is installed, automatically load and follow `check-version.md` to check for available updates.

## Edge Cases

| Scenario | Behavior |
|---|---|
| npm not found | Inform the user that Node.js/npm is required. Suggest installing via https://nodejs.org |
| Permission denied on npm install -g | Suggest `sudo npm install -g` or fixing npm global prefix |
| openclaw command not found | Inform the user that openclaw CLI is required |
| Gateway stop fails | Display error, attempt install anyway — the gateway may not have been running |
| Gateway start fails | Display error, suggest manual start |
| Already installed | Skip install (no gateway restart needed), proceed directly to version check |
