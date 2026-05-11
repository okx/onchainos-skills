# Strategy Execution Event Codes

> Codes returned in `strategy list` -> `executionHistoryList[].code`. Emitted by the TEE swap-trade engine while it attempts to execute an active limit order. **This is a separate stream from BE API error codes** (60018, 100012, ...) covered in `SKILL.md > Error code -> Agent action`.

A given order may emit multiple events over its lifetime — every retry the TEE engine attempts produces a new entry. Read the **latest** entry first; older entries are historical context.

When surfacing an event to the user, use the **message** column verbatim — it is the official client-facing string that matches the OKX wallet UI.

## Active series (3xxx — TEE swap-trade reminders)

| code | name | message | agent action |
|---|---|---|---|
| 0 | tradeSuccessed | Trade successful | report success; surface `txHash` and explorer link |
| 3005 | lessThanMinReceive | Quoted price is below the minimum amount to receive | live quote slipped below user min-receive; suggest widening `--slippage` or adjusting trigger |
| 3006 | preExecutionFailed | Pre-execution error. Try again | local simulation failed; engine retries automatically. If persistent, cancel and recreate |
| 3007 | signFailed | Failed to verify signature | TEE sign failed; transient. If recurring (>3) suggest `wallet status` |
| 3008 | broadcastFailed | Broadcast failed | RPC broadcast failed; engine retries |
| 3010 | onchainFailed | The transaction broadcast was unsuccessful due to an onchain service error | tx revert / chain service error; inspect `txHash` on explorer |
| 3013 | insufficientBalance | Insufficient funds in wallet | tell user to top up `from_token` or recreate with smaller amount |
| 3014 | insufficientLamports | Insufficient funds for network fee | tell user to fund the chain's native fee token (SOL on Solana, ETH on EVM) |
| 3015 | exceedSlippage | Price exceeded slippage at trade | suggest widening `--slippage` (default 15%) |
| **3016** | **noLiquidty** | **No quote due to low liquidity** | non-transient — suggest different pair, smaller amount, wider trigger, or different chain |
| 3017 | unableQuote | Unable to fetch a quote | aggregator routing/RPC issue; if recurring, treat similar to 3016 |
| 3018 | mevFail | Anti-MEV provider error | engine retries; if persistent, suggest disabling MEV protection |
| 3019 | riskToken | Failed to trade due to risky token | terminal — destination token is blocklisted, order won't execute |
| 3020 | blackAddress | Failed to trade due to blocklisted address | terminal — surface explicitly, the wallet address is flagged |
| 3023 | orderExpired | Limit order expired | recreate with longer `--expires-in` |

## Legacy series (2xxx — old order status)

May still appear on older orders. The active TEE engine emits 3xxx; treat 2xxx as lifecycle/state observations only.

| code | name | message |
|---|---|---|
| 2001 | oldCreated | Order created |
| 2002 | oldFailedToCreate | Failed to create order |
| 2003 | oldEdited | Order modified |
| 2004 | oldFailedToEdit | Failed to edit order |
| 2005 | oldCanceled | Order canceled |
| 2006 | oldFailedToCancel | Unable to cancel order |
| 2007 | oldAutoCanceled | Order auto-canceled |
| 2008 | oldFailedToAutoCancel | Unable to auto-cancel order |
| 2009 | oldExpired | Order expired |
| 2010 | oldExceedsSlippage | Price exceeded slippage at trade |
| 2011 | oldNoQuoteLowLiquidity | No quote due to low liquidity |
| 2012 | oldBroadcastFailed | Broadcast failed |
| 2013 | oldSuccessful | Trade successful |

## Reading patterns

**Recurring 3xxx without progressing to txHash**: the engine is in a soft retry loop. Read the latest code, surface its **official message**, and ask whether to wait, cancel, or adjust parameters. Don't let it grind silently — cancel after a few cycles if the cause is non-transient (3016, 3019, 3020, 3023).

**Single 0 with txHash**: success. Surface `txHash`, explorer link, and the realised `toAmount`.

**Mixed history (e.g. 3015 -> 3015 -> 0)**: order recovered after slippage adjusted on retry — final state wins.

**Terminal codes** (no point retrying): 3010 (after the fact), 3019, 3020, 3023.

**Soft / transient** (engine retries; user shouldn't intervene unless persistent): 3006, 3007, 3008, 3017, 3018.

**User-actionable** (must change inputs to succeed): 3005, 3013, 3014, 3015, 3016, 3023.

## Surfacing to the user

When the agent reports an event:

1. Prefix with the **message** verbatim (matches OKX UI wording).
2. Append an action hint pulled from the table above.
3. If multiple events, summarise the latest only and mention the count of repeats: "this is the 5th `No quote due to low liquidity` event on this order".

Example:

> Latest event on order 17266791540614656: **No quote due to low liquidity** (code 3016, repeated 5×). The aggregator can't route this pair at the current trigger — try a different pair or widen the trigger price.
