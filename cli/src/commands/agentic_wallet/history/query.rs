use anyhow::{bail, Result};

use crate::output;
use crate::wallet_api::WalletApiClient;
use crate::wallet_store;

use super::super::{
    auth::{ensure_tokens_refreshed, format_api_error},
    chain, common,
};
use super::response::{filter_detail_response, filter_list_response};

/// Queries the shared wallet order history API and emits its filtered response.
#[allow(clippy::too_many_arguments)]
pub(in crate::commands::agentic_wallet) async fn cmd_query_history(
    account_id: Option<&str>,
    chain: Option<&str>,
    address: Option<&str>,
    begin: Option<&str>,
    end: Option<&str>,
    cursor: Option<&str>,
    limit: Option<&str>,
    order_id: Option<&str>,
    tx_hash: Option<&str>,
    uop_hash: Option<&str>,
) -> Result<()> {
    let access_token = ensure_tokens_refreshed().await?;

    let resolved_account_id = match account_id {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => {
            let wallets = wallet_store::load_wallets()?
                .ok_or_else(|| anyhow::anyhow!(common::ERR_NOT_LOGGED_IN))?;
            if wallets.selected_account_id.is_empty() {
                bail!(common::ERR_NOT_LOGGED_IN);
            }
            wallets.selected_account_id
        }
    };

    let chain_index = match chain {
        Some(input) if !input.is_empty() => {
            let entry = chain::get_chain_by_real_chain_index(input)
                .await?
                .ok_or_else(|| anyhow::anyhow!("unsupported chain: {input}"))?;
            entry["chainIndex"]
                .as_str()
                .map(str::to_string)
                .or_else(|| entry["chainIndex"].as_i64().map(|value| value.to_string()))
                .unwrap_or_default()
        }
        _ => String::new(),
    };

    let mut client = WalletApiClient::new()?;
    if tx_hash.is_some() || order_id.is_some() || uop_hash.is_some() {
        if chain_index.is_empty() {
            bail!("--chain is required for order detail query");
        }

        let resolved_address = address.unwrap_or("");
        let mut query = vec![
            ("accountId", resolved_account_id.as_str()),
            ("chainIndex", chain_index.as_str()),
        ];
        if !resolved_address.is_empty() {
            query.push(("address", resolved_address));
        }
        if let Some(value) = tx_hash {
            query.push(("txHash", value));
        }
        if let Some(value) = order_id {
            query.push(("orderId", value));
        }
        if let Some(value) = uop_hash {
            query.push(("uopHash", value));
        }

        let data = client
            .get_authed(
                "/priapi/v5/wallet/agentic/order/detail",
                &access_token,
                &query,
            )
            .await
            .map_err(format_api_error)?;
        output::success(filter_detail_response(&data));
    } else {
        let mut query = vec![("accountId", resolved_account_id.as_str())];
        if let Some(value) = begin {
            query.push(("begin", value));
        }
        if let Some(value) = end {
            query.push(("end", value));
        }
        if let Some(value) = cursor {
            query.push(("cursor", value));
        }
        if let Some(value) = limit {
            query.push(("limit", value));
        }
        if !chain_index.is_empty() {
            query.push(("chainIndex", chain_index.as_str()));
        }

        let data = client
            .get_authed(
                "/priapi/v5/wallet/agentic/order/list",
                &access_token,
                &query,
            )
            .await
            .map_err(format_api_error)?;
        output::success(filter_list_response(&data));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::commands::agentic_wallet::WalletCommand;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: WalletCommand,
    }

    fn history_cursor(args: &[&str]) -> Option<String> {
        let cli = TestCli::try_parse_from(args).expect("history command should parse");
        match cli.command {
            WalletCommand::History { cursor, .. } => cursor,
            _ => panic!("expected wallet history command"),
        }
    }

    #[test]
    fn history_first_page_omits_cursor() {
        assert_eq!(
            history_cursor(&["onchainos", "history", "--limit", "20"]),
            None
        );
    }

    #[test]
    fn history_continuation_uses_returned_cursor_verbatim() {
        assert_eq!(
            history_cursor(&[
                "onchainos",
                "history",
                "--limit",
                "20",
                "--cursor",
                "next-page-token",
            ]),
            Some("next-page-token".to_string())
        );
    }

    #[test]
    fn history_rejects_legacy_page_num_flag() {
        assert!(TestCli::try_parse_from(["onchainos", "history", "--page-num", "1",]).is_err());
    }
}
