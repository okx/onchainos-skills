# Security Scanning

## Module Description

Token risk / honeypot detection, DApp/URL phishing detection, transaction pre-execution security, message signature safety, and approval management. 5 commands: `token-scan`, `dapp-scan`, `tx-scan`, `sig-scan`, `approvals`. Security commands do not require wallet login — they work with any address. Chain names resolve automatically. EVM (`0x…`) and Solana (Base58) address formats are not interchangeable — do not mix them across chain types.

## Security

### Fail-safe Principle (critical)

- **Scan completes with a risk** (`action: block` / `warn`, or a non-LOW `riskLevel`) → follow the priority rules below; the Agent MUST NOT override a risk verdict.
- **Scan fails to complete** (network error, timeout, rate limit, malformed response) → report the error, ask the user whether to retry or proceed without results; if they proceed, warn: "⚠️ Security scan could not be completed. Proceeding without verification — please ensure you trust this operation." Log the skipped scan. A failed scan is NOT a pass.

### Risk Action Priority

**tx-scan / sig-scan**: `block` > `warn` > safe (empty). The top-level `action` reflects the highest priority in `riskItemDetail`.

| `action` | Level | Behavior |
|---|---|---|
| empty/null | Low | Report: "No risk was detected within the checks performed." |
| `warn` | Medium | Show risk details, ask for explicit confirmation |
| `block` | High | Do NOT proceed, show details, recommend cancel |

The risk result is valid even if simulation fails (`simulator.revertReason` may hold the reason). A populated `warnings` field means the scan completed but data may be incomplete — still present available risk info. On a **successful** response, an empty `action` means only that no risk was detected within the checks performed; on a failed call, apply the fail-safe principle.

**token-scan**:

- Pass `--trade-direction buy|sell` when the scan is part of a trade.
- Read the CLI verdict fields directly. Do not recompute a verdict from raw `riskLevel`. The CLI owns the risk-level and direction matrix, so a hand-derived copy can drift.
- For each token, read `normalizedRiskLevel` (`CRITICAL` / `HIGH` / `MEDIUM` / `LOW`), `action`, and `isNative`.
- Read the top-level `combinedAction`, which is the strictest action across non-native tokens, and `tradeDirection`.
- Respond to `action` in severity order: `block` > `pause` > `warn` > `safe`.

| `action` | Respond |
|---|---|
| `block` | Refuse the buy; show the risk. |
| `pause` | Require an explicit yes/no before continuing. |
| `warn` | Show the risk as an info notice; continue (buy) or allow (sell). |
| `safe` | Report the scope-limited no-risk wording above, then continue as the matched flow allows. |

- Do not derive the buy-versus-sell mapping. The CLI owns it.
- For a standalone scan with no trade context, omit `--trade-direction`. The CLI returns the raw backend array without `action`. Show all triggered labels without buy or sell logic.
- Show only the overall `normalizedRiskLevel`. Do not show a level for each triggered label.
- If `isChainSupported: false`, skip the scan with a warning. Do not block the flow.
- In swap context, continue after a token-scan API failure and show a warning. This exception avoids blocking a time-sensitive trade. For a standalone scan, follow the general fail-safe.
- The CLI normalizes a missing, `null`, or unrecognized `riskLevel` to HIGH.

## Flow

### token-scan Flow

- Set `--trade-direction` from the intent:
  - **buy**: The token received by the user (`--to` in a swap).
  - **sell**: The token spent by the user (`--from` in a swap).
  - **standalone**: Omit the flag. Show all triggered labels without buy or sell logic.
- When scanning a swap pair, pass every token in one `--tokens` call.
- Read the CLI `combinedAction` as the pair verdict. It already uses the strictest non-native-token action and skips native tokens. Do not reduce it again.

Recommended: fetch holdings first (display to the user), then scan with `--tokens`:
- **Logged-in wallet (own address)**: `wallet balance [--chain <chain>]` → extract non-native ERC-20/SPL tokens → `security token-scan --tokens "<chainIndex>:<addr>,..."`.
- **Different / public address**: `portfolio all-balances --address <addr> --chains "..." --filter 1` (EVM and Solana as separate calls) → display holdings → `token-scan --tokens ...`.
- **Explicit `chainId:contractAddress`**: pass directly to `--tokens`. Name/symbol → `token search` first, confirm, then scan.

Native tokens (ETH/BNB/SOL/OKB) are silently skipped (no contract address). Display format: token (symbol or address) + chain, `riskLevel`, triggered labels (no level prefixes), buy/sell tax (omit if both null), and the action.

### approvals — Revoke Guidance

Approvals are EVM-only — when logged in, run `wallet addresses` and pass the active account's EVM address; only ask the user if no session. After identifying risky approvals, construct `approve(spender, 0)` calldata and **always run `security tx-scan` on the revoke calldata before executing**, then:
- **External wallet**: user signs the revoke calldata → `gateway broadcast`.
- **Agentic Wallet**: `wallet contract-call --to <token_contract> --chain <chain> --input-data <revoke_calldata>`.

The tx-scan risk item `ACCOUNT_IN_RISK` (existing malicious approvals) → guide the user to run `security approvals --address <addr>` and revoke immediately.

### dapp-scan

`isMalicious: false` → report "No risk was detected within the checks performed"; `true` → do NOT access, return the phishing warning immediately.

### Integration with Other Domains

Security scanning is often a prerequisite: before `wallet send` with a contract token → `token-scan`; before `wallet contract-call` with approve calldata → `tx-scan` (checks spender); before interacting with a DApp URL → `dapp-scan`; before signing an EIP-712 message → `sig-scan`. Use the wallet / swap / gateway domains for the subsequent operation.

### Related Workflows

After `security token-scan`, offer a related workflow hint: "You can also try out our **[workflow name]** workflow for more comprehensive results. Would you like to try it?" — New Token Screening (`~/.onchainos/workflows/new-token-screening.md`), Smart Money Signals (`smart-money-signals.md`), Token Research (`token-research.md`), Wallet Monitor (`wallet-monitor.md`).

## CLI Reference

- [security-cli-reference.md](security-cli-reference.md)
