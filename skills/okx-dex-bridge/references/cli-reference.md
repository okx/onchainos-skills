# Onchain OS DEX Cross-Chain Swap -- CLI Command Reference

Detailed parameter tables, return field schemas, and usage examples for all 7 cross-chain commands.

## 1. onchainos cross-chain chains

Get supported chain pairs for cross-chain. No parameters required.

```bash
onchainos cross-chain chains
```

Returns map of supported chain pairs. For display only -- chain pair validation is done by `/quote` (unsupported pairs return an error).

## 2. onchainos cross-chain bridge

Get available bridge protocols. No parameters required.

```bash
onchainos cross-chain bridge
```

Returns bridge list with `_name`, `code`, `type`. `bridgeType` is for internal use -- do not display or filter by it.

## 3. onchainos cross-chain quote

Get cross-chain quote (read-only).

```bash
onchainos cross-chain quote --from <address> --to <address> --from-chain <chain> --to-chain <chain> --readable-amount <amount> [--receive-address <addr>] [--sort <0|1|2>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--from` | Yes | - | Source token contract address or alias |
| `--to` | Yes | - | Destination token contract address or alias |
| `--from-chain` | Yes | - | Source chain (e.g., `ethereum`, `arbitrum`) |
| `--to-chain` | Yes | - | Destination chain (e.g., `optimism`, `base`) |
| `--readable-amount` | Yes | - | Human-readable amount (decimal, e.g., `"10"` for 10 USDC). Do NOT multiply by decimals. |
| `--receive-address` | No | wallet | Receive address on destination chain |
| `--sort` | No | `0` | 0=optimal, 1=fastest, 2=max output |

**Return fields** (per route in `pathSelectionRouterList`):

| Field | Type | Description |
|---|---|---|
| `bridgeName` | String | Bridge protocol name |
| `receiveAmount` | String | Expected arrival amount |
| `minimumReceived` | String | Guaranteed minimum (below this, auto-revert) |
| `totalFee` | String | Total fee in USD |
| `estimatedTime` | String | Estimated time in seconds |
| `needApprove` | String | **DEPRECATED — do NOT use.** Unreliable. Use `dexMultiTokenAllowanceOut.amount` vs `inputAmount * 10^decimal` instead |
| `crossMiniAmount` / `crossMaxAmount` | String | Min/max amount limit for this route |
| `openMev` | String | "1"=MEV threshold reached |
| `isNeedClaim` | String | "1"=requires manual redeem (not supported) |
| `bridge.dexMultiTokenAllowanceOut.amount` | String | Current allowance for this route's spender |
| `bridge.dexMultiTokenAllowanceOut.tokenContractAddress` | String | Token being approved |
| `bridge.dexMultiTokenAllowanceOut.needCancelApproveToken` | Boolean | true=must revoke before re-approve (USDT pattern) |
| `bridge.callDataMap.dynamicApproveAddress` | String? | Spender address (null for some bridges) |

## 4. onchainos cross-chain execute

Execute cross-chain transaction. Three modes controlled by `--skip-approve` / `--confirm-approve`.

```bash
onchainos cross-chain execute --from <address> --to <address> --from-chain <chain> --to-chain <chain> --readable-amount <amount> --wallet <addr> [--receive-address <addr>] [--route-index <n>] [--mev-protection] [--tips <sol>] [--skip-approve] [--confirm-approve]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--from` | Yes | - | Source token address or alias |
| `--to` | Yes | - | Destination token address or alias |
| `--from-chain` | Yes | - | Source chain |
| `--to-chain` | Yes | - | Destination chain |
| `--readable-amount` | Yes | - | Human-readable amount |
| `--wallet` | Yes | - | User wallet address |
| `--receive-address` | No | wallet | Destination chain receive address |
| `--route-index` | No | `0` | Route index from quote (0=recommended) |
| `--gas-level` | No | `average` | Gas priority (reserved, currently unused) |
| `--mev-protection` | No | `false` | Enable MEV protection (EVM) |
| `--tips` | No | - | Solana Jito tips (reserved, currently unused) |
| `--skip-approve` | No | `false` | Skip allowance check, execute trade directly |
| `--confirm-approve` | No | `false` | Execute approval (after user confirms) |

### Return: action=approve-required

Returned when allowance is insufficient (default mode, no flags).

| Field | Type | Description |
|---|---|---|
| `action` | String | `"approve-required"` |
| `spender` | String | Contract to be authorized |
| `tokenAddress` | String | Token contract address |
| `tokenSymbol` | String | Token symbol |
| `approveAmount` | String | Required amount (raw) |
| `readableAmount` | String | Required amount (human-readable) |
| `currentAllowance` | String | Current allowance |
| `bridgeName` | String | Selected bridge |
| `needCancelApprove` | Boolean | true=must revoke first (USDT pattern) |

### Return: action=approved

Returned after approval broadcast (`--confirm-approve`).

| Field | Type | Description |
|---|---|---|
| `action` | String | `"approved"` |
| `approveTxHash` | String | Approval transaction hash |
| `spender` | String | Authorized contract |
| `tokenAddress` | String | Token contract address |
| `tokenSymbol` | String | Token symbol |
| `approveAmount` | String | Approved amount (raw) |
| `readableAmount` | String | Approved amount (human-readable) |
| `bridgeName` | String | Selected bridge |

### Return: action=execute

