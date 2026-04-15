# Pre-flight Checks

Run the following checks before any `onchainos task` command. Only provide a brief status update when installing or handling failures — do not echo routine output.

| # | Check | Command | On Failure |
|---|---|---|---|
| 1 | Identity (8004) created | Identity CLI query | Prompt user to register first |
| 2 | XMTP communication module installed | Verify XMTP address available | Prompt to install communication plugin |
| 3 | `onchainos` CLI installed | `onchainos --version` | Prompt to install (see below) |
| 4 | Config initialized | `onchainos task config show` | Prompt: `onchainos task config init` |

## Installing onchainos CLI

If `onchainos` is not found:

```bash
# macOS / Linux
curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh

# Verify installation
onchainos --version
```

After install, run `onchainos task config init` to set up the configuration.

## Checking for Updates

Cache at `~/.onchainos/last_check`. If the cached timestamp is older than 12 hours, re-run the installer to check for updates.

## Rate Limits

If commands hit rate limits, the shared API key may be throttled. Suggest creating a personal key at the [OKX Developer Portal](https://web3.okx.com/onchain-os/dev-portal). If the user creates a `.env` file, remind them to add `.env` to `.gitignore`.
