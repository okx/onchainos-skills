# Cross-Chain Troubleshooting

> Load this file when a cross-chain transaction fails or an edge case is encountered.

## Failure Diagnostics

When a cross-chain transaction fails, generate a **diagnostic summary** before reporting to the user:

```
Diagnostic Summary:
  fromTxHash:    <source chain hash or "not yet broadcast">
  approveTxHash: <approve hash or "not needed / not run">
  fromChain:     <chain name (chainIndex)>
  toChain:       <chain name (chainIndex)>
  errorCode:     <API or on-chain error code>
  errorMessage:  <human-readable error>
  tokenPair:     <fromToken symbol> -> <toToken symbol>
  amount:        <amount in UI units>
  bridgeId:      <selected bridge id>
  bridgeName:    <bridge protocol name>
  mevProtection: <on|off>
  walletAddress: <address>
  receiveAddress:<address (if different from wallet)>
  timestamp:     <ISO 8601>
  cliVersion:    <onchainos --version>
```

## Error Code Reference

| Code | HTTP | Meaning | Action |
|---|---|---|---|
| 0 | 200 | Success | Continue |
| 50014 | 200 | Required parameter `{0}` missing | Surface which param is missing |
| 50125 | 200 | Region restriction / no API access to this endpoint | Display generic "Service unavailable in your region" — do NOT show raw code |
| 51000 | 200 | Param error `{0}` | Surface the offending param name to the user |
| 81362 | 200 | Backend risk system flagged the broadcast | WARN, ask user to confirm. If they explicitly confirm, retry with `--force` |
| 82000 | 200 | No liquidity / no available route. **Backend `msg` carries the human-readable reason** (e.g. "no available route for this token pair on this chain"). When the adapter is offline on an env, `msg` may be empty (CLI surfaces it as "unknown error"). | Surface the translated `msg` to the user; do NOT mention "82000". If `msg` is empty / "unknown error", trigger transit-token fallback (see SKILL.md "Fallback: No Direct Route"). When every transit also returns 82000 with empty `msg`, treat as "service unavailable on this environment" — do NOT loop further |
| 82104 | 200 | Token not supported | Trigger transit-token fallback OR tell user the token isn't supported |
| 82105 | 200 | Chain not supported | Tell user "This chain pair isn't currently supported by any bridge" — do NOT name protocols |
| 82106 | 200 | Bridge id not supported / wrong | Re-run `quote` without `--bridge-id` to let server pick |
| 82200 | 200 | Address blacklisted | BLOCK — tell user the address is flagged. Do NOT retry. |
| 82201 | 200 | Wallet address format invalid | Check user's wallet address; convert EVM to lowercase if mixed-case |
| 82202 | 200 | Receive address format invalid | Address doesn't match destination chain family. Ask user for correct format. |
| 82500 | 200 | Calldata build failed | Bridge server-side failure — retry once; if persistent, escalate |
| 5000 | 200 | System error, please retry | Retry once; if persistent, surface to user |

## Edge Cases

> The `Risk Controls` table in SKILL.md is the source of truth for token-scan, price impact, receive-address, balance, gas, and blacklist action levels. The `Error Handling` section of SKILL.md covers heterogeneous chain pairs, region restriction (50125), and the 81362 risk warning. The cases below are deeper failure modes that require operator-level diagnosis.

