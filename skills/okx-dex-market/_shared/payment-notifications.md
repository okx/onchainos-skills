# Payment Notifications (Market API x402)

Some Market API endpoints may require x402 payment after the free quota is
exhausted. The CLI handles signing automatically once the user is logged in
and surfaces the following events in the response `notifications[]` array.

This document is the canonical source for the 5 event codes, their user-facing
copy, placeholder sources, and the agent handling procedure. It is consumed by
`okx-dex-market`, `okx-dex-token`, `okx-dex-signal`, and `okx-dex-trenches`.

---

## Response Shapes

Every CLI call may include a `notifications[]` field. Two response patterns:

**Non-blocking (informational)**:

```json
{
  "ok": true,
  "data": { /* ... */ },
  "notifications": [{ "code": "...", "data": {} }]
}
```

Print the filled copy once, then display `data` as usual.

**Blocking (first-time charging flip)**:

```json
{
  "confirming": true,
  "notifications": [{
    "code": "MARKET_API_*_OVER_QUOTA",
    "data": {
      "tier": "basic",
      "payment": [{
        "amount": "100",
        "asset": "0x4ae4...2dc8",
        "network": "eip155:196",
        "payTo": "0xfa00...92a1b",
        "extra": { "name": "USDG" },
        "maxTimeoutSeconds": 86400
      }]
    }
  }]
}
```

**Never auto-retry.** Show the filled copy, wait for explicit user confirmation,
then rerun the exact same command (CLI will auto-sign on the second call).

---

## Handling Procedure

Before formatting the CLI result:

1. **Check `notifications[]`**. If absent or empty, proceed normally.
2. **For each `notification.code`**:
   - Look up the copy in the code table below.
   - Fill placeholders using the resolution rules.
3. **If `confirming: true` is present on the envelope**:
   - Do NOT auto-retry.
   - Present the filled copy to the user.
   - Wait for explicit confirmation ("yes" / "proceed" / "确认").
   - On confirmation → rerun the exact same command verbatim.
   - On refusal → stop and acknowledge.
4. **Otherwise**:
   - Print the filled copy once.
   - Then display `data` normally.

Do not track your own "already shown" state. The CLI persists per-code
`*_shown` flags in `~/.onchainos/payment_cache.json`, so one-shot codes fire at
most once per account lifetime.

---

## 1. `MARKET_API_NEW_USER_INTRO`

**Trigger**: New user (UserType=1) first call, Basic=0 Premium=0. One-shot per account lifetime. Non-blocking.

```
Welcome to Market API. Your monthly free quota has been allocated:
- Basic endpoints: {basicFreeQuota}
- Premium endpoints: {premiumFreeQuota}

Once exceeded, per-call pricing applies (Basic {basicUnitPrice}/call, Premium {premiumUnitPrice}/call). After you log in, the CLI will sign automatically when charging kicks in — no manual steps required. We recommend keeping a USDT balance on X Layer ahead of time to avoid service interruption.

Full rules → [Pricing documentation]({docUrl})
```

**Placeholders**: `{basicFreeQuota}`, `{premiumFreeQuota}`, `{basicUnitPrice}`, `{premiumUnitPrice}`, `{docUrl}`

---

## 2. `MARKET_API_OLD_USER_GRACE`

**Trigger**: Old user (UserType=0) first call within the grace period. One-shot per account lifetime. Non-blocking.

```
Market API pricing is now in effect. As an existing user, you have a {graceDays}-day free grace period during which all calls remain free. The grace period ends on {graceExpiresAt}, after which regular billing begins. Once billing is active: Basic endpoints {basicFreeQuota} free / Premium endpoints {premiumFreeQuota} free, with overage priced at Basic {basicUnitPrice}/call and Premium {premiumUnitPrice}/call.

Full rules → [Pricing documentation]({docUrl})
```

**Placeholders**: `{graceDays}`, `{graceExpiresAt}`, `{basicFreeQuota}`, `{premiumFreeQuota}`, `{basicUnitPrice}`, `{premiumUnitPrice}`, `{docUrl}`

---

## 3. `MARKET_API_OLD_USER_POST_GRACE_INTRO`

