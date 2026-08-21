# BRC-20 CLI Reference

Use a synthetic BRC-20 token address: `btc-brc20-<ticker>`. The CLI normalizes its ticker to lowercase and converts `--readable-amount` to token minimal units. Direct transfers use complete service-returned inscription UTXOs whose token amounts sum to the requested amount.

## `wallet balance`

Query the current balance, transferable amount, and remaining inscribable amount for one BRC-20 ticker.

### Syntax

```bash
onchainos wallet balance --chain bitcoin --token-address <btc-brc20-ticker> [--force]
```

### Parameters

| Parameter | Required | Default | Description |
| --- | --- | --- | --- |
| `--chain` | Yes | — | Use `bitcoin`. |
| `--token-address` | Yes | — | BRC-20 token identifier in `btc-brc20-<ticker>` form. |
| `--force` | No | Disabled | Bypass balance caches only when the user explicitly asks to refresh or sync. |

Reply with:

```text
Total balance: ${totalAmount} ${ticker}, worth approximately $${totalUsd}
Currently transferable (already inscribed): ${transferableAmount} ${ticker}, worth approximately $${transferableUsd}, across ${count} transferable inscriptions with denominations ${denominations}
Remaining available to inscribe: ${remainingInscribableAmount} ${ticker}, worth approximately $${remainingInscribableUsd}
```

When `${count}` is zero, replace the transferable line with:

```text
Currently transferable (already inscribed): 0 ${ticker}, worth approximately $${transferableUsd}, with no transferable inscriptions
```

## `wallet utxo brc20-transferable`

Query current transferable inscriptions for one BRC-20 ticker. `sumValue` is the transferable total; `choices[]` contains indivisible inscriptions. Preserve each `selection` (`<txHash>:<voutIndex>`) verbatim; `tokenAmount` is the BRC-20 quantity and `utxoAmountSats` is its carrier BTC value. With `--readable-amount`, use `selectionPlan`: `EXACT_MATCH` supplies up to three `combinations[]` ordered by fewer inputs; use each combination's `selectedOutpoints[]` verbatim. `NO_EXACT_MATCH` requires a refreshed ticker balance before offering inscription; `SEARCH_LIMIT_EXCEEDED` means show the choices without claiming that no exact match exists.

### Syntax

```bash
onchainos wallet utxo brc20-transferable --chain bitcoin --token-address <btc-brc20-ticker> [--readable-amount <amount>]
```

### Parameters

| Parameter | Required | Default | Description |
| --- | --- | --- | --- |
| `--chain` | Yes | — | Use `bitcoin`. |
| `--token-address` | Yes | — | BRC-20 token identifier in `btc-brc20-<ticker>` form. |
| `--readable-amount` | No | — | Human-readable target amount used to find up to three exact UTXO combinations. |

## `wallet send`

Transfer BRC-20 with one current exact combination returned by `wallet utxo brc20-transferable`. Proceed when one combination is returned; ask the user to choose when several are returned. Before the second confirmation, if a selected outpoint is no longer available, show the refreshed plan and ask again; afterward, follow the 10-second BTC/BRC-20 confirmation reuse rule in [wallet.md](wallet.md).

The initial command refreshes the selected outpoints, validates their availability, uniqueness, and amount sum, signs, and returns ordinary `confirming` before broadcast. Display the complete confirmation, then end with: `Confirm broadcasting and creating this inscription at the current fee rate? To change it, reply with a new sat/vB value.` Execute `next` only after explicit confirmation. If the user supplies a new sat/vB value, rerun without `--force`, display the fresh preview, and state: `The custom fee rate applies only to this transaction and does not change the default fee rate for future transactions.` After the user confirms that refreshed preview, apply the 10-second BTC/BRC-20 confirmation reuse rule in [wallet.md](wallet.md).

The confirmed continuation returns `state=PENDING`, `txHash`, and `orderId`.

### Syntax

```bash
onchainos wallet send --chain bitcoin --contract-token <btc-brc20-ticker> --readable-amount <amount> --recipient <address> --brc20-outpoint <txHash:voutIndex> [--brc20-outpoint <txHash:voutIndex> ...] [--fee-rate <sat-per-vB>] [--from <address>] [--force]
```

### Parameters

