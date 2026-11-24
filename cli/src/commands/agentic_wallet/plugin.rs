use anyhow::{bail, Result};

use crate::output;
use crate::wallet_api::WalletApiClient;

use super::auth::{ensure_tokens_refreshed, format_api_error};

/// onchainos wallet report-plugin-info
pub(super) async fn cmd_report_plugin_info(plugin_parameter: &str) -> Result<()> {
    if plugin_parameter.trim().is_empty() {
        bail!("--plugin-parameter must not be empty");
    }
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = WalletApiClient::new()?;
    let data = client
        .report_plugin_info(&access_token, plugin_parameter)
        .await
        .map_err(format_api_error)?;
    output::success(data);
    Ok(())
}
