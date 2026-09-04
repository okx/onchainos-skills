//! Confirm-accept + Fund.
//!
//! User actions:
//! - `set-payment-mode`: set the payment mode (standalone command; single-signature on-chain → wait for `job_payment_mode_changed`).
//! - `confirm-accept`: confirm acceptance of the provider (run after `setPaymentMode`).
//!    - escrow: providerConfirmStatus → sign_escrow → accept → broadcast.

use anyhow::{bail, Result};
use std::time::Duration;

use super::negotiate;
use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::util::{json_str, json_u64};
use crate::commands::agent_commerce::task::common::{
    self, PaymentMode, DEBUG_LOG, XLAYER_CHAIN_ID,
};
use crate::commands::agent_commerce::task::signing;
use crate::commands::payment::a2a_pay;

/// Resolve `(symbol, amount)` from CLI flags (required).
fn resolve_symbol_and_amount(
    token_symbol: Option<&str>,
    token_amount: Option<&str>,
    mode_label: &str,
) -> Result<(String, String)> {
    let symbol = token_symbol
        .ok_or_else(|| anyhow::anyhow!("{mode_label} requires --token-symbol"))?
        .to_string();
    let amount = token_amount
        .ok_or_else(|| anyhow::anyhow!("{mode_label} requires --token-amount"))?
        .to_string();
    Ok((symbol, amount))
}

/// Query whether the provider has already applied and fetch their quote (escrow parameters).
async fn fetch_provider_confirm_status(
    client: &mut TaskApiClient,
    job_id: &str,
    provider_agent_id: &str,
    token_symbol: &str,
    amount: &str,
    agent_id: &str,
) -> Result<serde_json::Value> {
    let path = format!(
        "/priapi/v1/aieco/task/{job_id}/providerConfirmStatus\
         ?providerAgentId={provider_agent_id}\
         &tokenSymbol={token_symbol}\
         &amount={amount}"
    );
    client
        .get_with_agent_id(&path, agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("providerConfirmStatus query failed: {e}"))
}

