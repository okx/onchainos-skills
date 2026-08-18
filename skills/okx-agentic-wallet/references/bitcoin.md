# Bitcoin Agentic Wallet

Use this flow for native BTC, Bitcoin transaction status, and UTXO protection. BTC transfer follows ordinary `wallet send`, ordinary transaction history uses the shared `wallet history` query, and UTXO management keeps its own classification, preview, and continuation behavior.

## Flow

1. Resolve one user intent and invoke the matching command with `--chain bitcoin`.
2. Relay returned facts. The current account supplies the sender address when `--from` is absent.
3. For `confirming:true`, show `message` and any returned `preview`; run `next` only after an explicit confirmation.
4. Run a returned `nextSteps` command only as its indicated read continuation. Every write begins with a separate explicit user intent and confirmation.

## Intent Routing

| Intent | Parameters | Command |
| --- | --- | --- |
| Bitcoin address | none | `wallet addresses --chain bitcoin` |
| BTC assets and value | optional refresh | `wallet balance --chain bitcoin` |
| Available BTC balance or available UTXOs | none | `wallet utxo available --chain bitcoin` |
| Native BTC transfer | recipient, readable amount | `wallet send --chain bitcoin` |
| UTXOs whose asset occupancy the user removed | none | `wallet utxo list --chain bitcoin` |
| Unavailable UTXO details | none | `wallet utxo list --chain bitcoin --unavailable` |
| Broad "my/all UTXOs" query | none | Run available, user-ignored, and unavailable UTXO views and report them separately |
| Explain locked BTC | none | `wallet utxo list --chain bitcoin --unavailable` |
| Remove protection | one outpoint or all | `wallet utxo unlock --chain bitcoin` |
| Restore protection | one outpoint or all UTXOs whose asset occupancy the user removed | `wallet utxo lock --chain bitcoin` |
| Reclaim removed transaction inputs | one or more original tx hashes | `wallet utxo reclaim --chain bitcoin` |
| Transaction history/status | optional tx hash or order ID | Shared `wallet history --chain bitcoin` query |

## BTC Balance Output

**MUST**: For a BTC-only balance query, select only the native BTC item (`symbol=BTC` and empty `tokenAddress`) and return exactly this two-sentence structure in the user's current language, without a heading, bullets, chain total, other assets, address, refresh note, or environment note:

`Your current balance is {balance} BTC, worth approximately ${usdValue}. This balance includes unlocked and locked BTC and does not represent the currently spendable balance.`

Use the returned UI-unit `balance` and round `usdValue` to two decimal places. Do not use chain-level `totalValueUsd` as the BTC valuation.

This BTC-only output contract overrides the generic rule to show full precision for USD values below `$0.01`.

If the user asks how much BTC is available/spendable, asks for available UTXOs, or follows a total-balance answer with “how much is available?”, run `wallet utxo available --chain bitcoin`. Report `availableUtxoList.sumSats` as the available BTC total and use its `utxos` as the available list. Do not derive this amount by subtracting unavailable or protected categories from total holdings.

## UTXO Query Output

- For a broad request such as "query my/all UTXOs", run all three BTC views. Report `AVAILABLE_UTXO_LIST`, `USER_IGNORED_LIST`, and `UNAVAILABLE_BREAKDOWN` separately; do not combine their totals.
- State their meanings precisely: `AVAILABLE_UTXO_LIST` is the currently available UTXO view; `USER_IGNORED_LIST` contains only UTXOs whose asset occupancy the user removed; `UNAVAILABLE_BREAKDOWN` contains unavailable UTXO details.
- In Chinese output, name them exactly `当前可用 UTXO`, `用户解除资产占用的 UTXO`, and `不可用 UTXO 详情`.
- Never rename `USER_IGNORED_LIST` as the complete available, spendable, or ordinary UTXO list.
- Before rendering UTXO amounts, run `wallet balance --chain bitcoin --force`, select the native BTC item (`symbol=BTC` with an empty `tokenAddress`), and use its returned `tokenPrice` for USD conversion. This read is only for display; it does not change UTXO availability or selection.
- Render every individual and aggregate UTXO amount as `{sats} sats ({btcAmount} BTC, worth approximately ${usdValue})`, including query results, protection previews, and post-action results. Compute `btcAmount = sats / 100000000` and `usdValue = btcAmount * tokenPrice` with exact decimal arithmetic, then apply the shared USD display rules. If the native BTC price is missing or the balance query fails, retain the sats/BTC amount and state that the USD value is unavailable; never invent a price.
- When the user asks for the list or details, render each returned item as `<txHash>:<voutIndex>`, `{valueRaw} sats ({btcAmount} BTC, worth approximately ${usdValue})`, and service-returned `source`; include `utxoId` only when non-null. Copy identifiers verbatim.
- For an available-balance question, report `availableUtxoList.sumSats`, exact BTC conversion, and approximate USD value. For an available-list question, also enumerate `availableUtxoList.utxos` and preserve `pending` and `userIgnoreAsset` exactly as returned.
- Treat inactive response branches being `null` as expected for the selected `queryType`. Preserve `source` exactly as returned, without normalizing `onchain` / `ON_CHAIN` / `MEMORY_POOL`.
- Do not derive an exact spendable balance by subtracting categories.

