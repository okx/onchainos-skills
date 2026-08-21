use anyhow::{Context, Result};
use zeroize::Zeroizing;

pub struct SigningSeed(Zeroizing<[u8; 32]>);

impl SigningSeed {
    /// Decrypts the current session seed and returns an automatically zeroized wrapper.
    pub fn load() -> Result<Self> {
        let session = crate::wallet_store::load_session()?
            .ok_or_else(|| anyhow::anyhow!(super::super::common::ERR_NOT_LOGGED_IN))?;
        let session_key = Zeroizing::new(
            crate::keyring_store::get("session_key")
                .map_err(|_| anyhow::anyhow!(super::super::common::ERR_NOT_LOGGED_IN))?,
        );
        let seed = crate::crypto::hpke_decrypt_session_sk(
            &session.encrypted_session_sk,
            session_key.as_str(),
        )?;
        Ok(Self(Zeroizing::new(seed)))
    }

    #[cfg(test)]
    /// Builds a deterministic zeroized seed for unit tests.
    pub fn from_bytes(seed: [u8; 32]) -> Self {
        Self(Zeroizing::new(seed))
    }

    /// Borrows the decrypted seed bytes for a signing operation.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

/// Returns the certificate attached to signed Agentic Wallet requests.
pub fn session_cert() -> Result<String> {
    Ok(crate::wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(super::super::common::ERR_NOT_LOGGED_IN))?
        .session_cert)
}

/// Decodes a hexadecimal field after accepting an optional `0x` prefix.
pub(super) fn decode_hex(value: &str, field: &str) -> Result<Vec<u8>> {
    hex::decode(value.strip_prefix("0x").unwrap_or(value))
        .with_context(|| format!("{field} is not valid hex"))
}