/// set-payment-mode — independently set the payment mode (split out of confirm-accept).
///
/// Unified for all payment modes: POST setPaymentMode → sign_uop → broadcast,
/// then return `confirming` (exit code 2) and wait for the `job_payment_mode_changed` system notification.
pub async fn handle_set_payment_mode(
    client: &mut TaskApiClient,
    job_id: &str,
    payment_mode: Option<&str>,
    token_symbol: Option<&str>,
    token_amount: Option<&str>,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;

    // Pre-check: only `created` status allows setting the payment mode (reuse `task_resp` to avoid duplicate requests later).
    let task_resp = client
        .get_with_identity(&client.task_path(job_id), &agent_id)
        .await?;
    let task_status =
        common::state_machine::Status::from_int(task_resp["status"].as_i64().unwrap_or(-1) as i32);
    if task_status != common::state_machine::Status::Created {
        bail!(
            "current task status is {:?}; setting the payment mode is only allowed in `created` status",
            task_status
        );
    }

    // Resolve the target payment mode (reuse `task_resp` to save the duplicate API request inside `resolve_payment_mode`).
    let explicitly_provided = payment_mode.is_some();
    let payment_mode = match payment_mode {
        Some(m) => PaymentMode::from_str(m),
        None => {
            let current_int = task_resp["paymentMode"].as_i64().unwrap_or(0) as i32;
            let mode = PaymentMode::from_int(current_int);
            if mode == PaymentMode::None {
                if DEBUG_LOG {
                    eprintln!("⚠ task paymentMode={current_int}; cannot recognize the payment mode, defaulting to escrow");
                }
                PaymentMode::Escrow
            } else {
                if DEBUG_LOG {
                    eprintln!("ℹ --payment-mode not provided; using task detail's paymentMode: {} ({current_int})", mode.as_str());
                }
                mode
            }
        }
    };

    if payment_mode == PaymentMode::X402 {
        bail!(
            "legacy task-based A2MCP/x402 payment was removed; use the invoke_a2mcp direct-invocation flow"
    );
    }

    // Check whether the current paymentMode is already the target (only when explicitly provided).
    let current_mode = PaymentMode::from_int(task_resp["paymentMode"].as_i64().unwrap_or(0) as i32);
    let already_set =
        explicitly_provided && current_mode == payment_mode && current_mode != PaymentMode::None;

    // A2A escrow balance pre-check.
    let (sym, amt_str) = resolve_symbol_and_amount(token_symbol, token_amount, "set-payment-mode")?;
    let amt: f64 = amt_str.parse().unwrap_or(0.0);
    if amt > 0.0 {
        if let Err(e) = common::ensure_sufficient_balance(amt, &sym).await {
            return print_payment_funding_block_from_error(e, &agent_id, "Payment mode update")
                .await;
        }
    }

    // If paymentMode is already the target, skip the on-chain call (the chain would not emit `job_payment_mode_changed`).
    if !already_set {
        let mode_int = payment_mode.as_int();
        let resp = client
            .post_with_identity(
                &client.endpoint(job_id, "setPaymentMode"),
                &serde_json::json!({ "paymentMode": mode_int }),
                &agent_id,
            )
            .await?;

        let tx_hash = signing::sign_uop_and_broadcast(
            client,
            &resp["uopData"],
            &account_id,
            &address,
            job_id,
            signing::extract_biz_type(&resp),
            &agent_id,
            None,
        )
        .await?;

        audit::log(
            "cli",
            "user/payment_mode_set",
            true,
            Duration::default(),
            Some(vec![
                format!("jobId={job_id}"),
                format!("agentId={agent_id}"),
                format!("paymentMode={}", payment_mode.as_str()),
                format!("txHash={tx_hash}"),
            ]),
            None,
        );
    } else {
        audit::log(
            "cli",
            "user/payment_mode_already_set",
            true,
            Duration::default(),
            Some(vec![
                format!("jobId={job_id}"),
                format!("agentId={agent_id}"),
                format!("paymentMode={}", payment_mode.as_str()),
            ]),
            None,
        );
    }

    let mode_str = payment_mode.as_str();
    if already_set {
        println!("✓ Payment mode is already {mode_str}; skipping on-chain call.");
        crate::output::success(serde_json::json!({
            "alreadySet": true,
            "paymentMode": mode_str,
            "next": "Payment mode already on-chain. Call next-action with `event=job_payment_mode_changed` in --message to get the script; then wait for the provider to submit their apply on-chain before confirm-accept.",
        }));
    } else {
        println!("✓ Payment mode set to {mode_str}; awaiting on-chain confirmation...");
        crate::output::confirming(
            &format!("setPaymentMode({mode_str}) complete."),
            "Wait for the on-chain confirmation, then the system will proceed automatically.",
        );
    }
    Ok(())
}

