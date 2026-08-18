# BRC-20 Agentic Wallet

Use this flow for a BRC-20 ticker balance, transferable inscription UTXOs, selected transfer, transfer inscription, and inscription status. Normalize a ticker to `btc-brc20-<ticker>` before any command. Direct transfer amount comes from the selected service-returned UTXO; transfer-inscription input remains an exact human-readable amount converted by the CLI without floating-point arithmetic.

## Flow

1. For a holdings query, query both total holdings and the current transferable inscription UTXOs. Report total holdings separately from the exact transferable inscription denominations. Label an amount as uninscribed/available to inscribe only when the CLI or service explicitly returns that fact; never derive it by subtracting the transferable sum from total holdings.
2. For “what BRC-20 can I transfer now?” or a BRC-20 transfer request, obtain the ticker/token address and run `wallet utxo brc20-transferable --chain bitcoin`. The active wallet supplies the sender address.
3. **MUST**: Treat each returned UTXO as one indivisible transfer inscription. One direct-transfer command spends exactly one selected UTXO because inscriptions cannot be split or combined locally.
4. If the user supplied an amount, use it only to match returned `tokenAmount` values. Propose one exact match; for multiple exact matches ask the user to select an outpoint. With no exact match, explain the limitation and list the complete transferable denominations. Without an amount, list every choice and wait for one selection.
5. **MUST**: Start a direct transfer only after the recipient and one current outpoint are fixed. The selected UTXO determines the amount. Run `wallet send` without `--force`; the CLI refreshes the list and owns preparation, signing, and ordinary single-transaction broadcast. Follow any ordinary backend confirmation exactly.
6. If no inscription matches, offer a separate transfer inscription for the full requested amount only when the CLI or service explicitly reports enough uninscribed amount. **NEVER** inscribe only the difference or start inscription automatically, because inscription is a separate on-chain write. Otherwise report only the service-known shortage or current whole denominations.
7. For an inscription, start its separate command without `--force` and follow its returned preview confirmation. For any `confirming:true`, display `message` and any returned `preview`; execute `next` only after explicit confirmation.
8. Preserve service errors, especially `44002` and `44003`; do not rewrite them as a local balance decision.
9. An inscription is asynchronous. After submission, show the returned Reveal `orderId` and the complete `nextSteps.checkInscriptionStatus` command verbatim. Tell the user that, later in this conversation, they may say “查询铭刻结果”; the complete command is the portable continuation for a different session. Never query with the Commit transaction hash. When status is `READY_TO_TRANSFER`, refresh the transferable BRC-20 UTXO list before any new transfer. **NEVER** auto-send after inscription completion.

## Intent Routing

| Intent | Parameters | Command |
| --- | --- | --- |
| BRC-20 holdings and transfer composition | ticker | Run both `wallet balance --chain bitcoin --token-address <btc-brc20-ticker>` and the transferable-UTXO query below |
| Currently transferable BRC-20 UTXOs/tokens | ticker | `wallet utxo brc20-transferable --chain bitcoin --token-address <btc-brc20-ticker>` |
| Transfer BRC-20 | ticker, recipient, one selected outpoint from the latest list | `wallet send --chain bitcoin --contract-token <token-address> --brc20-outpoint <txHash:voutIndex>` |
| Create transfer inscription | ticker, readable amount | `wallet inscription create --chain bitcoin` |
| BRC-20 direct transfer status | tx hash or order ID | Shared `wallet history --chain bitcoin` query |
| Transfer inscription status | tx hash or order ID | `wallet inscription status --chain bitcoin` |

## User-Facing Templates

Render the templates below in the user's current language and use only facts returned by the CLI. Substitute token ticker from the returned token address. Transfer uses the shared ordinary-send confirmation and has no chain-specific preview template. For inscription, show a network fee sentence only when `preview.feeReadable` is non-null and non-empty; otherwise omit the entire fee sentence. **NEVER** invent, estimate, or sum a fee.

| Situation | Output |
| --- | --- |
| Balance with transferable choices | `You currently hold {totalAmount} {ticker}. Your transferable inscriptions have these whole denominations: {denominations}. Each inscription is bound to one UTXO and can only be transferred in full.` Include an explicitly returned uninscribed amount only when the CLI/service supplies it. |
| Balance without transferable choices | `You currently hold {totalAmount} {ticker}, but no transferable {ticker} inscription UTXO is currently available.` Mention an explicitly returned uninscribed amount only when present. |
| Transferable choices | `You currently have {count} transferable {ticker} inscription UTXOs totaling {sumValue} {ticker}. Choose one outpoint to transfer:` Then list each exact `selection`, `tokenAmount {ticker}`, `utxoAmountSats sats`, `inscriptionId`, and `offset`. |
| Exact denomination available | `You have one transferable inscription for {amount} {ticker}; it can be transferred in full to {recipient}.` Append a network-fe sentence only when returned, then ask for confirmation. |
| Requested denomination unavailable | `You cannot currently send {amount} {ticker}. Your transferable inscriptions have these whole denominations: {denominations}. They cannot be split.` Offer only service-supported alternatives. |
| No transferable amount, inscription supported | `You do not currently have a transferable {ticker} inscription for {amount} {ticker}. A transfer inscription for the full {amount} {ticker} is a separate on-chain transaction and must confirm before it can be sent. Would you like to prepare that inscription?` |
| No transferable amount, inscription not established | `You do not currently have a transferable {ticker} inscription UTXO. A transfer inscription is a separate on-chain transaction.` Do not claim that the requested amount can be inscribed. |
| Inscription, fee returned | `A transfer inscription for {amount} {ticker} is ready. The network fee is {feeReadable} BTC. After it confirms, {amount} {ticker} becomes transferable. Reply "Confirm" to inscribe.` |
| Inscription, no fee returned | `A transfer inscription for {amount} {ticker} is ready. After it confirms, {amount} {ticker} becomes transferable. Reply "Confirm" to inscribe.` |
| Submitted inscription | `The transfer inscription was submitted with Reveal order ID {orderId}. This is asynchronous and is not transferable yet. Later in this conversation, say “查询铭刻结果”, or use this complete command in another session: {checkInscriptionStatusCommand}. After it reaches READY_TO_TRANSFER, query the transferable BRC-20 UTXO list and start a separate transfer.` |
| Service requires inscription | Relay the service message, then: `This transfer ended without signing or broadcasting. Creating a transfer inscription is a separate on-chain operation.` |

Do not claim an inscription count or uninscribed amount unless the CLI/service returns it. Do not promise an indexing duration. Copy every returned transaction hash, order ID, and continuation command verbatim. Never tell the user merely that they “can query later”: always include both the same-conversation phrase and the portable full status command after a successful inscription submission.

## Conversation Context

- Resolve “inscribe” only to a single previous BRC-20 request. If several ticker/amount choices are pending, ask the user to choose.
- In the same conversation, “查询铭刻结果” resolves only when exactly one Reveal inscription order is pending; otherwise ask which Reveal order ID to query. In a new conversation, require the user to provide the Reveal order ID or paste the complete status command shown at submission.
- After account/login/address changes, or when an inscription becomes ready, refresh the transferable list before the next BRC-20 transfer decision.
- Re-query the selected ticker during send. If the exact outpoint is gone, do not substitute another choice automatically.
- A completed inscription does not carry an old transfer intent forward; begin a new transfer command after explicit confirmation.
