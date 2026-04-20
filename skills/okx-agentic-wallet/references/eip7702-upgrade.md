# EIP-7702 Upgrade Flow Reference

EIP-7702 upgrades an EOA wallet to a smart contract wallet, enabling Gas Station (stablecoin gas payment). One-time operation per chain.

---

## When It Triggers

The `unsignedInfo` response contains `needUpdate7702: true` when:
- The wallet has never been upgraded on this chain (DB no record + on-chain not delegated)
- The wallet was upgraded with an old contract version
- DB shows disabled but on-chain still delegated → **no upgrade needed** (status `REENABLE_ONLY`)

The Agent does **not** need to check this manually — the backend handles the decision. The 7702 upgrade is bundled into the same transaction as the user's intent (e.g., token transfer).

---

## Signing Flow

When `needUpdate7702: true`, the response includes `authHashFor7702` alongside `hash` (712).

| Field | Signing Method | Location |
|---|---|---|
| `hash` | `ed25519_sign_eip191(hash, signing_seed, "hex")` | transfer.rs (existing, unchanged) |
| `authHashFor7702` | `sign_eip7702_auth(auth_hash)` | sign.rs — self-contained: loads session, decrypts key, ed25519_sign_hex, zeroizes |

Both signatures go into broadcast `extraData.msgForSign`:
```json
{
  "msgForSign": {
    "signature": "<712 hash signature>",
    "authSignatureFor7702": "<7702 auth hash signature>",
    "sessionCert": "<session cert>"
  }
}
```

### sign_eip7702_auth internals (sign.rs)

```
1. Load session from local store
2. Get session_key from keyring
3. HPKE decrypt → signing_seed
4. base64 encode → signing_seed_b64
5. ed25519_sign_hex(auth_hash, signing_seed_b64) → base64 signature
6. Zeroize signing_seed + signing_seed_b64
7. Return signature
```

Mirrors the `eip712_sign` pattern in sign.rs but skips the `gen-msg-hash` API call — the hash is already provided by `unsignedInfo`.

---

## Two Modes

7702 upgrade has two modes depending on how it's triggered:

### Silent mode — Gas Station send (current)

When 7702 upgrade happens as part of a Gas Station `wallet send` transaction:
- Upgrade is **bundled into the same transaction** — no separate confirmation
- User only confirms Gas Station enablement (Scene A), 7702 upgrade happens transparently
- Upgrade gas cost is included in the `serviceCharge`
- Per-chain: each supported chain requires one upgrade on first use

### Confirmation mode — independent / future scenarios

When 7702 is triggered outside of Gas Station send (e.g., standalone upgrade request, future DeFi/dApp scenarios):
- Must return a **CliConfirming** (exit code 2) prompting the user to acknowledge the 7702 upgrade
- User must explicitly confirm before the upgrade proceeds
- Message should explain: what 7702 is and that it's a one-time per-chain operation

```
# Future CliConfirming pattern:
{
  "confirming": true,
  "message": "Your wallet needs a one-time EIP-7702 upgrade on this chain to enable [feature]. This upgrades your wallet to support smart contract features. Proceed?",
  "next": "Re-run the same command with --force to confirm the 7702 upgrade."
}
```

---

## Disable Gas Station (关闭 Gas Station)

> **统一口径**: 对用户只说"关闭 Gas Station" / "enable/disable Gas Station"。**不要提"撤销 7702"、"取消授权"、"revoke 7702"** — 这些是内部技术术语，不对用户暴露。

调用 **disable** API (`POST /gas-station/disable`):
- 仅翻 DB 开关 (`gas_station_enabled = 0`)
- **不做链上动作**，on-chain 7702 委托保留
- `default_gas_token_address` 保留
- 下次开启时如链上已委托 → 状态 `REENABLE_ONLY`，只翻 DB 开关回来，不重新升级

**用户意图映射（输入 → 输出）：**

用户可能说：
- "取消 gas station 授权" / "撤销 7702" / "取消 7702 升级" / "关闭加油站" / "停用 Gas Station"

<MUST>
**Agent 响应统一口径**：都映射到"关闭 Gas Station"。示例话术：

> 好的，我来帮你关闭 Ethereum 上的 Gas Station。关闭后交易将切换回主网币支付 Gas，后续可随时重新开启。如果你只是想换一个支付代币，建议用"修改默认 Gas 代币"而不是关闭。是否确认关闭？

**NEVER 输出**：
- ❌ "撤销 7702 升级"
- ❌ "取消 7702 授权"
- ❌ "revoke 7702"
- ❌ "cancel 7702 upgrade"
- ❌ 任何提到 "7702"、"授权"、"委托" 的面向用户的说明

**只说**：
- ✅ "关闭 Gas Station" / "disable Gas Station"
- ✅ "切换回主网币支付 Gas"
- ✅ "后续可随时重新开启"
</MUST>

---

## Edge Cases

| Case | Detection | Response |
|---|---|---|
| Upgrade in progress | `hasPendingTx: true` from unsignedInfo | "A previous transaction (including 7702 upgrade) is still processing. Please wait." |
| Upgraded by third party | Backend returns gasStationUsed=false | Backend detects incompatible 7702 delegation and falls back to normal flow. If user asks, explain: "Your wallet has a 7702 delegation from another service that is not compatible with Gas Station." |
| Re-enable shortcut | state `REENABLE_ONLY` returned (DB disabled + on-chain delegated) | No 7702 upgrade, no token selection — just flips DB flag. CLI re-enables silently. |
| User asks "what is 7702" / "EIP-7702 是什么" | User **actively** asks (not Agent-initiated) | 可以简短解释：是一个让钱包支持用稳定币付 Gas 的底层协议升级，首次开启 Gas Station 时会自动完成。但 Agent **不主动**提 7702，避免技术概念干扰。 |
| User asks "how to revoke" / "取消授权" / "撤销 7702" | User inquiry | **不用"撤销"口径回答**。直接说："可以随时关闭 Gas Station，切换回主网币支付 Gas。" 引导到 `disable`。不提 7702、不提"授权"。 |
