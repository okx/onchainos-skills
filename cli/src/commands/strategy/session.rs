//! Wallet-session loader for strategy commands.
//!
//! Centralises the boilerplate every strategy subcommand needs:
//! 1. Load `SessionJson` (session_cert + encrypted_session_sk)
//! 2. HPKE-decrypt the session-key blob to the raw 32-byte ed25519 seed
//! 3. Base64-encode the seed for `crypto::ed25519_sign_*`
//! 4. Resolve the currently-active `accountId` + addresses
//!
//! JWT freshness is **already handled by `ctx.client_async()`** — call
//! `client_async()` first, then `session::load()`; do not re-do that work
//! here. Mirrors the pattern in `commands/agentic_wallet/sign.rs`.

use anyhow::{anyhow, Result};
use base64::Engine;
use zeroize::{Zeroize, Zeroizing};

use crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN;
use crate::keyring_store;
use crate::wallet_store;

/// Live wallet context every strategy subcommand needs to mint signatures
/// and call BE. `seed_b64` is wrapped in `Zeroizing<_>` so the in-memory
/// cleartext is wiped automatically on drop — mirrors the pattern used by
/// `agentic_wallet::payment_flow`.
pub struct WalletSession {
    pub account_id: String,
    pub session_cert: String,
    /// SA TEE id assigned by BE at login; embedded in `verifySignInfo.teeId`.
    pub tee_id: String,
    /// Base64-encoded raw ed25519 signing seed (32 bytes decoded). Wrapped
    /// in `Zeroizing` so dropping the session wipes the in-memory cleartext;
    /// the original copy in keyring is untouched.
    pub seed_b64: Zeroizing<String>,
    /// SA wallet address — currently sourced from `WalletsJson.accounts_map`.
    pub evm_address: String,
    pub sol_address: String,
}

/// Load the live wallet context. Returns the strategy-canonical
/// `not logged in` message when any required state is missing.
///
/// Call **after** `ctx.client_async()` — the latter handles JWT refresh.
pub fn load() -> Result<WalletSession> {
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow!(ERR_NOT_LOGGED_IN))?;

    let session_key =
        keyring_store::get("session_key").map_err(|_| anyhow!(ERR_NOT_LOGGED_IN))?;
    let mut seed = crate::crypto::hpke_decrypt_session_sk(
        &session.encrypted_session_sk,
        &session_key,
    )?;
    let seed_b64 = Zeroizing::new(
        base64::engine::general_purpose::STANDARD.encode(seed.as_slice()),
    );
    seed.zeroize();

    let wallets =
        wallet_store::load_wallets()?.ok_or_else(|| anyhow!(ERR_NOT_LOGGED_IN))?;
    let account_id =
        crate::commands::agentic_wallet::account::resolve_active_account_id(&wallets)?;
    let entry = wallets
        .accounts_map
        .get(&account_id)
        .ok_or_else(|| anyhow!("active account not found in wallets map"))?;

    let evm_address = entry
        .address_list
        .iter()
        .find(|a| a.chain_index != "501")
        .map(|a| a.address.clone())
        .unwrap_or_default();
    let sol_address = entry
        .address_list
        .iter()
        .find(|a| a.chain_index == "501")
        .map(|a| a.address.clone())
        .unwrap_or_default();

    Ok(WalletSession {
        account_id,
        session_cert: session.session_cert,
        tee_id: session.tee_id,
        seed_b64,
        evm_address,
        sol_address,
    })
}

impl WalletSession {
    /// Pick the wallet address the BE expects for a given chain.
    /// Solana = chain "501"; everything else uses the EVM address.
    pub fn wallet_address_for(&self, chain_id: &str) -> &str {
        match chain_id {
            "501" | "solana" => &self.sol_address,
            _ => &self.evm_address,
        }
    }
}
