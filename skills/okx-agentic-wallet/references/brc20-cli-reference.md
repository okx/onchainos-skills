# BRC-20 CLI Reference

Use a synthetic BRC-20 token address: `btc-brc20-<ticker>`. The CLI normalizes its ticker to lowercase. Transfer-inscription and direct-transfer target amounts use `--readable-amount` with exact token-decimal conversion. A direct BRC-20 transfer uses one or more complete service-returned inscription UTXOs whose token amounts sum to the requested amount.

### User-facing Reply Templates

For a ticker balance, reply with:

`Total balance: ${totalAmount} ${ticker}, worth approximately $${totalUsd}`

`Currently transferable (already inscribed): ${transferableAmount} ${ticker}, worth approximately $${transferableUsd}, across ${count} transferable inscriptions with denominations ${denominations}`

`Remaining available to inscribe: ${remainingInscribableAmount} ${ticker}, worth approximately $${remainingInscribableUsd}`

When `${count}` is zero, use: `Currently transferable (already inscribed): 0 ${ticker}, worth approximately $${transferableUsd}, with no transferable inscriptions`.

## Transferable BRC-20 UTXOs

```bash
onchainos wallet utxo brc20-transferable --chain bitcoin --token-address <btc-brc20-ticker> [--readable-amount <amount>]
```

For this query, the user only needs to provide the BRC-20 ticker/token address. The CLI derives the active Bitcoin address and chain from the logged-in wallet and returns the current transferable choices.

The command calls `POST /priapi/v5/wallet/agentic/utxo/availability-details` with the current Bitcoin `address`, the runtime-resolved Bitcoin `chainIndex`, `queryType=BRC20_TRANSFERABLE_UTXO_LIST`, and normalized `tokenAddress`. The CLI exposes the service total as `sumValue` and the individual transferable inscriptions as `choices[]`.

The response exposes `choices[]`. Preserve each `selection` (`<txHash>:<voutIndex>`) verbatim. `tokenAmount` / `tokenAmountRaw` is the BRC-20 quantity; `utxoAmountSats` is the carrier UTXO's BTC value in sats. Each choice is one whole, indivisible transfer inscription; multiple choices may be combined in one transaction.

With `--readable-amount`, read `selectionPlan`:

- `EXACT_MATCH`: `combinations[]` contains at most three exact options, ordered by fewer inputs. Each option provides `selectedCount`, `selectedOutpoints[]`, and `selectedChoices[]`.
- `NO_EXACT_MATCH`: the complete bounded search found no exact subset for the requested amount.
- `SEARCH_LIMIT_EXCEEDED`: the search reached 100,000 distinct amount states; present the current choices without claiming that no exact subset exists.

The CLI returns at most three combinations so the user can make one clear selection. It performs all arithmetic in token minimal units.

Use only a current service-returned exact combination for a direct transfer. For `NO_EXACT_MATCH`, refresh the ticker balance before offering a separate inscription. For `SEARCH_LIMIT_EXCEEDED`, show returned choices without claiming that no exact match exists.

## BRC-20 Transfer

```bash
onchainos wallet send --chain bitcoin --contract-token <btc-brc20-ticker> --readable-amount <amount> --recipient <address> --brc20-outpoint <txHash:voutIndex> [--brc20-outpoint <txHash:voutIndex> ...] [--fee-rate <sat-per-vB>]
```

BRC-20 transfer uses the same external CLI interaction as ordinary `wallet send`. Repeat `--brc20-outpoint` for every item in the confirmed combination. The CLI refreshes the transferable list, resolves every selected outpoint, rejects duplicates, verifies their token amount sum, and prepares one transaction.

The initial command signs and returns the ordinary `confirming` response before broadcast. The Agent **MUST** display its complete confirmation, then end with: `Confirm broadcasting and creating this inscription at the current fee rate? To change it, reply with a new sat/vB value.` If the user supplies a new sat/vB value, rerun the initial transfer command with `--fee-rate <value>` and without `--force`, display the complete fresh preview, and state: `The custom fee rate applies only to this transaction and does not change the default fee rate for future transactions.` Otherwise execute `next` only after explicit confirmation. The confirmed continuation broadcasts and returns `state=PENDING`, `txHash`, and `orderId` on success.

`--fee-rate` is optional, accepts a decimal value of at least `0.1` sat/vB, and is added as numeric `txParam.feeRate` alongside the selected inputs. It applies only to this transaction; omitting it on the next transfer uses the service default rate.

Start a direct transfer after the amount-aware query has been shown and one current combination is fixed. A single returned combination can proceed to the normal transfer confirmation; several returned combinations require the user to choose one. If the refreshed list no longer contains every outpoint, show the new plan and ask the user to select again. A later explicit inscription request remains the separate flow below.

Query a submitted direct transfer through the shared wallet history flow, not inscription status:

```bash
onchainos wallet history --chain bitcoin (--tx-hash <hash> | --order-id <id>)
```

## Transfer Inscription

```bash
onchainos wallet inscription create --chain bitcoin --token-address <btc-brc20-ticker> --readable-amount <amount> [--from <address>] [--fee-rate <sat-per-vB>]
onchainos wallet inscription status --chain bitcoin (--tx-hash <hash> | --order-id <id>)
```

1. **Prepare the preview.** The initial creation command stops after `unsignedInfo` and returns ordinary `confirming` with `scene="btc_inscription"`; `preview.feeReadable` is nullable. It has not signed or submitted. Display the complete preview, then end with: `Confirm broadcasting and creating this inscription at the current fee rate? To change it, reply with a new sat/vB value.`
2. **Confirm or update the fee rate.** Continue only after explicit confirmation. If the user supplies a new sat/vB value, rerun the initial creation command with `--fee-rate <value>` and without `--force`, display the complete fresh preview, and state: `The custom fee rate applies only to this transaction and does not change the default fee rate for future transactions.`
3. **Sign and submit.** After explicit confirmation, `next` completes local signing, calls `sign-tx`, and batch-broadcasts the ordered inscription transactions. Submitted output contains `state=INSCRIBING`, top-level `txHash` and `orderId`, ordered `broadcasts`, and `nextSteps.checkInscriptionStatus`.
4. **Render, guide, and stop.** Render the submission template below, including its prompt for the user to request a result check, then stop. Creation is a standalone asynchronous transfer inscription to the current Bitcoin address; do not query the result automatically.

`--fee-rate` is optional, accepts a decimal value of at least `0.1` sat/vB, and is sent as numeric `txParam.feeRate`. It applies only to this inscription; omitting it on the next transaction uses the service default rate.

### User-facing Submission Template

Translate this template to the user's language. Substitute only returned values and the fee from the confirmed preview; omit a line when its value is unavailable.

```
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

Return this response immediately after submission. Do not execute the status command automatically, loop, poll, sleep, or promise a later automatic check. When the user asks to check the result, run the returned status command once. If the result is still pending, render the current status and its returned continuation command, then stop again.

Status can be `INSCRIBING`, `WAITING_CONFIRMATION`, `WAITING_INDEXER`, `READY_TO_TRANSFER`, `FAILED`, or `UNKNOWN`. `READY_TO_TRANSFER` supplies read-only `nextSteps.queryBrc20TransferableUtxos`, which refreshes the transferable list for the returned token address.

Treat a transfer inscription as a separate asynchronous write: start without `--force`, follow its returned confirmation, show returned identifiers and status continuation verbatim, and never auto-send after `READY_TO_TRANSFER`.
