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

You should see the skill directories: `okx-agentic-wallet`, `okx-dex-market`, `okx-defi`,
`okx-ai`, `okx-guide`, `okx-growth-competition`.

## Available Skills

| Skill                    | When to Use                                                                                                                                               |
|--------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------|
| `okx-agentic-wallet`     | Wallet, swap, bridge, gateway, portfolio, security, audit — all wallet & on-chain execution                                                               |
| `okx-dex-market`         | Read-only on-chain DEX data: token prices/K-line/trade history, token search/rankings/holder distribution, crypto news/sentiment ranking/vibe/KOL chatter |
| `okx-defi`               | Earn yield: deposit/withdraw, stake, claim rewards, DeFi positions & portfolio                                                                            |
| `okx-ai`                 | ERC-8004 Agent identity + task marketplace (publish/accept/deliver/dispute) + task-progress monitor                                                       |
| `okx-guide`              | Onboarding & guide hub: Onchain OS intro, OKX.AI, customer support                                                                                        |
| `okx-growth-competition` | Agentic Wallet trading competitions: list, join, rank, claim rewards                                                                                      |

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