### Approval transaction failed
- Check gas balance on source chain
- Suggest retrying with `execute --confirm-approve`
- For USDT-pattern tokens: confirm `needCancelApprove=true` was respected (CLI handles this automatically; if backend hasn't yet emitted the field, the revoke step is skipped)

### Approval confirmation timeout (30 polls = 60s)
- Transaction may still be pending in mempool
- Suggest: `onchainos wallet history --tx-hash <approveTxHash>` to manually check
- For EVM stuck txs: user can submit a 0-value transaction with the same nonce (nonce 0 won't usually work — use the tx's actual nonce) to cancel

### Execute fails after approval confirmed
- TEE pre-execution may have failed (insufficient allowance not yet reflected, or price moved)
- Retry: `execute --skip-approve` (will re-quote with fresh pricing internally)
- If repeated failures, check on-chain allowance manually and re-run `quote --check-approve`

### fromTxHash not visible on public chain
- Possible cause: agentic wallet stuck (transaction not actually broadcast)
- Suggest the user check on the source chain explorer first
- If the broadcast genuinely never happened, escalate to OKX support with `fromTxHash` + bridge name + amount

### `status` returns NOT_FOUND
- **First 30 seconds**: expected. Bridge has not yet indexed the source tx. Wait and retry.
- **30 s – 5 min**: source tx might not be confirmed yet. Check the source chain explorer.
- **> 5 min**: source tx confirmed but bridge has not seen it. Likely bridge-side delay. Suggest checking the bridge's own scan page (Stargate / ACROSS / Relay). Wait up to original `estimateTime × 5`.
- **> 4 hours**: escalate to OKX support with `fromTxHash` + `bridgeName`.

### `status` stuck at PENDING
- Bridge has indexed source tx but destination delivery is delayed
- Check bridge's own scan page for delivery progress
- Wait up to original `estimateTime × 10` before escalating
- The status API does not return refund / failure sub-states. If long PENDING with no progress, support escalation is the path.

### Cross-chain failure with no `status` resolution
- Status only emits SUCCESS / PENDING / NOT_FOUND. There's no explicit failure state.
- Long-stuck NOT_FOUND or PENDING is the only failure signal we can surface.
- Always provide source chain explorer link as fallback so users can verify the source tx state independently.

### Multiple bridges available — which to pick
`routerList[]` is a multi-bridge list. When it has more than one entry:
- **Server-default sort (no `--bridge-id`, no `--sort`)**: top entry is the optimal route by `sort=0` (cheapest with reasonable speed). Recommend this as default.
- **User wants fastest**: re-run `quote` with `--sort=1` (when CLI exposes it) or pin a faster bridge from the table via `--bridge-id`.
- **User wants max output**: `--sort=2` or pin a low-fee bridge.
- **Want to enumerate all bridges explicitly**: loop `quote --bridge-id <id>` over each `bridgeId` returned by `bridges` — useful for debugging.

### Network error
Retry once. If still fails, generate diagnostic summary and prompt user.

## Status Polling Patterns

```bash
# Exponential backoff (recommended)
# Note: --bridge-id is REQUIRED (server returns 50014 without it).
# Use the bridgeId returned by `cross-chain execute` (selectedRoute) or pin from the user's quote choice.
DELAYS=(10 20 40 60 60 60 60 60 60 60)
for delay in "${DELAYS[@]}"; do
  sleep "$delay"
  RESP=$(onchainos cross-chain status --tx-hash <fromTxHash> --bridge-id <bridgeId> --from-chain <chainIndex>)
  STATUS=$(echo "$RESP" | python3 -c "import sys,json; print(json.load(sys.stdin)['data'][0]['status'])")
  case "$STATUS" in
    SUCCESS) echo "Cross-chain complete"; break;;
    PENDING) echo "Still bridging...";;
    NOT_FOUND) echo "Bridge has not yet indexed the tx";;
  esac
done
```

Total polling window ≈ original `estimateTime × 5`. After that window, escalate to support.

## Bridge Explorer References

For long-stuck cases, point users to the bridge's own scan page. The list below covers the protocols currently returned by `cross-chain bridges`. If a new protocol appears in `bridges`, look up its scan page on the project's own docs before referring users to it.

- Stargate / LayerZero: https://layerzeroscan.com/
- ACROSS V3: https://across.to/transactions
- Relay: https://relay.link/transactions
- Gas.zip: https://www.gas.zip/scan

(Map `bridgeId` → bridge name → scan URL via `cross-chain bridges` lookup.)
