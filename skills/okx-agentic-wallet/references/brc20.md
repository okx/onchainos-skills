# BRC-20 Agentic Wallet

Use this flow for a BRC-20 ticker balance, transferable inscription UTXOs, selected transfer, transfer inscription, and inscription status. Normalize a ticker to `btc-brc20-<ticker>` before any command. Direct transfer amount comes from the selected service-returned UTXO combination; transfer-inscription input remains an exact human-readable amount converted by the CLI without floating-point arithmetic.

## Flow

1. For a holdings query, run both `wallet balance --chain bitcoin --token-address <btc-brc20-ticker>` and `wallet utxo brc20-transferable --chain bitcoin --token-address <btc-brc20-ticker>`. Use the returned token balance as `totalAmount`, the transferable response's `sumValue` as `transferableAmount`, and exact decimal subtraction for `remainingInscribableAmount = totalAmount - transferableAmount`. Treat `transferableAmount` as the already-inscribed amount that is currently transferable and `remainingInscribableAmount` as the remaining amount available to inscribe. Use the balance response's `usdValue` for `totalUsd`; multiply its `tokenPrice` by each derived UI amount with exact decimal arithmetic for `transferableUsd` and `remainingInscribableUsd`, then apply the shared USD display rules. If `tokenPrice` is absent or invalid, show `USD value unavailable` for derived amounts. Output the matching Balance template below.
2. For “what BRC-20 can I transfer now?”, obtain the ticker/token address and run `wallet utxo brc20-transferable --chain bitcoin`. For a transfer request with an amount, add `--readable-amount <amount>` so the CLI returns an exact `selectionPlan`. The active wallet supplies the sender address.
3. Treat each returned UTXO as one indivisible transfer inscription. A direct transfer may combine multiple complete UTXOs in one transaction when their `tokenAmount` sum exactly equals the requested amount.
4. Follow `selectionPlan.status`: present exact combinations in returned order and ask the user to choose when needed; for `NO_EXACT_MATCH`, offer denominations, another amount, or the separate inscription flow; for `SEARCH_LIMIT_EXCEEDED`, report the incomplete result and current choices. Field and limit details are in the CLI reference.
5. Start a direct transfer after the recipient and one returned combination are fixed. Pass every `selectedOutpoints[]` item as a repeated `--brc20-outpoint` and include `--readable-amount <amount>`. Run `wallet send` without `--force`; the CLI refreshes every selected UTXO, verifies their exact combined amount, and owns preparation, signing, and ordinary single-transaction broadcast. Follow any ordinary backend confirmation exactly.
6. For `NO_EXACT_MATCH`, compare the requested amount with `remainingInscribableAmount`. When it is sufficient, offer a separate transfer inscription for the full requested amount. Inscription is a separate on-chain write with its own confirmation. Otherwise report the shortage and the current transferable denominations.
7. For an inscription, start its separate command without `--force` and follow its returned preview confirmation. For any `confirming:true`, display `message` and any returned `preview`; execute `next` only after explicit confirmation.
8. Preserve service errors, especially `44002` and `44003`; do not rewrite them as a local balance decision.
9. An inscription is asynchronous. After submission, show the returned `orderId`, `txHash`, and complete `nextSteps.checkInscriptionStatus` command verbatim. Tell the user they can request an inscription status check later in the current conversation; the complete command is the portable continuation for a different session. Use the returned top-level identifiers for status queries. When status is `READY_TO_TRANSFER`, refresh the transferable BRC-20 UTXO list before any new transfer. **NEVER** auto-send after inscription completion.

In a BRC-20 context, route user-removed asset-occupancy requests to the shared address-level `USER_IGNORED_LIST` view. Use `BRC20_TRANSFERABLE_UTXO_LIST` for ticker-specific transferable inscriptions.

## Intent Routing

| Intent | Parameters | Command |
| --- | --- | --- |
| BRC-20 holdings | ticker | Run both `wallet balance --chain bitcoin --token-address <btc-brc20-ticker>` and `wallet utxo brc20-transferable --chain bitcoin --token-address <btc-brc20-ticker>`; report total, currently transferable (already inscribed), and remaining inscribable amounts |
| Currently transferable BRC-20 UTXOs/tokens | ticker | `wallet utxo brc20-transferable --chain bitcoin --token-address <btc-brc20-ticker>` |
| UTXOs whose asset occupancy the user removed in a BRC-20 context | none | `wallet utxo list --chain bitcoin` |
| Transfer BRC-20 | ticker, amount, recipient, one selected combination from the latest list | `wallet send --chain bitcoin --contract-token <token-address> --readable-amount <amount> --brc20-outpoint <first> [--brc20-outpoint <next> ...]` |
| Create transfer inscription | ticker, readable amount | `wallet inscription create --chain bitcoin` |
| BRC-20 direct transfer status | tx hash or order ID | Shared `wallet history --chain bitcoin` query |
| Transfer inscription status | tx hash or order ID | `wallet inscription status --chain bitcoin` |

