<!--
 * @Author: zongyao.yang zongyao.yang@okg.com
 * @Date: 2026-08-19 16:50:34
 * @LastEditors: zongyao.yang zongyao.yang@okg.com
 * @LastEditTime: 2026-08-20 16:26:57
 * @FilePath: /feat-btcSuiExtension/skills/okx-agentic-wallet/references/brc20.md
 * @Description: 这是默认设置,请设置`customMade`, 打开koroFileHeader查看配置 进行设置: https://github.com/OBKoro1/koro1FileHeader/wiki/%E9%85%8D%E7%BD%AE
-->
# BRC-20 Agentic Wallet

Use this flow for a BRC-20 ticker balance, transferable inscription UTXOs, selected transfer, transfer inscription, and inscription status. When a ticker is provided, normalize it to `btc-brc20-<ticker>` before the command. Direct transfer amount comes from the selected service-returned UTXO combination; transfer-inscription input remains an exact human-readable amount converted by the CLI without floating-point arithmetic.

![alt text](image.png)

## Flow

1. For a holdings query with a ticker, run `wallet balance --chain bitcoin --token-address <btc-brc20-ticker>`.
2. For a BRC-20 holdings query without a ticker, run `wallet balance --all` once.
3. For “what BRC-20 can I transfer now?”, obtain the ticker/token address and run `wallet utxo brc20-transferable --chain bitcoin --token-address <btc-brc20-ticker>`. For a transfer request with an amount, add `--readable-amount <amount>` so the CLI returns an exact `selectionPlan`. The active wallet supplies the sender address.
4. Treat each returned UTXO as one indivisible transfer inscription. A direct transfer may combine multiple complete UTXOs in one transaction when their `tokenAmount` sum exactly equals the requested amount.
5. Follow `selectionPlan.status`: present exact combinations in returned order and ask the user to choose when needed; for `NO_EXACT_MATCH`, run a fresh BRC-20 `wallet balance` before deciding whether to offer the separate inscription flow; for `SEARCH_LIMIT_EXCEEDED`, report the incomplete result and current choices. Field and limit details are in the CLI reference.
6. Start a direct transfer after the recipient and one returned combination are fixed. Pass every `selectedOutpoints[]` item as a repeated `--brc20-outpoint` and include `--readable-amount <amount>`. Run `wallet send` without `--force`, adding `--fee-rate <sat-per-vB>` only when the user selected a valid custom rate. The CLI refreshes every selected UTXO, verifies their exact combined amount, signs, and returns the shared pre-broadcast confirmation. **MUST** display that complete confirmation, stop, and wait for a new explicit user confirmation. Execute `next` only after that confirmation.
7. For `NO_EXACT_MATCH`, compare the requested amount with `remainingInscribableAmount`. When it is sufficient, offer a separate transfer inscription for the full requested amount. Inscription is a separate on-chain write with its own confirmation. Otherwise report the shortage and the current transferable denominations.
8. For an inscription, start its separate command without `--force`. It stops after `unsignedInfo` and returns the fixed inscription confirmation below; it has not signed or submitted. **MUST** display that complete confirmation, stop, and wait for a new explicit user confirmation. Only after that confirmation, execute `next`, which completes local signing, calls `sign-tx`, and batch-broadcasts the inscription transactions. A custom fee rate re-runs the initial inscription command with `--fee-rate <sat-per-vB>` and without `--force`, then returns a fresh confirmation; do not execute the previous continuation. The minimum custom rate is `0.1` sat/vB. After the user confirms a valid custom rate, state: `The custom fee rate applies only to this transaction. Your next transaction will use the default rate.`
9. Preserve service errors, especially `44002` and `44003`; do not rewrite them as a local balance decision.
10. An inscription is asynchronous. After submission, show the returned `orderId`, `txHash`, and complete `nextSteps.checkInscriptionStatus` command verbatim. Use the returned top-level identifiers for status queries. When status is `READY_TO_TRANSFER`, refresh the transferable BRC-20 UTXO list before any new transfer. **NEVER** auto-send after inscription completion.

In a BRC-20 context, route user-removed asset-occupancy requests to the shared address-level `USER_IGNORED_LIST` view. Use `BRC20_TRANSFERABLE_UTXO_LIST` for ticker-specific transferable inscriptions.

## Intent Routing

| Intent | Parameters | Command |
| --- | --- | --- |
| BRC-20 holdings with a ticker | ticker | `wallet balance --chain bitcoin --token-address <btc-brc20-ticker>` |
| BRC-20 holdings without a ticker | none | `wallet balance --all`, then retain assets with `tokenAddress` starting `btc-brc20-` |
| Currently transferable BRC-20 UTXOs/tokens | ticker | `wallet utxo brc20-transferable --chain bitcoin --token-address <btc-brc20-ticker>` |
| UTXOs whose asset occupancy the user removed in a BRC-20 context | none | `wallet utxo list --chain bitcoin` |
| Transfer BRC-20 | ticker, amount, recipient, one selected combination from the latest list | `wallet send --chain bitcoin --contract-token <token-address> --readable-amount <amount> --brc20-outpoint <first> [--brc20-outpoint <next> ...] [--fee-rate <sat-per-vB>]` |
| Create transfer inscription | ticker, readable amount | `wallet inscription create --chain bitcoin [--fee-rate <sat-per-vB>]` |
| BRC-20 direct transfer status | tx hash or order ID | Shared `wallet history --chain bitcoin` query |
| Transfer inscription status | tx hash or order ID | `wallet inscription status --chain bitcoin` |

