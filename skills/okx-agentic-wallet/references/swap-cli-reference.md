# DEX Swap — CLI Reference

Syntax, parameters, and key return fields for `onchainos swap` subcommands. Verify with `onchainos swap <subcommand> --help` when unsure.

## `swap chains`

Supported chains for the DEX aggregator (no params). Returns per chain: `chainIndex`, `chainName`, `dexTokenApproveAddress` (router address for approvals).

## `swap liquidity --chain <chain>`

Available liquidity sources on a chain. Returns `id`, `name` (e.g. `Uniswap V3`), `logo`.

## `swap quote`

Read-only price estimate. **No `--slippage`.**

```bash
onchainos swap quote --from <addr> --to <addr> --readable-amount <amt> --chain <chain> [--swap-mode <exactIn|exactOut>]
```

`--readable-amount` (human units, CLI converts) or `--amount` (raw minimal units) — one of. Key return: `toTokenAmount`, `fromTokenAmount`, `estimateGasFee`, `tradeFee` (USD), `priceImpactPercent`, `dexRouterList[]` (`dexName`, `percentage`), and per-side `fromToken` / `toToken` with `isHoneyPot`, `taxRate`, `decimal`, `tokenUnitPrice`. Each route also carries the always-on SW2 fields `action` (`ok` / `warn` / `block`) and `reason` (semicolon-joined, deduplicated; empty string when `ok`) — the CLI classifies honeypot / tax-rate risk per route; read the returned verdict and do not recompute it.

The normal-quote response also carries **`walletBalance`** — the wallet's from-token balance. Always present: a string on success, JSON `null` when the balance query failed (never `0`, never omitted, never a string sentinel).

### Insufficient-balance scene (`swap_insufficient_balance`)

When the quote detects the from-token balance is below the requested amount, it emits a flat top-level object (`ok:false` at the root, NOT the `JsonOutput` envelope) instead of a normal quote. Business recovery: [swap.md](swap.md) → Insufficient-Balance Top-up Recovery. Presentation before the user chooses funding: [swap-output-templates.md](swap-output-templates.md).

| Field | Type | Meaning |
|---|---|---|
| `scene` | string | Always `"swap_insufficient_balance"`. |
| `phase` / `decision` / `reason` | string | `swap_funding` / `blocked` / `insufficient_balance`. |
| `nextAction` | array | Registered `fund_account` action with empty params. |
| `payload` | object | Business scene fields plus common `fundingTarget`, `qr`, and `fundingNeed`. |
| `chainIndex` | string | Source chain index. |
| `chainName` | string | Canonical chain display name. |
| `sameNetworkRequired` | bool | `true` — the top-up must arrive on this same `chainName` network. |
| `gasFree` | bool | `true` for X Layer (196) — render the gas-free note. |
| `fromAsset` | object | `{ symbol, tokenAddress }` (full from-token CA). |
| `requestedAmount` | string | Requested swap amount (readable units). |
| `walletBalance` | string | Current from-token balance (readable units). |
| `shortfall` | string \| null | Exact readable `requestedAmount - walletBalance`; `null` when either value is not a plain decimal. |
| `fundingAddress` | string | Current account's own receive address on the source chain. |
| `qr` | object | Deposit-address QR — same shape as [wallet-cli-reference.md](wallet-cli-reference.md#common-qr-object). |

The common `payload.fundingTarget`, `payload.qr`, and `payload.fundingNeed`
fields are owned by the CLI Funding helper. The result carries no saved quote,
resume token, executable command, or prior confirmation.

## `swap execute`

One-shot: quote → approve (if needed) → sign → broadcast. Honeypot and price impact >10% are blocked internally.

```bash
onchainos swap execute --from <addr> --to <addr> --readable-amount <amt> --chain <chain> --wallet <addr> \
  [--slippage <pct>] [--gas-level <slow|average|fast>] [--swap-mode <exactIn|exactOut>] \
  [--mev-protection] [--tips <sol>] [--max-auto-slippage <pct>] [--force]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--from` / `--to` | Yes | — | Source / destination token address. |
| `--readable-amount` / `--amount` | One of | — | Human units (converted) / raw minimal units. |
| `--chain` | Yes | — | Chain name or ID. |
| `--wallet` | Yes | — | User's wallet address. |
| `--slippage` | No | autoSlippage | Percent (e.g. `"1"`). Omit for autoSlippage. |
| `--gas-level` | No | `average` | `slow` / `average` / `fast`. |
| `--mev-protection` | No | — | EVM (Ethereum / BSC / Base). |
| `--tips` | No | — | Jito tips in SOL (Solana only). Mutually exclusive with `computeUnitPrice`. |
| `--max-auto-slippage` | No | — | Caps autoSlippage upper bound; only when `--slippage` omitted. |
| `--force` | No | — | Bypass risk warning 81362 — only after explicit user confirmation (see [swap-troubleshooting.md](swap-troubleshooting.md)). |

Returns `approveTxHash?`, `swapTxHash`, `fromAmount`, `toAmount`, `priceImpact`, `gasUsed`, `nextSteps`.

## `swap swap` (calldata only)

Returns unsigned tx data; does NOT sign or broadcast.

```bash
onchainos swap swap --from <addr> --to <addr> --readable-amount <amt> --chain <chain> --wallet <addr> \
  [--slippage <pct>] [--swap-mode <exactIn|exactOut>] [--tips <sol>] [--max-auto-slippage <pct>]
```

Returns `routerResult` (same shape as `quote`, including the always-on per-route `action` / `reason`) and `tx` (`to`, `data`, `gas`, `gasPrice`, `value`, `minReceiveAmount`). Present the pair summary + tx fields; for an EVM non-native token, run `swap approve` first and present its calldata separately. Solana: `--tips` embeds Jito calldata. EVM: `--mev-protection` is not supported here — recommend a MEV-protected RPC.

## `swap approve`

ERC-20 approval calldata (advanced/manual use).

```bash
onchainos swap approve --token <addr> --amount <minimal_units> --chain <chain>
```

Returns `data` (approval calldata — send the tx to the **token contract**, not `dexContractAddress`), `dexContractAddress` (spender, already encoded in `data`), `gasLimit`, `gasPrice`.

## `swap check-approvals`

Check an ERC-20 allowance for a token / spender.

```bash
onchainos swap check-approvals --chain <chain> --address <owner> --token <addr> [--spender <addr>]
```

`--spender` defaults to the OKX DEX router.
