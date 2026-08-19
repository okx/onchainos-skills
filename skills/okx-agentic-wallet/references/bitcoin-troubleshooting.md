# Native Bitcoin Troubleshooting

## Authentication And Input

- Login-required output: complete wallet login, then repeat the original command.
- Missing Bitcoin address: refresh account addresses through the current wallet flow. A different account requires a fresh query or transfer command.
- Sender mismatch: use the current account Bitcoin address from `wallet addresses --chain bitcoin`.
- Address errors: re-check the complete Bitcoin mainnet recipient address. The Agentic wallet sender is Taproot.
- Amount errors: provide a positive `--readable-amount` within BTC precision.
- Outpoint errors: use the complete `<txHash>:<voutIndex>` from the latest UTXO query.
- UTXO response shape: inactive branches may be `null` for the selected `queryType`. Do not retry or call another detail endpoint.
- Empty `USER_IGNORED_LIST`: means the service returned no UTXOs whose asset occupancy was removed by the user.
- Empty `AVAILABLE_UTXO_LIST`: means the service returned no currently available BTC UTXOs; report zero available sats without subtracting other views locally.
- Status ambiguity: provide one complete tx hash or order ID.

## Service Results

| Code/state | Meaning | Flow |
| --- | --- | --- |
| `44001` / `INSUFFICIENT_UTXO` / insufficient-or-occupied UTXOs | The available BTC UTXOs cannot complete the transfer | Output: `The currently available BTC UTXOs cannot complete this transfer. Would you like me to check your available UTXOs and transferable BTC amount?` After the user accepts, run `wallet utxo available --chain bitcoin`. |
| `STATE_CHANGED` | Queried UTXO or UTXO-management preview state changed | Re-run the read or management preview. |
| `PREVIEW_INTENT_MISMATCH` / `INCOMPLETE_TRANSACTION_PREVIEW` | UTXO-management preview cannot safely represent the request | Stop and report the error. |
| `MEMPOOL_REMOVED` | Original transaction left mempool and its inputs may remain occupied | Run `wallet utxo list --chain bitcoin --unavailable`; reclaim requires explicit confirmation. A new transfer command is required. |
| `82001` / `UTXO_PERMISSION_DENIED` | Address/UTXO is not owned by the current account or identity cannot manage it | End the operation and refresh account facts before a new request. |
| `82002` / `UTXO_NOT_FOUND` | Target UTXO no longer exists | Run `nextSteps.queryUnavailableUtxos` to refresh state. |
| `82003` / `INVALID_UTXO_REQUEST` | UTXO request/action/query/grouping/batch input is invalid | End the operation and report the service message. |
| `82005` / `UTXO_ALREADY_SPENT` | Target UTXO was already spent | Run `nextSteps.queryUnavailableUtxos` to refresh state. |
| `UTXO_MANAGE_REJECTED` | Current management batch was rejected | Report its outpoints and reason, then use returned post-operation state. |
| `UTXO_MANAGE_PARTIAL_FAILURE` | A later management batch failed | Report every batch result and use returned state as authoritative. |

Known failures use the output and continuation defined in the table. Unknown failures end the current operation with a concise user-facing summary and any returned read-only recovery option.

## Failed Write Diagnostics

**MUST**: After a user-requested safe retry also fails, create a support diagnostic with `txHash` when available, `bitcoin` chain, service code/reason, asset, amount, wallet address, timestamp, and CLI version. The user-facing response uses the matching Service Results template and its next safe action.
