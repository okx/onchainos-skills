# Wallet — CLI Reference

Exact syntax, parameters, and key return fields for `onchainos wallet` subcommands. Verify flags with `onchainos wallet <subcommand> --help` when unsure. Gas Station flags on `send` / `contract-call` are documented here; the Gas Station flow lives in [gas-station.md](gas-station.md).

---

## Account

### `wallet login`

Social login (Google / Apple / Email via browser), orchestrated in phases via `--phase` (default `init`).

```bash
onchainos wallet login [--phase init|open|poll] [--url <url>] [--session-id <id>]
```

| Param | Required | Description |
|---|---|---|
| `--phase` | No | `init` (default): mint + return the login URL, best-effort open the browser. `open`: open `--url` in the browser (internal orchestration step). `poll`: poll for the login result using the `init` session. |
| `--url` | For `open` | Login URL to open. Required when `--phase open`. |
| `--session-id` | No | Auth session id to poll (`--phase poll`). Defaults to the most recent `init` session when omitted. |

- `--phase init` → returns `loginUrl`, `authSessionId`, `opened`, and `nextSteps`. `nextSteps` always carries `completeLogin` (the exact `onchainos wallet login --phase poll --session-id <authSessionId>` command) and, only when `opened == false`, `openLoginUrl` (equal to `loginUrl`).
- `--phase poll` → persists the authenticated session, sends one best-effort device-registration heartbeat (`chainIndex=196`), then returns `accountId`, `accountName`, `loginType`, `isNew`, addresses, `totalValueUsd` (true `isNew` → new user; trigger the Policy Settings template only — no wallet-export guidance — see [portal-actions.md](wallet-portal-actions.md)). Only when a non-empty User `agenticId` is resolved may it query subscriptions/devices and return the best-effort `postLoginSubscriptions: { subscriptions, devices }` snapshot. The field is omitted when `agenticId` is unavailable or the lookup is empty/error/timeout, and `devices` is null when only the device lookup fails. A heartbeat failure never turns a successful login into a failed login.
- `status` → returns wallet/account/policy state only. It never queries or returns subscriptions/devices; the hidden legacy `--include-subscriptions` flag remains an accepted no-op for compatibility.

### `wallet add`

Add a new account under the logged-in user; auto-switches to it (no manual `switch` needed). Returns `accountId`, `accountName`.

### `wallet switch <account_id>`

Switch the active account.

### `wallet status`

Show login status and active account. Returns `email`, `loggedIn`, `currentAccountId`, `currentAccountName`, `accountCount`, and `policy` (null when not set). Policy fields: `singleTxLimit`/`singleTxFlag`, `dailyTransferTxLimit`/`dailyTransferTxFlag`/`dailyTransferTxUsed`, `dailyTradeTxLimit`/`dailyTradeTxFlag`/`dailyTradeTxUsed`. Also surfaces `loginType` (`email` / `ak`).

### `wallet addresses`

Show wallet addresses grouped by chain category (XLayer / EVM / Solana / Bitcoin / SUI).

```bash
onchainos wallet addresses [--chain <chain>]
```

Re-invoke this to copy any address verbatim — never reproduce an address from memory.

### `wallet qrcode --address <addr>`

Render a Unicode-block QR encoding `--address` verbatim (no URI scheme added). Output is plain art on stdout — render verbatim in a monospace block.

### `wallet logout`

Logout and clear stored credentials.

### `wallet chains`

List supported chains. Use `showName` for display, `realChainIndex` for the `--chain` value.

---

## Balance

### `wallet balance`

```bash
onchainos wallet balance [--all] [--chain <chain>] [--token-address <addr>] [--force]
```

| Param | Default | Description |
|---|---|---|
| `--all` | false | All accounts' assets (batch). Only when the user explicitly asks for all accounts. |
| `--chain` | all | Chain name or ID. Required with `--token-address`. |
| `--token-address` | — | Single token identifier. Requires `--chain`: `btc-brc20-<ticker>` for BRC-20, a Coin Type for SUI, or a contract address on account-model chains. |
| `--force` | false | Bypass caches; re-fetch accounts + balances. |

### User-facing Reply Templates

For one account, reply with:

```
Total assets: $${totalValueUsd}

- ${symbol}: ${balance} (approximately $${usdValue})
```

Repeat the asset line for every returned asset.

For `--all`, reply with:

```
Total assets across all accounts: $${totalValueUsd}

- ${symbol}: ${balance} (approximately $${usdValue})
```

Repeat the asset line for every returned asset across all accounts. Do not display `accountId` or `accountName`.

---

## Send

### `wallet send`

Send native or contract tokens.

```bash
onchainos wallet send --readable-amount <amount> --recipient <address> --chain <chain> \
  [--from <address>] [--contract-token <token>] [--fee-rate <sat-per-vB>] \
  [--brc20-outpoint <txHash:voutIndex>]... [--force] \
  [--gas-token-address <address>] [--relayer-id <id>] [--enable-gas-station]
```

| Param | Required | Description |
|---|---|---|
| `--readable-amount` | One of | Human-readable amount; required for Bitcoin and SUI, preferred otherwise. |
| `--amt` | One of | Raw minimal units for supported account-model chains. Mutually exclusive with `--readable-amount`. |
| `--recipient` | Yes | Recipient address for the selected chain. |
| `--chain` | Yes | Chain name or ID. |
| `--from` | No | Sender; defaults to selected account's address on the chain. |
| `--contract-token` | No | Non-native token identifier: contract address, SUI Coin Type, or `btc-brc20-<ticker>`. Omit for native. |
| `--fee-rate` | No | Bitcoin fee rate in sat/vB for the current BTC or BRC-20 transaction only; it does not change the default fee rate for future transactions. |
| `--brc20-outpoint` | No | Current transferable BRC-20 inscription selection; repeat to combine inputs. See [brc20-cli-reference.md](brc20-cli-reference.md). |
| `--force` | No | Re-run after a confirmed Confirming response. |
| `--gas-token-address`, `--relayer-id`, `--enable-gas-station` | No | Gas Station (Solana). Second-phase values from a Confirming response — never on the first call. See [gas-station.md](gas-station.md). |