## User-Facing Templates

Use only facts returned by the CLI. Substitute token ticker from the returned token address. Direct transfer uses the shared ordinary-send confirmation. Inscription uses the fixed confirmation below. **NEVER** invent, estimate, or sum a fee.

For a BRC-20 holdings query without a ticker, retain standard balance assets whose `tokenAddress` starts with `btc-brc20-` and render them in the standard `balance --all` format. If none remain, tell the user that they have no BRC-20-related assets.

For a ticker-specific BRC-20 balance response, output these three lines without a heading or bullets:

`Total balance: ${totalAmount} ${ticker}, worth approximately $${totalUsd}`

`Currently transferable (already inscribed): ${transferableAmount} ${ticker}, worth approximately $${transferableUsd}, across ${count} transferable inscriptions with denominations ${denominations}`

`Remaining available to inscribe: ${remainingInscribableAmount} ${ticker}, worth approximately $${remainingInscribableUsd}`

When `${count}` is zero, replace the second line with `Currently transferable (already inscribed): 0 ${ticker}, worth approximately $${transferableUsd}, with no transferable inscriptions`. Treat `inscription` as the transferable object and `create a transfer inscription` as the action. Use `USD value unavailable` instead of any unavailable USD placeholder.

| Situation | Output |
| --- | --- |
| Transferable choices without a target amount | `You currently have {count} transferable {ticker} inscription UTXOs totaling {sumValue} {ticker}. Choose one or more outpoints whose amounts total the transfer amount:` Then list each exact `selection`, `tokenAmount {ticker}`, `utxoAmountSats sats`, `inscriptionId`, and `offset`. |
| One exact combination | `One exact combination can transfer ${amount} ${ticker} to ${recipient}:` List every selected outpoint and its `tokenAmount`. |
| Several exact combinations | `The CLI found {combinationCount} exact combinations for {amount} {ticker}, ordered by fewer UTXOs:` Number each returned combination and list its selected outpoints and token amounts, then ask the user to choose one. |
| No exact combination | `The current transferable inscriptions have no exact combination for {amount} {ticker}. Available denominations: {denominations}. You can choose another amount or create a transfer inscription for {amount} {ticker}.` Offer inscription only when the current BRC-20 balance result establishes sufficient `remainingInscribableAmount`. |
| Combination search limit reached | `The bounded combination search reached its limit before proving whether {amount} {ticker} has an exact match. Here are the current transferable denominations: {denominations}.` Present the returned choices for a user-directed selection. |
| No transferable amount, inscription supported | `You do not currently have a transferable {ticker} inscription for {amount} {ticker}. A transfer inscription for the full {amount} {ticker} is a separate on-chain transaction and must confirm before it can be sent. Would you like to prepare that inscription?` |
| No transferable amount, inscription not established | `You do not currently have a transferable {ticker} inscription UTXO. A transfer inscription is a separate on-chain transaction.` Do not claim that the requested amount can be inscribed. |
| Submitted inscription | `The transfer inscription was submitted. Order ID: {orderId}. Tx Hash: {txHash}. This is asynchronous and is not transferable yet. Check its status with: {checkInscriptionStatusCommand}. After it reaches READY_TO_TRANSFER, query the transferable BRC-20 UTXO list and start a separate transfer.` |
| Service requires inscription | Relay the service message, then: `This transfer ended without signing or broadcasting. Creating a transfer inscription is a separate on-chain operation.` |

### Transfer-Inscription Confirmation

Render this complete structure for `scene="btc_inscription"`:

`Create transfer inscription`

`From: ${preview.from}`

`To: ${preview.to}`

`Amount: ${preview.asset.readableAmount} ${ticker}`

`Network fee: ${preview.feeReadable} ${preview.feeSymbol}`

`Network fee rate: ${preview.feeRate} sat/vB`

`This prepares a transaction to create a new transfer inscription. It has not been signed or submitted; after asynchronous confirmation, ${preview.asset.readableAmount} ${ticker} becomes transferable.`

Render the network-fee line only when `preview.feeReadable` is present, and the fee-rate line only when `preview.feeRate` is present. If the fee rate is present, finish with: `Do you confirm broadcasting at the current fee rate to create this inscription? To change the fee rate, reply directly with a new numeric rate in sat/vB.` Otherwise finish with: `Do you confirm broadcasting to create this inscription? To set a fee rate, reply directly with a numeric rate in sat/vB.`

After the user selects a valid custom fee rate, state: `The custom fee rate applies only to this transaction. Your next transaction will use the default rate.`

Present `remainingInscribableAmount` as an available amount, not as a required action. Do not promise an indexing duration. Copy every returned transaction hash, order ID, and continuation command verbatim.
