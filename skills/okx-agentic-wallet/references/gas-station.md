# Gas Station (Solana)

## Module Description

Gas Station lets users pay Solana gas with USDT, USDC, or USDG when they do not have enough SOL. The Relayer pays the network fee. The transaction collects the stablecoin fee through an SPL token transfer. It does not need an account upgrade, per-chain setup, or 7702.

**Supported scenarios**: SPL token transfers and contract interactions, including swaps, DeFi actions, bridge initiation, and other SPL-program interactions. Native SOL transfers do not use Gas Station. The enable setting and default token apply only to one `(account, Solana)` pair.

Edge cases, FAQ answers, and failure diagnosis live in [gas-station-troubleshooting.md](gas-station-troubleshooting.md).

## Security

### Critical Rules

**Let the backend choose the path.**

1. Run `wallet send` or `wallet contract-call` normally. Do not check the SOL balance or decide whether to use Gas Station.
2. Read the CLI `scene` and `gasStationUsed` fields.
3. Use the matching row in Outcome → render map. Do not derive a scene from raw backend fields.
4. When the CLI returns Confirming, show the matching fixed copy and ask the user to choose a token.

- Do not pass `--gas-token-address`, `--relayer-id`, or `--enable-gas-station` on the first call. Use them only after the user chooses a token from a Confirming response.
- Use the exact token address and relayer ID from the Confirming response's `next` field. Do not create either value.
- Do not suggest Gas Station when the user is browsing or asking an unrelated question.
- Do not call Gas Station free. Show `serviceCharge` and `serviceChargeSymbol` when the CLI provides them.
- Do not combine Gas Station with Jito Bundler. Stop and follow Troubleshooting Edge Case 2.

## Flow

Gas Station is not a separate transaction command. The backend decides whether to use it for each `wallet send` or `wallet contract-call` request.

- First-time setup and token changes return Confirming (exit code 2; Scene A or C).
- A sufficient default token uses the automatic path (Scene B or D).
- Order token choices by balance. Break ties as USDT, then USDC, then USDG.

Show the Gas Station path when SOL is insufficient and a supported stablecoin is sufficient. Do this for a new request and for a follow-up about a failed transfer. If the default token is insufficient but another supported stablecoin is sufficient, offer Scene C before suggesting a smaller transfer or a top-up.

### Outcome → render map

Map the CLI `scene` directly to the fixed copy below. Do not derive another scene or write replacement copy. For a Confirming result, `next` contains the exact command to run again.

| CLI `scene` | Render | Then |
|---|---|---|
| `gs_first_time` | Scene A | Get consent + token pick → follow `next` (re-run with `--enable-gas-station --gas-token-address <pick> --relayer-id <pick>`). On success → Scene A two-step. |
| `gs_reenable` | Scene A (re-enable variant) | Same as `gs_first_time`; backend overwrites the previous default. |
| `gs_token_switch` | Scene C | Get choice 1 / 2 / 3 → follow `next`. On success → Scene C post-success echo. |
| `gs_insufficient_all` | Scene E | Stop. Do not run the request again. |
| `gs_pending_tx` | Edge Case 4 (troubleshooting) | Stop. Do not retry automatically. |
| success + `gasStationUsed=true` (no `scene`) | Universal Success Reply | — |

The agent detects Jito conflicts, transaction-cap limits, asynchronous hashes, and native-SOL transfers. See the Troubleshooting Edge Cases.

### Scene A — Not enabled, SOL insufficient, has sufficient stablecoin

Backend returns `gasStationFirstTimePrompt=true` + `gasStationTokenList` (≥1 `sufficient=true`; if none → Scene E).

