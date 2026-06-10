# Step 4.2–4.4: Node CLI Install Flow

> **Precondition**: [node.md](node.md) Step 4.0 reported `okx_a2a=missing`. Do not run this flow when `okx-a2a` is already installed — use `node.md` Step 4.1 instead.

## Step 4.2: Install Node CLI

### 4.2.1 Choose Package Manager

Detect locally available package managers:

```bash
command -v yarn >/dev/null 2>&1 && echo "pm=yarn"
command -v pnpm >/dev/null 2>&1 && echo "pm=pnpm"
```

- If neither `yarn` nor `pnpm` is present, use `npm` and continue to Step 4.2.2.
- If one or both are present, ask the user which package manager they prefer among `npm` plus the detected options. Do not infer. Continue only after the user chooses one of the offered package managers.

### 4.2.2 Install `okx-a2a`

Run exactly one command based on the selected package manager:

```bash
npm install -g @okxweb3/a2a-node@latest
```

```bash
yarn global add @okxweb3/a2a-node@latest
```

```bash
pnpm add -g @okxweb3/a2a-node@latest
```

If installation fails, surface the error verbatim and stop.

After installation, verify the executable is available:

```bash
command -v okx-a2a >/dev/null 2>&1
```

If `okx-a2a` is still not found, tell the user the global package manager bin directory is not on `PATH`, then stop. Otherwise continue to Step 4.3.

## Step 4.3: Select AI Provider

Run:

```bash
okx-a2a ai-provider status 2>&1
```

Use the command output as the source of truth for provider names and installation state.

- If no supported AI provider CLI is installed, tell the user to install one supported provider CLI and retry. Flow ends here.
- If one or more supported provider CLIs are installed, ask the user to choose which provider CLI should be used as the task-communication agent. Continue only after the user chooses a provider name from the command output.

Then run:

```bash
okx-a2a config provider --provider <providerName>
```

If the config command fails, surface the error verbatim and stop. Otherwise continue to Step 4.4.

## Step 4.4: Start OKX A2A Daemon

This step is only reached after Step 4.2 installed missing `okx-a2a`. Do not show the bypass-permission prompt from this step when `okx-a2a` was already installed (that case is handled by `node.md` Step 4.1).

Run:

```bash
okx-a2a restart
```

If the command fails, surface the error verbatim and stop.

After the daemon restart succeeds, tell the user in English:

> Bypass permission mode is on by default to skip per-step confirmations. To approve actions manually, just tell the agent to run "okx-a2a agent bypass off"

On success, OKX A2A communication initialization is complete. Flow ends here.

## Edge Cases (Node install)

| Scenario | Behavior |
|---|---|
| No `yarn` / `pnpm` found | Use `npm` without asking package-manager preference. |
| `yarn` or `pnpm` found | Ask the user to choose among `npm` and the detected package managers. |
| `okx-a2a` still missing after install | Tell the user the global package-manager bin directory is not on `PATH` and stop. |
| No installed AI provider from `okx-a2a ai-provider status` | Tell the user to install one supported provider CLI and retry. |
| User chooses an AI provider after installing missing `okx-a2a` | Run `okx-a2a config provider --provider <providerName>`, then `okx-a2a restart`; after restart succeeds, show the bypass-permission prompt from Step 4.4. |