**Trigger**: Old user's first call after grace ends (now ≥ graceExpiresAt, Basic=0 Premium=0). One-shot per account lifetime. Non-blocking.

```
Your {graceDays}-day free grace period has ended, and Market API has entered the regular billing phase. Your monthly free quota has been reallocated:
- Basic endpoints: {basicFreeQuota}
- Premium endpoints: {premiumFreeQuota}

Once exceeded, per-call pricing applies (Basic {basicUnitPrice}/call, Premium {premiumUnitPrice}/call). After you log in, the CLI will sign automatically when charging kicks in. We recommend keeping a USDT balance on X Layer to ensure uninterrupted service.

Full rules → [Pricing documentation]({docUrl})
```

**Placeholders**: `{graceDays}`, `{basicFreeQuota}`, `{premiumFreeQuota}`, `{basicUnitPrice}`, `{premiumUnitPrice}`, `{docUrl}`

---

## 4. `MARKET_API_NEW_USER_OVER_QUOTA`

**Trigger**: New user — a tier's charging flag flips 0→1. Per-tier; each flip fires once. **Blocking** (`confirming: true`).

```
Your {tier} free quota has been used up, and this request has been paused.

- Further calls will be billed per call ({tier} {unitPrice}/call); the CLI is ready to sign automatically
- Rerun the original command to continue — the system will pay and return the result automatically
- We recommend keeping enough USDT in your X Layer wallet to avoid transaction failures
```

**Placeholders**: `{tier}`, `{unitPrice}`

---

## 5. `MARKET_API_OLD_USER_POST_GRACE_OVER_QUOTA`

**Trigger**: Old user after grace — a tier's charging flag flips 0→1. Per-tier; each flip fires once. **Blocking** (`confirming: true`).

```
Your {tier} free quota for this month has been used up (the first overage after the grace period), and this request has been paused.

- Further calls will be billed per call ({tier} {unitPrice}/call); the CLI is ready to sign automatically
- Rerun the original command to continue — the system will pay and return the result automatically
- We recommend keeping enough USDT in your X Layer wallet to avoid transaction failures
```

**Placeholders**: `{tier}`, `{unitPrice}`

---

## Placeholder Resolution

### Static (skill-side config; update this file when pricing changes)

| Placeholder | Default | Description |
|---|---|---|
| `{basicFreeQuota}` | `1M/month` | Basic endpoint monthly free quota |
| `{premiumFreeQuota}` | `100K/month` | Premium endpoint monthly free quota |
| `{basicUnitPrice}` | `0.0001 $` | Basic overage unit price |
| `{premiumUnitPrice}` | `0.005 $` | Premium overage unit price |
| `{graceDays}` | `30` | Free grace period length (days) for existing users |
| `{docUrl}` | _TODO — PM to provide_ | Pricing documentation URL |

### Dynamic (read from event payload)

| Placeholder | Source | Used by | Notes |
|---|---|---|---|
| `{graceExpiresAt}` | `notifications[].data.graceExpiresAt` | #2 | Server gap — currently `data = {}` for `OLD_USER_GRACE`. Fall back to the string `2026.5.31` until the backend ships this field. |
| `{tier}` | `notifications[].data.tier` | #4, #5 | `basic` / `premium`; capitalize first letter on display (`Basic` / `Premium`) |
| `{unitPrice}` | Derived from `{tier}` | #4, #5 | `basic` → use `{basicUnitPrice}` value / `premium` → use `{premiumUnitPrice}` value |

---

## Deduplication

- **One-shot codes** (`NEW_USER_INTRO`, `OLD_USER_GRACE`, `OLD_USER_POST_GRACE_INTRO`) fire at most once per account lifetime. Running `onchainos wallet logout` clears the cache; next login re-fires them.
- **OVER_QUOTA codes** (`NEW_USER_OVER_QUOTA`, `OLD_USER_POST_GRACE_OVER_QUOTA`) re-fire on each `charging 0→1` flip per tier. If a tier's charging flag drops back to 0 (server-side quota reset), the shown flag resets too.

Trust the CLI's persisted flags — do not track your own seen/unseen state.
