//! Confirm-accept + Fund.
//!
//! User actions:
//! - `set-payment-mode`: set the payment mode (standalone command; single-signature on-chain → wait for `job_payment_mode_changed`).
//! - `confirm-accept`: confirm acceptance of the provider (run after `setPaymentMode`).
//!    - escrow: providerConfirmStatus → sign_escrow → accept → broadcast.
//!    - x402: do NOT use this command (use `task-402-pay` instead).
//! - `direct-accept`: x402 phase 2b.
//! - `task-402-pay`: x402 phase 2 (signing + direct/accept + endpoint replay).

use anyhow::{bail, Context, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::util::{
    json_str, json_u64, fetch_token_detail,
    resolve_x402_params,
};
use crate::commands::agent_commerce::task::common::{
    self, PaymentMode, XLAYER_CHAIN_ID, DEBUG_LOG,
};
use crate::commands::agent_commerce::task::signing;
use crate::commands::payment::a2a_pay;
use super::negotiate;

/// Fetch token info for amount validation (best-effort: a lookup failure does not block the main flow).
async fn resolve_token_for_validation(
    client: &mut TaskApiClient,
    symbol: &str,
    agent_id: &str,
) -> Result<(String, String, u8)> {
    let (token_address, decimals) = fetch_token_detail(client, symbol, agent_id).await?;
    let decimals_u8 = u8::try_from(decimals)
        .map_err(|_| anyhow::anyhow!("decimals {decimals} is out of u8 range"))?;
    Ok((symbol.to_string(), token_address, decimals_u8))
}

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
    client.get_with_agent_id(&path, agent_id).await
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
    endpoint: Option<&str>,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;

    // Pre-check: only `open` status allows setting the payment mode (reuse `task_resp` to avoid duplicate requests later).
    let task_resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;
    let task_status = common::state_machine::Status::from_int(
        task_resp["status"].as_i64().unwrap_or(-1) as i32,
    );
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
                if DEBUG_LOG { eprintln!("⚠ task paymentMode={current_int}; cannot recognize the payment mode, defaulting to escrow"); }
                PaymentMode::Escrow
            } else {
                if DEBUG_LOG { eprintln!("ℹ --payment-mode not provided; using task detail's paymentMode: {} ({current_int})", mode.as_str()); }
                mode
            }
        }
    };

    // Check whether the current paymentMode is already the target (only when explicitly provided).
    let current_mode = PaymentMode::from_int(
        task_resp["paymentMode"].as_i64().unwrap_or(0) as i32,
    );
    let already_set = explicitly_provided
        && current_mode == payment_mode
        && current_mode != PaymentMode::None;

    // x402: resolve service parameters + balance pre-check.
    let x402_resolved = if payment_mode == PaymentMode::X402 {
        let resolved = resolve_x402_params(job_id, None, endpoint, token_symbol, token_amount).await?;
        if resolved.fee_amount > 0.0 && !resolved.fee_token_symbol.is_empty() {
            common::ensure_sufficient_balance(resolved.fee_amount, &resolved.fee_token_symbol).await?;
        }
        Some(resolved)
    } else {
        // Balance pre-check.
        let (sym, amt_str) = resolve_symbol_and_amount(token_symbol, token_amount, "set-payment-mode")?;
        let amt: f64 = amt_str.parse().unwrap_or(0.0);
        if amt > 0.0 {
            common::ensure_sufficient_balance(amt, &sym).await?;
        }
        None
    };

    // If paymentMode is already the target, skip the on-chain call (the chain would not emit `job_payment_mode_changed`).
    if !already_set {
        let mode_int = payment_mode.as_int();
        let resp = client.post_with_identity(
            &client.endpoint(job_id, "setPaymentMode"),
            &serde_json::json!({ "paymentMode": mode_int }),
            &agent_id,
        ).await?;

        let tx_hash = signing::sign_uop_and_broadcast(
            client, &resp["uopData"], &account_id, &address,
            job_id, signing::extract_biz_type(&resp), &agent_id,
            None,
        ).await?;

        audit::log(
            "cli",
            "buyer/payment_mode_set",
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
            "buyer/payment_mode_already_set",
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

    if let Some(resolved) = x402_resolved {
        if already_set {
            println!("✓ Payment mode is already x402; proceeding to payment.");
            crate::output::success(serde_json::json!({
                "alreadySet": true,
                "paymentMode": "x402",
                "endpoint": resolved.endpoint,
                "feeAmount": resolved.fee_amount.to_string(),
                "feeTokenSymbol": resolved.fee_token_symbol,
                "next": "Run task-402-pay directly (x402_pay signing + direct/accept + endpoint replay).",
            }));
        } else {
            println!("✓ Payment mode set to x402; awaiting on-chain confirmation...");
            crate::output::confirming(
                &format!(
                    "x402 setPaymentMode complete. endpoint={}, fee={} {}",
                    resolved.endpoint, resolved.fee_amount, resolved.fee_token_symbol,
                ),
                "Wait for the on-chain confirmation, then the system will proceed with x402 payment automatically.",
            );
        }
    } else {
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
    }
    Ok(())
}

/// confirm-accept — confirm acceptance of the provider.
///
/// All parameters (provider, token symbol, amount) are read from the task detail API.
pub async fn handle_confirm_accept(
    client: &mut TaskApiClient,
    job_id: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;

    let task_resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;

    let provider = task_resp["providerAgentId"].as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("task {job_id} has no providerAgentId; cannot confirm-accept"))?
        .to_string();
    let token_symbol = task_resp["tokenSymbol"].as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("task {job_id} has no tokenSymbol"))?
        .to_string();
    let token_amount = task_resp["tokenAmount"].as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("task {job_id} has no tokenAmount"))?
        .to_string();

    let payment_mode = PaymentMode::from_int(task_resp["paymentMode"].as_i64().unwrap_or(0) as i32);
    if payment_mode == PaymentMode::None {
        bail!(
            "task has no payment mode set yet (paymentMode=0); first run:\n  \
             onchainos agent set-payment-mode {job_id} --payment-mode <escrow|x402> --token-symbol <sym> --token-amount <amt>\n\
             then wait for the job_payment_mode_changed system notification and re-run confirm-accept"
        );
    }

    if payment_mode == PaymentMode::X402 {
        bail!("for the x402 flow, use `onchainos agent set-payment-mode` to set the payment mode, then `onchainos agent task-402-pay` for phase 2");
    }

    if payment_mode != PaymentMode::Escrow {
        bail!("confirm-accept only supports the escrow payment mode; current paymentMode={}. For x402, use task-402-pay.", payment_mode.as_str());
    }

    let amt: f64 = token_amount.parse().unwrap_or(0.0);
    if amt > 0.0 {
        common::ensure_sufficient_balance(amt, &token_symbol).await?;
    }

    if DEBUG_LOG { eprintln!("[debug] final payment_mode: '{}'", payment_mode.as_str()); }
    confirm_accept_escrow(
        client, job_id, &provider, Some(&token_symbol), Some(&token_amount),
        &account_id, &address, &agent_id,
    ).await?;

    if let Err(e) = negotiate::cleanup(job_id) {
        if DEBUG_LOG { eprintln!("⚠ failed to clean up negotiation state (safe to ignore): {e}"); }
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
) -> Result<()> {
    let (symbol, amount) = resolve_symbol_and_amount(token_symbol, token_amount, "escrow")?;

    // providerConfirmStatus confirms the provider has applied and returns the escrow parameters.
    let confirm_resp = fetch_provider_confirm_status(
        client, job_id, provider, &symbol, &amount, agent_id,
    ).await?;
    let amount_minimal = confirm_resp["amount"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("providerConfirmStatus response missing `amount`"))?
        .to_string();
    let currency = confirm_resp["currency"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("providerConfirmStatus response missing `currency`"))?
        .to_string();

    // Validate `currency` matches the task's tokenAddress.
    let task_resp = client.get_with_identity(&client.task_path(job_id), agent_id).await?;
    let task_token_address = task_resp["tokenAddress"]
        .as_str()
        .unwrap_or("")
        .to_lowercase();
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
            .ok_or_else(|| anyhow::anyhow!("expiredAt unix timestamp is invalid: {expired_at_raw}"))?
            .to_rfc3339()
    } else {
        expired_at_raw
    };
    let hook = json_str(escrow, "hook")?;
    let hook_data = json_str(escrow, "hookData")?;
    let salt = json_str(escrow, "salt")?;
    println!("✓ providerConfirmStatus: provider has applied; escrow parameters fetched.");

    // sign_escrow — TEE signs the EIP-3009 ReceiveWithAuthorization.
    if DEBUG_LOG {
        eprintln!("[debug] sign_escrow inputs:");
        eprintln!("  chain_id: {XLAYER_CHAIN_ID}, provider: {provider_addr}, receiver: {receiver}");
        eprintln!("  arbitrator: {arbitrator}, currency: {currency}, escrow_contract: {escrow_contract}");
        eprintln!("  amount: {amount_minimal}, submit_window: {submit_window}, dispute_window: {dispute_window}");
        eprintln!("  arbitration_window: {arbitration_window}, termination_window: {termination_window}");
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
    }).await?;
    if DEBUG_LOG {
        eprintln!("[debug] sign_escrow returned: signature={}, validAfter={}, validBefore={}",
            sign_output.signature, sign_output.authorization.valid_after, sign_output.authorization.valid_before);
    }
    println!("✓ escrow payment signing complete.");

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
    let resp = client.post_with_identity(
        &client.endpoint(job_id, "accept"),
        &body,
        agent_id,
    ).await?;

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
    if DEBUG_LOG { eprintln!("[debug] paymentVerify: {}", serde_json::to_string_pretty(&payment_verify).unwrap_or_default()); }

    let tx_hash = signing::sign_uop_and_broadcast_with_payment(
        client, &resp["uopData"], account_id, address,
        job_id, signing::extract_biz_type(&resp), agent_id,
        payment_verify,
    ).await?;
    audit::log(
        "cli",
        "buyer/confirm_accept_completed",
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
    println!("✓ Provider {provider} accepted (escrow); funds are now in escrow.");
    println!("  txHash: {tx_hash}");
    Ok(())
}

