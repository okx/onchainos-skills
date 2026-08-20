# Native Bitcoin CLI Reference

Use `--readable-amount` for human-readable BTC quantities. `bitcoin` is canonical; `btc` and `0` are accepted aliases.

## Assets And Address

```bash
onchainos wallet addresses --chain bitcoin
onchainos wallet balance --chain bitcoin [--force]
```

| Response field | Meaning |
| --- | --- |
| Token item `symbol=BTC` with empty `tokenAddress` | Native BTC. |
| Token item `balance` / `usdValue` | Native BTC holding and valuation. |
| Chain-level `totalValueUsd` | Aggregate value of all Bitcoin-chain assets, not native BTC alone. |

`wallet balance` reports total holdings, not spendable balance. For the currently available BTC total and UTXOs, use `wallet utxo available --chain bitcoin`; it calls `availability-details` with `AVAILABLE_UTXO_LIST`.

## Native BTC Transfer

```bash
onchainos wallet send --chain bitcoin --recipient <address> --readable-amount <amount> [--fee-rate <sat-per-vB>]
```

BTC transfer uses the same CLI interaction as ordinary `wallet send`: `unsignedInfo` → sign → single-transaction broadcast. The CLI signs every item in `unsignedHashList` and sends the signed array in one broadcast request. It does not add a BTC-specific preview, operation token, preview version, or second preparation stage.

The initial command signs and returns the ordinary `confirming` response before broadcast. The Agent **MUST** display its complete confirmation and wait for a new explicit user confirmation before executing `next`. The confirmed continuation broadcasts and returns `state=PENDING`, `txHash`, and `orderId` on success.

`--fee-rate` is optional, accepts a decimal value of at least `0.1` sat/vB, and is sent as numeric `txParam.feeRate`. It applies only to this transaction; omitting it on the next transfer uses the service default rate.

## UTXO Management

```bash
onchainos wallet utxo list --chain bitcoin
onchainos wallet utxo list --chain bitcoin --unavailable
onchainos wallet utxo available --chain bitcoin
onchainos wallet utxo unlock --chain bitcoin --outpoint <txHash:voutIndex>
onchainos wallet utxo unlock --chain bitcoin --all
onchainos wallet utxo lock --chain bitcoin --outpoint <txHash:voutIndex>
onchainos wallet utxo lock --chain bitcoin --all
onchainos wallet utxo reclaim --chain bitcoin --tx-hash <hash> [--tx-hash <hash> ...]
```

| Flow | Initial command | Read continuation or confirmation behavior |
| --- | --- | --- |
| Query unavailable UTXO details | `onchainos wallet utxo list --chain bitcoin --unavailable` | Calls `availability-details` with `UNAVAILABLE_BREAKDOWN` and returns its category totals and UTXO fields directly. |
| Query UTXOs whose asset occupancy the user removed | `onchainos wallet utxo list --chain bitcoin` | Calls `availability-details` with `USER_IGNORED_LIST` and returns its totals and UTXO fields directly. |
| Query currently available UTXOs and BTC total | `onchainos wallet utxo available --chain bitcoin` | Calls `availability-details` with `AVAILABLE_UTXO_LIST`; `sumSats` is the current available BTC total and `utxos` is the available UTXO list. |
| Unlock one | `onchainos wallet utxo unlock --chain bitcoin --outpoint <txHash:voutIndex>` | The non-`--force` command refreshes the protected snapshot, then returns `confirming`, `preview`, and `next`. |
| Unlock all | `onchainos wallet utxo unlock --chain bitcoin --all` | Uses fresh `assetLocked`; confirmation submits selected outpoints in batches of 50. |
| Re-lock one | `onchainos wallet utxo lock --chain bitcoin --outpoint <txHash:voutIndex>` | The non-`--force` command refreshes `USER_IGNORED_LIST`, then returns `confirming`, `preview`, and `next`. |
| Re-lock all | `onchainos wallet utxo lock --chain bitcoin --all` | Uses fresh `USER_IGNORED_LIST`; confirmation submits selected outpoints in batches of 50. |
| Reclaim removed inputs | `onchainos wallet utxo reclaim --chain bitcoin --tx-hash <hash>` | The non-`--force` command validates `mempoolRemovedSpending` and returns `confirming`, `preview`, and `next`; confirmation closes the original order without signing or broadcasting. |

