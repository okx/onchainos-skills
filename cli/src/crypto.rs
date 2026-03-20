//! Cryptographic helpers for the onchainos wallet.
//!
//! This module consolidates all signing & key-exchange primitives so that
//! wallet commands (`cmd_login_ak`, `cmd_verify`, `sign_and_broadcast`, …)
//! can share a single implementation without duplication.

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

/// EIP-191 (personal_sign) + Ed25519:
/// 1. Strip optional "0x" prefix from `hex_hash`, decode to raw bytes
/// 2. Build EIP-191 message: "\x19Ethereum Signed Message:\n" + len(raw_bytes) + raw_bytes
/// 3. Keccak-256 hash the message
/// 4. Ed25519 sign the hash with `signing_seed`
/// 5. Return base64-encoded signature
pub fn ed25519_sign_eip191(hex_hash: &str, signing_seed: &[u8]) -> Result<String> {
    use tiny_keccak::{Hasher, Keccak};

    let hex_clean = hex_hash.strip_prefix("0x").unwrap_or(hex_hash);
    if hex_clean.is_empty() {
        return Ok(String::new());
    }

    let data = hex::decode(hex_clean).context("unsigned.hash is not valid hex")?;

    // Build EIP-191 message
    let prefix = format!("\x19Ethereum Signed Message:\n{}", data.len());
    let mut eth_msg = prefix.into_bytes();
    eth_msg.extend_from_slice(&data);

    // Keccak-256
    let mut keccak = Keccak::v256();
    keccak.update(&eth_msg);
    let mut hash = [0u8; 32];
    keccak.finalize(&mut hash);

    // Sign & base64 encode
    let sig_bytes = ed25519_sign(signing_seed, &hash)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&sig_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn ak_sign_produces_base64() {
        let sig = ak_sign(1700000000, "GET", "/path", "?a=1", "secret");
        // HMAC-SHA256 output is 32 bytes → 44 base64 chars (with padding)
        assert_eq!(sig.len(), 44);
        assert!(base64::engine::general_purpose::STANDARD
            .decode(&sig)
            .is_ok());
    }
}
