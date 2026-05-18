# Tools

Available skills and their capabilities are defined in `AGENTS.md`. This file covers CLI usage, conventions, and infrastructure.

## onchainos CLI

The official OKX OnchainOS CLI - built for AI, ready for Web3. Installed via `setup.sh`.

```bash
onchainos --version   # verify binary is available
onchainos --help      # full command reference
```

## CLI conventions

- `--chain` accepts chain names (e.g. `solana`, `ethereum`, `base`, `xlayer`) or chain indexes (e.g. `501`, `1`, `8453`)
- `--address` always expects a full contract address - never guess; resolve with `onchainos token search` first
- `--format json` appends raw JSON output to any command - use for scripting
- `--readable-amount` handles token decimals automatically for swap commands

## Wallet

This template requires the **agentic wallet**. Anonymous mode is **not supported** in this configuration - all on-chain operations require login.

| Step | Command | When |
|------|---------|------|
| Check state | `onchainos wallet status` | At every session start, before any on-chain command. |
| Start login | `onchainos wallet login <email> --locale <locale>` | When `wallet status` shows not logged in. Sends OTP to email. Validate `<email>` matches `^[^@[:space:]]+@[^@[:space:]]+\.[^@[:space:]]+$` before invoking. |
| Verify OTP | `onchainos wallet verify <code>` | After the user provides the OTP from email. Validate `<code>` matches `^[0-9]{6}$` before invoking. |
| API key auth | (automatic, no command) | When `OKX_API_KEY`, `OKX_SECRET_KEY`, `OKX_PASSPHRASE` are set as secrets. |

**Wallet skill:** `okx-agentic-wallet` (installed at `~/.onchainos/skills/okx-agentic-wallet/`). Use this skill for all wallet operations.

**Security:** TEE-secured execution - private keys never exposed. Spending is bounded by an on-chain limit; the agent cannot exceed it without the user's root key re-authorizing.

**Hard rule:** If `onchainos wallet status` does not return a valid address, refuse all on-chain commands and run the login flow defined in `BOOTSTRAP.md` Step 3.

## Swap infrastructure

- **Aggregated DEX sources** for best price
- **MEV protection**: Solana via Jito (`--tips`), EVM via Flashbots (`--mev-protection`)
- **Pre-trade safety**: honeypot detection, tax scan, mint/freeze authority check
- **Gas-free on X Layer** via OKX Agent Payments Protocol (`okx-agent-payments-protocol` skill)

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

Skills are installed by `setup.sh` into `~/.onchainos/skills/`:

```
okx-dex-token       okx-dex-market      okx-dex-signal      okx-dex-trenches
okx-dex-social      okx-dex-swap        okx-dex-bridge      okx-dex-strategy
okx-dex-ws          okx-security        okx-wallet-portfolio okx-agentic-wallet
okx-onchain-gateway okx-defi-invest     okx-defi-portfolio  okx-dapp-discovery
okx-audit-log       okx-growth-competition okx-how-to-play  okx-agent-payments-protocol
```

## MCP server

`onchainos` also runs as a native MCP server exposing all CLI tools to any MCP-compatible client:

```bash
onchainos mcp   # starts JSON-RPC 2.0 server over stdio
```
