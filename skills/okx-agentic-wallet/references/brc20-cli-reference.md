# BRC-20 CLI Reference

Use a synthetic BRC-20 token address: `btc-brc20-<ticker>`. The CLI normalizes its ticker to lowercase. Transfer-inscription and direct-transfer target amounts use `--readable-amount` with exact token-decimal conversion. A direct BRC-20 transfer uses one or more complete service-returned inscription UTXOs whose token amounts sum to the requested amount.

## Balance

```bash
onchainos wallet balance --chain bitcoin --token-address <btc-brc20-ticker> [--force]
onchainos wallet balance --all
```

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

## Shared UTXO Availability Views

The same `availability-details` endpoint also serves this BRC-20-context UTXO request:

| Intent | Command | Request `queryType` |
| --- | --- | --- |
| UTXOs whose asset occupancy the user removed | `onchainos wallet utxo list --chain bitcoin` | `USER_IGNORED_LIST` |

`USER_IGNORED_LIST` is the address-level user-removed asset-occupancy view shared by native BTC and BRC-20 flows. `BRC20_TRANSFERABLE_UTXO_LIST` is the ticker-specific BRC-20 transferable-inscription view documented above. Render protocol or ticker attribution only when the response explicitly supplies it.

## BRC-20 Transfer

```bash
onchainos wallet send --chain bitcoin --contract-token <btc-brc20-ticker> --readable-amount <amount> --recipient <address> --brc20-outpoint <txHash:voutIndex> [--brc20-outpoint <txHash:voutIndex> ...] [--fee-rate <sat-per-vB>]
```

BRC-20 transfer uses the same external CLI interaction as ordinary `wallet send`. Repeat `--brc20-outpoint` for every item in the confirmed combination. The CLI refreshes the transferable list, resolves every selected outpoint, rejects duplicates, verifies their token amount sum, and prepares one transaction.

The initial command signs and returns the ordinary `confirming` response before broadcast. The Agent **MUST** display its complete confirmation and wait for a new explicit user confirmation before executing `next`. The confirmed continuation broadcasts and returns `state=PENDING`, `txHash`, and `orderId` on success.

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

The initial creation command stops after `unsignedInfo` and returns ordinary `confirming` with `scene="btc_inscription"`; `preview.feeReadable` is nullable. It has not signed or submitted. After explicit confirmation, `next` completes local signing, calls `sign-tx`, and batch-broadcasts the ordered inscription transactions. Submitted output contains `state=INSCRIBING`, top-level `txHash` and `orderId`, ordered `broadcasts`, and `nextSteps.checkInscriptionStatus`. Show those identifiers and the complete continuation command; creation is a standalone asynchronous transfer inscription to the current Bitcoin address.

`--fee-rate` is optional, accepts a decimal value of at least `0.1` sat/vB, and is sent as numeric `txParam.feeRate`. It applies only to this inscription; omitting it on the next transaction uses the service default rate.

Status can be `INSCRIBING`, `WAITING_CONFIRMATION`, `WAITING_INDEXER`, `READY_TO_TRANSFER`, `FAILED`, or `UNKNOWN`. `READY_TO_TRANSFER` supplies read-only `nextSteps.queryBrc20TransferableUtxos`, which refreshes the transferable list for the returned token address.
