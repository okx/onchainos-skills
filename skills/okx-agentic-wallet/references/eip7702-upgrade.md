# EIP-7702 Upgrade Flow Reference

EIP-7702 upgrades an EOA wallet to a smart contract wallet, enabling Gas Station (stablecoin gas payment). This is a one-time operation per chain.

---

## When It Triggers

The `unsignedInfo` response contains `needUpdate7702: true` when:
- The wallet has never been upgraded on this chain
- The wallet was upgraded with an old contract version
- The wallet's 7702 delegation was previously revoked

The Agent does **not** need to check this manually — the backend handles the decision. The 7702 upgrade is bundled into the same transaction as the user's intent (e.g., token transfer).

---

## Signing Flow

When `needUpdate7702: true`, the response includes an additional `authHashFor7702` field alongside the normal `hash` field.

| Field | Signing Method | Location |
|---|---|---|
| `hash` | `ed25519_sign_eip191(hash, signing_seed, "hex")` | transfer.rs (existing, unchanged) |
| `authHashFor7702` | `sign_eip7702_auth(auth_hash)` | sign.rs — self-contained: loads session, decrypts key, ed25519_sign_hex, zeroizes |

Both signatures are placed into `msgForSign` in the broadcast `extraData`:
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

This mirrors the `eip712_sign` pattern in sign.rs but skips the `gen-msg-hash` API call — the hash is already provided by `unsignedInfo`.

---

## Two Modes

7702 upgrade has two modes depending on how it's triggered:

### Silent mode — Gas Station send

When 7702 is triggered as part of a Gas Station `wallet send` transaction:
- The upgrade is **bundled into the same transaction** — no separate confirmation
- The user only confirms the Gas Station enablement (Scene A), the 7702 upgrade happens transparently
- Upgrade gas cost is included in the serviceCharge
- Per-chain: each supported chain requires one upgrade on first use

### Confirmation mode — independent / future scenarios

When 7702 is triggered outside of Gas Station send (e.g., standalone upgrade request, future DeFi/dApp scenarios):
- Must return a **CliConfirming** (exit code 2) prompting the user to acknowledge the 7702 upgrade
- The user must explicitly confirm before the upgrade proceeds
- Message should explain: what 7702 is, that it's a one-time per-chain operation, and that it's reversible

```
# Future CliConfirming pattern:
{
  "confirming": true,
  "message": "Your wallet needs a one-time EIP-7702 upgrade on this chain to enable [feature]. This upgrades your wallet to support smart contract features. The upgrade is reversible. Proceed?",
  "next": "Re-run the same command with --force to confirm the 7702 upgrade."
}
```

---

## Revocation

Users can revoke the 7702 upgrade via `wallet gas-station revoke-7702 --chain <chain>`.

**Effects of revocation:**
- Gas Station is disabled on that chain
- Future transactions require native tokens for gas
- Re-enabling Gas Station later triggers a new 7702 upgrade

**Requirements:**
- Must have sufficient native token balance (revocation is an on-chain transaction that cannot use Gas Station)

**Agent behavior before revocation:**

<MUST>
Always warn the user before executing revoke-7702:
> Revoking the 7702 upgrade will disable Gas Station on this chain. Future transactions will require native tokens for gas. If you just want to change the default gas payment token, you can do that without revoking. Would you like to change the default token instead?

Only proceed after explicit user confirmation.
</MUST>

---

## Edge Cases

| Case | Detection | Response |
|---|---|---|
| Upgrade in progress | `hasPendingTx: true` from unsignedInfo | "A previous transaction (including 7702 upgrade) is still processing. Please wait." |
| Upgraded by third party | Backend returns gasStationUsed=false | Backend detects incompatible 7702 delegation and falls back to normal flow. If user asks, explain: "Your wallet has a 7702 delegation from another service that is not compatible with Gas Station." |
| Revoke with no native token | revoke-7702 returns insufficient balance error | "Revoking requires native token to pay gas. Please top up first." |
| User asks "what is 7702" | User inquiry | "EIP-7702 is a standard that temporarily upgrades your wallet to support smart contract features like paying gas with stablecoins. The upgrade is reversible — you can revoke it at any time." |
