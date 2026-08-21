# Bitcoin UTXO Reference

Use this reference for Bitcoin UTXO queries, protection changes, and mempool-input reclaim. For wallet addresses, total BTC balance, transfer, and history, use the shared Wallet flow.

## Commands

```bash
onchainos wallet utxo available --chain bitcoin
onchainos wallet utxo user-ignored --chain bitcoin
onchainos wallet utxo unavailable --chain bitcoin
onchainos wallet utxo unlock --chain bitcoin (--outpoint <txHash:voutIndex>... | --all)
onchainos wallet utxo lock --chain bitcoin (--outpoint <txHash:voutIndex>... | --all)
onchainos wallet utxo reclaim --chain bitcoin --tx-hash <hash> [--tx-hash <hash> ...]
```

## Flow

1. For spendable BTC or available UTXOs, run `wallet utxo available --chain bitcoin`; use `sumSats` as the available BTC total.
2. For UTXOs whose asset occupancy was removed by the user, run `wallet utxo user-ignored --chain bitcoin`.
3. For locked or unavailable UTXOs, run `wallet utxo unavailable --chain bitcoin`.
4. For protection removal or restoration, query the relevant UTXO view and resolve the user's reference against the latest returned outpoints. A single amount or asset reference must resolve to exactly one current outpoint; when it matches zero or multiple UTXOs, ask the user to choose. Then run `wallet utxo unlock` or `wallet utxo lock` without `--force` and follow its confirmation.
5. For `MEMPOOL_REMOVED` transaction inputs, query unavailable UTXOs, then run `wallet utxo reclaim --chain bitcoin --tx-hash <hash>` without `--force` and follow its confirmation.
6. When the user follows a BTC balance answer by asking what the remaining or unavailable BTC is, run `wallet utxo unavailable --chain bitcoin` before replying.

## Response Semantics

| Command | Service view | Meaning |
| --- | --- | --- |
| `utxo available` | `AVAILABLE_UTXO_LIST` | Currently available BTC UTXOs. |
| `utxo user-ignored` | `USER_IGNORED_LIST` | UTXOs whose asset occupancy the user removed. |
| `utxo unavailable` | `UNAVAILABLE_BREAKDOWN` | Locked or unavailable BTC UTXOs. |
| `utxo brc20-transferable` | `BRC20_TRANSFERABLE_UTXO_LIST` | Ticker-specific BRC-20 transferable inscriptions; see [brc20-cli-reference.md](brc20-cli-reference.md). |

Keep these views separate. Display `sumSats` and `valueRaw` as sats and exact BTC, preserve returned outpoints, source, and unavailable categories verbatim, and do not derive spendable BTC by subtracting categories from total holdings.

## User-facing FAQ

When the user asks any semantic equivalent of one of the questions below, output only the matching template. Translate it to the user's language without adding an introduction, follow-up, command, balance query, or extra explanation.

### What is available balance?

Triggers include `available balance`, `available BTC`, `spendable balance`, and `spendable BTC` when the user is asking for the definition rather than their current amount.

```text
Available balance is the amount currently available for BTC transfers and network fees. It excludes locked and dust UTXOs.
```

### What is a locked UTXO?

Triggers include `locked UTXO`, `protected UTXO`, and questions asking why a UTXO is locked.

```text
A locked UTXO is excluded from ordinary BTC transactions to protect the inscription assets it carries. You can explicitly unlock that UTXO.
```

### What are the risks of unlocking a UTXO?

Triggers include `unlock UTXO risk`, `is unlocking safe`, and questions asking what happens after a UTXO is unlocked.

```text
After unlocking, the UTXO is treated as ordinary BTC. If it is spent, the inscription assets it carries will be permanently lost.
```

### What is a dust UTXO?

Triggers include `dust`, `dust UTXO`, `small UTXO`, and questions asking why a small BTC UTXO is unavailable.

```text
A dust UTXO contains a very small amount of BTC. Spending it increases transaction data size and network fees, so it is excluded from the available balance.
```

## Unavailable BTC Follow-up Reply

For a follow-up such as “what is the remaining BTC?” or “why is it unavailable?”, do not give only a generic definition. Query `UNAVAILABLE_BREAKDOWN`, then reply from its returned category totals and UTXOs.

- If no unavailable UTXO is returned: `There are currently no unavailable BTC UTXOs.`
- Otherwise: `These {totalAmount} BTC are distributed across {totalUnavailableCount} unavailable UTXOs: {breakdown}. To protect assets, they are excluded from transfers and transaction fees by default.`

Build `{breakdown}` from every non-empty returned category: `assetLocked` as asset-protected (include returned asset labels, such as BRC-20 or Ordinals, when present); `feeUneconomic` as small BTC that would increase network fees; `assetUncertain` as assets the service could not classify with certainty; and `mempoolRemovedSpending` as inputs occupied by a mempool-removed transaction. Preserve service-returned asset names and amounts; do not invent them.

For unlock, lock, and reclaim, a confirmation binds the current account, chain, operation, and selected outpoints. If a refreshed continuation returns a new confirmation, display it; do not reuse a previous confirmation. A reclaim closes the removed transaction and restores inputs for a new transfer; do not rebroadcast the removed transaction.