`availability-details` is the sole UTXO query source; the CLI does not call a secondary asset-info endpoint. Unlock and lock success output includes `batchResults` and refreshed unavailable and user-ignored snapshots. A rejected first management batch returns `UTXO_MANAGE_REJECTED`; a later failure returns `UTXO_MANAGE_PARTIAL_FAILURE` with latest state.

Confirmed UTXO management validates its local continuation before submitting a write. An empty, malformed, or incompatible continuation returns `INVALID_PREVIEW_CONTINUATION`. Correct the local invocation and retry the originally confirmed continuation without asking the user to confirm again. The confirmation binds only the critical intent: account, sender, chain, operation type, and exact target outpoints. Refreshed fee rate, category totals, source metadata, and changes to non-target UTXOs do not require another confirmation. If a valid continuation no longer matches those critical fields, the CLI returns a new confirmation preview without calling the management endpoint.

### UTXO Query Response Contract

The CLI wraps the selected service branch without synthesizing asset metadata:

| CLI mode | Request `queryType` | Response branch |
| --- | --- | --- |
| Default list | `USER_IGNORED_LIST` | `data.userIgnored.userIgnoredList` |
| `--unavailable` | `UNAVAILABLE_BREAKDOWN` | `data.unavailable.unavailableBreakdown` |
| `available` | `AVAILABLE_UTXO_LIST` | `data.available.availableUtxoList` |
| `brc20-transferable` | `BRC20_TRANSFERABLE_UTXO_LIST` | `data.brc20Transferable.brc20TransferableUtxoList` |

`UNAVAILABLE_BREAKDOWN` and `AVAILABLE_UTXO_LIST` are native BTC views. `USER_IGNORED_LIST` is the address-level user-removed asset-occupancy view shared by native BTC and BRC-20 flows. `BRC20_TRANSFERABLE_UTXO_LIST` is the ticker-specific BRC-20 transferable-inscription view. `USER_IGNORED_LIST` contains only UTXOs whose asset occupancy the user explicitly removed; never rename this result as the complete available or spendable UTXO list.

Inactive response branches being `null` are expected. Do not treat them as missing data. The CLI does not call another asset-detail endpoint or return a detail continuation.

| Response field | Meaning |
| --- | --- |
| `data.queryType` | Requested service view. |
| `data.outpointCount` | Unique outpoints found in the active response branch. |
| `data.unavailable.unavailableBreakdown.totalUnavailableCount` | Total unavailable UTXO count. |
| `data.unavailable.unavailableBreakdown.totalUnavailableSumSats` | Total unavailable BTC in sats. |
| `data.userIgnored.userIgnoredList.count` / `sumSats` | Count and sat total of UTXOs whose asset occupancy the user removed. |
| `data.available.availableUtxoList.count` / `sumSats` | Count and sat total of currently available UTXOs. `sumSats` is the available BTC balance. |
| `data.available.availableUtxoList.utxos[].pending` | Service-returned pending flag. |
| `data.available.availableUtxoList.utxos[].userIgnoreAsset` | Whether the user removed asset occupancy for that available UTXO. |
| Category `count` / `sumSats` / `utxos[]` | Count, sat total, and outpoints for `assetLocked`, `assetUncertain`, `feeUneconomic`, or `mempoolRemovedSpending`. Membership in the parent category is each UTXO's service-returned unavailable reason. |
| `utxos[].txHash` + `voutIndex` | Canonical outpoint `<txHash>:<voutIndex>`. |
| `utxos[].valueRaw` | Exact UTXO value in sats. |
| `utxos[].utxoId` | Optional service identifier; `null` is valid. |
| `utxos[].source` | Service-returned source. Preserve its spelling/case verbatim. |

Treat `sumSats` and `valueRaw` as decimal strings and convert to BTC exactly only for display. For unavailable-list output, include the raw parent category and its user-language meaning for every UTXO. Do not infer an asset protocol or name from a UTXO category.

## History And Status

Ordinary Bitcoin history and transaction detail use the shared wallet history request and response mapping.

```bash
onchainos wallet history --chain bitcoin [--page-num <cursor> --limit <n>]
onchainos wallet history --chain bitcoin (--tx-hash <hash> | --order-id <id>)
```

When the returned `txStatus` is `MEMPOOL_REMOVED`, query `wallet utxo list --chain bitcoin --unavailable` separately. Reclaim remains a separate explicit confirmed intent.
