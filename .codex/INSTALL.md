# Install OKX Onchain OS for Codex

Install the official OKX Onchain OS skills into Codex through native skill discovery.

This repository includes reusable skills for token discovery, market data, smart-money signals, wallet and DeFi analysis, swaps, bridge flows, security checks, payments, and transaction workflows.

## Prerequisites

- Git
- Codex
- OKX API credentials from [OKX Onchain OS Developer Portal](https://web3.okx.com/onchainos/dev-portal)

Recommended environment variables:

```bash
OKX_API_KEY="your-api-key"
OKX_SECRET_KEY="your-secret-key"
OKX_PASSPHRASE="your-passphrase"
```

Never commit these credentials to git or paste them into logs and chat transcripts.

## Install

1. Clone the repository:

   ```bash
   git clone https://github.com/okx/onchainos-skills ~/.codex/onchainos-skills
   ```

2. Symlink the bundled skills into Codex skill discovery:

   ```bash
   mkdir -p ~/.agents/skills
   ln -s ~/.codex/onchainos-skills/skills ~/.agents/skills/onchainos-skills
   ```

   Windows (PowerShell):

   ```powershell
   New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.agents\skills"
   cmd /c mklink /J "$env:USERPROFILE\.agents\skills\onchainos-skills" "$env:USERPROFILE\.codex\onchainos-skills\skills"
   ```

3. Restart Codex so it can discover the new skills.

## Verify

```bash
ls -la ~/.agents/skills/onchainos-skills
```

You should see the Onchain OS skill set, including categories such as:

- `okx-agentic-wallet`
- `okx-wallet-portfolio`
- `okx-security`
- `okx-dex-market`
- `okx-dex-signal`
- `okx-dex-swap`
- `okx-dex-token`
- `okx-onchain-gateway`
- `okx-defi-invest`
- `okx-defi-portfolio`

## Optional CLI Install

The repository also ships the native `onchainos` CLI and MCP server.

macOS / Linux:

```bash
curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/okx/onchainos-skills/main/install.ps1 | iex
```

Verify:

```bash
onchainos --version
```

## Update

```bash
cd ~/.codex/onchainos-skills && git pull
```

Updates take effect through the symlink after Codex reloads the skills.

## Uninstall

```bash
rm ~/.agents/skills/onchainos-skills
```

Optionally remove the clone:

```bash
rm -rf ~/.codex/onchainos-skills
```
