//! Executes BRC-20 transfer-inscription creation and status queries.

use anyhow::{bail, Result};
use serde_json::{json, Value};

use crate::commands::agentic_wallet::common::WalletPreviewConfirming;
use crate::commands::agentic_wallet::support::amount::readable_to_minimal;
use crate::commands::agentic_wallet::support::json::{find_string, shell_arg};

use super::super::chain_adapters::bitcoin::{
    api::{self, BtcApi},
    broadcast,
    context::BtcContext,
    error,
    models::{next_steps, ReadOnlyNextStep},
    signing, validation,
};

#[allow(clippy::too_many_arguments)]
/// Executes the BRC-20 inscription flow and emits confirmation or Reveal broadcast output.
pub async fn cmd_create(
    token_address: &str,
    readable_amount: &str,
    from: Option<&str>,
    operation_token: Option<&str>,
    fee_rate: Option<&str>,
    force: bool,
) -> Result<()> {
    let normalized_token_address = validation::normalize_brc20_token_address(token_address)?;
    let token_address = normalized_token_address.as_str();
    let fee_rate = fee_rate.map(validation::parse_fee_rate).transpose()?;
    if force && operation_token.is_none() {
        bail!("confirmed inscription requires --operation-token");
    }
    if !force && operation_token.is_some() {
        bail!("--operation-token is only valid with --force");
    }
    if operation_token.is_some_and(|value| !validation::is_local_continuation(value)) {
        bail!("invalid Bitcoin preview continuation");
    }

    let context = BtcContext::load(from).await?;
    let mut api = BtcApi::new()?;
    let metadata = api.token_metadata(&context, token_address).await?;
    let decimals = api::extract_token_decimals(&metadata)?;
    let amount = readable_to_minimal(readable_amount, decimals)?;
    let unavailable = api
        .availability_details(&context, "UNAVAILABLE_BREAKDOWN")
        .await?;
    let prepared = api
        .prepare_transaction(
            &context,
            &context.address.address,
            &amount,
            Some(token_address),
            Some("brc20Inscribe"),
            fee_rate.as_ref(),
        )
        .await
        .map_err(error::map_api_error)?;

    let mut preview = validation::preview_from_response(
        &prepared,
        "BRC20_INSCRIBE",
        &context.profile.chain_index,
        &context.address.address,
        &context.address.address,
        Some(token_address),
        &amount,
        readable_amount,
        context.profile.native_decimals,
    )?;
    validation::validate_preview_intent(
        &preview,
        "BRC20_INSCRIBE",
        &context.profile.chain_index,
        &context.address.address,
        Some(&context.address.address),
        Some(&amount),
    )?;
    validation::bind_utxo_availability(&mut preview, unavailable)?;
    let local_token = validation::local_transaction_token(&prepared, &preview)?;
    let next = build_inscription_next_command(
        token_address,
        readable_amount,
        fee_rate.as_ref(),
        &local_token,
    );

    if force && operation_token != Some(local_token.as_str()) {
        return Err(WalletPreviewConfirming {
            message: "The BRC-20 inscription changed after the previous preview. Review the refreshed funding inputs and fees before confirming again.".to_string(),
            next,
            scene: "btc_inscription".to_string(),
            preview,
        }
        .into());
    }

    if force {
        let seed = context.signing_seed()?;
        let signatures = signing::sign_unsigned_hashes(&prepared, &seed)?;
        let broadcasts = broadcast::submit_inscription_transactions(
            &mut api,
            &context,
            &prepared,
            &signatures,
            token_address,
            &amount,
            force,
        )
        .await
        .map_err(error::map_api_error)?;
        let reveal = select_reveal_broadcast(&broadcasts);
        let reveal_tx_hash = reveal.map(|item| item.tx_hash.clone()).unwrap_or_default();
        let reveal_order_id = reveal.map(|item| item.order_id.clone()).unwrap_or_default();
        let status_next_steps = if !reveal_order_id.is_empty() {
            Some(next_steps([ReadOnlyNextStep::CheckInscriptionStatus {
                tx_hash: None,
                order_id: Some(reveal_order_id.clone()),
            }])?)
        } else if !reveal_tx_hash.is_empty() {
            Some(next_steps([ReadOnlyNextStep::CheckInscriptionStatus {
                tx_hash: Some(reveal_tx_hash.clone()),
                order_id: None,
            }])?)
        } else {
            None
        };
        let broadcasts: Vec<Value> = broadcasts
            .iter()
            .map(|broadcast| {
                json!({
                    "txHash": broadcast.tx_hash,
                    "orderId": broadcast.order_id,
                })
            })
            .collect();
        let mut result = json!({
            "message": "BRC-20 inscription submitted. Inscription is asynchronous; query it later with the returned Reveal order ID.",
            "state": "INSCRIBING",
            "accountId": context.account_id,
            "chainIndex": context.profile.chain_index,
            "from": context.address.address,
            "tokenAddress": token_address,
            "amount": amount,
            "txHash": reveal_tx_hash,
            "orderId": reveal_order_id,
            "broadcasts": broadcasts,
        });
        if let Some(status_next_steps) = status_next_steps {
            result["nextSteps"] = status_next_steps;
        }
        crate::output::success(result);
        return Ok(());
    }

    Err(WalletPreviewConfirming {
        message: format!(
            "Review BRC-20 inscription. From: {}. Ticker/token: {}. Amount: {}. This submits an asynchronous inscription only; review every funding input, output, fee, and warning before confirming.",
            context.address.address, token_address, readable_amount
        ),
        next,
        scene: "btc_inscription".to_string(),
        preview,
    }
    .into())
}

