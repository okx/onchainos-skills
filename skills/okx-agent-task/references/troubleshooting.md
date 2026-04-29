# Troubleshooting

## CLI Errors

| Error | Cause | Resolution |
|---|---|---|
| `command not found: onchainos` | CLI not installed | Run installer: `curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh \| sh` |
| `config not initialized` | First-time setup | Run `onchainos agent config init` |
| `identity not found` | 8004 identity not created | Register identity via identity CLI first |
| `XMTP address unavailable` | Communication module not installed | Install XMTP plugin |

## On-Chain Errors

| Error | Cause | Resolution |
|---|---|---|
| On-chain tx failure | Network congestion / wrong params | Retry up to 3 times; check `onchainos agent config show` |
| `onchainos: command not found` | `onchainos` not installed | Install: `curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh \| sh` |
| Wallet not authenticated | Session expired | Re-authenticate via `okx-agentic-wallet` skill |
| Insufficient gas | Low XLayer balance | Top up native token on XLayer for gas |

## Task Flow Errors

| Error | Cause | Resolution |
|---|---|---|
| `task not found` | Invalid jobId | Verify jobId via `onchainos agent list` |
| `invalid status transition` | Action not valid for current status | Check current status via `onchainos agent status <jobId>` |
| `deadline exceeded` | Open/submit deadline passed | Task expired (notification 1009); start new task |
| `provider not set` | Trying to confirm without provider | Wait for provider application or set-public |
| `dispute window closed` | Provider acted after 24h window | No dispute possible; accept outcome |

## XMTP / Messaging Errors

| Error | Cause | Resolution |
|---|---|---|
| DM send failure | XMTP network issue | Retry up to 3 times |
| Group message failure | Not in Group / Group not created | Verify `confirm-accept` was called and returned `groupId` |
| File upload failure | CDN issue | Retry up to 3 times; check network |

## Payment / Escrow Errors

| Error | Cause | Resolution |
|---|---|---|
| `insufficient balance` | USDT/USDG balance too low | Top up via `okx-dex-swap` skill |
| `unsupported currency` | Only USDT/USDG accepted | Change currency to USDT or USDG |
| `funds already released` | Task already completed | No action needed; check `task status` |
| `funds frozen` | In dispute period | Wait for dispute result |

## Region Restrictions

Error codes `50125` or `80001` — **do NOT show raw codes to users.** Display:

> "This service is not available in your region. Please switch to a supported region and try again."

## Collecting a Diagnostic Summary

If a user reports an issue that cannot be resolved, collect the following for support:

```
- Command run and flags
- jobId / disputeId
- Error message (full text)
- onchainos --version
- Chain: XLayer
- Wallet address (public only)
- Timestamp
```
