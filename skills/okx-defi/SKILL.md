---
name: okx-defi
description: "For discovering and managing OKX-aggregated DeFi products and positions across protocols and chains. Use it for yield, APY, or TVL discovery and history; DeFi deposits, staking, liquidity, withdrawals, rewards, lending, CLMM analysis, and positions or holdings. Trigger phrases: earn yield, best APY, DeFi product, APY/TVL history, deposit or stake for yield, provide/remove liquidity, withdraw/redeem a DeFi position, claim DeFi rewards, borrow/repay a lending position, CLMM, DeFi portfolio, DeFi 持仓. Requests targeting a named third-party DApp—Aave, Lido, PancakeSwap, Uniswap, Curve, Compound, Morpho, Pendle, Kamino, Raydium, Hyperliquid, or Polymarket—route to okx-dapp-discovery; generic swaps, wallet balances, and transaction broadcast route to okx-agentic-wallet; token search, market prices, K-lines, and token charts route to okx-dex-market. DeFi APY/TVL and V3 liquidity charts stay here."
license: MIT
metadata:
  author: okx
  version: "4.8.2-beta"
  homepage: "https://web3.okx.com"
---

# OKX DeFi (experimental merge of okx-defi-invest + okx-defi-portfolio)

Multi-chain, OKX-aggregated DeFi in two capabilities behind one skill. Both wrap the same `onchainos defi` CLI command group.

## Pre-flight Checks

At the start of each thread, complete the checks in `../okx-agentic-wallet/_shared/preflight.md`. If missing, read `_shared/preflight.md`.

## Intent Routing

| User Intent | Reference |
|---|---|
| Discover / search DeFi products, find best APY | [invest.md](references/invest.md) |
| Product detail (APY, TVL, accepted tokens) | [invest.md](references/invest.md) |
| Deposit / stake / provide liquidity | [invest.md](references/invest.md) |
| Withdraw / redeem a position (full or partial) | [invest.md](references/invest.md) |
| Claim rewards (platform / investment / V3 fee / bonus / unlocked principal) | [invest.md](references/invest.md) |
| APY history, TVL history, V3 depth / price charts | [invest.md](references/invest.md) |
| View DeFi positions / holdings overview (持仓) | [portfolio.md](references/portfolio.md) |
| Per-protocol position detail (持仓详情) | [portfolio.md](references/portfolio.md) |
| Exact parameters / return schemas — invest & charts commands | [invest-cli-reference.md](references/invest-cli-reference.md) |
| Exact parameters / return schemas — positions commands | [portfolio-cli-reference.md](references/portfolio-cli-reference.md) |
| Errors / failed deposits / expired calldata | [invest-troubleshooting.md](references/invest-troubleshooting.md) |
| Errors / empty positions / address-format issues | [portfolio-troubleshooting.md](references/portfolio-troubleshooting.md) |

Typical flow spans both: view positions (Portfolio) → redeem or claim (Invest). Read both reference files when the request chains them.

## Skill Routing

- For DApp-named investing/lending/staking/positions ("on Aave", "my Hyperliquid balance") → use `okx-dapp-discovery`
- For token price/chart or token search by name/contract → use `okx-dex-market`
- For DEX spot swap execution → use `okx-agentic-wallet`
- For wallet token balances → use `okx-agentic-wallet`
- For broadcasting signed transactions → use `okx-agentic-wallet`
- For Agentic Wallet login, balance, contract-call → use `okx-agentic-wallet`

## Chain Support

CLI resolves chain names automatically (e.g. `ethereum` → `1`, `bsc` → `56`, `solana` → `501`). Full alias table: `references/portfolio.md` §Chain Support.

## Step 0: Address Resolution (shared by both capabilities)

When the user does NOT provide a wallet address, resolve it automatically from the Agentic Wallet **before** running any defi command:

```
1. onchainos wallet status          → check if logged in, get active account
2. onchainos wallet addresses       → get addresses grouped by chain category:
                                       - XLayer addresses
                                       - EVM addresses (Ethereum, BSC, Polygon, etc.)
                                       - Solana addresses
3. Match address to target chain:
   - EVM chains → use EVM address
   - Solana     → use Solana address
   - XLayer     → use XLayer address
```

Rules:
- If the user provides an explicit address, use it directly — skip this step
- If wallet is not logged in, ask the user to log in first (→ `okx-agentic-wallet`) or provide an address manually
- If the user says "check all accounts" or "all wallets", use `wallet balance --all` to get all account IDs, then `wallet switch <id>` + `wallet addresses` for each account
- Always confirm the resolved address with the user before proceeding if the account has multiple addresses of the same type

## Address-Chain Compatibility (shared — CRITICAL)

The `--address` and chain parameters must be compatible. EVM addresses (`0x…`) can only query EVM chains; Solana addresses (base58) can only query `solana`. Never mix them — the API will return error 84019 (Address format error).

- `0x…` address → only pass EVM chains: `ethereum,bsc,polygon,arbitrum,base,xlayer,avalanche,optimism,fantom,linea,scroll,zksync`
- base58 address → only pass `solana`
- Sui address → only pass `sui`; Tron address (`T…`) → only pass `tron`; TON address → only pass `ton`
- If the user wants positions across both EVM and Solana, make **two separate calls** with the respective addresses

## Global Notes

- The wallet address parameter for ALL defi commands is `--address`
- `defi positions` uses `--chains` (plural, comma-separated); `defi position-detail` uses `--chain` (singular)
- For CLI parameter details, see `references/invest-cli-reference.md` (invest & charts) and `references/portfolio-cli-reference.md` (positions)
