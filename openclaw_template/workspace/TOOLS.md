# Tools

## Capabilities

- **Token research** — price, security scan (honeypot/tax/mint/freeze), holder cluster analysis, top traders, smart money signals
- **Smart money tracking** — real-time KOL, whale, and insider wallet activity via signal aggregation
- **New token screening** — pump.fun/Believe/Trenches launchpad scanning with dev reputation and bundle detection
- **Market data** — prices, K-line charts, index prices, wallet PnL across chains
- **Safe swap execution** — 500+ DEX liquidity sources, MEV protection (Jito/Flashbots), pre-trade security checks
- **DeFi management** — deposit, withdraw, claim across Aave, Lido, PancakeSwap, Kamino, NAVI, and hundreds more
- **Agentic wallet** — TEE-secured execution, private keys never exposed, gas-free payments on X Layer via x402 protocol
- **Real-time monitoring** — WebSocket-based wallet monitoring, smart money alerts, meme scan feeds

## onchainos CLI

The official OKX OnchainOS CLI — built for AI, ready for Web3. Pre-installed via `setup.sh`.

```bash
onchainos --version   # verify binary is available
onchainos --help      # full command reference
```

**Infrastructure:** Sub-100ms average response times · 99.9% uptime · 130+ networks

## CLI conventions

- `--chain` accepts chain names (e.g. `solana`, `ethereum`, `base`, `xlayer`) or chain indexes (e.g. `501`, `1`, `8453`)
- `--address` always expects a full contract address — never guess; resolve with `onchainos token search` first
- `--format json` appends raw JSON output to any command — use for scripting
- `--readable-amount` handles token decimals automatically for swap commands

## Wallet modes

| Mode | Setup needed | Capabilities |
|------|-------------|-------------|
| Anonymous | None | Read-only: prices, token data, signals, portfolio lookup by address |
| Agentic wallet | `onchainos wallet login` | Full: swap execution, send tokens, view own portfolio |

**Agentic wallet security:** TEE-secured execution — private keys never exposed. Supports 17+ networks with full OKX Wallet backing.

## Swap infrastructure

- **500+ DEX sources** aggregated for best price
- **MEV protection**: Solana via Jito (`--tips`), EVM via Flashbots (`--mev-protection`)
- **Pre-trade safety**: honeypot detection, tax scan, mint/freeze authority check
- **Gas-free on X Layer** via x402 protocol (`okx-x402-payment` skill)

## Workflow CLI commands

Run a complete multi-step workflow in one command:

```bash
onchainos workflow token-research --address <addr> [--chain solana]
onchainos workflow smart-money [--chain solana]
onchainos workflow new-tokens [--chain solana] [--stage MIGRATED]
onchainos workflow wallet-analysis --address <addr> [--chain ethereum]
onchainos workflow portfolio --address <addr> [--chains ethereum,solana]
```

## Composite CLI commands

Single commands that replace multiple individual tool calls:

```bash
# Token report: info + price-info + advanced-info + security scan (parallel)
onchainos token report --address <addr> --chain solana
```

## Skills location

Skills are available at `~/.openclaw/skills/` (symlinked from the template's `skills/` directory by `setup.sh`):

```
okx-dex-token      okx-dex-market     okx-dex-signal    okx-dex-trenches
okx-dex-swap       okx-dex-ws         okx-security       okx-wallet-portfolio
okx-agentic-wallet okx-onchain-gateway okx-defi-invest   okx-defi-portfolio
okx-x402-payment   okx-audit-log
```

## MCP server

`onchainos` also runs as a native MCP server exposing all CLI tools to any MCP-compatible client:

```bash
onchainos mcp   # starts JSON-RPC 2.0 server over stdio
```
