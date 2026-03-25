# Installing onchainos Skills for OpenClaw

Enable onchainos skills in OpenClaw via native skill discovery. Just clone, symlink.

## Prerequisites

- Git
- OKX API credentials from [OKX Developer Portal](https://web3.okx.com/onchain-os/dev-portal)

## Installation

1. **Clone the repository:**

   ```bash
   git clone https://github.com/okx/onchainos-skills ~/.openclaw/onchainos-skills
   ```

2. **Create the skills symlink:**

   ```bash
   mkdir -p ~/.agents/skills
   ln -s ~/.openclaw/onchainos-skills/skills ~/.agents/skills/onchainos-skills
   ```

   **Windows (PowerShell):**

   ```powershell
   New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.agents\skills"
   cmd /c mklink /J "$env:USERPROFILE\.agents\skills\onchainos-skills" "$env:USERPROFILE\.openclaw\onchainos-skills\skills"
   ```

3. **Restart OpenClaw** (quit and relaunch) to discover the skills.

## Verify

```bash
ls -la ~/.agents/skills/onchainos-skills
```

You should see the skill directories: `okx-wallet-portfolio`, `okx-dex-market`, `okx-dex-swap`, `okx-dex-token`,
`okx-onchain-gateway`.

## Available Skills

| Skill                  | When to Use                                                          |
|------------------------|----------------------------------------------------------------------|
| `okx-wallet-portfolio` | Wallet balance, token holdings, portfolio value                      |
| `okx-dex-market`       | Token prices, K-line charts, trade history                           |
| `okx-dex-swap`         | Swap / trade / buy / sell tokens on-chain                            |
| `okx-dex-token`        | Token search, rankings, holder distribution                          |
| `okx-onchain-gateway`  | Gas estimation, transaction simulation, broadcasting, order tracking |

## Updating

```bash
cd ~/.openclaw/onchainos-skills && git pull
```

Skills update instantly through the symlink.

## Uninstalling

```bash
rm ~/.agents/skills/onchainos-skills
```

Optionally delete the clone: `rm -rf ~/.openclaw/onchainos-skills`.