/// confirm-accept — confirm acceptance of the provider.
///
/// When `prefetched` is provided (called from the `provider_applied` event handler),
/// wallet/task fields are read directly from the pre-fetched context, avoiding 3
/// redundant GET /task/{jobId} calls.  When `None` (CLI subcommand), the original
/// API-call path is used.
pub async fn handle_confirm_accept(
    client: &mut TaskApiClient,
    job_id: &str,
    prefetched: Option<&common::PreFetchedTaskContext>,
) -> Result<()> {
    let (
        account_id,
        address,
        agent_id,
        provider,
        token_symbol,
        token_amount,
        payment_mode,
        token_address,
    ) = if let Some(p) = prefetched {
        let user_addr = p
            .user_agent_address
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("prefetched missing buyerAgentAddress"))?;
        let agent_id = p
            .user_agent_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("prefetched missing buyerAgentId"))?
            .to_string();
        let (acct, addr) = signing::resolve_wallet(None, Some(user_addr))?;
        let prov = p
            .provider_agent_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("task {job_id} has no providerAgentId; cannot confirm-accept")
            })?
            .to_string();
        let sym = if p.token_symbol == "?" || p.token_symbol.is_empty() {
            bail!("task {job_id} has no tokenSymbol");
        } else {
            p.token_symbol.clone()
        };
        let amt = if p.token_amount.is_empty() {
            bail!("task {job_id} has no tokenAmount");
        } else {
            p.token_amount.clone()
        };
        let pm = PaymentMode::from_int(p.payment_mode.unwrap_or(0) as i32);
        let ta = p.token_address.clone();
        (acct, addr, agent_id, prov, sym, amt, pm, ta)
    } else {
        let (acct, addr, aid) =
            signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;
        let task_resp = client
            .get_with_identity(&client.task_path(job_id), &aid)
            .await?;
        let prov = task_resp["providerAgentId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("task {job_id} has no providerAgentId; cannot confirm-accept")
            })?
            .to_string();
        let sym = task_resp["tokenSymbol"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("task {job_id} has no tokenSymbol"))?
            .to_string();
        let amt = task_resp["tokenAmount"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("task {job_id} has no tokenAmount"))?
            .to_string();
        let pm = PaymentMode::from_int(task_resp["paymentMode"].as_i64().unwrap_or(0) as i32);
        let ta = task_resp["tokenAddress"].as_str().map(String::from);
        (acct, addr, aid, prov, sym, amt, pm, ta)
    };

    if payment_mode == PaymentMode::None {
        bail!(
            "task has no payment mode set yet (paymentMode=0); first run:\n  \
             onchainos agent set-payment-mode {job_id} --payment-mode escrow --token-symbol <sym> --token-amount <amt>\n\
             then wait for the job_payment_mode_changed system notification and re-run confirm-accept"
        );
    }

    if payment_mode != PaymentMode::Escrow {
        bail!(
            "confirm-accept only supports A2A escrow; legacy task-based A2MCP/x402 payment was removed"
        );
    }

    let amt: f64 = token_amount.parse().unwrap_or(0.0);
    if amt > 0.0 {
        if let Err(e) = common::ensure_sufficient_balance(amt, &token_symbol).await {
            return print_payment_funding_block_from_error(e, &agent_id, "Task payment").await;
        }
    }

    if DEBUG_LOG {
        eprintln!("[debug] final payment_mode: '{}'", payment_mode.as_str());
    }
    confirm_accept_escrow(
        client,
        job_id,
        &provider,
        Some(&token_symbol),
        Some(&token_amount),
        &account_id,
        &address,
        &agent_id,
        token_address.as_deref(),
    )
    .await?;

    if let Err(e) = negotiate::cleanup(job_id) {
        if DEBUG_LOG {
            eprintln!("⚠ failed to clean up negotiation state (safe to ignore): {e}");
        }
    }
    Ok(())
}

