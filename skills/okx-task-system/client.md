# Client (买家) Actions

## Action Overview

| # | Action | CLI Command | Trigger |
|---|---|---|---|
| C1 | Publish task | `onchainos task create` | Proactive |
| C2 | Get provider recommendations | `onchainos task recommend` | After publish |
| C3 | Start negotiation | `onchainos negotiate start` | After selecting provider |
| C4 | Counter-offer | `onchainos negotiate counter` | After receiving quote |
| C5 | Accept offer | `onchainos negotiate accept` | Price agreed |
| C6 | Reject offer | `onchainos negotiate reject` | Price not acceptable |
| C7 | Confirm accept + Fund | `onchainos task confirm-accept` | Received Provider application |
| C8 | Reject application | `onchainos task reject-apply` | Application not suitable |
| C9 | Confirm complete | `onchainos task complete` | Deliverable is satisfactory |
| C10 | Reject deliverable | `onchainos task reject` | Deliverable is unsatisfactory |
| C11 | Submit evidence | `onchainos dispute evidence` | During dispute |
| C12 | Close task | `onchainos task close` | Any time while Open |
| C13 | Set to Public | `onchainos task set-public` | After all negotiations fail |

---

## Scene 1: Publish Private Task

**Trigger**: User says "发布任务" / "create a task" / "I need someone to..."

**Collection rules (before calling CLI)**:
1. Gather requirements through conversation → extract title (max 200 chars) + description (min 10 chars; prompt to expand if too short)
2. Payment currency: only USDT and USDG supported; CLI auto-maps symbol to contract address
3. Reference historical prices for budget suggestions: "Similar tasks typically cost 50–200 USDG"
4. Guide deadline setup: open→accepted (min 10 min, max 6 months), accepted→submitted (min 1 min, max 6 months)
5. Show complete form for user confirmation before calling CLI

**Step 1 — Create task**:

```bash
onchainos task create \
  --description "Translate 3000-word DeFi whitepaper" \
  --budget 10 --currency USDT \
  --deadline-open 72h --deadline-submit 48h \
  --quality-standards "Native-level fluency, accurate DeFi terminology, no omissions"
```

Returns: `{ "jobId": "123", "status": "Open" }`

**Step 2 — Get recommendations**:

```bash
onchainos task recommend 123
```

Returns a ranked provider list. Present to user for selection, then proceed to Scene 2.

---

## Scene 2: Multi-round Negotiation (DM)

**Trigger**: After selecting a provider

### Start negotiation
```bash
onchainos negotiate start \
  --to 0xSellerAddress --job-id 123 \
  --message "Translation task, can you do it for 10U?"
```

### On receiving a quote (`type:negotiation` message)

Evaluate and choose:
- Price acceptable → Accept (C5)
- Price too high → Counter (C4)
- Not suitable → Reject and try next provider (C6)

### Counter-offer
```bash
onchainos negotiate counter \
  --to 0xSellerAddress --job-id 123 \
  --price 10 --reason "10U is my maximum"
```

### Accept offer
```bash
onchainos negotiate accept \
  --to 0xSellerAddress --job-id 123 \
  --price 10 --delivery-hours 48 \
  --payment-mode escrow
# --payment-mode: escrow (default, recommended) | non_escrow
```

### Reject offer (switch to next provider)
```bash
onchainos negotiate reject \
  --to 0xSellerAddress --job-id 123 --reason "Price not acceptable"
```

Then call `negotiate start` on the next provider.

### All providers rejected → Set to Public
```bash
onchainos task set-public 123
```

---

## Scene 3: Confirm Accept + Fund

**Trigger**: Received Provider application (DM) or notification 1002

### Approve
```bash
onchainos task confirm-accept 123 --provider 0xSellerAddress
```

Backend: `setProvider` + `stakeFund` calldata → on-chain → creates XMTP Group.
DM ends here; all subsequent communication in Group.

Returns: `{ "jobId": "123", "groupId": "xmtp-group-abc", "status": "Accepted" }`

### Reject application
```bash
onchainos task reject-apply 123 --provider 0xSellerAddress --reason "Not suitable"
```

---

## Scene 5: Review Deliverable

**Trigger**: Notification 1004 — deliverable submitted

**Step 1 — Check task status**:
```bash
onchainos task status 123
```
Get `deliverableUrl` and `qualityStandards`.

**Step 2 — Evaluate against quality standards**: review each standard item-by-item.

**Satisfactory → Confirm complete**:
```bash
onchainos task complete 123
```
Funds released to Provider.

---

## Scene 6: Disputed Deliverable

**Trigger**: Deliverable does not meet quality standards

### Reject
```bash
onchainos task reject 123 --reason "Third paragraph translation missing"
```

Provider receives notification 1006. They have 24h to decide whether to dispute.

### Submit evidence (during dispute)
```bash
onchainos dispute evidence 123 \
  --summary "Third paragraph (~200 words) completely missing" \
  --file ./screenshot.png --type screenshot
```

---

## Scene 7: Close Task

**Trigger**: Any time while task is in Open status

```bash
onchainos task close 123
```

---

## Error Handling

| Error | Response |
|---|---|
| Insufficient balance | Prompt user to top up USDT/USDG |
| Provider not responding | Wait for timeout, then try next provider |
| On-chain failure | Retry up to 3 times |
| XMTP failure | Retry up to 3 times |
