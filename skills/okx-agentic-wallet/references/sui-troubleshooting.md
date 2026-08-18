# SUI Troubleshooting

## Authentication And Account

- Login-required output: run the wallet login flow, then repeat the original command.
- Missing SUI address: refresh account addresses through the current wallet flow. A different account requires a fresh query or transfer command.
- Sender mismatch: use the current account SUI address returned by `wallet addresses --chain sui`.

## Input

- Address errors: provide a SUI address with up to 64 hexadecimal characters; the CLI emits canonical `0x` plus 64 lowercase hex.
- Coin Type errors: provide the complete `<package>::<module>::<type>` value returned by the asset query.
- Amount errors: provide a positive `--readable-amount` within the asset decimal precision.
- Status lookup ambiguity: supply one complete transaction hash or order ID.

## Service Results

| Code/state | Meaning | Flow |
| --- | --- | --- |
| `PRE_EXECUTION_FAILED` | The service simulation rejected the current transaction | Relay the service reason and end this operation |
| `confirming=true` | Broadcast service requires the shared transfer confirmation | Relay the service message; after explicit confirmation, repeat the same command with `--force` |
| `LOCAL_SIGNING_FAILED` | Session signing material or SUI encoding validation failed | End this operation and report the error |

Service `error` and `data.message` are the user-facing explanation. Unknown failures end the current operation with the returned message; retain the code only for diagnostics, then establish fresh facts with a new query.

## Failed Write Diagnostics

**MUST**: After a user-requested safe retry also fails, create a support diagnostic with `txHash` when available, `sui` chain, Coin Type, amount, service code/reason, wallet address, timestamp, and CLI version. Include `gasBudget` and `gasPrice` only when the CLI returned them. **NEVER** expose raw codes in the user-facing response; show the returned service message and the next safe action.
