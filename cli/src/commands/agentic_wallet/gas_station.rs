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
        GasStationCommand::Revoke7702 { chain } => {
            let data = fetch_revoke_7702(&chain).await?;
            output::success(data);
            Ok(())
        }
    }
}

/// Public fetch function for MCP and CLI
pub async fn fetch_update_default_token(chain: &str, gas_token_address: &str) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let chain_index = crate::chains::resolve_chain(chain);
    let client = WalletApiClient::new()?;
    let req = serde_json::json!({
        "chainIndex": &chain_index,
        "gasTokenAddress": gas_token_address,
    });
    let data = client
        .gas_station_update_default_token(&access_token, &chain_index, gas_token_address)
        .await
        .map_err(format_api_error)?;
    super::debug_dump::dump("04-update-default-token", &req, &data);
    Ok(data)
}

/// Public fetch function for MCP and CLI
pub async fn fetch_revoke_7702(chain: &str) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let chain_index = crate::chains::resolve_chain(chain);
    let client = WalletApiClient::new()?;
    let req = serde_json::json!({ "chainIndex": &chain_index });
    let data = client
        .gas_station_revoke_7702(&access_token, &chain_index)
        .await
        .map_err(format_api_error)?;
    super::debug_dump::dump("05-revoke-7702", &req, &data);
    Ok(data)
}
