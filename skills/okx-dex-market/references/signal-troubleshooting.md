# Capability: Signal — Troubleshooting

## Recovery

Error handling, edge cases, and region restrictions for the Signal capability. For normal flow, return to [signal.md](signal.md); read [signal-cli-reference.md](signal-cli-reference.md) only when command facts are needed.

- **Unsupported chain for signals**: not all chains support signals — always verify with `onchainos signal chains` first
- **Empty signal list**: no signals on this chain for the given filters — suggest relaxing `--wallet-type`, `--min-amount-usd`, or `--min-address-count`, or try a different chain
- **Unsupported chain for leaderboard**: always verify with `onchainos leaderboard supported-chains` first
- **Empty leaderboard**: no traders match the filter combination — suggest relaxing `--wallet-type`, PnL range, or win rate filters
- **Max 20 leaderboard results per request**: inform user if they need more

## Failure Diagnostics

When a command fails with error code `50125` or `80001`, display:

> DEX is not available in your region. Please switch to a supported region and try again.

Do not expose raw error codes or internal error messages to the user.
