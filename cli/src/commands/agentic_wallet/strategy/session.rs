//! Wallet-session loader: HPKE-decrypt seed + resolve active accountId / addresses.
//! Call AFTER `ctx.client_async()` (which refreshes JWT).

use anyhow::{anyhow, Result};
use base64::Engine;
use zeroize::{Zeroize, Zeroizing};

use crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN;
use crate::keyring_store;
use crate::wallet_store;

pub struct WalletSession {
    pub account_id: String,
    pub session_cert: String,
    /// SA TEE id (embedded in `verifySignInfo.teeId`).
    pub tee_id: String,
    /// Base64 ed25519 signing seed; `Zeroizing` wipes on drop.
    pub seed_b64: Zeroizing<String>,
    pub evm_address: String,
    pub sol_address: String,
}

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
    /// Solana ("501") uses SOL address; everything else EVM.
    pub fn wallet_address_for(&self, chain_id: &str) -> &str {
        match chain_id {
            "501" | "solana" => &self.sol_address,
            _ => &self.evm_address,
        }
    }
}
