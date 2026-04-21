# Cross-Chain Troubleshooting

> Load this file when a cross-chain transaction fails or an edge case is encountered.

## Failure Diagnostics

When a cross-chain transaction fails, generate a **diagnostic summary** before reporting to the user:

```
Diagnostic Summary:
  orderId:       <order ID or "not yet created">
  txHash:        <source chain hash or "simulation failed">
  fromChain:     <chain name (chainIndex)>
  toChain:       <chain name (chainIndex)>
  errorCode:     <API or on-chain error code>
  errorMessage:  <human-readable error>
  tokenPair:     <fromToken symbol> -> <toToken symbol>
  amount:        <amount in UI units>
  bridgeName:    <selected bridge protocol>
  mevProtection: <on|off>
  walletAddress: <address>
  receiveAddress:<address (if different from wallet)>
  timestamp:     <ISO 8601>
  cliVersion:    <onchainos --version>
```

## Edge Cases

> Items covered by the **Risk Controls** table (honeypot, price impact, tax, limits, receive-address) are not repeated here. Refer to Risk Controls for action levels.

### Insufficient balance
Check balance first (`wallet balance --chain <from-chain>`). Show current balance and required amount. Suggest adjusting amount or depositing more tokens.

### Insufficient gas
Source chain native token balance is zero. Prompt user to deposit gas tokens before retrying.

### Region restriction (error code 50125 or 80001)
Do NOT show raw error code. Display:
`Service is not available in your region. Please switch to a supported region and try again.`

### Approval transaction failed
- Check gas balance on source chain
- Suggest retrying with `execute --confirm-approve`
- If repeated failures, check if token has non-standard approval behavior

### Approval confirmation timeout
- 30 poll attempts exhausted without terminal status
- Transaction may still be pending in mempool
- Suggest: `onchainos wallet history --tx-hash <approveTxHash>` to manually check
- For EVM: if stuck, user can submit a 0-value transaction with the same nonce to cancel

### Execute fails after approval confirmed
- TEE pre-execution may have failed (insufficient allowance not yet reflected, or price moved)
- Retry: `execute --skip-approve` (will re-quote with fresh pricing)
- If repeated failures, check balance and allowance status via fresh `quote`

### txHash not visible on public chain
- Possible cause: custody wallet stuck order (transaction not actually broadcast)
- Suggest user cancel the order; EVM can reset with nonce 0
- If user cannot self-resolve, escalate to OKX support with orderId + fromTxHash

### Order status stuck at "0" for extended time
- Check `bridgeChildOrderDetailVo.status`:
  - status=100 -> bridge is processing, suggest checking bridge explorer
  - Other -> suggest waiting and re-checking
- If stuck > 4 hours, escalate to OKX support

### Order status "-1" (failed)
Two outcomes:
1. **Source chain failure**: funds not sent, no refund needed. Check balance -- should be unchanged.
2. **Bridge/destination failure**: 
   - Wait for WAIT_REFUND -> REFUNDED status transition
   - Show `refundTxHash` when available
   - Explain: fee difference between sent and refunded amounts is normal (bridge protocol fee + gas)
   - If no refund after 4 hours, escalate to OKX support

### /order/update error
- Code 0 = success, ignore other codes
- If error msg present, show to user: "Order update note: {error_msg}. Transaction was broadcast. Check status for final result."
- Transaction was already broadcast; order update failure does not mean transaction failed

### Network error
Retry once. If still fails, generate diagnostic summary and prompt user.
