# BRC-20 Troubleshooting

## Authentication And Input

- Login-required output: complete wallet login, then repeat the original command.
- Ticker errors: use `btc-brc20-<ticker>`; do not supply a token contract address from another chain.
- Transfer selection errors: use one complete `<txHash>:<voutIndex>` from the latest `brc20-transferable` result. Do not substitute a different UTXO locally.
- Transfer-inscription amount errors: provide a positive exact decimal string in `--readable-amount`; the CLI converts it with the token metadata decimal before `unsignedInfo`.
- Recipient errors: use a complete Bitcoin mainnet address.
- Status ambiguity: provide one complete tx hash or order ID.

## Balance And Transfer States

| Code/state | Meaning | Flow |
| --- | --- | --- |
| Requested amount has no exact transferable inscription denomination | The existing transfer inscriptions are indivisible and no current `tokenAmount` exactly matches | Do not submit a partial or combined direct transfer. Show the whole denominations returned by the latest transferable query. Offer a new inscription only when the service explicitly reports enough uninscribed amount for the full requested denomination. |
| `44003` / `NEED_INSCRIBE` | Service requires a separate transfer inscription before this BRC-20 transfer | Preserve the service response and end the transfer. Offer inscription only for an explicit user request and confirm it separately. |
| Selected BRC-20 UTXO is no longer transferable | The refreshed transferable list no longer contains the selected outpoint | Show the refreshed choices and ask the user to select again. Do not auto-select or continue to `unsignedInfo`. |
| `44002` / `INSUFFICIENT_BTC_FOR_INSCRIPTION` | BTC funding UTXOs for inscription are insufficient | Relay service message. Use returned read-only address and BTC-balance next steps; do not rewrite this as a BRC-20 balance error. |
| `INSCRIBING`, `WAITING_CONFIRMATION`, `WAITING_INDEXER` | Inscription is asynchronous and not yet transferable | Show the complete `nextSteps.checkInscriptionStatus` command. In the current conversation the user may later say “查询铭刻结果”; in another session they must use that command or provide its Reveal order ID. |
| `READY_TO_TRANSFER` | Inscription is ready | Run returned `nextSteps.queryBrc20TransferableUtxos`, show the current choices, then require a separate fresh transfer request and confirmation. |
| Inscription `STATE_CHANGED` | UTXO state or inscription preview changed | Start a new inscription preview if the user still wants the write. |
| Inscription `PREVIEW_INTENT_MISMATCH` / `INCOMPLETE_TRANSACTION_PREVIEW` | CLI cannot safely represent the inscription request | Stop this operation and report the error. |

Never unify service codes into a generic BRC-20 failure: preserve the service message and code for diagnostics. Do not infer transferable or inscribable amounts from total holdings.

## Failed Write Diagnostics

**MUST**: After a user-requested safe retry also fails, create a support diagnostic with `txHash` when available, `bitcoin` chain, BRC-20 token address, amount, service code/reason, wallet address, timestamp, and CLI version. Include preview fee and fee rate only for an inscription when returned. **NEVER** expose raw codes in user-facing output; show the service message and the next safe action.