```
Your SOL balance is not enough to pay Gas. Two ways to proceed:

1. Top up SOL and pay with the native token.
2. Enable "Gas Station" and pay Gas directly with a stablecoin.

About Gas Station: Gas Station aggregates third-party services, automatically compares rates and picks the cheapest one to cover Gas on your behalf. It accepts USDT, USDC, or USDG — you do not need to hold Solana. Learn more: https://web3.okx.com/learn/wallet-gas-station

- Once enabled, whenever the native token is insufficient, the system will automatically pay Gas with the stablecoin — no manual confirmation needed.
- By default the token with the highest balance is used; you can also pin a specific token as the default for every transaction. Tokens supported on this chain: USDT, USDC, USDG.

Confirm enabling Gas Station and paying this transaction's Gas with stablecoin?
```

Keep this template complete. Do not remove the academy link, either bullet, or the supported-token line. Do not replace it with a bare yes/no question.

**Consent**:

1. After a token choice, follow `next`. It reruns the command with that token's `--gas-token-address` and `--relayer-id`.
2. If the user confirms without choosing a token, use the sufficient token with the highest balance.
3. If the user declines, do not rerun the command. Tell them to top up SOL at `{fromAddr}`.
4. If the choice is ambiguous or not in the list, ask once more. Do not guess.

**Post-success (MANDATORY two-step, on every Scene A trigger):**

Step 1 — Set the used token as the default. Take `{chosen_token_address}` from `serviceChargeFeeTokenAddress`. If that field is absent, use the `--gas-token-address` from the second call. Run this command once without showing it to the user. If it fails, continue with the result of the completed transaction:
```
onchainos wallet gas-station update-default-token --chain solana --gas-token-address {chosen_token_address}
```
Step 2 — echo verbatim (keep every sentence):
```
Gas Station enabled. This transaction will pay Gas with {chosen_token}, and {chosen_token} is now set as the default Gas token. Whenever the native token is insufficient from now on, {chosen_token} will be used automatically — no further confirmation needed. You can change the default Gas token at any time.
```
`{chosen_token}` = the stablecoin symbol actually used. Then append the Universal Success Reply.

### Scene B / D — Silent auto-path

The backend returns `gasStationUsed=true`, `autoSelectedToken=true`, and a non-empty `hash`. The CLI signs and broadcasts without another prompt.

- In Scene B, the configured default token is sufficient.
- In Scene D, no default is set and exactly one stablecoin is sufficient. The backend selects that token.
- Show only the Universal Success Reply.
- For Scene D, add one line that names the automatically selected token.

### Scene C — Enabled, default token insufficient

The backend returns `gasStationUsed=true`, an empty `hash`, `gasStationFirstTimePrompt=false`, and `insufficientAll=false`. The default token has `sufficient=false`, and at least one alternative has `sufficient=true`. The CLI returns Confirming.

```
The default Gas token {prev_token} has insufficient balance (need ${serviceChargeUsd}, current balance ${prev_balance_usd}).
The following stablecoins in your account have enough balance to pay this transaction's Gas:

- {alt_token_1} (balance ${alt_balance_1_usd})
- {alt_token_2} (balance ${alt_balance_2_usd})

How would you like to proceed?
1. Change the default Gas token to {alt_token_X} and use it for this transaction (this chain will default to {alt_token_X} going forward).
2. Use {alt_token_X} for this transaction only; keep the default as {prev_token}.
3. Top up {prev_token} and continue using it.
```

- Fill `{prev_token}` from `defaultGasTokenAddress`.
- Take `{serviceChargeUsd}` and `{prev_balance_usd}` from the response.
- Fill `{alt_token_N}` with each sufficient non-default token in token-priority order.
- If there is one alternative, set `{alt_token_X}` to its symbol. Otherwise use `<your-pick>`.
- Always show all three choices and the top-up fallback. Do not add leading or trailing text.

**Response parsing:**

