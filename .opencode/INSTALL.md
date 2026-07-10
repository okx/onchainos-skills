# Installing onchainos Skills for OpenCode

## Prerequisites

- [OpenCode.ai](https://opencode.ai) installed
- Git installed
- OKX API credentials from [OKX Developer Portal](https://web3.okx.com/onchain-os/dev-portal)

## Installation Steps

### 1. Clone the Repository

```bash
git clone https://github.com/okx/onchainos-skills ~/.config/opencode/onchainos-skills
```

### 2. Register the Plugin

Create a symlink so OpenCode discovers the plugin:

```bash
mkdir -p ~/.config/opencode/plugins
rm -f ~/.config/opencode/plugins/onchainos-skills.js
ln -s ~/.config/opencode/onchainos-skills/.opencode/plugins/onchainos-skills.js ~/.config/opencode/plugins/onchainos-skills.js
```

### 3. Symlink Skills

Create a symlink so OpenCode's native skill tool discovers the onchainos skills:

```bash
mkdir -p ~/.config/opencode/skills
rm -rf ~/.config/opencode/skills/onchainos-skills
ln -s ~/.config/opencode/onchainos-skills/skills ~/.config/opencode/skills/onchainos-skills
```

### 4. Restart OpenCode

Restart OpenCode. The plugin will automatically inject onchainos skill context.

Verify by asking: `"check my ETH balance"` or `"what's the price of SOL?"`

## Usage

### Available Skills

| Skill | When to Use |
|-------|-------------|
| `okx-agentic-wallet` | Wallet, swap, bridge, transaction gateway, public-address portfolio, security, audit log — all wallet & on-chain execution |
| `okx-dex-market` | Read-only on-chain DEX data: token prices/K-line/trade history, search tokens/trending rankings/holder analysis, crypto news/sentiment ranking/KOL/vibe chatter |
| `okx-defi` | Earn yield: deposit/withdraw, stake, claim rewards, DeFi positions & portfolio |
| `okx-ai` | ERC-8004 Agent identity + task marketplace (publish/accept/deliver/dispute) + task-progress monitor |
| `okx-guide` | Onboarding & guide hub: Onchain OS intro, OKX.AI, customer support |
| `okx-growth-competition` | Agentic Wallet trading competitions: list, join, rank, claim rewards |

### Loading a Skill Manually

Use OpenCode's native `skill` tool:

```
use skill tool to load onchainos-skills/okx-dex-market
```

## Updating

```bash
cd ~/.config/opencode/onchainos-skills
git pull
```

## Troubleshooting

### Plugin not loading

1. Check plugin symlink: `ls -l ~/.config/opencode/plugins/onchainos-skills.js`
2. Check source exists: `ls ~/.config/opencode/onchainos-skills/.opencode/plugins/onchainos-skills.js`
3. Check OpenCode logs for errors

### Skills not found

1. Check skills symlink: `ls -l ~/.config/opencode/skills/onchainos-skills`
2. Verify it points to: `~/.config/opencode/onchainos-skills/skills`
3. Use `skill` tool in OpenCode to list discovered skills

## Getting Help

- Report issues: [GitHub Issues](https://github.com/okx/onchainos-skills/issues)
- OKX Developer Docs: [Developer Documentation](https://web3.okx.com/onchain-os/dev-docs)
