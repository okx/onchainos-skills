# Wallet — Troubleshooting

Load on a wallet operation failure or edge case.

## Send
- **Insufficient balance**: check balance first; warn if too low (include gas estimate for EVM).
- **Wrong chain for token**: `--contract-token` must exist on the specified chain.

## History
- **No transactions**: display "No transactions found" — not an error.
- **Detail mode without chain**: `--chain` is required with `--tx-hash` / `--order-id` / `--uop-hash`. Ask which chain.
- **Empty cursor**: no more pages.

## Contract Call
- **Neither `--input-data` nor `--unsigned-tx`**: exactly one is required; the command fails otherwise.
- **Invalid calldata**: malformed hex causes an API error — help re-encode.
- **Simulation failure**: show `executeErrorMsg`, do NOT broadcast.
- **Insufficient gas**: suggest a higher `--gas-limit`.

## Common
- **Region restriction (error code 50125 or 80001)**: do NOT show the raw code. Display: "Service is not available in your region. Please switch to a supported region and try again."
- **Not logged in** (`not logged in`): session expired or store missing. **MUST**: recover by running `wallet login --phase init`, then the `nextSteps.completeLogin` command it returns (`wallet login --phase poll --session-id <authSessionId>`).
- **Credentials corrupted** (`Credentials corrupted. Please login again`): the credential store (`keyring.enc` / session) exists but is unreadable — distinct from *not logged in*. Do not retry the failing command blindly (it keeps hitting the same unreadable store); have the user re-authenticate with `wallet login`, which overwrites the unreadable store with a fresh one. If `wallet login` itself still errors, run `wallet logout` first (it clears the store without reading it) and then `wallet login`.
- **Confirming response (exit code 2, error code 81362)**: not an error — the backend needs confirmation. Handle via SKILL.md → Confirming Response.