/// direct-accept — x402 phase 2b: after receiving `job_payment_mode_changed`, the agent completes the
/// x402 endpoint interaction and then calls this command to run `direct/accept` on-chain.
pub async fn handle_direct_accept(
    client: &mut TaskApiClient,
    job_id: &str,
    provider: &str,
    token_symbol: Option<&str>,
    token_amount: Option<&str>,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;

    let body = serde_json::json!({
        "providerAgentId": provider,
        "tokenSymbol": token_symbol.unwrap_or(""),
        "tokenAmount": token_amount.unwrap_or(""),
    });
    if DEBUG_LOG { eprintln!("[debug] direct-accept inputs: {}", serde_json::to_string_pretty(&body).unwrap_or_default()); }

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "direct/accept"),
        &body,
        &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), &agent_id,
        None,
    ).await?;
    audit::log(
        "cli",
        "buyer/direct_accept_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("provider={provider}"),
            format!("paymentMode=x402"),
            format!("tokenSymbol={}", token_symbol.unwrap_or("")),
            format!("tokenAmount={}", token_amount.unwrap_or("")),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );
    println!("✓ x402 acceptance complete; task status → accepted.");
    println!("  txHash: {tx_hash}");
    println!("  Wait for the on-chain confirmation before proceeding.");

    if let Err(e) = negotiate::cleanup(job_id) {
        if DEBUG_LOG { eprintln!("⚠ failed to clean up negotiation state (safe to ignore): {e}"); }
    }
    Ok(())
}