| Parameter | Required | Default | Description |
| --- | --- | --- | --- |
| `--chain` | Yes | — | Use `bitcoin`. |
| `--contract-token` | Yes | — | BRC-20 token identifier in `btc-brc20-<ticker>` form. |
| `--readable-amount` | Yes | — | Exact human-readable BRC-20 amount. |
| `--recipient` | Yes | — | Recipient Bitcoin address. |
| `--brc20-outpoint` | Yes | — | Selected transferable inscription in `<txHash>:<voutIndex>` form; repeat for every selected input. |
| `--fee-rate` | No | Service default | Decimal fee rate of at least `0.1` sat/vB for this transaction only. |
| `--from` | No | Active wallet address | Sender Bitcoin address. |
| `--force` | Continuation only | Disabled | Use only through the exact `next` returned after explicit confirmation. |

Query a submitted direct transfer through the shared wallet history flow, not inscription status:

```bash
onchainos wallet history --chain bitcoin (--tx-hash <hash> | --order-id <id>)
```

## `wallet inscription create`

Create a standalone asynchronous transfer inscription to the current Bitcoin address only after an explicit inscription request. If no direct-transfer combination exists, refresh the ticker balance before offering this command.

Run initially without `--force`. It stops after `unsignedInfo` and returns ordinary `confirming` with `scene="btc_inscription"`; `preview.feeReadable` is nullable, and nothing has been signed or submitted. Display the complete preview and the same fee-rate prompt and one-transaction fee statement used by `wallet send`. A new sat/vB value requires a fresh preview without `--force`. After the user confirms that refreshed preview, apply the 10-second BTC/BRC-20 confirmation reuse rule in [wallet.md](wallet.md).

The confirmed `next` signs, calls `sign-tx`, and batch-broadcasts the ordered inscription transactions. Show returned `state=INSCRIBING`, `txHash`, `orderId`, `broadcasts`, and `nextSteps.checkInscriptionStatus` verbatim, render the submission template, and stop. Do not query automatically or auto-send after `READY_TO_TRANSFER`.

### Syntax

```bash
onchainos wallet inscription create --chain bitcoin --token-address <btc-brc20-ticker> --readable-amount <amount> [--from <address>] [--fee-rate <sat-per-vB>] [--operation-token <token>] [--force]
```

### Parameters

| Parameter | Required | Default | Description |
| --- | --- | --- | --- |
| `--chain` | Yes | — | Use `bitcoin`. |
| `--token-address` | Yes | — | BRC-20 token identifier in `btc-brc20-<ticker>` form. |
| `--readable-amount` | Yes | — | Exact human-readable amount to inscribe. |
| `--from` | No | Active wallet address | Sender Bitcoin address. |
| `--fee-rate` | No | Service default | Decimal fee rate of at least `0.1` sat/vB for this inscription only. |
| `--operation-token` | Continuation only | — | Use only when supplied by the exact `next` returned after preview. |
| `--force` | Continuation only | Disabled | Use only through the exact `next` returned after explicit confirmation. |

### Submission template

Translate this template to the user's language. Substitute only returned values and the fee from the confirmed preview; omit a line when its value is unavailable.

```text
The inscription transaction was submitted but is not fully confirmed:

- Asset: ${readableAmount} ${ticker}
- Current status: ${state}
- Bitcoin confirmations: ${confirmations}
- Reveal order ID: ${orderId}
- Reveal txHash: ${txHash}
- Current inscription fee: ${inscriptionFeeSats} sats
- Transferability: ${transferability}

You can reply "Check the result", and I will run this complete command for you:

${nextSteps.checkInscriptionStatus}
```

## `wallet inscription status`

Check one submitted BRC-20 transfer inscription after the user asks for its result. Run once; if still pending, show the current status and returned continuation, then stop. Do not loop, poll, sleep, or promise automatic checks. Status can be `INSCRIBING`, `WAITING_CONFIRMATION`, `WAITING_INDEXER`, `READY_TO_TRANSFER`, `FAILED`, or `UNKNOWN`; `READY_TO_TRANSFER` supplies read-only `nextSteps.queryBrc20TransferableUtxos` to refresh the transferable list.

### Syntax

```bash
onchainos wallet inscription status --chain bitcoin (--tx-hash <hash> | --order-id <id>)
```

### Parameters

| Parameter | Required | Default | Description |
| --- | --- | --- | --- |
| `--chain` | Yes | — | Use `bitcoin`. |
| `--tx-hash` | One ID required | — | Reveal transaction hash. |
| `--order-id` | One ID required | — | Reveal order ID. |
