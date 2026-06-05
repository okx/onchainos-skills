# Payment Mode Differences

> The state machine itself is payment-mode-agnostic (see [`state-machine.md`](./state-machine.md));
> this document lists **how the two payment modes differ in action at each state node**.

## Overview

| Mode | Symbol | Use case | Fund flow |
|---|---|---|---|
| **Escrow** | `escrow` / `1` | Default, recommended; new relationships where the two sides don't trust each other | Funds are locked into the escrow contract at buyer's `confirm-accept`; on `complete` the contract auto-releases to the provider |
| **x402 (on-demand micropayment)** | `x402` / `3` | Pay-per-call APIs / services | Provider registers an HTTP endpoint via `service-list`; buyer GETs the endpoint → receives a 402 challenge → signs `x402_pay` → replays the endpoint to synchronously receive the deliverable. **No paymentId** |

## State node × payment-mode matrix

### `confirm-accept` (created → accepted)

| Mode | Preconditions | Buyer CLI | On-chain side effects |
|---|---|---|---|
| escrow | Provider apply on chain (provider_applied) | `onchainos agent confirm-accept <jobId> --provider <p> --payment-mode escrow` | Funds escrowed into the contract; pre-accept two-sided signing flow |
| x402 | None (auto-matched) | `... --payment-mode x402` | Direct/accept single-signed + auto-triggered x402 payment flow (request → 402 → sign → replay) |

### `deliver`

| Mode | Trigger timing | Notes |
|---|---|---|
| escrow | accepted → submitted (after executing the task and submitting) | Standard flow: provider executes the task and delivers after accepted |
| x402 | **N/A — no deliver step** | x402 skips submit entirely; the buyer obtains the deliverable by replaying the provider's HTTP endpoint (402 → sign x402_pay → replay), then calls `/direct/complete` to transition directly from accepted → completed |

CLI command (**escrow only**):
```bash
onchainos agent deliver <jobId> --file "<url>" --message "<msg>"
```

### `complete`

| Mode | Trigger timing | Buyer CLI | Fund action |
|---|---|---|---|
| escrow | submitted → completed (after accepting the deliverable) | `onchainos agent complete <jobId>` | Contract pre-complete two-sided signing → auto-release escrowed funds to provider |
| x402 | accepted → completed (skips submitted) | `onchainos agent complete <jobId>` (internally calls `/direct/complete`) | Funds were already paid at the accept stage; complete only changes status |

### `reject` (submitted → rejected, escrow only)

⚠️ **Only escrow supports rejection**. For x402, funds were already paid at the accept stage.

Escrow buyer rejects: `onchainos agent reject <jobId> --reason "..."`

### `dispute raise` + evidence + adjudication

The dispute flow is payment-mode-agnostic:
- raise: `onchainos agent dispute raise <jobId> --reason "..."`
- Off-chain evidence upload: **auto-triggered** by the buyer / provider sub session on the `job_disputed` event (chat history + saved deliverables under `~/.onchainos/deliverables/<role>/<jobId>/`); manual upload not supported.
- Evaluator voting → `job_completed` (provider wins) or `job_refunded` (buyer wins)

**Fund settlement**: applies post-verdict per each payment mode's rules (escrow contract executes automatically; x402 already-paid, no fund movement).

## Security comparison

| Dimension | escrow | x402 |
|---|---|---|
| Buyer default risk (receive and don't pay) | ❌ None (contract automatic) | ❌ None (already paid) |
| Provider default risk | Protected by reject / dispute | Protected by reject (but x402 funds are already paid) |
| On-chain transaction count | Many (pre + main + broadcast) | Minimal |
| Gas cost | High | Low |

**Default recommendation: escrow**. x402 requires explicit business-scenario support.
