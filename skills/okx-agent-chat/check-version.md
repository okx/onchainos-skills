# Check XMTP Plugin Version

Check whether a newer version of the XMTP plugin (`openclaw-plugin-xmtp`) is available. If an update exists, ask the user whether to update. After updating, restart the openclaw gateway.

This flow can be run independently or is automatically invoked after `ensure-installed.md`.

> **TODO**: Confirm the npm package name is `openclaw-plugin-xmtp`. Update if different.

## Command Index

| # | Command | Description |
|---|---|---|
| 1 | `npm list -g openclaw-plugin-xmtp --depth=0` | Get current installed version |
| 2 | `npm view openclaw-plugin-xmtp version` | Get latest available version from npm |
| 3 | `openclaw gateway stop` | Stop the gateway before update |
| 4 | `npm install -g openclaw-plugin-xmtp@latest` | Update to latest version |
| 5 | `openclaw gateway start` | Start the gateway after update |

## Why Gateway Restart Is Required

The XMTP extension is an **OpenClaw plugin** that registers its channel, daemon, hooks, and services inside `register()` at load time. Updating the npm package does not reload the plugin — a full gateway stop/start cycle is required for the new version to take effect.

## Execution Flow

### Step 1: Get current installed version

Run:
```bash
npm list -g openclaw-plugin-xmtp --depth=0 2>/dev/null
```

Extract the installed version number from the output (e.g., `openclaw-plugin-xmtp@1.0.0`).

- If not installed → inform the user and load `ensure-installed.md` first. Stop.

### Step 2: Get latest available version

Run:
```bash
npm view openclaw-plugin-xmtp version
```

This returns the latest published version on npm.

### Step 3: Compare versions

- If installed version equals latest version → inform the user: "XMTP plugin is up to date (version X.Y.Z)." Stop.
- If installed version is older → proceed to Step 4.

### Step 4: Ask user to update

Display to the user:
> A new version of the XMTP plugin is available.
> - Current: X.Y.Z
> - Latest: A.B.C
>
> Would you like to update?

- If user says **yes** → proceed to Step 5.
- If user says **no** → inform: "Update skipped. You can update later by running this check again." Stop.

### Step 5: Update the extension

<MUST>
**Gateway must be stopped before update and started after.** The plugin's `register()` method runs at gateway startup — updating while the gateway is running will not load the new version.
</MUST>

First, stop the gateway:
```bash
openclaw gateway stop
```

Then update:
```bash
npm install -g openclaw-plugin-xmtp@latest
```

If update succeeds:
- Inform the user: "XMTP plugin updated to version A.B.C."
- Start the gateway:
  ```bash
  openclaw gateway start
  ```
- Inform the user: "openclaw gateway started."

If update fails:
- Display the error message to the user.
- Suggest checking npm permissions or network connectivity.
- Start the gateway even on failure (it was stopped):
  ```bash
  openclaw gateway start
  ```

## Edge Cases

| Scenario | Behavior |
|---|---|
| Extension not installed | Inform user, load `ensure-installed.md` first |
| npm not found | Inform user that Node.js/npm is required |
| npm view fails (network) | Inform user, suggest checking network connectivity |
| Permission denied on npm install -g | Suggest `sudo npm install -g` or fixing npm global prefix |
| openclaw command not found | Inform user that openclaw CLI is required |
| Gateway stop fails | Display error, attempt update anyway |
| Gateway start fails | Display error, suggest manual start |
| Already on latest version | Inform user, no action needed (no gateway restart) |
