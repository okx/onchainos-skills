use anyhow::Result;
use serde_json::Value;

use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::output;
use crate::wallet_api::WalletApiClient;

use super::wallet::GasStationCommand;

pub async fn execute(cmd: GasStationCommand) -> Result<()> {
    match cmd {
        GasStationCommand::UpdateDefaultToken {
            chain,
            gas_token_address,
        } => {
            let data = fetch_update_default_token(&chain, &gas_token_address).await?;
            output::success(data);
            Ok(())
        }
        GasStationCommand::Enable { chain } => {
            let data = fetch_update(&chain, true).await?;
            output::success(data);
            Ok(())
        }
        GasStationCommand::Disable { chain } => {
            let data = fetch_update(&chain, false).await?;
            output::success(data);
            Ok(())
        }
    }
}

/// Public fetch function for MCP and CLI
pub async fn fetch_update_default_token(chain: &str, gas_token_address: &str) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let chain_index = crate::chains::resolve_chain(chain);

    // Resolve fromAddr from currently selected account for this chain
    let chain_entry = super::chain::get_chain_by_real_chain_index(&chain_index)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unsupported chain: {chain}"))?;
    let chain_name = chain_entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("chain entry missing chainName"))?;
    let wallets = crate::wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let (_, addr_info) = super::transfer::resolve_address(&wallets, None, chain_name)?;
    let from_addr = addr_info.address;

    let mut client = WalletApiClient::new()?;
    let data = client
        .gas_station_update_default_token(&access_token, &chain_index, gas_token_address, &from_addr)
        .await
        .map_err(format_api_error)?;
    Ok(data)
}

/// Flip Gas Station DB flag for a chain (`enable=true` to enable / `false` to disable).
/// DB flag only, no on-chain action. On-chain 7702 delegation is preserved on disable,
/// so a later enable does NOT require a new 7702 upgrade (backend returns a msg if the
/// chain was never delegated to begin with).
pub async fn fetch_update(chain: &str, enable: bool) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let chain_index = crate::chains::resolve_chain(chain);
    // Both enable and disable require fromAddr — backend contract is consistent across both.
    let chain_entry = super::chain::get_chain_by_real_chain_index(&chain_index)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unsupported chain: {chain}"))?;
    let chain_name = chain_entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("chain entry missing chainName"))?;
    let wallets = crate::wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let (_, addr_info) = super::transfer::resolve_address(&wallets, None, chain_name)?;
    let from_addr = addr_info.address;

    let mut client = WalletApiClient::new()?;
    let data = client
        .gas_station_update(&access_token, &chain_index, enable, Some(&from_addr))
        .await
        .map_err(format_api_error)?;
    Ok(data)
}
