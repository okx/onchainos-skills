use anyhow::{bail, Context, Result};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainKind {
    Evm,
    Solana,
    Bitcoin,
    Sui,
    Tron,
    Ton,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferDriver {
    LegacyAccount,
    Bitcoin,
    Sui,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InscriptionDriver {
    Bitcoin,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageSignDriver {
    LegacyAccount,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetModel {
    Account,
    Utxo,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainCapabilities {
    pub transfer: TransferDriver,
    pub inscription: InscriptionDriver,
    pub contract_call: bool,
    pub message_sign: MessageSignDriver,
    pub asset_model: AssetModel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedChainProfile {
    pub kind: ChainKind,
    pub chain_index: String,
    pub real_chain_index: String,
    pub chain_name: String,
    pub native_symbol: String,
    pub native_decimals: u32,
    pub capabilities: ChainCapabilities,
}

impl ResolvedChainProfile {
    /// Returns whether this resolved profile uses the Bitcoin transaction flow.
    pub fn is_bitcoin(&self) -> bool {
        self.kind == ChainKind::Bitcoin
    }
}

/// Resolves a chain name, alias, or index and returns its runtime capabilities.
pub async fn resolve(input: &str) -> Result<ResolvedChainProfile> {
    let entry = match super::chain::get_chain_by_real_chain_index(input).await? {
        Some(entry) => entry,
        None => super::chain::get_all_chains()
            .await?
            .into_iter()
            .find(|entry| entry_matches_name_or_alias(entry, input))
            .ok_or_else(|| anyhow::anyhow!("unsupported chain: {input}"))?,
    };
    from_entry(&entry)
}

/// Checks whether a backend chain entry matches the requested name, alias, or index.
fn entry_matches_name_or_alias(entry: &Value, input: &str) -> bool {
    let input = input.trim();
    let direct_match = string_field(entry, "chainIndex").as_deref() == Some(input)
        || entry
            .get("chainName")
            .and_then(Value::as_str)
            .is_some_and(|name| name.eq_ignore_ascii_case(input))
        || entry
            .get("alias")
            .and_then(Value::as_array)
            .is_some_and(|aliases| {
                aliases.iter().any(|alias| {
                    alias
                        .as_str()
                        .is_some_and(|alias| alias.eq_ignore_ascii_case(input))
                })
            });
    if direct_match {
        return true;
    }

    matches!(input.to_ascii_lowercase().as_str(), "bitcoin" | "btc")
        && entry
            .get("chainName")
            .and_then(Value::as_str)
            .is_some_and(|name| matches!(name.to_ascii_lowercase().as_str(), "bitcoin" | "btc"))
}

/// Reads a string or integer field from a backend chain entry as a string.
fn string_field(entry: &Value, key: &str) -> Option<String> {
    entry.get(key).and_then(|value| {
        value
            .as_str()
            .map(str::to_string)
            .or_else(|| value.as_i64().map(|number| number.to_string()))
    })
}

/// Converts one backend chain entry into the profile used for command routing.
pub(crate) fn from_entry(entry: &Value) -> Result<ResolvedChainProfile> {
    let chain_index = string_field(entry, "chainIndex")
        .context("chain profile: chain entry missing chainIndex")?;
    let real_chain_index = string_field(entry, "realChainIndex")
        .context("chain profile: chain entry missing realChainIndex")?;
    let chain_name =
        string_field(entry, "chainName").context("chain profile: chain entry missing chainName")?;
    let lower_name = chain_name.to_ascii_lowercase();

    let kind = if matches!(real_chain_index.as_str(), "0" | "5")
        || matches!(lower_name.as_str(), "bitcoin" | "btc")
    {
        ChainKind::Bitcoin
    } else if real_chain_index == "501" || lower_name == "solana" {
        ChainKind::Solana
    } else if real_chain_index == "784" || lower_name == "sui" {
        ChainKind::Sui
    } else if real_chain_index == "195" || lower_name == "tron" {
        ChainKind::Tron
    } else if real_chain_index == "607" || lower_name == "ton" {
        ChainKind::Ton
    } else if entry
        .get("isEvmChain")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || lower_name == "tempo"
        || crate::chains::SUPPORTED_CHAIN_INDICES.contains(&real_chain_index.as_str())
    {
        ChainKind::Evm
    } else {
        ChainKind::Unknown
    };

    let (native_symbol, native_decimals, capabilities) = overlay(kind, entry);
    if chain_index.trim().is_empty() || real_chain_index.trim().is_empty() {
        bail!("chain profile: chain identifiers must not be empty");
    }

    Ok(ResolvedChainProfile {
        kind,
        chain_index,
        real_chain_index,
        chain_name,
        native_symbol,
        native_decimals,
        capabilities,
    })
}

/// Selects native-asset metadata and command drivers for the resolved chain kind.
fn overlay(kind: ChainKind, entry: &Value) -> (String, u32, ChainCapabilities) {
    let server_symbol = ["nativeSymbol", "chainSymbol", "symbol"]
        .iter()
        .find_map(|key| entry.get(key).and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    match kind {
        ChainKind::Bitcoin => (
            server_symbol.unwrap_or_else(|| "BTC".to_string()),
            8,
            ChainCapabilities {
                transfer: TransferDriver::Bitcoin,
                inscription: InscriptionDriver::Bitcoin,
                contract_call: false,
                message_sign: MessageSignDriver::Unsupported,
                asset_model: AssetModel::Utxo,
            },
        ),
        ChainKind::Solana => legacy_overlay(server_symbol.unwrap_or_else(|| "SOL".to_string()), 9),
        ChainKind::Sui => (
            server_symbol.unwrap_or_else(|| "SUI".to_string()),
            9,
            ChainCapabilities {
                transfer: TransferDriver::Sui,
                inscription: InscriptionDriver::Unsupported,
                contract_call: true,
                message_sign: MessageSignDriver::Unsupported,
                asset_model: AssetModel::Account,
            },
        ),
        ChainKind::Tron => legacy_overlay(server_symbol.unwrap_or_else(|| "TRX".to_string()), 6),
        ChainKind::Ton => legacy_overlay(server_symbol.unwrap_or_else(|| "TON".to_string()), 9),
        ChainKind::Evm => legacy_overlay(server_symbol.unwrap_or_default(), 18),
        ChainKind::Unknown => (
            server_symbol.unwrap_or_default(),
            0,
            ChainCapabilities {
                transfer: TransferDriver::Unsupported,
                inscription: InscriptionDriver::Unsupported,
                contract_call: false,
                message_sign: MessageSignDriver::Unsupported,
                asset_model: AssetModel::Unknown,
            },
        ),
    }
}

/// Builds the existing account-model capabilities for legacy supported chains.
fn legacy_overlay(symbol: String, decimals: u32) -> (String, u32, ChainCapabilities) {
    (
        symbol,
        decimals,
        ChainCapabilities {
            transfer: TransferDriver::LegacyAccount,
            inscription: InscriptionDriver::Unsupported,
            contract_call: true,
            message_sign: MessageSignDriver::LegacyAccount,
            asset_model: AssetModel::Account,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bitcoin_profile_uses_runtime_chain_index() {
        let profile = from_entry(&json!({
            "chainIndex": "0",
            "realChainIndex": "5",
            "chainName": "Bitcoin"
        }))
        .unwrap();
        assert_eq!(profile.kind, ChainKind::Bitcoin);
        assert_eq!(profile.chain_index, "0");
        assert_eq!(profile.native_decimals, 8);
        assert_eq!(profile.capabilities.transfer, TransferDriver::Bitcoin);
    }

    #[test]
    fn preprod_bitcoin_profile_and_literal_are_supported() {
        let entry = json!({
            "chainIndex": 0,
            "realChainIndex": 0,
            "chainName": "btc",
            "isEvmChain": false,
            "alias": []
        });
        let profile = from_entry(&entry).unwrap();
        assert_eq!(profile.kind, ChainKind::Bitcoin);
        assert_eq!(profile.chain_index, "0");
        assert_eq!(profile.real_chain_index, "0");
        assert!(entry_matches_name_or_alias(&entry, "bitcoin"));
        assert!(entry_matches_name_or_alias(&entry, "0"));
    }

    #[test]
    fn sui_profile_supports_transfer_and_contract_call() {
        let profile = from_entry(&json!({
            "chainIndex": 784,
            "realChainIndex": 784,
            "chainName": "sui",
            "isEvmChain": false,
            "alias": []
        }))
        .unwrap();
        assert_eq!(profile.kind, ChainKind::Sui);
        assert_eq!(profile.capabilities.transfer, TransferDriver::Sui);
        assert_eq!(
            profile.capabilities.message_sign,
            MessageSignDriver::Unsupported
        );
        assert!(profile.capabilities.contract_call);
    }

    #[test]
    fn unknown_chain_has_no_write_capabilities() {
        let profile = from_entry(&json!({
            "chainIndex": "999999",
            "realChainIndex": "999999",
            "chainName": "Future Chain"
        }))
        .unwrap();
        assert_eq!(profile.kind, ChainKind::Unknown);
        assert_eq!(profile.capabilities.transfer, TransferDriver::Unsupported);
        assert!(!profile.capabilities.contract_call);
    }

    #[test]
    fn existing_tempo_driver_remains_legacy_evm() {
        let profile = from_entry(&json!({
            "chainIndex": "4217",
            "realChainIndex": "4217",
            "chainName": "Tempo",
            "isEvmChain": true
        }))
        .unwrap();
        assert_eq!(profile.kind, ChainKind::Evm);
        assert_eq!(profile.capabilities.transfer, TransferDriver::LegacyAccount);
    }

    #[test]
    fn backend_declared_dynamic_evm_keeps_legacy_capabilities() {
        let profile = from_entry(&json!({
            "chainIndex": "81457",
            "realChainIndex": "81457",
            "chainName": "blast_eth",
            "isEvmChain": true
        }))
        .unwrap();
        assert_eq!(profile.kind, ChainKind::Evm);
        assert_eq!(profile.capabilities.transfer, TransferDriver::LegacyAccount);
    }

    #[test]
    fn chain_alias_matching_is_case_insensitive() {
        let entry = json!({"chainName": "btc", "alias": ["bitcoin", "比特币"]});
        assert!(entry_matches_name_or_alias(&entry, "Bitcoin"));
        assert!(!entry_matches_name_or_alias(&entry, "ethereum"));
    }
}