/// Returns the final batch item, which represents the Reveal transaction.
fn select_reveal_broadcast(
    broadcasts: &[crate::wallet_api::BroadcastResponse],
) -> Option<&crate::wallet_api::BroadcastResponse> {
    broadcasts.last()
}

/// Builds the confirmed inscription continuation command from the reviewed inputs.
fn build_inscription_next_command(
    token_address: &str,
    readable_amount: &str,
    fee_rate: Option<&Value>,
    operation_token: &str,
) -> String {
    let mut command = format!(
        "onchainos wallet inscription create --chain bitcoin --token-address {} --readable-amount {} --operation-token {}",
        shell_arg(token_address),
        shell_arg(readable_amount),
        shell_arg(operation_token),
    );
    if let Some(fee_rate) = fee_rate {
        command.push_str(&format!(" --fee-rate {fee_rate}"));
    }
    command.push_str(" --force");
    command
}

/// Queries and emits inscription status by Reveal transaction hash or order ID.
pub async fn cmd_query_status(tx_hash: Option<&str>, order_id: Option<&str>) -> Result<()> {
    if tx_hash.is_none() && order_id.is_none() {
        bail!("either --tx-hash or --order-id is required");
    }
    let context = BtcContext::load(None).await?;
    let mut api = BtcApi::new()?;
    let detail = api.order_detail(&context, tx_hash, order_id).await?;
    let raw_status =
        find_string(&detail, &["status", "txStatus"]).unwrap_or_else(|| "UNKNOWN".to_string());
    let status = normalize_inscription_status(&raw_status);
    let pending = matches!(
        status.as_str(),
        "INSCRIBING" | "WAITING_CONFIRMATION" | "WAITING_INDEXER"
    );
    let has_poll_schedule = find_string(&detail, &["nextQueryAt", "pollAfterSeconds"]).is_some();
    let mut result = json!({
        "message": inscription_status_message(&status, has_poll_schedule),
        "status": status,
        "txHash": tx_hash,
        "orderId": order_id,
        "detail": detail,
    });
    if pending && has_poll_schedule {
        result["nextSteps"] = next_steps([ReadOnlyNextStep::CheckInscriptionStatus {
            tx_hash: tx_hash.map(str::to_string),
            order_id: order_id.map(str::to_string),
        }])?;
    } else if status == "READY_TO_TRANSFER" {
        if let Some(token_address) =
            find_string(&result["detail"], &["tokenAddress", "contractAddr"])
        {
            result["nextSteps"] =
                next_steps([ReadOnlyNextStep::QueryBrc20TransferableUtxos { token_address }])?;
        }
    }
    crate::output::success(result);
    Ok(())
}

/// Maps service order status to the BRC-20 inscription lifecycle.
fn normalize_inscription_status(raw: &str) -> String {
    match raw.to_ascii_uppercase().as_str() {
        "1" | "2" => "INSCRIBING".to_string(),
        "3" | "6" => "FAILED".to_string(),
        "4" => "READY_TO_TRANSFER".to_string(),
        other => other.to_string(),
    }
}

/// Returns the message corresponding to one inscription lifecycle state.
fn inscription_status_message(status: &str, has_poll_schedule: bool) -> String {
    match status {
        "READY_TO_TRANSFER" => "The BRC-20 inscription is ready. Refresh the transferable balance before starting a separate transfer.".to_string(),
        "FAILED" | "UNKNOWN" => "The BRC-20 inscription is not available; review the service detail before deciding whether to create another inscription.".to_string(),
        _ if has_poll_schedule => "The BRC-20 inscription is asynchronous and is not ready to transfer yet. Query again at the service-recommended time.".to_string(),
        _ => "The BRC-20 inscription is asynchronous and is not ready to transfer yet. Query it again later with the returned transaction hash or order ID.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_broadcast(tx_hash: &str, order_id: &str) -> crate::wallet_api::BroadcastResponse {
        crate::wallet_api::BroadcastResponse {
            pkg_id: String::new(),
            order_id: order_id.to_string(),
            order_type: String::new(),
            tx_hash: tx_hash.to_string(),
        }
    }

    #[test]
    fn inscription_result_uses_reveal_broadcast() {
        let broadcasts = [
            build_broadcast("commit-hash", "commit-order"),
            build_broadcast("reveal-hash", "reveal-order"),
        ];

        let reveal = select_reveal_broadcast(&broadcasts).unwrap();

        assert_eq!(reveal.tx_hash, "reveal-hash");
        assert_eq!(reveal.order_id, "reveal-order");
    }

    #[test]
    fn inscription_status_uses_brc20_lifecycle() {
        assert_eq!(normalize_inscription_status("2"), "INSCRIBING");
        assert_eq!(normalize_inscription_status("4"), "READY_TO_TRANSFER");
        assert_eq!(normalize_inscription_status("6"), "FAILED");
    }

    #[test]
    fn inscription_continuation_preserves_custom_fee_rate() {
        let command =
            build_inscription_next_command("btc-brc20-pizza", "1", Some(&json!(1.25)), "token");

        assert!(command.contains("--fee-rate 1.25 --force"));
        assert!(!command.contains("--preview-version"));
    }
}
