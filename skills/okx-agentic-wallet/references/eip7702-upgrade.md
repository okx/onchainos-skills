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

## Disable Gas Station

Backend mechanism reference: calling `POST /gas-station/disable` only flips the DB enable flag (`gas_station_enabled = 0`); no on-chain action, existing delegation retained, `default_gas_token_address` retained. Next enable on this chain routes to `REENABLE_ONLY` — instant DB flip, no re-upgrade.

For user intent mapping ("revoke 7702" / "cancel authorization" / "turn off gas station" / "disable Gas Station" / equivalents in any language → disable Gas Station), Agent output vocabulary rules, and user-facing reply templates: see `gas-station.md` — **User Intent Recognition** table (row "disable gas station") and **User-Facing Reply Templates (Management Commands)** section. Do not maintain a second copy of the vocabulary bans or reply templates here.

---

## Edge Cases

| Case | Detection | Response |
|---|---|---|
| Upgrade in progress | `gasStationStatus="HAS_PENDING_TX"` + `hasPendingTx: true` | Use the authoritative user message in `gas-station.md` Step 1 table `HAS_PENDING_TX` row. Do NOT mention 7702 / upgrade to the user — the pending is opaque from the user's perspective. |
| Incompatible on-chain wallet state | Backend returns `gasStationStatus="NOT_APPLICABLE"` + `gasStationUsed=false` on a supported chain where Gas Station should otherwise apply | Backend detects incompatible on-chain wallet state and falls back to normal flow. If user asks, respond: "Gas Station is not available for your wallet on this chain. Please use native tokens to pay gas." Do NOT explain "7702 delegation" to the user. |
| Re-enable shortcut | `gasStationStatus="REENABLE_ONLY"` returned (DB disabled + on-chain already delegated) | No on-chain upgrade, no token selection — CLI re-enables silently via the auto-path handler. User-facing: treat like a normal Gas Station send (show `serviceCharge` + `orderId`). Do NOT expose "7702" / "delegation" to the user. |
| User asks "what is 7702" / "EIP-7702 是什么" | User **actively** asks (not Agent-initiated) | 可以简短解释：是一个让钱包支持用稳定币付 Gas 的底层协议升级，首次开启 Gas Station 时会自动完成。但 Agent **不主动**提 7702，避免技术概念干扰。 |
| User asks "how to revoke" / "取消授权" / "撤销 7702" | User inquiry | **不用"撤销"口径回答**。直接说："可以随时关闭 Gas Station，切换回主网币支付 Gas。" 引导到 `disable`。不提 7702、不提"授权"。 |