## User-Facing Templates

Render the templates below in the user's current language and use only facts returned by the CLI. Substitute token ticker from the returned token address. Transfer uses the shared ordinary-send confirmation and has no chain-specific preview template. For inscription, show a network fee sentence only when `preview.feeReadable` is non-null and non-empty; otherwise omit the entire fee sentence. **NEVER** invent, estimate, or sum a fee.

For every BRC-20 balance response, output exactly these three lines without a heading or bullets:

`Total balance: {totalAmount} {ticker}, worth approximately ${totalUsd}`

`Currently transferable (already inscribed): {transferableAmount} {ticker}, worth approximately ${transferableUsd}, across {count} transferable inscriptions with denominations {denominations}`

`Remaining available to inscribe: {remainingInscribableAmount} {ticker}, worth approximately ${remainingInscribableUsd}`

When `count` is zero, replace the second line with `Currently transferable (already inscribed): 0 {ticker}, worth approximately ${transferableUsd}, with no transferable inscriptions`. Treat `inscription` as the transferable object and `create a transfer inscription` as the action; translate each naturally into the user's language. Use `USD value unavailable` instead of any unavailable USD placeholder.

| Situation | Output |
| --- | --- |
| Transferable choices without a target amount | `You currently have {count} transferable {ticker} inscription UTXOs totaling {sumValue} {ticker}. Choose one or more outpoints whose amounts total the transfer amount:` Then list each exact `selection`, `tokenAmount {ticker}`, `utxoAmountSats sats`, `inscriptionId`, and `offset`. |
| One exact combination | `One exact combination can transfer {amount} {ticker} to {recipient}:` List every selected outpoint and its `tokenAmount`, then ask for confirmation. |
| Several exact combinations | `The CLI found {combinationCount} exact combinations for {amount} {ticker}, ordered by fewer UTXOs:` Number each returned combination and list its selected outpoints and token amounts, then ask the user to choose one. |
| No exact combination | `The current transferable inscriptions have no exact combination for {amount} {ticker}. Available denominations: {denominations}. You can choose another amount or create a transfer inscription for {amount} {ticker}.` Offer inscription only when the paired current reads establish sufficient `remainingInscribableAmount`. |
| Combination search limit reached | `The bounded combination search reached its limit before proving whether {amount} {ticker} has an exact match. Here are the current transferable denominations: {denominations}.` Present the returned choices for a user-directed selection. |
| No transferable amount, inscription supported | `You do not currently have a transferable {ticker} inscription for {amount} {ticker}. A transfer inscription for the full {amount} {ticker} is a separate on-chain transaction and must confirm before it can be sent. Would you like to prepare that inscription?` |
| No transferable amount, inscription not established | `You do not currently have a transferable {ticker} inscription UTXO. A transfer inscription is a separate on-chain transaction.` Do not claim that the requested amount can be inscribed. |
| Inscription, fee returned | `A transfer inscription for {amount} {ticker} is ready. The network fee is {feeReadable} BTC. After it confirms, {amount} {ticker} becomes transferable. Reply "Confirm" to inscribe.` |
| Inscription, no fee returned | `A transfer inscription for {amount} {ticker} is ready. After it confirms, {amount} {ticker} becomes transferable. Reply "Confirm" to inscribe.` |
| Submitted inscription | `The transfer inscription was submitted. Order ID: {orderId}. Tx Hash: {txHash}. This is asynchronous and is not transferable yet. You can ask me to check the inscription status later in this conversation, or use this complete command in another session: {checkInscriptionStatusCommand}. After it reaches READY_TO_TRANSFER, query the transferable BRC-20 UTXO list and start a separate transfer.` |
| Service requires inscription | Relay the service message, then: `This transfer ended without signing or broadcasting. Creating a transfer inscription is a separate on-chain operation.` |

Derive `remainingInscribableAmount` only from the paired current reads as `totalAmount - transferableAmount`, using exact decimal arithmetic. Present it as an available amount, not as a required action. Do not promise an indexing duration. Copy every returned transaction hash, order ID, and continuation command verbatim. After a successful inscription submission, include both the same-conversation status-check guidance and the portable full status command.

## Conversation Context

- Resolve “inscribe” only to a single previous BRC-20 request. If several ticker/amount choices are pending, ask the user to choose.
- In the same conversation, a later inscription-status request resolves only when exactly one inscription order is pending; otherwise ask which order ID to query. In a new conversation, require the user to provide the order ID or paste the complete status command shown at submission.
- After account/login/address changes, or when an inscription becomes ready, refresh the transferable list before the next BRC-20 transfer decision.
- Re-query the selected ticker during send. If any outpoint in the confirmed combination is gone, show the refreshed plan and obtain a fresh confirmation.
- A completed inscription does not carry an old transfer intent forward; begin a new transfer command after explicit confirmation.
