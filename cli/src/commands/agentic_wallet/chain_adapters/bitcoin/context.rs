//! Loads the authenticated Bitcoin account and chain context.

use crate::commands::agentic_wallet::chain_profile::{ChainKind, ResolvedChainProfile};
use crate::commands::agentic_wallet::support::context as shared_context;
use crate::commands::agentic_wallet::support::session::{session_cert, SigningSeed};
use crate::wallet_store::AddressInfo;
use anyhow::Result;

#[derive(Clone, Debug)]
pub struct BtcContext {
    pub access_token: String,
    pub account_id: String,
    pub login_type: String,
    pub profile: ResolvedChainProfile,
    pub address: AddressInfo,
}

impl BtcContext {
    /// Loads the authenticated Bitcoin account context for an optional source address.
    pub async fn load(from: Option<&str>) -> Result<Self> {
        let loaded = shared_context::load_chain_context(
            "bitcoin",
            ChainKind::Bitcoin,
            "Bitcoin",
            from,
            super::validation::validate_wallet_address,
            super::validation::same_address,
        )
        .await?;

        Ok(Self {
            access_token: loaded.access_token,
            account_id: loaded.account_id,
            login_type: loaded.login_type,
            profile: loaded.profile,
            address: loaded.address,
        })
    }

    /// Parses the resolved Bitcoin chain index for numeric API fields.
    pub fn chain_index_u64(&self) -> Result<u64> {
        self.profile.chain_index.parse().map_err(|_| {
            anyhow::anyhow!(
                "Bitcoin runtime chainIndex '{}' is not numeric",
                self.profile.chain_index
            )
        })
    }

    /// Returns the session certificate attached to Bitcoin signing requests.
    pub fn session_cert(&self) -> Result<String> {
        session_cert()
    }

    /// Decrypts the session seed and returns it in an automatically zeroized wrapper.
    pub fn signing_seed(&self) -> Result<SigningSeed> {
        SigningSeed::load()
    }

    /// Maps a social login mode to the optional wallet type required by BRC-20 requests.
    pub fn social_wallet_type(&self) -> Option<&'static str> {
        matches!(self.login_type.as_str(), "email" | "google" | "apple").then_some("12")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::agentic_wallet::chain_profile::{
        AssetModel, ChainCapabilities, InscriptionDriver, MessageSignDriver, TransferDriver,
    };
    use crate::wallet_store::{AccountMapEntry, WalletsJson};
    use std::collections::HashMap;

    fn profile() -> ResolvedChainProfile {
        ResolvedChainProfile {
            kind: ChainKind::Bitcoin,
            chain_index: "0".to_string(),
            real_chain_index: "5".to_string(),
            chain_name: "Bitcoin".to_string(),
            native_symbol: "BTC".to_string(),
            native_decimals: 8,
            capabilities: ChainCapabilities {
                transfer: TransferDriver::Bitcoin,
                inscription: InscriptionDriver::Bitcoin,
                contract_call: false,
                message_sign: MessageSignDriver::Unsupported,
                asset_model: AssetModel::Utxo,
            },
        }
    }

    #[test]
    fn from_cannot_select_another_account() {
        let mut accounts_map = HashMap::new();
        accounts_map.insert(
            "current".to_string(),
            AccountMapEntry {
                address_list: vec![AddressInfo {
                    account_id: "current".to_string(),
                    address: "bc1p5cyxnuxmeuwuvkwfem96llyxf2lpvszn2h8p5h".to_string(),
                    chain_index: "0".to_string(),
                    chain_name: "Bitcoin".to_string(),
                    address_type: "taproot".to_string(),
                    chain_path: String::new(),
                }],
            },
        );
        let wallets = WalletsJson {
            selected_account_id: "current".to_string(),
            login_type: "email".to_string(),
            accounts_map,
            ..Default::default()
        };
        assert!(shared_context::select_current_address(
            &wallets,
            "current",
            &profile(),
            "Bitcoin",
            Some("bc1pqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqc8247j"),
            super::super::validation::same_address,
        )
        .is_err());
    }
}
