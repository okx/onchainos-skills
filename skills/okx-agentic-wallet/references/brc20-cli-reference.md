# BRC-20 CLI Reference

Use a synthetic BRC-20 token address: `btc-brc20-<ticker>`. The CLI normalizes its ticker to lowercase. Transfer-inscription amounts use `--readable-amount` and exact token-decimal conversion. A direct BRC-20 transfer instead uses one user-selected transferable inscription UTXO and takes its exact token amount from the service response.

## Balance

```bash
onchainos wallet balance --chain bitcoin --token-address <btc-brc20-ticker> [--force]
```

This reports total holdings only. For a user-facing BRC-20 balance composition, also run the transferable-UTXO query below and report its exact whole-inscription denominations separately. Do not derive an uninscribed amount by subtraction unless the CLI/service explicitly returns that semantic.

## Transferable BRC-20 UTXOs

```bash
onchainos wallet utxo brc20-transferable --chain bitcoin --token-address <btc-brc20-ticker>
```

For this query, the user only needs to provide the BRC-20 ticker/token address. The CLI derives the active Bitcoin address and chain from the logged-in wallet and returns the current transferable choices.

The response exposes `choices[]`. Preserve each `selection` (`<txHash>:<voutIndex>`) verbatim for the user's choice. `tokenAmount` / `tokenAmountRaw` is the BRC-20 quantity; `utxoAmountSats` is the carrier UTXO's BTC value in sats. Do not interchange them. Each choice is one whole, indivisible transfer inscription. A requested user amount may filter exact `tokenAmount` matches, but it never changes the amount represented by a choice.

## BRC-20 Transfer

```bash
onchainos wallet send --chain bitcoin --contract-token <btc-brc20-ticker> --recipient <address> --brc20-outpoint <txHash:voutIndex>
```

BRC-20 transfer uses the same external CLI interaction as ordinary `wallet send`. The CLI refreshes the transferable list and resolves the selected outpoint before preparing the transaction, so stale choices are not silently used. It owns minimal-unit conversion, request construction, signing, and ordinary single-transaction broadcast; do not reproduce those steps in the Skill or persist intermediate transaction state in Keyring. It does not add a BRC-20-specific preview or preparation stage.

If broadcast returns the shared backend confirmation code, the CLI returns the ordinary `confirming` response with the service `message`; after explicit confirmation, re-run the same command with `--force`. Otherwise, success returns `state=PENDING`, `txHash`, and `orderId`.

Do not start a direct transfer until the transferable list has been shown and exactly one current choice is fixed. If a requested amount has one exact match, the confirmation may fix that choice without a redundant selection question; if several choices match, ask the user to select one outpoint. If the refreshed list no longer contains that choice, show the new list and ask the user to select again. A later explicit inscription request remains the separate flow below.

Query a submitted direct transfer through the shared wallet history flow, not inscription status:

```bash
onchainos wallet history --chain bitcoin (--tx-hash <hash> | --order-id <id>)
```

## Transfer Inscription

```bash
onchainos wallet inscription create --chain bitcoin --token-address <btc-brc20-ticker> --readable-amount <amount> [--from <address>]
onchainos wallet inscription status --chain bitcoin (--tx-hash <hash> | --order-id <id>)
```

Creation returns the same confirmation shape, with `scene="btc_inscription"`; the preview's `feeReadable` is nullable. After confirmation, success returns `state=INSCRIBING` plus the Reveal transaction's top-level `txHash` and `orderId`; `broadcasts` preserves every ordered Commit/Reveal result. Query status with the returned Reveal `orderId` through `nextSteps.checkInscriptionStatus`, never with the Commit transaction hash. Always display that complete continuation command after submission so it can be copied into another session, and also tell the user they can later say “查询铭刻结果” in the current conversation. Creation is a standalone asynchronous transfer inscription to the current Bitcoin address; the CLI owns its preparation, signing, and broadcast.

Status can be `INSCRIBING`, `WAITING_CONFIRMATION`, `WAITING_INDEXER`, `READY_TO_TRANSFER`, `FAILED`, or `UNKNOWN`. `READY_TO_TRANSFER` supplies read-only `nextSteps.queryBrc20TransferableUtxos`, which refreshes the transferable list for the returned token address.
