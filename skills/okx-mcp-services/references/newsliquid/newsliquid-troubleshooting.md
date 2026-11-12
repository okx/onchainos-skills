# NewsLiquid Troubleshooting

## Error Codes

| Response | Diagnosis |
| --- | --- |
| 401 | Invalid/expired login; restore this wallet's login, not a quota purchase |
| Business 402 `quota_insufficient` | One quota category cannot cover this request; total positive quota is not enough evidence |
| Business 402 `subscription_required` | Required plan rights absent; a quota package cannot grant them |
| `failureKind=unknown_402` | Original gateway denial has no recognized, unambiguous error field; the cause is unresolved |
| Order `/pay` 402 with `PAYMENT-REQUIRED` | Actual payment challenge; inspect and confirm terms before signing |
| 202/processing | Payment/authorization/cancellation is not yet confirmed |
| 403/404 | Actual access/path/target failure; preserve the status and error |
| 409 | Inspect actual conflict, including renewal, outstanding orders or imminent plan expiry |
| 429 | Respect returned `Retry-After`; do not hammer the service |
| 200 with nested failure/empty body | Content retrieval may still have failed; inspect actual fields |

## Failure Diagnostics

Record operation, wallet, time, HTTP status, original order/target ID, non-secret parameter snapshot and actual business error when available. Never record Tokens, private keys, signatures, payment headers or authenticated connection URLs. There is no uniform numeric business code; do not invent one.

## Recovery

### Login and storage

- On 401 or `login_required`, use the account Flow to restore the current wallet's login. A wallet switch selects that wallet's managed credential; stop the old wallet's operation.
- On credential-store failure, report the error and retain the order ID in task context. Keep unrelated credentials unchanged.
- On `credentialInvalidationFailed=true`, resolve the credential-store failure before restoring login. The original 401 response remains available.
- On an actual wallet warning, show its message and wait for consent. For login/cancellation, use the returned continuation with `params.confirmWalletWarning=true` and its original `wallet`. The CLI rechecks identity and current service state, then builds fresh unsigned data without saving a file. Other signing errors do not authorize `--force` or a new signature.

### Quota and plan rights

- On `quota_insufficient` or `subscription_required`, enter billing to proactively show its purchase options and allow refusal. Use billing's category/rights rules; neither error authorizes payment.
- On `unknown_402`, retain the original HTTP status/body and `requestContext` origin/path/wallet. Explain the unresolved denial and check this wallet's account status without replaying the query or initiating purchase. A generic “Payment Required” exception does not prove insufficient quota or permit signing.
- A response from `ai.6551.io/open/...` or a separate Twitter MCP is outside this wallet-backed gateway. Identify its source and independent credential context; do not infer its rights from the current wallet's Max status, forward the managed Token to it, or silently treat it as a native query result.

### Original orders and allowance

1. On `permit2_allowance_insufficient`, inspect the actual token, authoritative approval target, current allowance and required cap internally. Explain the real authorization amount/change in plain language, obtain bounded consent and use the existing wallet approval flow. Routine copy follows billing templates; real wallet warnings remain visible.
2. Keep the original order. After approval, prepare it again if needed. Only a typed `permit2_allowance_insufficient` failure with `stage=before_signing` proves that no payment signature was made. Re-prepare and retain the new unsigned approval in task context. Message text alone cannot authorize a retry.
3. If quote or allowance cap changed, prepare and confirm the new terms before payment. Stop on quote, period, wallet or contract mismatch.
4. On `newsliquid_operation_outcome_unknown`, `order_not_payable`, timeout, 5xx, 202/processing or unknown signing/submission, read the original service order. Never replay pay or create a second payment while the result is unknown. Bound status checks by the existing payment procedure and current session; retain unresolved order IDs in task context and report pending. The CLI has no durable attempt memory: keep attempts sequential and never invoke pay again merely because a status still reads unpaid after an unknown signing/submission result.
5. Charged queries and monitor changes with unknown results are not automatically replayed. After verified purchase arrival, billing resumes an interrupted query only once; another failure does not start another purchase loop.

### Paid but unfulfilled

1. If the original order is paid but still unfulfilled past the service's approximately five-minute window, use `billing-orders-id-reconcile`. This does not charge or require another payment signature.
2. `republished` is asynchronous; inspect the original order and quota before claiming arrival. Respect 409/429 and `Retry-After`; successful reconciliation is rate-limited for 60 seconds.
3. Repeated unresolved reconciliation or `manual_review` needs support. Retain the original order and actual status; do not pay again.

### Renewal and cancellation

- Active renewal conflicts with manual repurchase or plan changes. Explain the service 409 and obtain separate cancellation consent before changing it; never cancel automatically.
- For cancellation Confirming, use the actual warning procedure above with the same wallet and subId. Fresh unsigned CancelAuth is built after acceptance.
- After a definite cancellation rejection, inspect the current subscription, show the actual error and obtain fresh consent before another cancellation attempt. Use the same subId and current wallet; there is no restart flag or local cancellation history.
- On unknown/processing results, stop the attempt and inspect actual service state. The Agent must not automatically sign cancellation again.
- Verify actual cancellation before saying future charging stopped; paid rights remain to expiry. Keep each target ID distinct in task context.

### Content and streams

- For `text_not_cached`, explain that the report exists but its text was not obtained. A summary remains a summary. After timed-out PDF collection, a later read with `auto_collect=false` may succeed; do not imply automatic charged retries.
- Treat an early empty user-tweets page as a known stability limitation, not complete historical coverage.
- For custom WSS clients, inspect actual handshake and subscription responses. On 401, check the account through the CLI to invalidate a rejected credential before restoring login. The client then reads `token` again and resubscribes. If the account check succeeds but WSS still returns 401, stop for diagnosis. An ACK/count is not a business event.

## FAQ

### Why is a zero-cost call refused?

The gateway currently requires available quota even for zero-cost operations. Apply the real quota/plan error, not an invented price.

### Does stopping reception stop renewal?

No. Closing the local socket, deleting a server account monitor and cancelling future paid renewal are distinct operations.
