# Bitcoin UTXO CLI Reference

Use this reference for Bitcoin UTXO queries, asset-protection changes, and mempool-input reclaim. For wallet addresses, total BTC balance, transfer, and history, use the shared Wallet reference.

## `wallet utxo available`

Query currently spendable BTC UTXOs. Use returned `sumSats` as the available BTC total; do not derive it from total holdings.

### Syntax

```bash
onchainos wallet utxo available --chain bitcoin
```

### Parameters

| Parameter | Required | Default | Description |
| --- | --- | --- | --- |
| `--chain` | Yes | — | Use `bitcoin`. |

## `wallet utxo user-ignored`

Query UTXOs whose asset occupancy the user previously removed from asset protection.

### Syntax

```bash
onchainos wallet utxo user-ignored --chain bitcoin
```

### Parameters

| Parameter | Required | Default | Description |
| --- | --- | --- | --- |
| `--chain` | Yes | — | Use `bitcoin`. |

## `wallet utxo unavailable`

Query locked or otherwise unavailable BTC UTXOs. Use only returned categories, amounts, and UTXOs without inference. Use it before replying when the user follows a BTC balance answer by asking what the remaining or unavailable BTC is.

### Syntax

```bash
onchainos wallet utxo unavailable --chain bitcoin
```

### Parameters

| Parameter | Required | Default | Description |
| --- | --- | --- | --- |
| `--chain` | Yes | — | Use `bitcoin`. |

## `wallet utxo unlock`

Remove asset protection from selected currently protected UTXOs. Query the latest unavailable view and resolve the user's reference against its current outpoints first. A single amount or asset reference must resolve to exactly one outpoint; if it matches zero or multiple UTXOs, show the matches and wait for the user's decision.

### Syntax

```bash
onchainos wallet utxo unlock --chain bitcoin (--outpoint <txHash:voutIndex>... | --all) [--operation-token <token>] [--force]
```

### Parameters

| Parameter | Required | Default | Description |
| --- | --- | --- | --- |
| `--chain` | Yes | — | Use `bitcoin`. |
| `--outpoint` | One selector required | — | Current protected UTXO in `<txHash>:<voutIndex>` form; repeat for multiple selections. |
| `--all` | One selector required | Disabled | Select every currently protected UTXO instead of individual outpoints. |
| `--operation-token` | Continuation only | — | Use only when supplied by the exact `next` returned after preview. |
| `--force` | Continuation only | Disabled | Use only through the exact `next` returned after explicit confirmation. |

## `wallet utxo lock`

Restore asset protection for selected user-ignored UTXOs. Query the latest user-ignored view and resolve the user's reference against its current outpoints first. A single amount or asset reference must resolve to exactly one outpoint; if it matches zero or multiple UTXOs, show the matches and wait for the user's decision.

### Syntax

```bash
onchainos wallet utxo lock --chain bitcoin (--outpoint <txHash:voutIndex>... | --all) [--operation-token <token>] [--force]
```

### Parameters

| Parameter | Required | Default | Description |
| --- | --- | --- | --- |
| `--chain` | Yes | — | Use `bitcoin`. |
| `--outpoint` | One selector required | — | Current user-ignored UTXO in `<txHash>:<voutIndex>` form; repeat for multiple selections. |
| `--all` | One selector required | Disabled | Select every current user-ignored UTXO instead of individual outpoints. |
| `--operation-token` | Continuation only | — | Use only when supplied by the exact `next` returned after preview. |
| `--force` | Continuation only | Disabled | Use only through the exact `next` returned after explicit confirmation. |

## `wallet utxo reclaim`

Reclaim still-unspent inputs from transactions whose history state is `MEMPOOL_REMOVED`. Query unavailable UTXOs first and use the returned transaction hashes.

### Syntax

```bash
onchainos wallet utxo reclaim --chain bitcoin --tx-hash <hash> [--tx-hash <hash> ...] [--force]
```

### Parameters

| Parameter | Required | Default | Description |
| --- | --- | --- | --- |
| `--chain` | Yes | — | Use `bitcoin`. |
| `--tx-hash` | Yes | — | Original `MEMPOOL_REMOVED` transaction hash; repeat for multiple transactions. |
| `--force` | Continuation only | Disabled | Use only through the exact `next` returned after explicit confirmation. |

## User-Facing Templates

### Bound BRC-20 Assets

`wallet utxo available`, `wallet utxo unavailable`, `wallet utxo user-ignored`, and `wallet utxo brc20-transferable` return `assets` for UTXOs carrying BRC-20 assets.

For each returned asset, append:

```text
This UTXO carries: ${readableAmount} ${symbol} (${protocol}).
Spending this UTXO can permanently lose these assets.
```

### UTXO FAQ

When the user asks any semantic equivalent of one of the questions below, follow the matching entry and translate its reply template to the user's language without extra explanation.

#### What is available balance?

Triggers include `available balance`, `available BTC`, `spendable balance`, and `spendable BTC` when the user is asking for the definition rather than their current amount.

```text
Available balance is the amount currently available for BTC transfers and network fees. It excludes locked and dust UTXOs.
```

#### What is a locked UTXO?

Triggers include `locked UTXO`, `protected UTXO`, and questions asking why a UTXO is locked.

```text
A locked UTXO is excluded from ordinary BTC transactions to protect the inscription assets it carries. You can explicitly unlock that UTXO.
```

#### What are the risks of unlocking a UTXO?

Triggers include `unlock UTXO risk`, `is unlocking safe`, and questions asking what happens after a UTXO is unlocked.

```text
After unlocking, the UTXO is treated as ordinary BTC. If it is spent, the inscription assets it carries will be permanently lost.
```

#### What is a dust UTXO?

Triggers include `dust`, `dust UTXO`, `small UTXO`, and questions asking why a small BTC UTXO is unavailable.

```text
A dust UTXO contains a very small amount of BTC. Spending it increases transaction data size and network fees, so it is excluded from the available balance.
```

#### What is unavailable balance?

Triggers include `unavailable balance`, `unavailable BTC`, `locked balance`, `what is the remaining BTC?`, `why is it unavailable?`, and questions asking what unavailable balance means or how much is currently unavailable. Run `wallet utxo unavailable --chain bitcoin`; do not use a stored balance or derive this amount from total BTC holdings.

First reply:

```text
Unavailable balance is BTC that currently cannot be used for BTC transfers or network fees, including locked and dust UTXOs.
```

Then use the current `unavailable.unavailableBreakdown` to append one matching reply:

- If `totalUnavailableCount` is `0`: `You currently have no unavailable BTC balance.`
- If `assetLocked` contains UTXOs: `You currently have {unavailableBalance} BTC unavailable. The following UTXOs are locked:` Then list each returned locked UTXO as `- {txHash}:{voutIndex}: {amount} BTC`. If `feeUneconomic` also contains UTXOs, append: `The remaining {dustBalance} BTC is dust UTXOs. Spending them increases network fees.`
- If `assetLocked` is empty and `feeUneconomic` is the only non-empty category: `You currently have {unavailableBalance} BTC unavailable, all of which is dust UTXOs. Spending them increases network fees.`
- If any other category is non-empty: show its returned category, UTXOs, and amounts. Do not describe it as locked or dust.

Use the returned `totalUnavailableSumSats` for `{unavailableBalance}` and returned UTXO amounts for `{amount}` / `{dustBalance}`. If a required amount is absent, omit that aggregate sentence rather than calculating or inferring it.