Returned on trade completion (default mode with sufficient allowance, or `--skip-approve`).

| Field | Type | Description |
|---|---|---|
| `action` | String | `"execute"` |
| `orderId` | String | Order ID for status queries |
| `crosschainTxHash` | String | Source chain transaction hash |
| `selectedRoute` | String | Bridge protocol used |
| `fromAmount` | String | Amount sent |
| `estimatedReceiveAmount` | String | Expected arrival (from latest quote) |
| `minimumReceived` | String | Guaranteed minimum |
| `totalFee` | String | Total fee in USD |
| `estimatedTime` | String | Estimated arrival time (seconds) |
| `calldataType` | Number | Transaction type (100/101/110) |

## Input / Output Examples

**User says:** "Bridge 1 USDC from Arbitrum to Optimism"

```bash
# 1. Quote
onchainos cross-chain quote --from usdc --to usdc --from-chain arbitrum --to-chain optimism --readable-amount 1
# -> Routes: ACROSS V3 (0.9996 USDC, $0.0004 fee, ~37s), STARGATE V2 (0.9994, ~46s)

# 2. Execute (check approval)
onchainos cross-chain execute --from usdc --to usdc --from-chain arbitrum --to-chain optimism --readable-amount 1 --wallet 0xf290...
# -> action=execute (if already approved): { orderId, crosschainTxHash, ... }
# -> action=approve-required (if not approved): { spender, tokenSymbol, approveAmount, ... }

# 3. If approve needed: confirm then execute approval
onchainos cross-chain execute --from usdc --to usdc --from-chain arbitrum --to-chain optimism --readable-amount 1 --wallet 0xf290... --confirm-approve
# -> action=approved: { approveTxHash: "0x..." }

# 4. Poll approval status
onchainos wallet history --tx-hash 0x...
# -> Wait for txStatus success

# 5. Execute trade (skip approval check)
onchainos cross-chain execute --from usdc --to usdc --from-chain arbitrum --to-chain optimism --readable-amount 1 --wallet 0xf290... --skip-approve
# -> action=execute: { orderId: "17137...", crosschainTxHash: "0x72d2...", selectedRoute: "ACROSS V3", ... }

# 6. Check status
onchainos cross-chain status --order-id 17137664269175104
```

## 5. onchainos cross-chain calldata

Calldata only -- returns unsigned transaction data. Does NOT sign or broadcast.

```bash
onchainos cross-chain calldata --from <address> --to <address> --from-chain <chain> --to-chain <chain> --readable-amount <amount> --wallet <addr> [--receive-address <addr>] [--route-index <n>]
```

Returns: `calldataType`, `callData` (to/data/value/gas), `mevConfig`, `unsignedTx`.

## 6. onchainos cross-chain status

Query cross-chain order status.

```bash
onchainos cross-chain status --order-id <orderId>
```

| Param | Required | Description |
|---|---|---|
| `--order-id` | Yes | Order ID from execute result |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `status` | String | "1"=success, "0"=in progress, "-1"=failed, "100"=awaiting update |
| `fromChainId` | String | Source chain ID |
| `toChainId` | String | Destination chain ID |
| `fromAmount` | Number | Amount sent |
| `toAmount` | Number | Amount received (-1 if in progress) |
| `transactionHash` | String | Source chain tx hash |
| `fromChildOrderDetailVo.status` | Number | Source chain sub-order status |
| `bridgeChildOrderDetailVo.status` | Number | Bridge sub-order status |
| `bridgeChildOrderDetailVo.transactionHash` | String | Bridge/destination tx hash |
| `bridgeChildOrderDetailVo.bridgeOrderInfoVo.bridgeName` | String | Bridge protocol name |

**Success condition**: `fromChildOrderDetailVo.status=1` AND `bridgeChildOrderDetailVo.status=1`.

## 7. onchainos cross-chain probe

Probe which common tokens (USDC/USDT/native) can be bridged between two chains. Used as automatic fallback when direct quote returns no routes.

```bash
onchainos cross-chain probe --from-chain <chain> --to-chain <chain> [--readable-amount <n>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--from-chain` | Yes | - | Source chain name or index |
| `--to-chain` | Yes | - | Destination chain name or index |
| `--readable-amount` | No | 100 | Amount for estimation |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `fromChain` | String | Source chain index |
| `toChain` | String | Destination chain index |
| `readableAmount` | String | Amount used for estimation |
| `bridgeableTokens` | Array | List of tokens that have bridgeable routes |
| `bridgeableTokens[].token` | String | Token alias (usdc, usdt, native) |
| `bridgeableTokens[].fromTokenAddress` | String | Token address on source chain |
| `bridgeableTokens[].toTokenAddress` | String | Token address on destination chain |
| `bridgeableTokens[].fromTokenSymbol` | String | Token symbol on source chain |
| `bridgeableTokens[].toTokenSymbol` | String | Token symbol on destination chain |
| `bridgeableTokens[].receiveAmount` | String | Best route estimated receive |
| `bridgeableTokens[].minimumReceived` | String | Best route minimum guaranteed |
| `bridgeableTokens[].totalFee` | String | Best route total fee (USD) |
| `bridgeableTokens[].estimatedTime` | String | Best route estimated time (seconds) |
| `bridgeableTokens[].bridgeName` | String | Best route bridge protocol name |
| `bridgeableTokens[].routeCount` | Number | Total available routes for this token |
