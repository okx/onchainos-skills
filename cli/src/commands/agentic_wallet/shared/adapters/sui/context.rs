//! Loads the authenticated SUI account and chain context.

use crate::commands::agentic_wallet::chain_profile::{ResolvedChainProfile, TransferDriver};
use crate::commands::agentic_wallet::shared::common::context as shared_context;
use crate::commands::agentic_wallet::shared::common::session::{session_cert, SigningSeed};
use crate::wallet_store::AddressInfo;
use anyhow::Result;

#[derive(Clone, Debug)]
pub struct SuiContext {
    pub access_token: String,
    pub account_id: String,
    pub profile: ResolvedChainProfile,
    pub address: AddressInfo,
}

impl SuiContext {
    /// Loads the authenticated SUI account context for an optional source address.
    pub async fn load(from: Option<&str>) -> Result<Self> {
        let loaded = shared_context::load_chain_context(
            "sui",
            TransferDriver::Sui,
            "SUI",
            from,
            validate_sui_address,
            super::identifiers::same_address,
        )
        .await?;

        Ok(Self {
            access_token: loaded.access_token,
            account_id: loaded.account_id,
            profile: loaded.profile,
            address: loaded.address,
        })
    }

    /// Parses the resolved SUI chain index for numeric API fields.
    pub fn chain_index_u64(&self) -> Result<u64> {
        self.profile.chain_index.parse().map_err(|_| {
            anyhow::anyhow!(
                "SUI runtime chainIndex '{}' is not numeric",
                self.profile.chain_index
            )
        })
    }

    /// Returns the session certificate attached to SUI signing requests.
    pub fn session_cert(&self) -> Result<String> {
        session_cert()
    }

    /// Decrypts the session seed and returns it in an automatically zeroized wrapper.
    pub fn signing_seed(&self) -> Result<SigningSeed> {
        SigningSeed::load()
    }
}

/// Validates that a value can be normalized as a SUI account address.
fn validate_sui_address(value: &str) -> Result<()> {
    super::identifiers::normalize_address(value).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::agentic_wallet::chain_profile::{
        ChainCapabilities, InscriptionDriver, MessageSignDriver, TransferDriver,
    };
    use crate::wallet_store::{AccountMapEntry, WalletsJson};
    use std::collections::HashMap;

    fn profile() -> ResolvedChainProfile {
        ResolvedChainProfile {
            chain_index: "784".to_string(),
            real_chain_index: "784".to_string(),
            chain_name: "sui".to_string(),
            native_symbol: "SUI".to_string(),
            native_decimals: 9,
            capabilities: ChainCapabilities {
                transfer: TransferDriver::Sui,
                inscription: InscriptionDriver::Unsupported,
                contract_call: true,
                message_sign: MessageSignDriver::Unsupported,
            },
        }
    }

    #[test]
    fn from_cannot_select_another_account() {
        let current_address = format!("0x{}1", "0".repeat(63));
        let other_address = format!("0x{}2", "0".repeat(63));
        let mut accounts_map = HashMap::new();
        accounts_map.insert(
            "current".to_string(),
            AccountMapEntry {
                address_list: vec![AddressInfo {
                    account_id: "current".to_string(),
                    address: current_address,
                    chain_index: "784".to_string(),
                    chain_name: "sui".to_string(),
                    address_type: String::new(),
                    chain_path: String::new(),
                }],
            },
        );
        let wallets = WalletsJson {
            selected_account_id: "current".to_string(),
            accounts_map,
            ..Default::default()
        };
        assert!(shared_context::select_current_address(
            &wallets,
            "current",
            &profile(),
            "SUI",
            Some(&other_address),
            super::super::identifiers::same_address,
        )
        .is_err());
    }
}