/// escrow path: providerConfirmStatus → sign_escrow → accept → broadcast.
#[allow(clippy::too_many_arguments)]
async fn confirm_accept_escrow(
    client: &mut TaskApiClient,
    job_id: &str,
    provider: &str,
    token_symbol: Option<&str>,
    token_amount: Option<&str>,
    account_id: &str,
    address: &str,
    agent_id: &str,
    prefetched_token_address: Option<&str>,
) -> Result<()> {
    let (symbol, amount) = resolve_symbol_and_amount(token_symbol, token_amount, "escrow")?;

    // providerConfirmStatus confirms the provider has applied and returns the escrow parameters.
    let confirm_resp =
        fetch_provider_confirm_status(client, job_id, provider, &symbol, &amount, agent_id).await?;
    let amount_minimal = confirm_resp["amount"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("providerConfirmStatus response missing `amount`"))?
        .to_string();
    let currency = confirm_resp["currency"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("providerConfirmStatus response missing `currency`"))?
        .to_string();

    // Validate `currency` matches the task's tokenAddress.
    let task_token_address = if let Some(ta) = prefetched_token_address {
        ta.to_lowercase()
    } else {
        let task_resp = client
            .get_with_identity(&client.task_path(job_id), agent_id)
            .await?;
        task_resp["tokenAddress"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
    };
    if !task_token_address.is_empty() && currency.to_lowercase() != task_token_address {
        bail!(
            "token mismatch: providerConfirmStatus returned currency={currency} but task tokenAddress={task_token_address}. \
             Please check that the negotiated token matches the task's published token (--token-symbol)."
        );
    }

    // Parse the escrow parameters.
    let escrow = &confirm_resp["escrow"];
    let escrow_contract = json_str(escrow, "escrowContract")?;
    let provider_addr = json_str(escrow, "provider")?;
    let arbitrator = json_str(escrow, "arbitrator")?;
    let receiver = json_str(escrow, "receiver")?;
    let submit_window = json_u64(escrow, "submitWindow")?;
    let dispute_window = json_u64(escrow, "disputeWindow")?;
    let arbitration_window = json_u64(escrow, "arbitrationWindow")?;
    let termination_window = json_u64(escrow, "terminationWindow")?;
    let expired_at_raw = json_str(escrow, "expiredAt")?;
    let expired_at = if let Ok(ts) = expired_at_raw.parse::<i64>() {
        chrono::DateTime::from_timestamp(ts, 0)
            .ok_or_else(|| {
                anyhow::anyhow!("expiredAt unix timestamp is invalid: {expired_at_raw}")
            })?
            .to_rfc3339()
    } else {
        expired_at_raw
    };
    let hook = json_str(escrow, "hook")?;
    let hook_data = json_str(escrow, "hookData")?;
    let salt = json_str(escrow, "salt")?;

    // sign_escrow — TEE signs the EIP-3009 ReceiveWithAuthorization.
    if DEBUG_LOG {
        eprintln!("[debug] sign_escrow inputs:");
        eprintln!("  chain_id: {XLAYER_CHAIN_ID}, provider: {provider_addr}, receiver: {receiver}");
        eprintln!(
            "  arbitrator: {arbitrator}, currency: {currency}, escrow_contract: {escrow_contract}"
        );
        eprintln!("  amount: {amount_minimal}, submit_window: {submit_window}, dispute_window: {dispute_window}");
        eprintln!(
            "  arbitration_window: {arbitration_window}, termination_window: {termination_window}"
        );
        eprintln!("  hook: {hook}, hook_data: {hook_data}, salt: {salt}, expired_at: {expired_at}");
    }
    let sign_output = a2a_pay::sign_escrow(a2a_pay::SignEscrowParams {
        chain_id: XLAYER_CHAIN_ID as u64,
        provider: provider_addr.clone(),
        receiver: receiver.clone(),
        arbitrator,
        currency: currency.clone(),
        escrow_contract,
        amount: amount_minimal,
        submit_window,
        dispute_window,
        arbitration_window,
        termination_window,
        hook,
        hook_data,
        salt,
        expired_at,
    })
    .await?;
    if DEBUG_LOG {
        eprintln!(
            "[debug] sign_escrow returned: signature={}, validAfter={}, validBefore={}",
            sign_output.signature,
            sign_output.authorization.valid_after,
            sign_output.authorization.valid_before
        );
    }

    // accept → calldata → sign → broadcast.
    let body = serde_json::json!({
        "providerAddress": provider_addr,
        "providerAgentId": provider,
        "signatureData": {
            "signature": sign_output.signature,
            "validAfter": sign_output.authorization.valid_after,
            "validBefore": sign_output.authorization.valid_before,
        },
        "tokenSymbol": symbol,
        "tokenAmount": amount,
    });
    let resp = client
        .post_with_identity(&client.endpoint(job_id, "accept"), &body, agent_id)
        .await?;

    let payment_verify = serde_json::json!({
        "authorizationType": "receive",
        "from": sign_output.authorization.from,
        "to": sign_output.authorization.to,
        "value": sign_output.authorization.value,
        "validAfter": sign_output.authorization.valid_after,
        "validBefore": sign_output.authorization.valid_before,
        "nonce": sign_output.authorization.nonce,
        "signature": sign_output.signature,
        "tokenAddress": currency,
        "chainIndex": XLAYER_CHAIN_ID,
    });
    if DEBUG_LOG {
        eprintln!(
            "[debug] paymentVerify: {}",
            serde_json::to_string_pretty(&payment_verify).unwrap_or_default()
        );
    }

    let tx_hash = signing::sign_uop_and_broadcast_with_payment(
        client,
        &resp["uopData"],
        account_id,
        address,
        job_id,
        signing::extract_biz_type(&resp),
        agent_id,
        payment_verify,
    )
    .await?;
    audit::log(
        "cli",
        "user/confirm_accept_completed",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("provider={provider}"),
            format!("paymentMode=escrow"),
            format!("tokenSymbol={symbol}"),
            format!("tokenAmount={amount}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );
    Ok(())
}

async fn print_payment_funding_block_from_error(
    err: anyhow::Error,
    agent_id: &str,
    action: &str,
) -> Result<()> {
    match err.downcast_ref::<common::deposit_qr::InsufficientBalanceError>() {
        Some(ib) => {
            let ib_owned = ib.clone();
            let (warning, _) = common::deposit_qr::balance_warning_json(&ib_owned, agent_id).await;
            Err(crate::output::CliFundingBlocked {
                data: common::funding_notice::funding_blocked_envelope(
                    &warning,
                    "task-payment",
                    action,
                ),
            }
            .into())
        }
        None => Err(err),
    }
}
