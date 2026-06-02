# Third-Party Plugin Pre-flight

> Load before dispatching ANY third-party DeFi plugin command that performs an on-chain write.

When the user invokes a **third-party DeFi plugin** (e.g. `aave-v3-plugin`, `uniswap-plugin`) that internally calls `onchainos wallet contract-call --force`, the plugin is a **black box** — its error messages may not surface Gas Station issues. The agent MUST proactively pre-flight Gas Station status on the target chain.

## Pre-flight checklist

Before dispatching ANY third-party plugin command that performs an on-chain write (`--confirm` / `execute` / `--broadcast` / etc.), the agent MUST:

1. Resolve `<chain>` and `<from>` from the plugin invocation.
2. Run:
   ```bash
   onchainos wallet gas-station status --chain <chain> [--from <addr>]
   ```
3. Branch on `data.recommendation`:

| Recommendation | Action |
|---|---|
| `READY` | Proceed directly to plugin invocation. |
| `ENABLE_GAS_STATION` | Render `references/gas-station.md` Scene A using `data.tokenList`. After user confirms a token pick, run `wallet gas-station setup --chain <C> --gas-token-address <picked> --relayer-id <picked>`. Then proceed to the original plugin command. |
| `REENABLE_GAS_STATION` | Render Scene B'. After user confirms, `wallet gas-station setup ...`. Then proceed. |
| `PENDING_UPGRADE` | Render Scene A'. After user confirms, `wallet gas-station setup ...` (carries 7702 material). Then proceed. |
| `INSUFFICIENT_ALL` | Tell user to top up native or stablecoin. Do NOT invoke plugin. |
| `HAS_PENDING_TX` | Tell user to wait for the pending tx (or run `wallet gas-station disable --chain <C>` to bypass). Do NOT invoke plugin. |

## Pre-flight skip conditions

- Plugin invocation is dry-run / simulation (no on-chain write)
- Plugin is a read-only command (e.g. `aave-v3-plugin positions`, `health-factor`, `reserves`, `quickstart`)
- The agent has already pre-flighted this `(chain, from)` tuple in the current conversation and confirmed `gasStationActivated = true`

## Reactive diagnosis (post-failure fallback)

If a third-party plugin returned a vague error (e.g. `"Pool.supply() failed"`, `"swap failed"`) and the message does NOT clearly explain the cause, follow the canonical recovery flow in `references/gas-station.md` → "Plugin Bail Recovery".

In short, in priority order:

1. **Fast path** — parse the plugin's bubbled-up stderr/stdout for an onchainos response with `"errorCode": "GAS_STATION_SETUP_REQUIRED"` (exit code 3). Extract `data.tokenList` directly and proceed to Scene A → `wallet gas-station setup` → re-invoke plugin. No extra CLI call.
2. **Slow path** — if the plugin ate stdout, run `onchainos wallet gas-station status --chain <chain> [--from <addr>]` and branch on `recommendation` per the Pre-flight checklist above.
3. Otherwise — surface the plugin's raw error to the user.

## Exit codes from `wallet contract-call --force` / `wallet send --force`

| Exit | Meaning | Agent action |
|---|---|---|
| `0` | Success | Continue |
| `1` | Real error (logic / chain / etc.) | Surface error to user |
| `2` | Confirming required (non-`--force` path; should NOT happen with `--force`) | Treat as bug; show message |
| `3` | `errorCode: GAS_STATION_SETUP_REQUIRED` — `--force` cannot silently auto-enable GS | Render Scene A from `data.tokenList`, run `wallet gas-station setup`, re-invoke same command |
