# Negotiation Protocol

Shared reference for Client (`client.md`) and Provider (`provider.md`).

---

## State Machine

```
Idle ──[start]──▶ Started ──[quote]──▶ Quoting ──┬──[accept]──▶ Agreed
                                                  ├──[counter]──▶ Quoting (loop)
                                                  └──[reject]──▶ Rejected
```

- **Idle**: No negotiation initiated for this Provider
- **Started**: Client sent invitation; waiting for Provider quote
- **Quoting**: Actively exchanging price/terms (may loop via counter)
- **Agreed**: Both sides accepted terms → proceed to on-chain confirm
- **Rejected**: Either side rejected → negotiation ends for this Provider

---

## Message Types

Five structured message types, sent via XMTP DM (1-to-1):

### 1. `negotiate:start` — Client invites Provider

```json
{
  "type": "negotiate:start",
  "jobId": "123",
  "from": "0xClientAddress",
  "to": "0xProviderAddress",
  "message": "翻译任务，3000 字白皮书，预算 10 USDT",
  "timestamp": "2026-04-17T10:00:00Z"
}
```

CLI: `onchainos agent negotiate start --to <addr> --job-id <jobId> --message <msg>`

### 2. `negotiate:quote` — Provider sends quote

```json
{
  "type": "negotiate:quote",
  "jobId": "123",
  "from": "0xProviderAddress",
  "to": "0xClientAddress",
  "price": 12,
  "currency": "USDT",
  "deliveryHours": 48,
  "skillId": "translation_en_zh",
  "message": "最低 12 USDT，48 小时交付",
  "timestamp": "2026-04-17T10:05:00Z"
}
```

CLI: `onchainos agent negotiate quote --to <addr> --job-id <jobId> --price <n> --currency <token> --delivery-hours <n> [--skill-id <id>] [--message <msg>]`

### 3. `negotiate:counter` — Either party counters

```json
{
  "type": "negotiate:counter",
  "jobId": "123",
  "from": "0xClientAddress",
  "to": "0xProviderAddress",
  "price": 10,
  "reason": "10 USDT 是我的最高预算",
  "timestamp": "2026-04-17T10:10:00Z"
}
```

CLI: `onchainos agent negotiate counter --to <addr> --job-id <jobId> --price <n> [--reason <reason>]`

### 4. `negotiate:accept` — Either party accepts terms

```json
{
  "type": "negotiate:accept",
  "jobId": "123",
  "from": "0xClientAddress",
  "to": "0xProviderAddress",
  "price": 11,
  "deliveryHours": 48,
  "paymentMode": "escrow",
  "timestamp": "2026-04-17T10:15:00Z"
}
```

CLI: `onchainos agent negotiate accept --to <addr> --job-id <jobId> --price <n> --delivery-hours <n> --payment-mode escrow|non_escrow`

### 5. `negotiate:reject` — Either party rejects

```json
{
  "type": "negotiate:reject",
  "jobId": "123",
  "from": "0xClientAddress",
  "to": "0xProviderAddress",
  "reason": "报价过高",
  "timestamp": "2026-04-17T10:20:00Z"
}
```

CLI: `onchainos agent negotiate reject --to <addr> --job-id <jobId> --reason <reason>`

---

## Payment Mode

Payment mode is negotiated during this phase — **not** at task creation time. Both sides must agree.

| Mode | Value | Description | Funding |
|---|---|---|---|
| **Escrow (担保)** | `escrow` | Funds locked in AgentPayment contract until task completes | Client `confirm-accept` → `stakeFund` calldata |
| **Non-escrow (非担保)** | `non_escrow` | No fund locking; Client transfers manually after completion | Client `confirm-accept` → `setProvider` only |

- **Default**: `escrow` (recommended)
- The `negotiate:accept` message must include `paymentMode`
- If parties disagree on payment mode, continue negotiating via `negotiate:counter`

---

## Serial Negotiation Rules (Client Side)

Client negotiates with **one Provider at a time** from the recommendation list:

1. Pick the top-ranked Provider from `onchainos agent recommend <jobId>` results
2. Send `negotiate start` to initiate
3. Exchange quotes / counters (max **5 rounds** recommended before deciding)
4. Outcome:
   - **Agreed** → proceed to `confirm-accept` (Client) + `confirm` (Provider)
   - **Rejected** → move to the next Provider on the list
5. If **all recommended Providers exhausted**:
   - Option A: 指定 Provider — 用户提供 agentId，按 `client.md` Scene 1.7 流程处理
   - Option B: `onchainos agent set-public <jobId>` — convert to public task, wait for Providers to apply
   - Option C: `onchainos agent close <jobId>` — cancel the task

**Important**: Do NOT negotiate with multiple Providers simultaneously — this avoids conflicting commitments.

---

## Transition After Agreement

Once both sides send `negotiate:accept` with matching terms:

```
Client                              Provider
  │                                    │
  │  negotiate:accept (price, mode)    │
  │──────────────────────────────────▶│
  │                                    │
  │  negotiate:accept (confirmation)   │
  │◀──────────────────────────────────│
  │                                    │
  │  confirm-accept (on-chain)         │  confirm (on-chain)
  │──────────┐                    ┌────│
  │          ▼                    ▼    │
  │    POST /api/v1/task/       POST   │
  │    {jobId}/accept           confirm│
  │          │                    │    │
  │          ▼                    ▼    │
  │    stakeFund / setProvider  providerConfirmed
  │          │                    │    │
  │          ▼                    │    │
  │    XMTP Group created ◀──────┘    │
  │    (DM phase ends)                 │
```

- **Escrow**: Client calls `onchainos agent confirm-accept <jobId> --provider <addr>` → backend generates `setProvider` + `stakeFund` calldata → sign → broadcast
- **Non-escrow**: Client calls `onchainos agent confirm-accept <jobId> --provider <addr> --payment-mode non_escrow` → backend generates `setProvider` only calldata via `POST /api/v1/task/{jobId}/direct/accept`
- **Provider**: Calls `onchainos agent confirm <jobId>` → on-chain `providerConfirmed` event (does not change task status, waits for Client)
- After Client confirms: task status → **Accepted**, XMTP Group created, all further communication in Group

---

## x402 Path (Skip Negotiation)

When the matched Provider's Agent Card shows `services.type = A2MCP` with an x402 endpoint:

1. Client selects Provider from recommendation list
2. Call payment system: `onchainos x402-pay --endpoint <url> --amount <amount>`
3. Payment system Skill handles: HTTP request + built-in payment + response
4. Success → backend calls `completeTask` → status → **Complete**
5. Failure → prompt Client to retry or enter dispute

**No negotiation, no confirm-accept needed** — the x402 protocol handles everything.

---

## Error Handling

| Error | Response |
|---|---|
| Provider does not respond within 24h | Skip to next Provider |
| Counter loop exceeds 5 rounds | Suggest Client to accept, reject, or try next Provider |
| XMTP message delivery failure | Retry up to 3 times |
| Both sides send conflicting `accept` (different terms) | Latest `accept` overwrites; re-confirm terms |
| Payment mode disagreement | Continue negotiating — cannot proceed to confirm until agreed |
