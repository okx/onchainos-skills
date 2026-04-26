//! Cryptographic helpers for the onchainos wallet.
//!
//! This module consolidates all signing & key-exchange primitives so that
//! wallet commands (`cmd_login_ak`, `cmd_verify`, `sign_and_broadcast`, …)
//! can share a single implementation without duplication.

use alloy_primitives::Address;
use alloy_sol_types::sol;
use anyhow::{bail, Context, Result};
use base64::Engine;

// ── X25519 session keypair ──────────────────────────────────────────────

/// Generate an X25519 keypair for HPKE key exchange.
///
/// Returns `(session_private_key_b64, temp_pub_key_b64)` — both
/// base64-encoded.  The server will use `temp_pub_key` to HPKE-encrypt
/// the Ed25519 signing seed; the client stores `session_private_key`
/// (a.k.a. `session_key`) to decrypt it later.
pub fn generate_x25519_session_keypair() -> (String, String) {
    let secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
    let public = x25519_dalek::PublicKey::from(&secret);
    let session_private_key = base64::engine::general_purpose::STANDARD.encode(secret.to_bytes());
    let temp_pub_key = base64::engine::general_purpose::STANDARD.encode(public.as_bytes());
    (session_private_key, temp_pub_key)
}

// ── HMAC-SHA256 for AK login ────────────────────────────────────────────