Returns `txHash` (normal). Gas Station responses (`gasStationUsed`, `orderId`, Confirming scenes) → [gas-station.md](gas-station.md). On simulation failure, the CLI surfaces `executeErrorMsg` and does not broadcast.

---

## History

Providing any of `--order-id` / `--tx-hash` / `--uop-hash` → **detail mode** (single record); otherwise **list mode** (paged).

For BRC-20, this shared query handles direct-transfer history. Transfer-inscription status uses `wallet inscription status`.

```bash
# List
onchainos wallet history [--account-id <id>] [--chain <chain>] [--begin <ms>] [--end <ms>] [--page-num <cursor>] [--limit <n>]
# Detail (any one identifier)
onchainos wallet history --chain <chain> --order-id <id>
onchainos wallet history --chain <chain> --tx-hash <hash> [--address <addr>]
onchainos wallet history --chain <chain> --uop-hash <hash>
```

`--chain` is required in detail mode. Right after a Gas Station broadcast, poll by `--order-id` (txHash may be async).

List mode: always pass --limit (page size, default 20) and --page-num (page number) for paging. Detail mode returns a single record — do not pass --limit.

List fields: `cursor`, `orderList[]` with `txHash`, `txStatus`, `txTime`, `direction` (send/receive), `chainSymbol`, `coinSymbol`, `coinAmount`, `serviceCharge`, `confirmedCount`, `assetChange[]` (`coinSymbol`/`coinAmount`/`direction` in/out). Detail adds `failReason`, `explorerUrl`, `input[]`, `output[]`.

Transaction status is normalized by the CLI: `PENDING` (service `1` or `2`) · `ERROR` (service `3`) · `SUCCESS` (service `4`) · `CANCELLED` (service `6`). An unrecognized service value is preserved unchanged. `txTime` is Unix ms — convert for display.

---

## Contract Call

### `wallet contract-call`

Call an EVM contract (`--input-data`), Solana program (`--unsigned-tx`), or SUI PTB (`--sui-tx-bytes`) with TEE signing + auto-broadcast.

```bash
onchainos wallet contract-call --chain <chain> [--to <contract>] \
  [--amt <minimal_units>] [--input-data <hex>] [--unsigned-tx <base58>] [--sui-tx-bytes <base64>] \
  [--gas-limit <n>] [--from <address>] [--mev-protection] [--jito-unsigned-tx <base58>] \
  [--biz-type <type>] [--strategy <name>] [--aa-dex-token-addr <addr>] [--aa-dex-token-amount <amt>] \
  [--gas-token-address <addr>] [--relayer-id <id>] [--enable-gas-station] [--force]
```

| Param | Required | Description |
|---|---|---|
| `--to` | EVM/Solana | Contract/program address. Optional service metadata for SUI; do not invent it. |
| `--chain` | Yes | Chain name or ID. |
| `--amt` | No | Native value in minimal units (payable functions only). Default `"0"`. |
| `--input-data` | EVM | Hex calldata. Required for EVM. |
| `--unsigned-tx` | Solana | Base58 unsigned tx. Required for Solana. |
| `--sui-tx-bytes` | SUI | Base64 BCS TransactionData/PTB for the current wallet. Required for a SUI contract call. Never display or log it. |
| `--gas-limit` | No | EVM gas override; auto-estimated if omitted. |
| `--mev-protection` | No | MEV protection (Ethereum / BSC / Base / Solana); not supported with `--sui-tx-bytes`. See [mev-protection.md](wallet-mev-protection.md). |
| `--jito-unsigned-tx` | No | Jito bundle base58 tx. Required when `--mev-protection` on Solana. Never substitute `--unsigned-tx`. |
| `--biz-type` | No | Service business-type metadata. Do not set it unless the matched flow requires it. |
| `--gas-token-address`, `--relayer-id`, `--enable-gas-station` | No | Gas Station (Solana), second-phase only. See [gas-station.md](gas-station.md). |
| `--force` | No | Re-run after a confirmed Confirming response. |

Use exactly one chain-native payload: `--input-data` (EVM), `--unsigned-tx` (Solana), or `--sui-tx-bytes` (SUI). Returns `txHash` and `orderId`. Run `onchainos security tx-scan` before EVM/Solana calls. SUI PTB scanning is unavailable: do not claim the transaction is safe; require an integration preview, explicit user confirmation, and successful backend simulation.

---

## Sign Message

### `wallet sign-message`

personalSign (EIP-191, EVM + Solana) or EIP-712 typed data (EVM only).

```bash
onchainos wallet sign-message --chain <chain> --from <address> --message <message> [--type <type>] [--force]
```

| Param | Required | Description |
|---|---|---|
| `--chain` | Yes | Chain name or ID. |
| `--from` | Yes | Signer address. |
| `--message` | Yes | `personal`: arbitrary string. `eip712`: JSON typed-data string. |
| `--type` | No | `personal` (default, EVM + Solana) or `eip712` (EVM only — Solana returns an error). |
| `--force` | No | Re-run after a confirmed Confirming response. |

Returns `signature` (hex on EVM; base58 on Solana, plus `publicKey`).