| User choice | CLI action |
|---|---|
| **1** — alternative token and new default | Rerun the original command with `--gas-token-address <alt_addr> --relayer-id <alt_relayer_id>`. After it broadcasts, run `wallet gas-station update-default-token --chain solana --gas-token-address <alt_addr>` without showing it to the user. Use the “replace default” echo. |
| **2** — alternative token for this transaction | Rerun with `--gas-token-address <alt_addr> --relayer-id <alt_relayer_id>`. Do not pass `--enable-gas-station` or run `update-default-token`. Use the “keep default” echo. |
| **3** — top up the default token or cancel | Do not rerun the command. Tell the user to top up `{fromAddr}` and retry. |
| Ambiguous, unavailable token, or no default-token choice | Ask once whether to change the default token. A yes selects 1. A no selects 2. Do not guess. |

Post-success echo (then append Universal Success Reply):
- keep default: `Done — this transaction will pay Gas with {chosen_token}. The default token remains {prev_token}, unchanged.`
- replace default: `Done — this transaction will pay Gas with {chosen_token}, and the default Gas token is now {chosen_token}.`

### Scene E — All stablecoins insufficient

The backend returns `insufficientAll=true`, all entries have `sufficient=false`, and the result contains `fromAddr`. The CLI stops.

```
You don't have enough balance to pay Gas. Please top up first:
  Top-up address: {fromAddr}
  Accepted tokens: SOL, USDT, USDC
```

Stop the flow. Do not offer Gas Station.

## Output Rules

### Output discipline (applies to every template here and in troubleshooting)

Treat each template as product copy.

- Substitute only its bracketed slots.
- Translate at output time when the user does not use English.
- Keep the template's structure and every fact.
- Keep the chain name “Solana” unchanged.
- Do not add an introduction or notes after the template.

**Output vocabulary**:

- Call the feature “Gas Station”.
- Call the choice “which stablecoin to pay gas”.
- Do not show internal field names, numeric error codes, or debug and log paths. This includes `gasStationFirstTimePrompt`, `gasStationUsed`, `autoSelectedToken`, `hasPendingTx`, `insufficientAll`, `signType`, `multiSignerTx`, `Phase 1/2`, and `DB flag`.

### Universal Gas Station Success Reply (all commands)

When Gas Station pays a transaction fee, include all four items below. This applies to `wallet send`, `contract-call`, `swap`, `bridge`, and DeFi plugins. Detect it from `gasStationUsed=true` or from non-empty `serviceCharge` and `serviceChargeSymbol`.

1. **Acknowledgment** — state plainly this tx's gas was paid via Gas Station with a stablecoin, not SOL. Never imply "free".
2. **Service charge** — show raw amount + symbol (`{serviceCharge} {serviceChargeSymbol}`) and USD equivalent. E.g. `Network fee: 0.8 USDC (≈ $0.80, paid via Gas Station)`.
3. **orderId** — copy verbatim; never omit or truncate.
4. **Follow-up prompt** — `You can tell me: **check order {orderId}** to check the status.` Translate but keep the literal `check order {orderId}` idiom.

**`networkFeeLabel` rule**:

- Use the CLI `networkFeeLabel` as the one fee-row label.
- The value is `Network fee` or `Network fee and Rent fee`. Translate the label.
- Do not render it as a separate row or add a `Gas fee` label.
- If the field is absent, use `Network fee`.

**When `txHash` is empty**:

- The Relayer usually returns the on-chain hash asynchronously.
- Say that the transaction was submitted and ask the user to check again shortly.
- Do not create a `txHash`.
- Do not show raw CLI commands to the user.

Example:
```
Sent 100 USDC to CYXWm...

- Network fee: 0.8 USDC (≈ $0.80, paid via Gas Station)
- orderId: ord_ghi789rst
- txHash: submitted, on-chain hash will be returned asynchronously by the Relayer — check again shortly.

You can tell me: **check order ord_ghi789rst** to check the status.
```

When the user asks to check an order in any language, run `wallet history --chain solana --order-id <orderId>` without showing the command. Then follow Troubleshooting Edge Case 5.

## Intent Routing

### Management Commands

Syntax below; all `--chain solana`. User-facing reply wording follows.