/// HMAC-SHA256 sign for AK login.
///
/// `message = "{timestamp}{method}{path}{params}"` → HMAC-SHA256 → base64.
pub fn ak_sign(timestamp: u64, method: &str, path: &str, params: &str, secret_key: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let message = format!("{}{}{}{}", timestamp, method, path, params);
    let mut mac = Hmac::<Sha256>::new_from_slice(secret_key.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(message.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

// ── HPKE decryption ─────────────────────────────────────────────────────

/// HPKE-decrypt `encrypted_session_sk` using the X25519 private key (`session_key`).
///
/// The server encrypts the Ed25519 signing seed via HPKE:
///   Suite = DHKEM(X25519, HKDF-SHA256) + HKDF-SHA256 + AES-256-GCM
///   info  = b"okx-tee-sign"
///
/// Wire format:  enc(32 bytes) || ciphertext(plaintext_len + 16 tag)
///
/// Returns the 32-byte Ed25519 signing seed.
pub fn hpke_decrypt_session_sk(encrypted_b64: &str, session_key_b64: &str) -> Result<[u8; 32]> {
    use hpke::{
        aead::AesGcm256, kdf::HkdfSha256, kem::X25519HkdfSha256, single_shot_open, Deserializable,
        OpModeR,
    };

    const HPKE_INFO: &[u8] = b"okx-tee-sign";
    const ENC_SIZE: usize = 32;

    // Decode inputs from base64
    let encrypted = base64::engine::general_purpose::STANDARD
        .decode(encrypted_b64)
        .context("encrypted_session_sk is not valid base64")?;

    let sk_bytes = base64::engine::general_purpose::STANDARD
        .decode(session_key_b64)
        .context("session_key is not valid base64")?;
    if sk_bytes.len() != 32 {
        bail!("session_key must be 32 bytes, got {}", sk_bytes.len());
    }
    let mut sk_arr = [0u8; 32];
    sk_arr.copy_from_slice(&sk_bytes);

    // Split: enc(32 bytes) || ciphertext
    if encrypted.len() <= ENC_SIZE {
        bail!(
            "encrypted_session_sk too short: {} bytes (need > {})",
            encrypted.len(),
            ENC_SIZE
        );
    }
    let (enc_bytes, ciphertext) = encrypted.split_at(ENC_SIZE);

    // Deserialize HPKE primitives
    let sk = <X25519HkdfSha256 as hpke::Kem>::PrivateKey::from_bytes(&sk_arr)
        .map_err(|e| anyhow::anyhow!("invalid X25519 private key: {e}"))?;
    let encapped_key = <X25519HkdfSha256 as hpke::Kem>::EncappedKey::from_bytes(enc_bytes)
        .map_err(|e| anyhow::anyhow!("invalid HPKE encapped key: {e}"))?;

    // HPKE single-shot decryption
    let plaintext = single_shot_open::<AesGcm256, HkdfSha256, X25519HkdfSha256>(
        &OpModeR::Base,
        &sk,
        &encapped_key,
        HPKE_INFO,
        ciphertext,
        &[], // empty AAD
    )
    .map_err(|e| anyhow::anyhow!("HPKE decryption failed: {e}"))?;

    if plaintext.len() != 32 {
        bail!(
            "decrypted signing seed must be 32 bytes, got {}",
            plaintext.len()
        );
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&plaintext);
    Ok(seed)
}

// ── Ed25519 signing ─────────────────────────────────────────────────────

/// Sign a raw message with Ed25519 using a 32-byte seed.
///
/// This is the lowest-level signing primitive. It is also used by the x402
/// module, so it is `pub`.
pub fn ed25519_sign(seed: &[u8], message: &[u8]) -> Result<Vec<u8>> {
    use ed25519_dalek::{Signer, SigningKey};
    let seed_bytes: [u8; 32] = seed
        .try_into()
        .map_err(|_| anyhow::anyhow!("session key must be 32 bytes, got {}", seed.len()))?;
    let signing_key = SigningKey::from_bytes(&seed_bytes);
    Ok(signing_key.sign(message).to_bytes().to_vec())
}

/// Ed25519-sign an encoded message with a 32-byte signing seed (base64).
///
/// 1. Decode the message according to `encoding` ("hex", "base64", or "base58")
/// 2. Create an Ed25519 SigningKey from the seed (base64 32-byte)
/// 3. Sign the decoded bytes
/// 4. Return base64-encoded signature
pub fn ed25519_sign_encoded(msg: &str, session_key_b64: &str, encoding: &str) -> Result<String> {
    use ed25519_dalek::{Signer, SigningKey};

    let msg_bytes = match encoding {
        "hex" => {
            let hex_clean = msg.strip_prefix("0x").unwrap_or(msg);
            if hex_clean.is_empty() {
                return Ok(String::new());
            }
            hex::decode(hex_clean).context("failed to decode hex message")?
        }
        "base64" => {
            if msg.is_empty() {
                return Ok(String::new());
            }
            base64::engine::general_purpose::STANDARD
                .decode(msg)
                .context("failed to decode base64 message")?
        }
        "base58" => {
            if msg.is_empty() {
                return Ok(String::new());
            }
            bs58::decode(msg)
                .into_vec()
                .context("failed to decode base58 message")?
        }
        _ => bail!("unsupported encoding: {encoding}, expected hex/base64/base58"),
    };

    let sk_bytes = base64::engine::general_purpose::STANDARD
        .decode(session_key_b64)
        .context("session_key is not valid base64")?;
    if sk_bytes.len() != 32 {
        bail!("session_key must be 32 bytes, got {}", sk_bytes.len());
    }
    let mut sk_arr = [0u8; 32];
    sk_arr.copy_from_slice(&sk_bytes);
    let signing_key = SigningKey::from_bytes(&sk_arr);

    let signature = signing_key.sign(&msg_bytes);

    Ok(base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()))
}

/// Convenience wrapper: Ed25519-sign a hex-encoded hash.
pub fn ed25519_sign_hex(hex_hash: &str, session_key_b64: &str) -> Result<String> {
    ed25519_sign_encoded(hex_hash, session_key_b64, "hex")
}

// ── secp256k1 (ECDSA) signing ───────────────────────────────────────────

/// Sign a 32-byte hash with secp256k1 ECDSA using a 32-byte private key.
///
/// Returns a 65-byte recoverable signature: `r (32) || s (32) || v (1)`.
/// **`v` is normalized to modern format (0 or 1).** Callers that need
/// legacy Ethereum `v` values (27/28) must add 27 themselves — see
/// [`eip3009_sign`] for an example.
pub fn secp256k1_sign(seed: &[u8], message: &[u8]) -> Result<Vec<u8>> {
    use alloy_primitives::B256;
    use alloy_signer::SignerSync;
    use alloy_signer_local::PrivateKeySigner;

    let seed_bytes: [u8; 32] = seed
        .try_into()
        .map_err(|_| anyhow::anyhow!("private key must be 32 bytes, got {}", seed.len()))?;
    let msg_bytes: [u8; 32] = message
        .try_into()
        .map_err(|_| anyhow::anyhow!("message hash must be 32 bytes, got {}", message.len()))?;

    let signer = PrivateKeySigner::from_slice(&seed_bytes)
        .map_err(|e| anyhow::anyhow!("invalid secp256k1 private key: {e}"))?;
    let hash = B256::from(msg_bytes);
    let sig = signer
        .sign_hash_sync(&hash)
        .map_err(|e| anyhow::anyhow!("secp256k1 signing failed: {e}"))?;

    // Verify: recover address from signature and compare to signer
    let recovered = sig
        .recover_address_from_prehash(&hash)
        .map_err(|e| anyhow::anyhow!("signature recovery failed: {e}"))?;
    if recovered != signer.address() {
        bail!(
            "signature verification failed: recovered {} but expected {}",
            recovered,
            signer.address()
        );
    }

    let v = sig.v() as u8;
    let mut out = Vec::with_capacity(65);
    out.extend_from_slice(&sig.r().to_be_bytes::<32>());
    out.extend_from_slice(&sig.s().to_be_bytes::<32>());
    out.push(if v < 27 { v } else { v - 27 }); // normalize to 0/1
    Ok(out)
}

// ── EIP-3009 (Transfer With Authorization) ──────────────────────────────

sol! {
    #[derive(Debug, PartialEq)]
    struct TransferWithAuthorization {
        address from;
        address to;
        uint256 value;
        uint256 validAfter;
        uint256 validBefore;
        bytes32 nonce;
    }
}

/// EIP-712 domain parameters for EIP-3009.
#[derive(Debug, Clone)]
pub struct Eip3009DomainParams {
    pub name: String,
    pub version: String,
    pub chain_id: u64,
    pub verifying_contract: Address,
}

/// Sign an EIP-3009 TransferWithAuthorization using EIP-712 typed data signing.
///
/// Returns a base64-encoded 65-byte recoverable signature: `r (32) || s (32) || v (1)`,
/// where `v` is 27 or 28.
pub fn eip3009_sign(
    auth: &TransferWithAuthorization,
    domain_params: &Eip3009DomainParams,
    private_key: &[u8],
) -> Result<String> {
    use alloy_sol_types::{eip712_domain, SolStruct};

    let domain = eip712_domain! {
        name: domain_params.name.clone(),
        version: domain_params.version.clone(),
        chain_id: domain_params.chain_id,
        verifying_contract: domain_params.verifying_contract,
    };

    let signing_hash = auth.eip712_signing_hash(&domain);

    let mut sig = secp256k1_sign(private_key, signing_hash.as_ref())?;
    // Convert v from modern (0/1) to legacy (27/28)
    sig[64] += 27;
    Ok(base64::engine::general_purpose::STANDARD.encode(&sig))
}

/// EIP-191 (personal_sign) + Ed25519:
/// 1. Decode `msg` according to `encoding`:
///    - `"hex"`: strip optional "0x" prefix, hex-decode to raw bytes
///    - `"utf8"`: use the raw UTF-8 bytes directly
/// 2. Build EIP-191 message: "\x19Ethereum Signed Message:\n" + len(raw_bytes) + raw_bytes
/// 3. Keccak-256 hash the message
/// 4. Ed25519 sign the hash with `signing_seed`
/// 5. Return base64-encoded signature
pub fn ed25519_sign_eip191(msg: &str, signing_seed: &[u8], encoding: &str) -> Result<String> {
    use tiny_keccak::{Hasher, Keccak};

    if msg.is_empty() {
        return Ok(String::new());
    }

    let data = match encoding {
        "hex" => {
            let hex_clean = msg.strip_prefix("0x").unwrap_or(msg);
            hex::decode(hex_clean).context("msg is not valid hex")?
        }
        "utf8" => msg.as_bytes().to_vec(),
        _ => bail!("unsupported encoding for eip191: {encoding}, expected \"hex\" or \"utf8\""),
    };

    // Build EIP-191 message
    let prefix = format!("\x19Ethereum Signed Message:\n{}", data.len());
    let mut eth_msg = prefix.into_bytes();
    eth_msg.extend_from_slice(&data);

    // Keccak-256
    let mut keccak = Keccak::v256();
    keccak.update(&eth_msg);
    let mut hash = [0u8; 32];
    keccak.finalize(&mut hash);
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][ed25519_sign_eip191] keccak256 hash={}",
            hex::encode(hash)
        );
    }

    // Sign & base64 encode
    let sig_bytes = ed25519_sign(signing_seed, &hash)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&sig_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{FixedBytes, U256};

    #[test]
    fn x25519_keypair_deterministic_from_same_secret() {
        // Given the same 32-byte secret, the X25519 public key should be deterministic
        let secret_bytes = [42u8; 32];
        let sk1 = x25519_dalek::StaticSecret::from(secret_bytes);
        let pk1 = x25519_dalek::PublicKey::from(&sk1);

        let sk2 = x25519_dalek::StaticSecret::from(secret_bytes);
        let pk2 = x25519_dalek::PublicKey::from(&sk2);

        assert_eq!(pk1.as_bytes(), pk2.as_bytes());
    }

    #[test]
    fn x25519_keypair_different_secrets_yield_different_pubkeys() {
        let sk1 = x25519_dalek::StaticSecret::from([1u8; 32]);
        let sk2 = x25519_dalek::StaticSecret::from([2u8; 32]);
        let pk1 = x25519_dalek::PublicKey::from(&sk1);
        let pk2 = x25519_dalek::PublicKey::from(&sk2);
        assert_ne!(pk1.as_bytes(), pk2.as_bytes());
    }

    #[test]
    fn generate_x25519_session_keypair_returns_valid_base64() {
        let (sk, pk) = generate_x25519_session_keypair();
        let sk_bytes = base64::engine::general_purpose::STANDARD
            .decode(&sk)
            .unwrap();
        let pk_bytes = base64::engine::general_purpose::STANDARD
            .decode(&pk)
            .unwrap();
        assert_eq!(sk_bytes.len(), 32);
        assert_eq!(pk_bytes.len(), 32);
    }

    #[test]
    fn generate_x25519_session_keypair_unique_each_call() {
        let (sk1, _) = generate_x25519_session_keypair();
        let (sk2, _) = generate_x25519_session_keypair();
        assert_ne!(sk1, sk2);
    }

    #[test]
    fn ed25519_sign_roundtrip() {
        let seed = [7u8; 32];
        let message = b"hello world";
        let sig = ed25519_sign(&seed, message).unwrap();
        assert_eq!(sig.len(), 64);
    }

    #[test]
    fn ed25519_sign_rejects_wrong_seed_length() {
        let short_seed = [0u8; 16];
        assert!(ed25519_sign(&short_seed, b"msg").is_err());
    }

    #[test]
    fn secp256k1_sign_returns_65_bytes() {
        let seed = [7u8; 32];
        let hash = [0xabu8; 32];
        let sig = secp256k1_sign(&seed, &hash).unwrap();
        assert_eq!(sig.len(), 65);
    }

    #[test]
    fn secp256k1_sign_deterministic() {
        let seed = [7u8; 32];
        let hash = [0xabu8; 32];
        let sig1 = secp256k1_sign(&seed, &hash).unwrap();
        let sig2 = secp256k1_sign(&seed, &hash).unwrap();
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn secp256k1_sign_rejects_wrong_seed_length() {
        let short_seed = [0u8; 16];
        let hash = [0u8; 32];
        assert!(secp256k1_sign(&short_seed, &hash).is_err());
    }

    #[test]
    fn secp256k1_sign_rejects_wrong_message_length() {
        let seed = [7u8; 32];
        let short_msg = [0u8; 16];
        assert!(secp256k1_sign(&seed, &short_msg).is_err());
    }

    #[test]
    fn eip3009_sign_returns_65_bytes() {
        let auth = TransferWithAuthorization {
            from: Address::from([0x11; 20]),
            to: Address::from([0x22; 20]),
            value: U256::from(1_000_000),
            validAfter: U256::from(0),
            validBefore: U256::from(u64::MAX),
            nonce: FixedBytes::<32>::from([0x33; 32]),
        };
        let domain = Eip3009DomainParams {
            name: "USD Coin".to_string(),
            version: "2".to_string(),
            chain_id: 1,
            verifying_contract: Address::from([0x44; 20]),
        };
        let sig_b64 = eip3009_sign(&auth, &domain, &[0x55; 32]).unwrap();
        let sig = base64::engine::general_purpose::STANDARD
            .decode(&sig_b64)
            .unwrap();
        assert_eq!(sig.len(), 65);
        let v = sig[64];
        assert!(v == 27 || v == 28);
    }

    #[test]
    fn eip3009_sign_deterministic() {
        let auth = TransferWithAuthorization {
            from: Address::from([0x11; 20]),
            to: Address::from([0x22; 20]),
            value: U256::from(1_000_000),
            validAfter: U256::from(0),
            validBefore: U256::from(u64::MAX),
            nonce: FixedBytes::<32>::from([0x33; 32]),
        };
        let domain = Eip3009DomainParams {
            name: "USD Coin".to_string(),
            version: "2".to_string(),
            chain_id: 1,
            verifying_contract: Address::from([0x44; 20]),
        };
        let pk = [0x55; 32];
        let sig1 = eip3009_sign(&auth, &domain, &pk).unwrap();
        let sig2 = eip3009_sign(&auth, &domain, &pk).unwrap();
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn eip3009_sign_rejects_invalid_private_key() {
        let auth = TransferWithAuthorization {
            from: Address::ZERO,
            to: Address::ZERO,
            value: U256::from(0),
            validAfter: U256::from(0),
            validBefore: U256::from(0),
            nonce: FixedBytes::<32>::ZERO,
        };
        let domain = Eip3009DomainParams {
            name: "Test".to_string(),
            version: "1".to_string(),
            chain_id: 1,
            verifying_contract: Address::ZERO,
        };
        assert!(eip3009_sign(&auth, &domain, &[0x00; 32]).is_err());
    }

    #[test]
    fn eip3009_sign_rejects_wrong_key_length() {
        let auth = TransferWithAuthorization {
            from: Address::ZERO,
            to: Address::ZERO,
            value: U256::from(0),
            validAfter: U256::from(0),
            validBefore: U256::from(0),
            nonce: FixedBytes::<32>::ZERO,
        };
        let domain = Eip3009DomainParams {
            name: "Test".to_string(),
            version: "1".to_string(),
            chain_id: 1,
            verifying_contract: Address::ZERO,
        };
        assert!(eip3009_sign(&auth, &domain, &[0x55; 16]).is_err());
    }

    #[test]
    fn eip3009_sign_cross_validates_with_reference_sdk() {
        // Inputs from 3009-rust/examples/basic_usage.rs
        let auth = TransferWithAuthorization {
            from: "0x5B38Da6a701c568545dCfcB03FcB875f56beddC4"
                .parse()
                .unwrap(),
            to: "0xAb8483F64d9C6d1EcF9b849Ae677dD3315835cb2"
                .parse()
                .unwrap(),
            value: U256::from(1_000_000),
            validAfter: U256::from(0),
            validBefore: U256::from(u64::MAX),
            nonce: FixedBytes::<32>::from([
                0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
                0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x01, 0x23, 0x45, 0x67,
                0x89, 0xab, 0xcd, 0xef,
            ]),
        };
        let domain = Eip3009DomainParams {
            name: "USD Coin".to_string(),
            version: "2".to_string(),
            chain_id: 1,
            verifying_contract: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
                .parse()
                .unwrap(),
        };
        let private_key: [u8; 32] = [
            0xac, 0x09, 0x74, 0xe4, 0x77, 0x11, 0xc0, 0x01, 0x54, 0x93, 0xdc, 0x81, 0x12, 0xc6,
            0x82, 0x79, 0x50, 0x69, 0xfb, 0x28, 0x9c, 0x02, 0xba, 0x8b, 0x85, 0xf6, 0xbc, 0x12,
            0xf0, 0x93, 0xd8, 0x8f,
        ];

        // Reference output from 3009-rust SDK (r || s || v)
        let expected_hex = "ed28be892229aa598e67b22b32f971cc0c5568723a634013368eedbd100db85f\
                            64d080891ec962b4229f07ce4f55531f587ad7e8cbc591ee5fefbbd9ec96e980\
                            1c";

        let sig_b64 = eip3009_sign(&auth, &domain, &private_key).unwrap();
        let sig_bytes = base64::engine::general_purpose::STANDARD
            .decode(&sig_b64)
            .unwrap();
        assert_eq!(hex::encode(&sig_bytes), expected_hex);
    }

    // ── EIP-3009 test vectors (verified with foundry cast & viem) ─────────

    /// Helper: build auth + domain, sign, assert hex matches expected.
    fn assert_eip3009_sig(
        pk_hex: &str,
        from: &str,
        to: &str,
        value: U256,
        valid_after: U256,
        valid_before: U256,
        nonce: FixedBytes<32>,
        domain_name: &str,
        domain_version: &str,
        chain_id: u64,
        contract: &str,
        expected_hex: &str,
    ) {
        let pk = hex::decode(pk_hex).unwrap();
        let auth = TransferWithAuthorization {
            from: from.parse().unwrap(),
            to: to.parse().unwrap(),
            value,
            validAfter: valid_after,
            validBefore: valid_before,
            nonce,
        };
        let domain = Eip3009DomainParams {
            name: domain_name.to_string(),
            version: domain_version.to_string(),
            chain_id,
            verifying_contract: contract.parse().unwrap(),
        };
        let sig = base64::engine::general_purpose::STANDARD
            .decode(eip3009_sign(&auth, &domain, &pk).unwrap())
            .unwrap();
        assert_eq!(hex::encode(&sig), expected_hex);
    }

    const USDC_MAINNET: &str = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
    const HH0: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
    const HH1: &str = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
    const HH2: &str = "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC";
    const HH0_PK: &str = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const HH1_PK: &str = "59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
    const HH2_PK: &str = "5de4111afe1b170e56a4e0ee6c276e0589e99db33164f00c2a7b4a9e10c904fb";

    /// TV1: Circle alice account, USDC mainnet, 7 USDC
    #[test]
    fn eip3009_sign_tv1_circle_alice_usdc_mainnet() {
        assert_eip3009_sig(
            "84132dd41f32804774a98647c308c0c94a54c0f3931128c0210b6f3689d2b7e7",
            "0x8e81C8f0CFf3d6eA2Fe72c1A5ee49Fc377401c2D",
            "0x244A0A1d21f21167c17e04EBc5FA33c885990674",
            U256::from(7_000_000),
            U256::ZERO,
            U256::MAX,
            FixedBytes::from([0xaa; 32]),
            "USD Coin",
            "2",
            1,
            USDC_MAINNET,
            "07b4ad544d883e681552db26de55a707be5a5a5cd814386e0d411daef71715ea\
             66ee9a96d4c1dc65b023ceb8d89c523b80e233baa95347ff4a212817d593963b\
             1c",
        );
    }

    /// TV2: Hardhat #0, USDC mainnet, 1 USDC, zero nonce
    #[test]
    fn eip3009_sign_tv2_hardhat0_usdc_mainnet() {
        assert_eip3009_sig(
            HH0_PK,
            HH0,
            HH1,
            U256::from(1_000_000),
            U256::ZERO,
            U256::MAX,
            FixedBytes::ZERO,
            "USD Coin",
            "2",
            1,
            USDC_MAINNET,
            "9c7cc05c1539ce2fee00de51df7e0a13696469b2dd1bb112832d8fe19715aaab\
             416f67528f2f176e837c0b950149c2211a7651b90a62d22c9ecc27c1ebf0b263\
             1b",
        );
    }

    /// TV3: Hardhat #1, USDC on chainId 31337
    #[test]
    fn eip3009_sign_tv3_hardhat1_chain31337() {
        assert_eip3009_sig(
            HH1_PK,
            HH1,
            HH2,
            U256::from(500_000),
            U256::ZERO,
            U256::MAX,
            FixedBytes::from([0xbb; 32]),
            "USD Coin",
            "2",
            31337,
            USDC_MAINNET,
            "04d2db5c670d69cdfeae007c5ab39ca53b5f144e66c5e26744c8efc920347d15\
             7eadfb128664e03242bd673d27a2d9108e845582228ae5b4a94865e3926f1e6c\
             1c",
        );
    }

    /// TV4: Zero value transfer
    #[test]
    fn eip3009_sign_tv4_zero_value() {
        assert_eip3009_sig(
            HH0_PK,
            HH0,
            HH1,
            U256::ZERO,
            U256::ZERO,
            U256::MAX,
            FixedBytes::from([
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x01,
            ]),
            "USD Coin",
            "2",
            1,
            USDC_MAINNET,
            "fb96cf2c611c860e2fd0cd4ee3cf71236871d6a1d8052971e530ef3cd45e9f61\
             253e32b78311aec9470ad22aa52f0ddc2061357d87fa8cf2c384411d14035a54\
             1c",
        );
    }

    /// TV5: Max uint256 value, max nonce
    #[test]
    fn eip3009_sign_tv5_max_value() {
        assert_eip3009_sig(
            HH1_PK,
            HH1,
            HH2,
            U256::MAX,
            U256::ZERO,
            U256::MAX,
            FixedBytes::from([0xff; 32]),
            "USD Coin",
            "2",
            1,
            USDC_MAINNET,
            "b114bc5ccf7cbcbdde716ce0cafb20ed5c18ddfb7ad06334af87117c6cfceddf\
             4b050e0c3def7c403b9d08b449db2a31521c4d0a1d5b3147718a4a86406418d6\
             1b",
        );
    }

    /// TV6: Specific time window (validAfter=1700000000, validBefore=1800000000)
    #[test]
    fn eip3009_sign_tv6_time_window() {
        assert_eip3009_sig(
            HH2_PK,
            HH2,
            "0x90F79bf6EB2c4f870365E785982E1f101E93b906",
            U256::from(50_000_000),
            U256::from(1_700_000_000u64),
            U256::from(1_800_000_000u64),
            FixedBytes::from([
                0xde, 0xad, 0xbe, 0xef, 0xde, 0xad, 0xbe, 0xef, 0xde, 0xad, 0xbe, 0xef, 0xde, 0xad,
                0xbe, 0xef, 0xde, 0xad, 0xbe, 0xef, 0xde, 0xad, 0xbe, 0xef, 0xde, 0xad, 0xbe, 0xef,
                0xde, 0xad, 0xbe, 0xef,
            ]),
            "USD Coin",
            "2",
            1,
            USDC_MAINNET,
            "5af722bee4884f192b5c9373a71f1110da8f844bc8ea77bc0d95b6c5114f506e\
             6fba6639af57a30e9a9a34a8d609c5e7650b3625a7ee9ad9a780633836c084eb\
             1b",
        );
    }

    /// TV7: USDC on Polygon (chainId 137)
    #[test]
    fn eip3009_sign_tv7_polygon() {
        assert_eip3009_sig(
            "7c852118294e51e653712a81e05800f419141751be58f605c371e15141b007a6",
            "0x15d34AAf54267DB7D7c367839AAf71A00a2C6A65",
            "0x9965507D1a55bcC2695C58ba16FB37d819B0A4dc",
            U256::from(1_000_000_000u64),
            U256::ZERO,
            U256::MAX,
            FixedBytes::from([
                0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab,
                0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78,
                0x90, 0xab, 0xcd, 0xef,
            ]),
            "USD Coin",
            "2",
            137,
            "0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359",
            "b43b84668eda289dc9e28973b020cf1289b08e2c620c80a5e9863d2b8194421f\
             0568ed2f6a2cb496cf4348bb67354c266e0c62dee5e1b38dc96491f88b73f318\
             1c",
        );
    }

    /// TV8: Tether-like domain (different name/version/contract)
    #[test]
    fn eip3009_sign_tv8_tether_domain() {
        assert_eip3009_sig(
            "84132dd41f32804774a98647c308c0c94a54c0f3931128c0210b6f3689d2b7e7",
            "0x8e81C8f0CFf3d6eA2Fe72c1A5ee49Fc377401c2D",
            "0x244A0A1d21f21167c17e04EBc5FA33c885990674",
            U256::from(100_000_000u64),
            U256::ZERO,
            U256::MAX,
            FixedBytes::from([0x55; 32]),
            "Tether USD",
            "1",
            1,
            "0xdAC17F958D2ee523a2206206994597C13D831ec7",
            "a225847251a9804adf9b6a3e88e247600404db83d5a10b69caedc5aca720339e\
             0f919473e41e6ab718b7d0c4d79a630365c9e221860e4501d2a36573990d9f9f\
             1c",
        );
    }

    /// TV9: USDC on Arbitrum (chainId 42161), specific time window
    #[test]
    fn eip3009_sign_tv9_arbitrum() {
        assert_eip3009_sig(
            HH0_PK,
            HH0,
            HH1,
            U256::from(250_000),
            U256::from(1_600_000_000u64),
            U256::from(1_900_000_000u64),
            FixedBytes::from([
                0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45,
                0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01,
                0x23, 0x45, 0x67, 0x89,
            ]),
            "USD Coin",
            "2",
            42161,
            "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
            "fa2f5b36f4a60b3a25ea350fb5e335cf113b2d7eb00685599b48680f9ddc324e\
             340b12d7cc9d37312a6aa327dcc27dc71ecb7ac9c699cccfc33283bdd292a94b\
             1b",
        );
    }

    /// TV10: Self-transfer (from == to)
    #[test]
    fn eip3009_sign_tv10_self_transfer() {
        assert_eip3009_sig(
            HH2_PK,
            HH2,
            HH2,
            U256::from(1),
            U256::ZERO,
            U256::MAX,
            FixedBytes::ZERO,
            "USD Coin",
            "2",
            1,
            USDC_MAINNET,
            "7be5ae042f7f2c2a3fae133465c4aa5e83a39341ed756beaae6b07f36dc36e29\
             4f243883c33f044991cdfc88d4424999424c81705ee5377623a12d16a8e2ad3f\
             1c",
        );
    }

    #[test]
    fn ak_sign_produces_base64() {
        let sig = ak_sign(1700000000, "GET", "/path", "?a=1", "secret");
        // HMAC-SHA256 output is 32 bytes → 44 base64 chars (with padding)
        assert_eq!(sig.len(), 44);
        assert!(base64::engine::general_purpose::STANDARD
            .decode(&sig)
            .is_ok());
    }
}
