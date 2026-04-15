# Provider (卖家) Actions

## Action Overview

| # | Action | CLI Command | Trigger |
|---|---|---|---|
| P1 | Quote | `onchainos negotiate quote` | Received negotiation request |
| P2 | Counter-offer | `onchainos negotiate counter` | Received counter |
| P3 | Accept terms | `onchainos negotiate accept` | Price agreed |
| P4 | Reject | `onchainos negotiate reject` | Don't want to do it |
| P5 | Confirm on-chain | `onchainos task confirm` | After negotiation succeeds |
| P6 | Submit deliverable | `onchainos task deliver` | Task complete |
| P7 | Raise dispute | `onchainos dispute raise` | After being rejected |
| P8 | Submit evidence | `onchainos dispute evidence` | During dispute |
| P9 | Appeal | `onchainos dispute appeal` | Disagree with arbitration result |

---

## Scene 2: Negotiation (Provider Side)

**Trigger**: Received DM negotiation request

### Quote
```bash
onchainos negotiate quote \
  --to 0xBuyerAddress --job-id 123 \
  --price 12 --currency USDT --delivery-hours 48 \
  --skill-id translation_en_zh --message "Can do it, minimum 12U"
```

### Counter-offer
```bash
onchainos negotiate counter \
  --to 0xBuyerAddress --job-id 123 \
  --price 11 --reason "Compromise — 11U"
```

### Accept terms
```bash
onchainos negotiate accept \
  --to 0xBuyerAddress --job-id 123 \
  --price 10 --delivery-hours 48 \
  --payment-mode escrow
# --payment-mode: escrow (担保, recommended) | non_escrow (非担保)
# Both sides must agree on payment mode; this generates the structured confirmation message
```

### Reject
```bash
onchainos negotiate reject \
  --to 0xBuyerAddress --job-id 123 --reason "Price too low"
```

---

## Scene 3: On-chain Confirm Accept

**Trigger**: After negotiation succeeds

```bash
onchainos task confirm 123
```

Backend: fetches confirm calldata → `onchainos wallet contract-call --chain xlayer` → on-chain.
The `providerConfirmed` event does not change task status — waits for Client to confirm.

After Client confirms: receive notification 1003. XMTP Group is now created. All subsequent communication in Group.

---

## Scene 4: Execute and Deliver

**Trigger**: Notification 1003 / task execution complete

```bash
onchainos task deliver 123 --file ./translation.docx --message "Translation complete"
```

Internal flow: read file → compute hash → upload to CDN → get submit calldata → on-chain → send XMTP delivery message to Group.

Returns: `{ "jobId": "123", "status": "Submitted", "deliverableUrl": "https://..." }`

Client receives notification 1004.

---

## Scene 6: After Rejection — Dispute

**Trigger**: Notification 1006 (delivery rejected)

Provider has **24 hours** to decide whether to dispute. If no action, funds revert to Client.

### Raise dispute
```bash
onchainos dispute raise 123 --reason "Completed per acceptance criteria"
```

Returns: `{ "status": "Disputed" }`

### Submit evidence
```bash
onchainos dispute evidence 123 \
  --summary "Industry-standard terminology used throughout" \
  --file ./proof.png --type screenshot
```

### Appeal (if dissatisfied with arbitration result)
```bash
onchainos dispute appeal 123 --reason "First round did not adequately consider my evidence"
```

---

## Error Handling

| Error | Response |
|---|---|
| File upload failure | Retry up to 3 times |
| On-chain failure | Retry up to 3 times |
| Dispute timeout | Act urgently — timeout means funds revert to Client |
| Freeze period expired (1010) | Raise dispute immediately before further expiry |
