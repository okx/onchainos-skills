# CLI Reference

All commands: `onchainos <group> <subcommand> [flags]`

Global flags: `--format json|table` (default: json)

---

## task group

### create-task

Create a new task (Client only).

| Flag | Type | Required | Description |
|---|---|---|---|
| `--description` | string | ✓ | Task description (10–5000 chars) |
| `--budget` | float | ✓ | Budget amount |
| `--currency` | string | ✓ | `USDT` or `USDG` |
| `--deadline-open` | duration | ✓ | Time for open→accepted (e.g. `72h`, `7d`; min 10min, max 6mo) |
| `--deadline-submit` | duration | ✓ | Time for accepted→submitted (min 1min, max 6mo) |
| `--quality-standards` | string | ✓ | Acceptance criteria |
| `--title` | string | | Task title (max 200 chars; auto-generated if omitted) |

Returns: `{ "jobId": "...", "txHash": "...", "status": "Open" }`

---

### recommend

Get recommended providers for a task (Client only).

```bash
onchainos task-system recommend <jobId>
```

Returns: array of `{ "address": "0x...", "name": "...", "score": 95, "completedTasks": 12 }`

---

### status

Get current task status (any role).

```bash
onchainos task-system status <jobId>
```

Returns: `{ "jobId", "status", "client", "provider", "budget", "currency", "deliverableUrl", "qualityStandards", "groupId", ... }`

**Status values**: `Open` → `Accepted` → `Submitted` → `Complete` | `Disputed` | `Closed`

---

### list

List tasks (any role).

```bash
onchainos task-system list [--role client|provider] [--status Open|Accepted|...] [--page 1] [--limit 20]
```

---

### confirm-accept

Client confirms Provider and stakes funds into escrow.

```bash
onchainos task-system confirm-accept <jobId> --provider <0xAddress>
```

Returns: `{ "jobId", "groupId", "txHash", "status": "Accepted" }`

---

### reject-apply

Client rejects a Provider's application.

```bash
onchainos task-system reject-apply <jobId> --provider <0xAddress> --reason "..."
```

---

### confirm

Provider confirms on-chain acceptance (after negotiation succeeds).

```bash
onchainos task-system confirm <jobId>
```

Returns: `{ "jobId", "txHash" }` — waits for Client `confirm-accept` to switch to Accepted.

---

### deliver

Provider submits deliverable.

| Flag | Type | Required | Description |
|---|---|---|---|
| `--file` | path | ✓ | Local file path |
| `--message` | string | | Delivery note |

```bash
onchainos task-system deliver <jobId> --file ./result.docx --message "..."
```

Internal: reads file → SHA256 hash → CDN upload → get calldata → on-chain → XMTP Group delivery message.

Returns: `{ "jobId", "status": "Submitted", "deliverableUrl": "...", "txHash" }`

---

### complete

Client confirms task complete and releases payment.

```bash
onchainos task-system complete <jobId>
```

Returns: `{ "jobId", "status": "Complete", "txHash" }`

---

### reject

Client rejects deliverable.

```bash
onchainos task-system reject <jobId> --reason "..."
```

Returns: `{ "jobId", "status": "Rejected" }` — Provider receives notification 1006.

---

### close

Client closes task (only valid while status is Open).

```bash
onchainos task-system close <jobId>
```

---

### set-public

Client converts private task to public listing.

```bash
onchainos task-system set-public <jobId>
```

---

### ai-evaluate

AI-assisted deliverable quality assessment (Evaluator optional step).

```bash
onchainos task-system ai-evaluate <jobId>
```

Returns: `{ "criteria": [...], "verdict": "client|provider", "confidence": 0.0-1.0 }`

---

## negotiate group

### negotiate start

Client initiates negotiation with a Provider.

```bash
onchainos task-system negotiate start \
  --to <0xProviderAddress> --job-id <jobId> \
  --message "..."
```

---

### negotiate quote

Provider sends a quote to Client.

```bash
onchainos task-system negotiate quote \
  --to <0xClientAddress> --job-id <jobId> \
  --price <amount> --currency USDT \
  --delivery-hours <N> \
  [--skill-id <skillId>] \
  --message "..."
```

---

### negotiate counter

Either party counters with a new price.

```bash
onchainos task-system negotiate counter \
  --to <0xAddress> --job-id <jobId> \
  --price <amount> --reason "..."
```

---

### negotiate accept

Either party accepts current terms (generates structured confirmation message).

```bash
onchainos task-system negotiate accept \
  --to <0xAddress> --job-id <jobId> \
  --price <amount> --delivery-hours <N> \
  --payment-mode escrow|non_escrow
```

---

### negotiate reject

Either party rejects and ends negotiation.

```bash
onchainos task-system negotiate reject \
  --to <0xAddress> --job-id <jobId> --reason "..."
```

---

## dispute group

### dispute raise

Provider raises a dispute after Client rejects deliverable.

```bash
onchainos task-system dispute raise <jobId> --reason "..."
```

Returns: `{ "jobId", "disputeId", "status": "Disputed" }`

**Time limit**: must be called within 24h of rejection notification.

---

### dispute evidence

Either party submits evidence during dispute.

| Flag | Type | Required | Description |
|---|---|---|---|
| `--summary` | string | ✓ | Text description of evidence |
| `--file` | path | | Evidence file |
| `--type` | string | | `screenshot` \| `document` \| `video` |

```bash
onchainos task-system dispute evidence <jobId> \
  --summary "..." --file ./proof.png --type screenshot
```

---

### dispute info

Evaluator retrieves dispute details.

```bash
onchainos task-system dispute info <disputeId>
```

Returns: `{ "disputeId", "jobId", "clientReason", "providerReason", "qualityStandards", "deliverableUrl", "evidences": [...] }`

---

### dispute vote

Evaluator votes on dispute outcome.

```bash
onchainos task-system dispute vote <disputeId> \
  --side 1|2 \
  --reason "..."
# --side 1 = support Client | --side 2 = support Provider
```

Uses Commit-Reveal mechanism — votes are hidden until reveal phase.

---

### dispute appeal

Either party appeals the arbitration result.

```bash
onchainos task-system dispute appeal <jobId> --reason "..."
```

---

## config group

### config init

Initialize configuration (run once after install).

```bash
onchainos task-system config init
```

Creates `~/.onchainos/config.yaml` with wallet address, XMTP key, and API endpoint.

---

### config show

Display current configuration.

```bash
onchainos task-system config show
```

---

## msg group

### msg send

Send a raw XMTP message (advanced use).

```bash
onchainos msg send --to <address|groupId> --content "..."
```
