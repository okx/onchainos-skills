# BRC-20 CLI Reference

Use a synthetic BRC-20 token address: `btc-brc20-<ticker>`. The CLI normalizes its ticker to lowercase and converts `--readable-amount` to token minimal units. Direct transfers use complete service-returned inscription UTXOs whose token amounts sum to the requested amount.

## `wallet balance`

### Intent

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

### Response

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

### Intent

Query current transferable inscriptions for one BRC-20 ticker. Add `--readable-amount` when the user wants exact transfer combinations for a target amount.

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

### Response

The CLI derives the active Bitcoin address from the logged-in wallet. The response exposes the transferable total as `sumValue` and individual inscriptions as `choices[]`. Preserve each `selection` (`<txHash>:<voutIndex>`) verbatim. `tokenAmount` / `tokenAmountRaw` is the BRC-20 quantity, and `utxoAmountSats` is the carrier UTXO's BTC value in sats. Each choice is indivisible; several choices may be combined in one transaction.

With `--readable-amount`, read `selectionPlan`:

- `EXACT_MATCH`: `combinations[]` contains at most three exact options, ordered by fewer inputs. Each option provides `selectedCount`, `selectedOutpoints[]`, and `selectedChoices[]`.
- `NO_EXACT_MATCH`: the complete bounded search found no exact subset for the requested amount. Refresh the ticker balance before offering a separate inscription.
- `SEARCH_LIMIT_EXCEEDED`: the search reached 100,000 distinct amount states. Show the returned choices without claiming that no exact subset exists.

Use only a current service-returned exact combination for a direct transfer.

## `wallet send`

### Intent

Transfer BRC-20 with a current exact combination returned by `wallet utxo brc20-transferable`. If one combination is returned, proceed to preview; if several are returned, ask the user to select one. If any selected outpoint is no longer available after refresh, show the new plan and ask again.

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

### Response

The CLI refreshes the transferable list, resolves every selected outpoint, rejects duplicates, verifies their token amount sum, and prepares one transaction.

The initial command signs and returns ordinary `confirming` before broadcast. The Agent **MUST** display the complete confirmation, then end with: `Confirm broadcasting and creating this inscription at the current fee rate? To change it, reply with a new sat/vB value.` Execute `next` only after explicit confirmation.

If the user supplies a new sat/vB value, rerun the initial command with `--fee-rate <value>` and without `--force`, display the complete fresh preview, and state: `The custom fee rate applies only to this transaction and does not change the default fee rate for future transactions.`

The confirmed continuation returns `state=PENDING`, `txHash`, and `orderId`. Query a submitted direct transfer through the shared wallet history flow, not inscription status:

```bash
onchainos wallet history --chain bitcoin (--tx-hash <hash> | --order-id <id>)
```

## `wallet inscription create`

### Intent

Create a standalone asynchronous transfer inscription to the current Bitcoin address when the requested amount needs a new transferable inscription. Do not start this command merely because an exact direct-transfer combination is unavailable; refresh the ticker balance first and proceed only for an explicit inscription request.

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

### Response

1. Run the initial command without `--force`. It stops after `unsignedInfo` and returns ordinary `confirming` with `scene="btc_inscription"`; `preview.feeReadable` is nullable. It has not signed or submitted. Display the complete preview, then end with: `Confirm broadcasting and creating this inscription at the current fee rate? To change it, reply with a new sat/vB value.`
2. Continue only after explicit confirmation. If the user supplies a new sat/vB value, rerun the initial command with `--fee-rate <value>` and without `--force`, display the complete fresh preview, and state: `The custom fee rate applies only to this transaction and does not change the default fee rate for future transactions.`
3. After explicit confirmation, `next` completes local signing, calls `sign-tx`, and batch-broadcasts the ordered inscription transactions. Submitted output contains `state=INSCRIBING`, top-level `txHash` and `orderId`, ordered `broadcasts`, and `nextSteps.checkInscriptionStatus`; show returned identifiers and the status continuation verbatim.
4. Render the template below, including its prompt for the user to request a result check, then stop. Do not query the result automatically or auto-send after `READY_TO_TRANSFER`.

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

### Intent

Check one submitted BRC-20 transfer inscription after the user asks for its result.

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

### Response

Run the returned status command once. If the result is still pending, render the current status and its returned continuation command, then stop again. Do not loop, poll, sleep, or promise a later automatic check.

Status can be `INSCRIBING`, `WAITING_CONFIRMATION`, `WAITING_INDEXER`, `READY_TO_TRANSFER`, `FAILED`, or `UNKNOWN`. `READY_TO_TRANSFER` supplies read-only `nextSteps.queryBrc20TransferableUtxos`, which refreshes the transferable list for the returned token address.
