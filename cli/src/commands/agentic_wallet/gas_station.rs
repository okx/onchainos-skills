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
        GasStationCommand::Disable { chain } => {
            let data = fetch_disable(&chain).await?;
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
    let data = match client
        .gas_station_update_default_token(&access_token, &chain_index, gas_token_address)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            super::debug_dump::dump_error("04-update-default-token", &req, &format!("{e:#}"));
            return Err(format_api_error(e));
        }
    };
    super::debug_dump::dump("04-update-default-token", &req, &data);
    Ok(data)
}

/// Disable Gas Station for a chain. DB flag only, no on-chain action.
/// The 7702 delegation on-chain is preserved, so re-enabling later skips 7702 upgrade.
pub async fn fetch_disable(chain: &str) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let chain_index = crate::chains::resolve_chain(chain);
    let client = WalletApiClient::new()?;
    let req = serde_json::json!({ "chainIndex": &chain_index });
    let data = match client
        .gas_station_disable(&access_token, &chain_index)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            super::debug_dump::dump_error("05-disable", &req, &format!("{e:#}"));
            return Err(format_api_error(e));
        }
    };
    super::debug_dump::dump("05-disable", &req, &data);
    Ok(data)
}
