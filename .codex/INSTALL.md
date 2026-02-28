# Installing onchainos Skills for Codex

Enable onchainos skills in Codex via native skill discovery. Just clone, symlink, and set credentials.

## Prerequisites

- Git
- OKX API credentials from [OKX Developer Portal](https://web3.okx.com/onchain-os/dev-portal)

## Installation

1. **Clone the repository:**

   ```bash
   git clone https://github.com/okx/onchainos-skills ~/.codex/onchainos-skills
   ```

2. **Create the skills symlink:**

   ```bash
   mkdir -p ~/.agents/skills
   ln -s ~/.codex/onchainos-skills/skills ~/.agents/skills/onchainos-skills
   ```

   **Windows (PowerShell):**

   ```powershell
   New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.agents\skills"
   cmd /c mklink /J "$env:USERPROFILE\.agents\skills\onchainos-skills" "$env:USERPROFILE\.codex\onchainos-skills\skills"
   ```

3. **Set API credentials:**

   Add to your shell profile (`~/.zshrc`, `~/.bashrc`, etc.):

   ```bash
   export OKX_API_KEY="your-api-key"
   export OKX_SECRET_KEY="your-secret-key"
   export OKX_PASSPHRASE="your-passphrase"
   ```

   Then reload: `source ~/.zshrc`

4. **Restart Codex** (quit and relaunch the CLI) to discover the skills.

## Verify

```bash
ls -la ~/.agents/skills/onchainos-skills
```

You should see the five skill directories: `okx-wallet-portfolio`, `okx-dex-market`, `okx-dex-swap`, `okx-dex-token`, `okx-onchain-gateway`.

## Available Skills

| Skill | When to Use |
|-------|-------------|
| `okx-wallet-portfolio` | Wallet balance, token holdings, portfolio value |
| `okx-dex-market` | Token prices, K-line charts, trade history |
| `okx-dex-swap` | Swap / trade / buy / sell tokens on-chain |
| `okx-dex-token` | Token search, rankings, holder distribution |
| `okx-onchain-gateway` | Gas estimation, transaction simulation, broadcasting, order tracking |

## Updating

```bash
cd ~/.codex/onchainos-skills && git pull
```

Skills update instantly through the symlink.

## Uninstalling

```bash
rm ~/.agents/skills/onchainos-skills
```

Optionally delete the clone: `rm -rf ~/.codex/onchainos-skills`.
