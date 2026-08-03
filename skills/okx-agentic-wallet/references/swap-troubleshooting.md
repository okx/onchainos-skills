# DEX Swap — Troubleshooting

Load on a swap failure or edge case. Items in the swap.md Risk Controls table (honeypot, price impact, tax, new tokens, insufficient liquidity, no quote) are not repeated here.

## Error Retry (after `swap execute` returns an error)

**Chain not support (`81104`) — evaluate this FIRST, before every rule below.** "Chain not support" is a deterministic, permanent condition: the target chain does not support swap, so the outcome cannot change between attempts.

- **NEVER**: issue a second `swap execute` on code `81104` — a retry re-hits the same unsupported chain and returns the identical error, wasting a backend round-trip.
- **NEVER**: run the pending-approval wait below for `81104` — no block-time delay and no "waiting for on-chain confirmation" message; this early-exit takes precedence over every branch that follows.
- **NEVER**: show the raw code `81104` to the user — mirror the region-restriction row's hardcoded-message pattern.
- **MUST**: display a fixed reminder, then stop: "⚠️ This chain does not currently support swap. Please try a different chain."

If the error is **not** `81104`, it may be caused by a preceding approval tx not yet confirmed on-chain:

1. **Wait** by chain block time before retrying: Ethereum ~15s · BSC ~5s · Arbitrum / Base ~3s · XLayer ~3s · other EVM ~10s.
2. **Inform the user**, e.g. "Swap failed, possibly due to a pending approval — waiting for on-chain confirmation before retrying."
3. **Non-recoverable (82000, 51006)**: token is dead / rugged / no liquidity — retrying may not help. Do NOT retry after 5 consecutive errors for the same `(wallet, fromToken, toToken)`. Run `token advanced-info`; warn if `devRugPullTokenCount > 0` or `tokenTags` contains `lowLiquidity`.
4. **Risk warning (81362)**: backend flagged the broadcast as potentially dangerous (possible honeypot / poisoned contract). Do NOT auto-retry. Explicitly warn the user that forcing execution may cause fund loss; only if they confirm, re-run the **same** `swap execute` with `--force` (passes `skipWarning: true`). Never add `--force` without explicit confirmation.
5. **All other errors**: retry once; if it fails again, surface the error.

## Failure Diagnostics

When a swap fails (broadcast error, on-chain revert, or timeout), produce a diagnostic summary before reporting:

```
Diagnostic Summary:
  txHash:        <hash or "simulation failed">
  chain:         <chain name (chainIndex)>
  errorCode:     <API or on-chain error code>
  errorMessage:  <human-readable error>
  tokenPair:     <fromToken symbol> → <toToken symbol>
  amount:        <amount in UI units>
  slippage:      <value used, or "auto">
  mevProtection: <on|off>
  walletAddress: <address>
  timestamp:     <ISO 8601>
  cliVersion:    <onchainos --version>
```

## Edge Cases

- **Insufficient balance**: check balance first, show current balance, suggest adjusting the amount.
- **Network error**: retry once, then produce the diagnostic summary and prompt the user.
- **Region restriction (error code 50125 or 80001)**: do NOT show the raw code. Display: "⚠️ Service is not available in your region. Please switch to a supported region and try again."