```bash
onchainos wallet gas-station update-default-token --chain solana --gas-token-address <spl_mint>
onchainos wallet gas-station enable  --chain solana
onchainos wallet gas-station disable --chain solana
onchainos wallet gas-station status  --chain solana [--from <solana_address>]
onchainos wallet gas-station setup   --chain solana --gas-token-address <spl_mint> --relayer-id <id> [--from <solana_address>]
```

- `status` is a read-only readiness check. It does not broadcast and is safe to repeat. Read `recommendation`: `READY` means proceed; `ENABLE_GAS_STATION` means render Scene A; `INSUFFICIENT_ALL` means render Scene E; `HAS_PENDING_TX` means ask the user to wait. It also returns `gasStationEnabled`, `gasStationDefaultToken`, and `tokenList[]` (`symbol`, `feeTokenAddress`, `relayerId`, `balance`, `serviceCharge`, `sufficient`). Third-party plugin pre-flight uses this command ([plugin-preflight.md](wallet-plugin-preflight.md)).
- `setup` activates Gas Station for the first time. It is idempotent: repeating it with the same default returns `alreadyActivated=true`. It works only when the readiness check allows first-time activation.
- `enable`, `disable`, and `update-default-token` update backend settings. They do not make an on-chain transaction.

**User-facing reply templates** (translate at output time, semantics must not drift):

Before `disable` (confirmation prompt):
> "Once disabled, transactions on Solana will pay Gas with SOL again. You can re-enable any time. If you only want to switch the Gas-payment token, use 'change default Gas token' instead of disabling. Confirm disabling?"

After any management command succeeds (`enable` / `disable` / `update-default-token`): render `data.message` verbatim (the CLI fills the copy).

### User Intent Recognition

| User intent | Action |
|---|---|
| Wants to send but lacks SOL | Run `wallet send`. Gas Station can activate automatically. |
| Asks whether stablecoins can pay gas | Give the matching Troubleshooting FAQ answer. Then continue if the user supplied a transaction. |
| Gas Station FAQ | Give the matching Troubleshooting FAQ answer. |
| Change the default Gas token | `wallet gas-station update-default-token --chain solana --gas-token-address <addr>`. |
| Enable Gas Station | `wallet gas-station enable --chain solana` (use confirmation + success templates). |
| Disable Gas Station / stop paying with stablecoin | `wallet gas-station disable --chain solana`. If they only want to switch token, suggest `update-default-token`. |
| Jito Bundle and stablecoin gas | Stop. Follow Troubleshooting Edge Case 2. |
| Asks which transaction types are supported | Give the matching Troubleshooting FAQ answer. |
| Transaction type is not supported | Follow Troubleshooting Edge Case 8. Do not rerun through Gas Station. |
| Asks why Gas Station did not activate | Check for a pending transaction, a transaction above 100,000 U, Jito Bundler, a native-SOL transfer, or an unsupported type. Use the matching template. |
| Asks for a not-yet-returned tx hash / why hash is slow | Troubleshooting Edge Case 3. |

## Troubleshooting

### Plugin Bail Recovery

Third-party plugins such as `kamino-plugin` and `raydium-plugin` run `wallet contract-call` as a subprocess. For a Scene A or Scene C Confirming result, the wrapper stops because the command exits with code 2. Identify this case only when exit code is **2** and stdout JSON contains `"confirming": true`.

Recovery:

1. Read `scene` from stdout JSON.
2. Use Outcome → render map. Show the matching copy and get consent when it is required.
3. Run any required management command, such as `update-default-token` after a Scene C “replace default” choice.
4. Rerun the same plugin command verbatim. The plugin rebuilds the calldata and uses the automatic path.
5. Do not retry `gs_insufficient_all` or `gs_pending_tx`.

- Parse Confirming JSON before deciding that recovery is available. Other failures can have the same non-zero exit.
- Get user consent for every Scene A or Scene C token choice.
- Rerun the same plugin command verbatim. This path stops before broadcast, so rerunning is idempotent. Do not rebuild the plugin's calldata.