/// task-402-pay — x402 phase 2: signing + direct/accept + endpoint replay.
#[allow(clippy::too_many_arguments)]
pub async fn handle_task_402_pay(
    client: &mut TaskApiClient,
    job_id: &str,
    provider: &str,
    accepts: &str,
    endpoint: &str,
    token_symbol: &str,
    token_amount: &str,
    from: Option<&str>,
    business_body: Option<&str>,
) -> Result<()> {
    use crate::commands::payment::payment_flow;
    use super::x402_flow;

    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;

    // Step 0: amount validation — the amount in `402 accepts` must match the business-negotiated amount.
    let accepts_vec: Vec<serde_json::Value> = serde_json::from_str(accepts)
        .map_err(|e| anyhow::anyhow!("accepts JSON parse failed: {e}"))?;
    let pricing = x402_flow::extract_x402_pricing(&accepts_vec)?;

    let (_, token_address, decimals) = match resolve_token_for_validation(client, token_symbol, &agent_id).await {
        Ok(v) => v,
        Err(e) => {
            if DEBUG_LOG { eprintln!("[task-402-pay] ⚠ token-info lookup failed; skipping amount validation: {e}"); }
            (String::new(), String::new(), 0u8)
        }
    };
    if decimals > 0 {
        if !token_address.is_empty()
            && !pricing.asset.is_empty()
            && token_address.to_lowercase() != pricing.asset.to_lowercase()
        {
            bail!(
                "x402 token mismatch: 402 returned asset={}, expected tokenAddress={} ({})",
                pricing.asset, token_address, token_symbol
            );
        }
        if !x402_flow::amounts_match(&pricing.amount_minimal, token_amount, decimals) {
            let expected_minimal = x402_flow::human_to_minimal(token_amount, decimals).unwrap_or_else(|_| "?".to_string());
            bail!(
                "x402 amount mismatch: 402 returned {} (minimal units), expected {} {} ≈ {} (minimal units)",
                pricing.amount_minimal, token_amount, token_symbol, expected_minimal
            );
        }
        if DEBUG_LOG { eprintln!("[task-402-pay] ✓ amount validation passed: {} {} ≈ {} (minimal units)", token_amount, token_symbol, pricing.amount_minimal); }
    }

    // Step 1: x402_pay signing.
    if DEBUG_LOG {
        eprintln!("[task-402-pay] Step 1: x402_pay signing");
        eprintln!("[task-402-pay] accepts: {accepts}");
    }
    let proof = payment_flow::x402_pay_from_accepts(accepts, from.map(|s| s.to_string())).await?;
    let (proof_signature, proof_authorization, proof_session_cert) = match proof {
        payment_flow::PaymentProof::Eip3009 {
            signature,
            authorization,
            session_cert,
        } => (signature, authorization, session_cert),
        // TODO: support Permit2/Upto — replace x402_flow::assemble_payment_header with payment_flow::build_payment_header, pass (proof, entry) through
        payment_flow::PaymentProof::Permit2 { .. } | payment_flow::PaymentProof::Upto { .. } => {
            bail!(
                "task-402-pay only supports the EIP-3009 (exact / aggr_deferred) x402 schemes; \
                 got a Permit2/upto proof from x402_pay_from_accepts"
            );
        }
    };
    if DEBUG_LOG { eprintln!("[task-402-pay] x402_pay complete: signature={proof_signature}"); }

    // Step 2: direct/accept on-chain (tolerant: if already accepted, skip).
    if DEBUG_LOG { eprintln!("[task-402-pay] Step 2: direct/accept on-chain"); }

    let body = serde_json::json!({
        "providerAgentId": provider,
        "tokenSymbol": token_symbol,
        "tokenAmount": token_amount,
    });
    let accept_result: Result<String> = async {
        let resp = client.post_with_identity(
            &client.endpoint(job_id, "direct/accept"),
            &body,
            &agent_id,
        ).await?;
        let hash = signing::sign_uop_and_broadcast(
            client, &resp["uopData"], &account_id, &address,
            job_id, signing::extract_biz_type(&resp), &agent_id,
            None,
        ).await?;
        Ok(hash)
    }.await;

    let tx_hash = match accept_result {
        Ok(hash) => {
            if DEBUG_LOG { eprintln!("[task-402-pay] direct/accept broadcast complete: txHash={hash}"); }
            hash
        }
        Err(e) => {
            if DEBUG_LOG { eprintln!("[task-402-pay] direct/accept failed (possibly already accepted); skipping to replay: {e}"); }
            String::new()
        }
    };

    // Step 3: build payment header from the already-signed accepts[], then replay.
    // No re-fetch — avoids the double-GET inconsistency and supports body-required endpoints.
    if DEBUG_LOG { eprintln!("[task-402-pay] Step 3: assemble payment header from signed accepts[] → replay endpoint"); }
    let x402_payload = match x402_flow::payload_from_accepts(accepts) {
        Ok(p) => p,
        Err(e) => {
            if DEBUG_LOG { eprintln!("[task-402-pay] failed to build x402 payload from accepts: {e}"); }
            crate::output::success(serde_json::json!({
                "replaySuccess": false,
                "replayStatus": 0,
                "replayBody": { "error": format!("failed to build x402 payload from accepts: {e}") },
                "signature": proof_signature,
                "authorization": proof_authorization,
                "sessionCert": proof_session_cert,
                "txHash": tx_hash,
                "endpoint": endpoint,
                "retryHint": "Signing and direct/accept are done; you may retry with corrected --accepts.",
            }));
            return Ok(());
        }
    };

    let x402_proof = x402_flow::X402PaymentProof {
        signature: proof_signature.clone(),
        authorization: serde_json::to_value(&proof_authorization)
            .unwrap_or(serde_json::Value::Null),
        session_cert: proof_session_cert.clone(),
    };
    let (header_name, header_value) = match x402_flow::assemble_payment_header(&x402_proof, &x402_payload) {
        Ok(hv) => hv,
        Err(e) => {
            if DEBUG_LOG { eprintln!("[task-402-pay] failed to assemble payment header: {e}"); }
            crate::output::success(serde_json::json!({
                "replaySuccess": false,
                "replayStatus": 0,
                "replayBody": { "error": format!("failed to assemble payment header: {e}") },
                "signature": proof_signature,
                "authorization": proof_authorization,
                "sessionCert": proof_session_cert,
                "txHash": tx_hash,
                "endpoint": endpoint,
                "retryHint": "Signing and direct/accept are done; you may retry with corrected --accepts.",
            }));
            return Ok(());
        }
    };

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client")?;

    let has_body = business_body.filter(|s| !s.is_empty()).is_some();
    if DEBUG_LOG {
        eprintln!("[task-402-pay] replaying endpoint ({header_name}: ...) method={}",
            if has_body { "POST" } else { "GET" });
    }

    let replay_resp = if let Some(biz) = business_body.filter(|s| !s.is_empty()) {
        http.post(endpoint)
            .header(&header_name, &header_value)
            .header("content-type", "application/json")
            .body(biz.to_string())
            .send()
            .await
    } else {
        http.get(endpoint)
            .header(&header_name, &header_value)
            .send()
            .await
    };

    let (replay_success, replay_status, replay_body) = match replay_resp {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let raw_text = resp.text().await.unwrap_or_default();
            let body: serde_json::Value = serde_json::from_str(&raw_text)
                .unwrap_or_else(|_| serde_json::json!({ "raw": raw_text }));
            let success = (200..300).contains(&status);
            if DEBUG_LOG { eprintln!("[task-402-pay] replay result: HTTP {status}, success={success}"); }
            (success, status, body)
        }
        Err(e) => {
            if DEBUG_LOG { eprintln!("[task-402-pay] replay request failed: {e}"); }
            (false, 0u16, serde_json::json!({ "error": e.to_string() }))
        }
    };

    // Step 4: auto-save deliverable when replay succeeded.
    let mut saved_path: Option<String> = None;
    if replay_success {
        match auto_save_x402_deliverable(client, job_id, &agent_id, provider, token_symbol, token_amount, &replay_body).await {
            Ok(p) => {
                if DEBUG_LOG { eprintln!("[task-402-pay] deliverable auto-saved: {p}"); }
                saved_path = Some(p);
            }
            Err(e) => { if DEBUG_LOG { eprintln!("[task-402-pay] deliverable auto-save failed (non-blocking): {e}"); } }
        }
    }

    // Step 5: emit the complete result.
    audit::log(
        "cli",
        if replay_success { "buyer/task_402_pay_completed" } else { "buyer/task_402_pay_replay_failed" },
        replay_success,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("provider={provider}"),
            format!("tokenSymbol={token_symbol}"),
            format!("tokenAmount={token_amount}"),
            format!("replayStatus={replay_status}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );
    let mut result = serde_json::json!({
        "replaySuccess": replay_success,
        "replayStatus": replay_status,
        "replayBody": replay_body,
        "replayBodyDisplay": format_replay_body_display(&replay_body),
        "signature": proof_signature,
        "authorization": proof_authorization,
        "sessionCert": proof_session_cert,
        "txHash": tx_hash,
    });
    if let Some(p) = saved_path {
        result["deliverableSavedPath"] = serde_json::Value::String(p);
    }
    crate::output::success(result);

    if let Err(e) = negotiate::cleanup(job_id) {
        if DEBUG_LOG { eprintln!("⚠ failed to clean up negotiation state (safe to ignore): {e}"); }
    }
    Ok(())
}

fn format_replay_body_display(replay_body: &serde_json::Value) -> String {
    if let Some(raw) = replay_body.get("raw").and_then(|v| v.as_str()) {
        raw.to_string()
    } else if replay_body.is_string() {
        replay_body.as_str().unwrap_or_default().to_string()
    } else {
        serde_json::to_string_pretty(replay_body).unwrap_or_else(|_| replay_body.to_string())
    }
}

/// Auto-save the x402 replay result as a deliverable (best-effort).
///
/// Fetches task context from the API to get title/short_id, writes replayBody
/// to a temp file, then calls `deliverables::handle_save`. Returns the saved path on success.
async fn auto_save_x402_deliverable(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
    provider: &str,
    token_symbol: &str,
    token_amount: &str,
    replay_body: &serde_json::Value,
) -> Result<String> {
    use crate::commands::agent_commerce::task::common::deliverables;

    let resp = client.get_with_identity(&client.task_path(job_id), agent_id).await?;
    let title = resp["title"].as_str().unwrap_or("x402 deliverable").to_string();
    let short_id = if job_id.len() >= 8 { &job_id[..8] } else { job_id }.to_string();
    let provider_name = resp["providerName"].as_str().map(|s| s.to_string());

    let display = format_replay_body_display(replay_body);
    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join(format!("x402-deliverable-{job_id}.txt"));
    std::fs::write(&tmp_path, &display)?;

    let params = deliverables::SaveParams {
        job_id,
        role: "buyer",
        file_path: tmp_path.to_str().unwrap_or_default(),
        deliverable_type: "text",
        title: &title,
        short_id: &short_id,
        file_key: None,
        token_symbol: Some(token_symbol),
        token_amount: Some(token_amount),
        counterparty_agent_id: Some(provider),
        counterparty_name: provider_name.as_deref(),
    };
    let result = deliverables::handle_save(&params)?;
    Ok(result.path)
}

/// x402-check — validate whether the endpoint is a legitimate x402 service and extract pricing info.
pub async fn handle_x402_check(client: &mut TaskApiClient, endpoint: &str, agent_id: Option<&str>, body: Option<&str>) -> Result<()> {
    use super::x402_flow;

    let check = x402_flow::check_x402_endpoint(endpoint, body).await?;

    if !check.valid {
        if let Some(ref ir) = check.input_required {
            crate::output::success(serde_json::json!({
                "valid": false,
                "inputRequired": true,
                "statusCode": check.status_code,
                "message": ir.message,
                "requiredAnyOf": ir.required_any_of,
                "fields": ir.fields,
            }));
            return Ok(());
        }
        crate::output::success(serde_json::json!({
            "valid": false,
            "statusCode": check.status_code,
            "reason": if check.status_code == 402 {
                "The 402 response's `accepts` is empty; not a valid x402 service.".to_string()
            } else {
                format!("Endpoint returned HTTP {} (not 402); not a valid x402 service.", check.status_code)
            },
        }));
        return Ok(());
    }

    let pricing = check.pricing.as_ref().unwrap();

    let aid = match agent_id {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => super::create::resolve_buyer_agent()
            .await
            .map(|(id, _)| id)
            .unwrap_or_default(),
    };
    let resolved = x402_flow::enrich_pricing(client, pricing, &aid).await;

    let mut data = serde_json::json!({
        "valid": true,
        "amountMinimal": pricing.amount_minimal,
        "asset": pricing.asset,
        "payTo": pricing.pay_to,
        "network": pricing.network,
        "scheme": pricing.scheme,
        "acceptsJson": check.accepts_json,
        "x402Version": check.x402_version,
    });

    match resolved {
        Ok(r) => {
            data["amountHuman"] = serde_json::json!(r.amount_human);
            data["tokenSymbol"] = serde_json::json!(r.token_symbol);
            data["decimals"] = serde_json::json!(r.decimals);
        }
        Err(e) => {
            if DEBUG_LOG { eprintln!("⚠ token resolution failed (does not affect validity): {e}"); }
            data["tokenResolveError"] = serde_json::json!(e.to_string());
        }
    }

    crate::output::success(data);
    Ok(())
}
