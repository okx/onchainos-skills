# Step 4.2–4.4: Node CLI Install Flow

> **Precondition**: [node.md](node.md) Step 4.0 reported `okx_a2a=missing`. Do not run this flow when `okx-a2a` is already installed — use `node.md` Step 4.1 instead.

## Step 4.2: Install Node CLI

### 4.2.1 Install `okx-a2a`

Run:

```bash
npm install -g @okxweb3/a2a-node@latest
```

Use npm for this install, even if yarn or pnpm is available.

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

Then detect the current host AI provider:

```bash
detect_current_ai_provider() {
  codex_signal=false
  claude_signal=false

  if [ -n "${CODEX_THREAD_ID:-}" ] || [ "${CODEX_SHELL:-}" = "1" ] || [ "${CODEX_CI:-}" = "1" ]; then
    codex_signal=true
  fi

  if [ "${CLAUDECODE:-}" = "1" ]; then
    claude_signal=true
  fi

  if [ "$codex_signal" = "true" ] && [ "$claude_signal" != "true" ]; then
    echo "codex"
  elif [ "$claude_signal" = "true" ] && [ "$codex_signal" != "true" ]; then
    echo "claude"
  fi
}

current_provider=$(detect_current_ai_provider)
echo "current_provider=${current_provider:-unknown}"
```

- If no supported AI provider is available, tell the user to install or open a supported provider app/CLI and retry. Flow ends here.
- If `current_provider` is `codex` or `claude`, and the `okx-a2a ai-provider status` output reports that same provider as installed/available (for example `codex=true` or `claude=true`), use that provider automatically. Do not ask the user to choose.
- If the current provider is unknown, ambiguous, or not installed/available according to `okx-a2a ai-provider status`, ask the user to choose which available provider should be used as the task-communication agent. Continue only after the user chooses a provider name from the command output.

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
| `yarn` or `pnpm` is installed locally | Still use `npm install -g @okxweb3/a2a-node@latest`. |
| `okx-a2a` still missing after install | Tell the user the global package-manager bin directory is not on `PATH` and stop. |
| No available AI provider from `okx-a2a ai-provider status` | Tell the user to install or open a supported provider app/CLI and retry. |
| Current runtime is clearly Codex and `codex=true` | Run `okx-a2a config provider --provider codex` without asking the user. |
| Current runtime is clearly Claude Code and `claude=true` | Run `okx-a2a config provider --provider claude` without asking the user. |
| Current runtime is unknown, ambiguous, or unavailable in provider status | Ask the user to choose among installed providers. |
| User chooses or the runtime detector selects an AI provider after installing missing `okx-a2a` | Run `okx-a2a config provider --provider <providerName>`, then `okx-a2a restart`; after restart succeeds, show the bypass-permission prompt from Step 4.4. |