## Locked BTC Output

**MUST**: For a question about locked BTC, query unavailable UTXOs. The returned `UNAVAILABLE_BREAKDOWN` is the only source of facts.

If `totalUnavailableCount` is zero or no unavailable UTXOs are returned, respond:

`Your account currently has no locked UTXOs.`

Otherwise, use this structure without a heading, bullets, outpoints, address, or environment note:

`These {totalSats} sats ({totalBtc} BTC, worth approximately ${usdValue}) are distributed across {utxoCount} locked UTXOs: {breakdown}. To protect your assets, they are excluded by default and are not used for transfers or transaction fees.`

Build `{breakdown}` from every non-empty unavailable category:

| Category | Breakdown entry |
| --- | --- |
| `assetLocked` | `{sats} sats ({btcAmount} BTC, worth approximately ${usdValue}) is classified as asset-protected` |
| `feeUneconomic` | `another {sats} sats ({btcAmount} BTC, worth approximately ${usdValue}) consists of small BTC that would increase network fees` |
| `assetUncertain` | `{sats} sats ({btcAmount} BTC, worth approximately ${usdValue}) carries assets that the service could not classify with certainty` |
| `mempoolRemovedSpending` | `{sats} sats ({btcAmount} BTC, worth approximately ${usdValue}) remains occupied by a transaction removed from the mempool` |

Convert sats to BTC exactly and omit empty categories. **NEVER** infer an asset protocol or name from the category.

## UTXO Protection Changes

**MUST**: Treat protection removal and restoration as separate writes. Query the relevant UTXO group and resolve the user reference against the latest returned outpoints. Then start the matching `wallet utxo unlock` or `wallet utxo lock` command without `--force`; it refreshes the snapshot before it can write.

**MUST**: Resolve a single amount or asset reference to exactly one current outpoint. If it matches zero or multiple UTXOs, ask the user to choose. **NEVER** pass `--force` on an initial protection command.

Display the complete CLI preview and the matching template. Execute returned `next` verbatim only after explicit confirmation.

One explicit confirmation covers the exact account, sender, chain, operation type, and target outpoints. On continuation, refresh the availability snapshot but do not ask again for fee-rate, category-total, source-metadata, or non-target UTXO changes. Ask again only when a critical bound field changes. A local `INVALID_PREVIEW_CONTINUATION` means the write was not attempted; correct the invocation and retry the already confirmed continuation without asking again.

Keep continuation implementation details internal. Do not mention or explain operation tokens, continuation flags, preview metadata, or internal version labels in user-facing prompts and results.

| Intent | Preview output |
| --- | --- |
| Unlock one asset-protected UTXO | `This {sats} sats ({btcAmount} BTC, worth approximately ${usdValue}) UTXO is classified as asset-protected. After its asset occupancy is removed, it appears in USER_IGNORED_LIST. If it is later spent in a transfer or transaction fee, carried assets can be permanently lost. Reply "Confirm" to unlock it.` |
| Unlock all asset-protected UTXOs | `You have {utxoCount} asset-protected UTXOs totaling {totalSats} sats ({totalBtc} BTC, worth approximately ${usdValue}). After their asset occupancy is removed, they appear in USER_IGNORED_LIST. If they are later spent in a transfer or transaction fee, carried assets can be permanently lost. Reply "Confirm" to unlock all.` |
| Restore protection | `This {sats} sats ({btcAmount} BTC, worth approximately ${usdValue}) UTXO is in USER_IGNORED_LIST because its asset occupancy was previously removed by the user. Restoring protection excludes it from transfers and transaction fees. Reply "Confirm" to lock it again.` |

All-unlock covers only the current `assetLocked` group; if `assetUncertain` is non-empty, the CLI refuses all-unlock. All-relock covers current `USER_IGNORED_LIST` UTXOs.

After success, report the returned latest UTXO snapshots using the same sats/BTC/USD amount format. **NEVER** claim a spendable-balance change unless the CLI returned it; `wallet balance` is total holdings and includes protected BTC.

## BTC Transfer And Mempool Recovery

**MUST**: Handle BTC transfer exactly like ordinary `wallet send`. After the shared confirmation for the user's transfer intent, invoke the command without `--force`. Do not add a BTC-specific preview, operation token, preview version, or extra availability query. If broadcast returns the shared backend confirming response, relay its service `message` and append `--force` only after the user confirms. **NEVER** invent or calculate a fee.

For `MEMPOOL_REMOVED`, say: `Your transaction was removed from the mempool and its inputs may still be occupied. You can reclaim the UTXOs to restore them for selection. To send again, start a new transfer request.` Then run `wallet utxo list --chain bitcoin --unavailable` as a separate read.

**MUST**: Reclaim is a separate confirmed write. Show its preview before following `next`. **NEVER** offer to rebroadcast the old raw transaction: a fresh transfer command is required.

## Conversation Context

- Keep current-conversation addresses, outpoints, tx hashes, order IDs, balance facts, and intent. Ask for an identifier when reference resolution is ambiguous or the conversation is new.
- After an account switch, login change, or address change, query the required facts again.
