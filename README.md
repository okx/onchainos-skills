# onchainos-skills

Self-contained AI skills package for the onchainOS CLI. Provides token search, market data, wallet balance queries, swap execution, and transaction broadcasting across 20+ blockchains — all through simple CLI commands.

## Supported Platforms

Pre-built binaries are available for **10 platform/architecture combinations**:

| OS      | Architecture          | Binary Name                               |
| ------- | --------------------- | ----------------------------------------- |
| macOS   | ARM64 (Apple Silicon) | `onchainos-aarch64-apple-darwin`          |
| macOS   | x64 (Intel)           | `onchainos-x86_64-apple-darwin`           |
| Linux   | x64                   | `onchainos-x86_64-unknown-linux-gnu`      |
| Linux   | x86 (32-bit)          | `onchainos-i686-unknown-linux-gnu`        |
| Linux   | ARM64                 | `onchainos-aarch64-unknown-linux-gnu`     |
| Linux   | ARM32 (armv7)         | `onchainos-armv7-unknown-linux-gnueabihf` |
| Windows | x64                   | `onchainos-x86_64-pc-windows-msvc.exe`    |
| Windows | x86 (32-bit)          | `onchainos-i686-pc-windows-msvc.exe`      |
| Windows | ARM64                 | `onchainos-aarch64-pc-windows-msvc.exe`   |

All binaries are single-file executables with zero runtime dependencies.

## Install CLI

### Homebrew (macOS / Linux)

```bash
brew tap okx/onchainos-skills https://github.com/okx/onchainos-skills
brew install onchainos
```

### Shell Script (macOS / Linux)

Auto-detects your platform, downloads the matching binary, verifies SHA256 checksum, and installs to `/usr/local/bin`:

```bash
curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
```

### Windows (PowerShell)

Run in any PowerShell terminal (Windows 10/11). The script auto-detects the best download method available on your system:

```powershell
powershell -ExecutionPolicy Bypass -Command "& { [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; $s = (New-Object Net.WebClient).DownloadString('https://raw.githubusercontent.com/okx/onchainos-skills/main/install.ps1'); Invoke-Expression $s }"
```

Installs to `%LOCALAPPDATA%\onchainos\bin\onchainos.exe` and automatically adds it to your user PATH.

### Build from Source (Any Platform)

Requires [Rust toolchain](https://rustup.rs/). The CLI source is inside the `cli` directory:

```bash
git clone https://github.com/okx/onchainos-skills.git
cd onchainos-skills/cli
cargo install --path .
```

The compiled binary will be placed in `~/.cargo/bin/onchainos` (or `%USERPROFILE%\.cargo\bin\onchainos.exe` on Windows).

## Quick Start

### 1. Install CLI

Pick any install method above, then verify:

```bash
onchainos --version
```

### 2. Configure Environment

```bash
cp .env.example .env
```

Edit `.env` and fill in your OKX API credentials:

```
OKX_API_KEY=your-api-key
OKX_SECRET_KEY=your-secret-key
OKX_PASSPHRASE=your-passphrase
```

Get credentials at: [OKX Developer Portal](https://web3.okx.com/onchain-os/dev-portal)

#### Quick Trial

Use the shared API key to try it out immediately:

```
OKX_API_KEY="9fc58c11-e2d3-4f52-b5e9-d863a094c50f"
OKX_SECRET_KEY="146127D9883D97E00799C59BE9CFCEBB"
OKX_PASSPHRASE="onchainOS666!"
```

> **Note**: This shared key has rate limits. For production use, apply for your own key.

### 3. Verify

```bash
onchainos swap chains
```

## Available Skills

| Skill                  | CLI Commands                                                       | Description                                       |
| ---------------------- | ------------------------------------------------------------------ | ------------------------------------------------- |
| `okx-wallet-portfolio` | `onchainos wallet balance/total/token-balance/chains/set/show`     | Wallet balance, token holdings, portfolio value   |
| `okx-dex-market`       | `onchainos market price/prices/kline/trades/index`                 | Real-time prices, K-line charts, trade history    |
| `okx-dex-swap`         | `onchainos swap quote/swap/approve/chains/liquidity`               | Token swap via DEX aggregation (500+ sources)     |
| `okx-dex-token`        | `onchainos token search/info/holders/trending/price-info`          | Token search, metadata, rankings, holder analysis |
| `okx-onchain-gateway`  | `onchainos gateway gas/gas-limit/simulate/broadcast/orders/chains` | Gas estimation, tx simulation, broadcasting       |

## Supported Chains

ethereum, solana, bsc, polygon, arbitrum, base, xlayer, avalanche, optimism, fantom, sui, tron, ton, linea, scroll, zksync — and more.

Chain names can be used directly: `--chain ethereum` instead of `--chain 1`.

## CLI Usage Examples

```bash
# Check wallet balance
onchainos wallet balance --address 0xYourWallet --chains ethereum,bsc

# Get token price
onchainos market price 0xTokenAddress --chain ethereum

# Search for a token
onchainos token search BONK --chains solana

# Get swap quote
onchainos swap quote --from 0xFromToken --to 0xToToken --amount 1000000 --chain ethereum

# Estimate gas
onchainos gateway gas --chain ethereum

# View trending tokens
onchainos token trending --chains solana --sort-by volume --time-frame 24h
```

## Skill Workflows

**Search and Buy**: token search → wallet check → swap quote → swap execute

**Portfolio Overview**: wallet balance → token analytics → price charts

**Market Research**: trending tokens → K-line charts → trade history → swap

**Swap and Broadcast**: swap → sign locally → broadcast → track order

**Full Trading Flow**: token search → price check → balance check → swap → simulate → broadcast → track

## Installation for AI Clients

### Claude Code (Recommended)

```bash
npx skills add okx/onchainos-skills
```

Or manually:

```bash
/plugin marketplace add okx/onchainos-skills
/plugin install onchainos-skills
```

### Cursor

Skills are auto-detected from `.cursor-plugin/plugin.json`.

### Codex CLI

```
Fetch and follow instructions from https://raw.githubusercontent.com/okx/onchainos-skills/refs/heads/main/.codex/INSTALL.md
```

### OpenCode

```
Fetch and follow instructions from https://raw.githubusercontent.com/okx/onchainos-skills/refs/heads/main/.opencode/INSTALL.md
```

## Release Artifacts

Every push to the `main` branch triggers GitHub Actions to build binaries for all 10 platforms listed above and publish them to [GitHub Releases](https://github.com/okx/onchainos-skills/releases). Non-main branches produce pre-release builds.

All release assets include a `checksums.txt` file with SHA256 hashes. The install scripts (`install.sh` / `install.ps1`) automatically verify checksums before installation.

## License

Apache-2.0
