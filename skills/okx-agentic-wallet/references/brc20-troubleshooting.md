# BRC-20 Troubleshooting

## Authentication And Input

- Login-required output: complete wallet login, then repeat the original command.
- Ticker errors: use `btc-brc20-<ticker>`; do not supply a token contract address from another chain.
- Transfer selection errors: repeat `--brc20-outpoint <txHash:voutIndex>` for every item in one current CLI-returned combination.
- Transfer-inscription amount errors: provide a positive exact decimal string in `--readable-amount`; the CLI converts it with the token metadata decimal before `unsignedInfo`.
- Recipient errors: use a complete Bitcoin mainnet address.
- Status ambiguity: provide one complete tx hash or order ID.

## Balance And Transfer States

| Code/state | Meaning | Flow |
| --- | --- | --- |
| `selectionPlan.status=NO_EXACT_MATCH` | The current complete UTXOs have no subset whose token amounts sum exactly to the request | Show denominations. Offer another amount or inscription only when the current flow establishes sufficient `remainingInscribableAmount`. |
| `selectionPlan.status=SEARCH_LIMIT_EXCEEDED` | The search reached 100,000 distinct amount states before completion | Show the returned choices and describe the result as incomplete. Continue with a user-selected exact combination or a simpler amount. |
| `44003` / `NEED_INSCRIBE` | Service requires a separate transfer inscription before this BRC-20 transfer | Preserve the service response and end the transfer. Offer inscription only for an explicit user request and confirm it separately. |
| A selected BRC-20 UTXO is no longer transferable | The refreshed transferable list no longer contains every outpoint in the confirmed combination | Show the refreshed amount-aware plan and obtain a fresh selection before `unsignedInfo`. |
| `44002` / `INSUFFICIENT_BTC_FOR_INSCRIPTION` | BTC funding UTXOs for inscription are insufficient | Relay service message. Use returned read-only address and BTC-balance next steps; do not rewrite this as a BRC-20 balance error. |
| `INSCRIBING`, `WAITING_CONFIRMATION`, `WAITING_INDEXER` | Inscription is asynchronous and not yet transferable | Show the returned `orderId`, `txHash`, and complete `nextSteps.checkInscriptionStatus` command. In the current conversation the user may request another status check; in another session they must use that command or provide the order ID. |
| `READY_TO_TRANSFER` | Inscription is ready | Run returned `nextSteps.queryBrc20TransferableUtxos`, show the current choices, then require a separate fresh transfer request and confirmation. |
| Inscription `STATE_CHANGED` | UTXO state or inscription preview changed | Start a new inscription preview if the user still wants the write. |
| Inscription `PREVIEW_INTENT_MISMATCH` / `INCOMPLETE_TRANSACTION_PREVIEW` | CLI cannot safely represent the inscription request | Stop this operation and report the error. |

Never unify service codes into a generic BRC-20 failure: preserve the service message and code for diagnostics. Use the current flow's paired reads; present any remaining inscribable amount as available, not required.

## Failed Write Diagnostics

**MUST**: After a user-requested safe retry also fails, create a support diagnostic with `txHash` when available, `bitcoin` chain, BRC-20 token address, amount, service code/reason, wallet address, timestamp, and CLI version. Include preview fee and fee rate only for an inscription when returned. **NEVER** expose raw codes in user-facing output; show the service message and the next safe action.
