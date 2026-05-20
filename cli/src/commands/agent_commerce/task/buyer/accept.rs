//! Confirm-accept + Fund.
//!
//! User actions:
//! - `set-payment-mode`: set the payment mode (standalone command; single-signature on-chain → wait for `job_payment_mode_changed`).
//! - `confirm-accept`: confirm acceptance of the provider (run after `setPaymentMode`).
//!    - escrow: providerConfirmStatus → sign_escrow → accept → broadcast.
//!    - x402: do NOT use this command (use `task-402-pay` instead).
//! - `direct-accept`: x402 phase 2b.
//! - `task-402-pay`: x402 phase 2 (signing + direct/accept + endpoint replay).
//!
//! API docs:    https://okg-block.sg.larksuite.com/wiki/UumqwSyM5i1AuakBNLClJo9igIb
//! Payment design: https://okg-block.sg.larksuite.com/docx/CwWbd6eCOopgq6x6VwTlWEivgrc

use anyhow::{bail, Context, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::util::{
    json_str, json_u64, fetch_token_detail,
    resolve_x402_params,
};
use crate::commands::agent_commerce::task::common::{
    self, PaymentMode, XLAYER_CHAIN_ID,
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

/// Resolve `(symbol, amount)` from CLI flags / local negotiation record.
fn resolve_symbol_and_amount(
    token_symbol: Option<&str>,
    token_amount: Option<&str>,
    job_id: &str,
    provider_agent_id: Option<&str>,
    mode_label: &str,
) -> Result<(String, String)> {
    let agreed = negotiate::load_agreed(job_id, provider_agent_id)?;
    let symbol = match token_symbol {
        Some(s) => s.to_string(),
        None => match &agreed {
            Some((sym, _)) => {
                eprintln!("ℹ --token-symbol not provided; using locally saved negotiation record: {sym}");
                sym.clone()
            }
            None => bail!("{mode_label} requires --token-symbol, or run `save-agreed` first to persist the negotiation result"),
        },
    };
    let amount = match token_amount {
        Some(a) => a.to_string(),
        None => match &agreed {
            Some((_, amt)) => {
                eprintln!("ℹ --token-amount not provided; using locally saved negotiation record: {amt}");
                amt.clone()
            }
            None => bail!("{mode_label} requires --token-amount, or run `save-agreed` first to persist the negotiation result"),
        },
    };
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
                eprintln!("⚠ task paymentMode={current_int}; cannot recognize the payment mode, defaulting to escrow");
                PaymentMode::Escrow
            } else {
                eprintln!("ℹ --payment-mode not provided; using task detail's paymentMode: {} ({current_int})", mode.as_str());
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
        let (sym, amt_str) = resolve_symbol_and_amount(token_symbol, token_amount, job_id, None, "set-payment-mode")?;
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
            println!("✓ Payment mode is already x402; skipping on-chain call, proceeding to task-402-pay.");
            crate::output::success(serde_json::json!({
                "alreadySet": true,
                "paymentMode": "x402",
                "endpoint": resolved.endpoint,
                "feeAmount": resolved.fee_amount.to_string(),
                "feeTokenSymbol": resolved.fee_token_symbol,
                "next": "Run task-402-pay directly (x402_pay signing + direct/accept + endpoint replay).",
            }));
        } else {
            let mode_int = payment_mode.as_int();
            println!("✓ Payment mode set: x402 ({mode_int}); awaiting on-chain confirmation...");
            crate::output::confirming(
                &format!(
                    "x402 setPaymentMode complete. endpoint={}, fee={} {}",
                    resolved.endpoint, resolved.fee_amount, resolved.fee_token_symbol,
                ),
                "Wait for the job_payment_mode_changed system notification → agent runs task-402-pay (x402_pay signing + direct/accept + endpoint replay).",
            );
        }
    } else {
        let mode_str = payment_mode.as_str();
        if already_set {
            println!("✓ Payment mode is already {mode_str}; skipping on-chain call.");
            crate::output::success(serde_json::json!({
                "alreadySet": true,
                "paymentMode": mode_str,
                "next": format!("Run onchainos agent confirm-accept {job_id} --payment-mode {mode_str} directly"),
            }));
        } else {
            let mode_int = payment_mode.as_int();
            println!("✓ Payment mode set: {mode_str} ({mode_int}); awaiting on-chain confirmation...");
            crate::output::confirming(
                &format!("setPaymentMode({mode_str}) complete."),
                &format!("Wait for the job_payment_mode_changed system notification → onchainos agent confirm-accept {job_id} --payment-mode {mode_str}"),
            );
        }
    }
    Ok(())
}

/// confirm-accept — confirm acceptance of the provider (setPaymentMode must already have run via set-payment-mode).
#[allow(clippy::too_many_arguments)]
pub async fn handle_confirm_accept(
    client: &mut TaskApiClient,
    job_id: &str,
    provider: &str,
    _payment_mode: Option<&str>,
    token_symbol: Option<&str>,
    token_amount: Option<&str>,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;

    // Pre-check: has setPaymentMode been confirmed on-chain?
    let task_resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;
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

    // escrow is the only legal path for confirm-accept.
    if payment_mode != PaymentMode::Escrow {
        bail!("confirm-accept only supports the escrow payment mode; current paymentMode={}. For x402, use task-402-pay.", payment_mode.as_str());
    }

    // Balance pre-check.
    let (sym, amt_str) = resolve_symbol_and_amount(token_symbol, token_amount, job_id, Some(provider), payment_mode.as_str())?;
    let amt: f64 = amt_str.parse().unwrap_or(0.0);
    if amt > 0.0 {
        common::ensure_sufficient_balance(amt, &sym).await?;
    }

    eprintln!("[debug] final payment_mode: '{}'", payment_mode.as_str());
    confirm_accept_escrow(
        client, job_id, provider, token_symbol, token_amount,
        &account_id, &address, &agent_id,
    ).await?;

    if let Err(e) = negotiate::cleanup(job_id) {
        eprintln!("⚠ failed to clean up negotiation state (safe to ignore): {e}");
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
    let (symbol, amount) = resolve_symbol_and_amount(token_symbol, token_amount, job_id, Some(provider), "escrow")?;

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
    eprintln!("[debug] sign_escrow inputs:");
    eprintln!("  chain_id: {XLAYER_CHAIN_ID}, provider: {provider_addr}, receiver: {receiver}");
    eprintln!("  arbitrator: {arbitrator}, currency: {currency}, escrow_contract: {escrow_contract}");
    eprintln!("  amount: {amount_minimal}, submit_window: {submit_window}, dispute_window: {dispute_window}");
    eprintln!("  arbitration_window: {arbitration_window}, termination_window: {termination_window}");
    eprintln!("  hook: {hook}, hook_data: {hook_data}, salt: {salt}, expired_at: {expired_at}");
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
    eprintln!("[debug] sign_escrow returned: signature={}, validAfter={}, validBefore={}",
        sign_output.signature, sign_output.authorization.valid_after, sign_output.authorization.valid_before);
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
    eprintln!("[debug] paymentVerify: {}", serde_json::to_string_pretty(&payment_verify).unwrap_or_default());

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
    eprintln!("[debug] direct-accept inputs: {}", serde_json::to_string_pretty(&body).unwrap_or_default());

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "direct/accept"),
        &body,
        &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), &agent_id,
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
    println!("✓ direct/accept complete (x402); task status → accepted.");
    println!("  txHash: {tx_hash}");
    println!("  Wait for the job_accepted system notification before running complete.");

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
            eprintln!("[task-402-pay] ⚠ token-info lookup failed; skipping amount validation: {e}");
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
        eprintln!("[task-402-pay] ✓ amount validation passed: {} {} ≈ {} (minimal units)", token_amount, token_symbol, pricing.amount_minimal);
    }

    // Step 1: x402_pay signing.
    eprintln!("[task-402-pay] Step 1: x402_pay signing");
    eprintln!("[task-402-pay] accepts: {accepts}");
    let proof = payment_flow::x402_pay_from_accepts(accepts, from.map(|s| s.to_string())).await?;
    eprintln!("[task-402-pay] x402_pay complete: signature={}", proof.signature);

    // Step 2: direct/accept on-chain (tolerant: if already accepted, skip).
    eprintln!("[task-402-pay] Step 2: direct/accept on-chain");

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
        ).await?;
        Ok(hash)
    }.await;

    let tx_hash = match accept_result {
        Ok(hash) => {
            eprintln!("[task-402-pay] direct/accept broadcast complete: txHash={hash}");
            hash
        }
        Err(e) => {
            eprintln!("[task-402-pay] direct/accept failed (possibly already accepted); skipping to replay: {e}");
            String::new()
        }
    };

    // Step 3: GET endpoint → 402 → assemble header → replay.
    eprintln!("[task-402-pay] Step 3: GET endpoint {endpoint} → fetch the full 402 payload");
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client")?;

    let initial_resp = match http.get(endpoint).send().await {
        Ok(resp) => resp,
        Err(e) => {
            eprintln!("[task-402-pay] GET endpoint failed (signing + on-chain accept already done): {e}");
            crate::output::success(serde_json::json!({
                "replaySuccess": false,
                "replayStatus": 0,
                "replayBody": { "error": format!("GET endpoint failed: {e}") },
                "signature": proof.signature,
                "authorization": proof.authorization,
                "sessionCert": proof.session_cert,
                "txHash": tx_hash,
                "endpoint": endpoint,
                "retryHint": "Signing and direct/accept are done; you may retry GET endpoint → 402 → assemble header → replay.",
            }));
            return Ok(());
        }
    };
    let initial_status = initial_resp.status().as_u16();

    if initial_status != 402 {
        let raw_text = initial_resp.text().await.unwrap_or_default();
        let body: serde_json::Value = serde_json::from_str(&raw_text)
            .unwrap_or_else(|_| serde_json::json!({ "raw": raw_text }));
        let success = (200..300).contains(&initial_status);
        eprintln!("[task-402-pay] endpoint returned HTTP {initial_status} (not 402); using as the result directly");
        crate::output::success(serde_json::json!({
            "replaySuccess": success,
            "replayStatus": initial_status,
            "replayBody": body,
            "signature": proof.signature,
            "authorization": proof.authorization,
            "sessionCert": proof.session_cert,
            "txHash": tx_hash,
        }));
        return Ok(());
    }

    let resp_headers = initial_resp.headers().clone();
    let resp_body_text = match initial_resp.text().await {
        Ok(text) => text,
        Err(e) => {
            eprintln!("[task-402-pay] failed to read 402 response body (signing + on-chain accept already done): {e}");
            crate::output::success(serde_json::json!({
                "replaySuccess": false,
                "replayStatus": 402,
                "replayBody": { "error": format!("failed to read 402 response body: {e}") },
                "signature": proof.signature,
                "authorization": proof.authorization,
                "sessionCert": proof.session_cert,
                "txHash": tx_hash,
                "endpoint": endpoint,
                "retryHint": "Signing and direct/accept are done; you may retry GET endpoint → 402 → assemble header → replay.",
            }));
            return Ok(());
        }
    };
    let x402_payload = match x402_flow::decode_402_response(&resp_headers, &resp_body_text) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[task-402-pay] failed to decode 402 response (signing + on-chain accept already done): {e}");
            crate::output::success(serde_json::json!({
                "replaySuccess": false,
                "replayStatus": 402,
                "replayBody": { "error": format!("failed to decode 402 response: {e}"), "rawBody": resp_body_text },
                "signature": proof.signature,
                "authorization": proof.authorization,
                "sessionCert": proof.session_cert,
                "txHash": tx_hash,
                "endpoint": endpoint,
                "retryHint": "Signing and direct/accept are done; you may retry GET endpoint → 402 → assemble header → replay.",
            }));
            return Ok(());
        }
    };
    eprintln!("[task-402-pay] 402 payload: x402Version={}, accepts={} entries, resource={}",
        x402_payload.x402_version, x402_payload.accepts.len(),
        x402_payload.resource.is_some());

    let x402_proof = x402_flow::X402PaymentProof {
        signature: proof.signature.clone(),
        authorization: serde_json::to_value(&proof.authorization)
            .unwrap_or(serde_json::Value::Null),
        session_cert: proof.session_cert.clone(),
    };
    let (header_name, header_value) = match x402_flow::assemble_payment_header(&x402_proof, &x402_payload) {
        Ok(hv) => hv,
        Err(e) => {
            eprintln!("[task-402-pay] failed to assemble payment header (signing + on-chain accept already done): {e}");
            crate::output::success(serde_json::json!({
                "replaySuccess": false,
                "replayStatus": 402,
                "replayBody": { "error": format!("failed to assemble payment header: {e}") },
                "signature": proof.signature,
                "authorization": proof.authorization,
                "sessionCert": proof.session_cert,
                "txHash": tx_hash,
                "endpoint": endpoint,
                "retryHint": "Signing and direct/accept are done; you may retry GET endpoint → 402 → assemble header → replay.",
            }));
            return Ok(());
        }
    };

    eprintln!("[task-402-pay] replaying endpoint ({header_name}: ...)");
    let replay_resp = http
        .get(endpoint)
        .header(&header_name, &header_value)
        .send()
        .await;

    let (replay_success, replay_status, replay_body) = match replay_resp {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let raw_text = resp.text().await.unwrap_or_default();
            let body: serde_json::Value = serde_json::from_str(&raw_text)
                .unwrap_or_else(|_| serde_json::json!({ "raw": raw_text }));
            let success = (200..300).contains(&status);
            eprintln!("[task-402-pay] replay result: HTTP {status}, success={success}");
            (success, status, body)
        }
        Err(e) => {
            eprintln!("[task-402-pay] replay request failed: {e}");
            (false, 0u16, serde_json::json!({ "error": e.to_string() }))
        }
    };

    // Step 4: emit the complete result.
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
    crate::output::success(serde_json::json!({
        "replaySuccess": replay_success,
        "replayStatus": replay_status,
        "replayBody": replay_body,
        "signature": proof.signature,
        "authorization": proof.authorization,
        "sessionCert": proof.session_cert,
        "txHash": tx_hash,
    }));
    Ok(())
}

/// x402-check — validate whether the endpoint is a legitimate x402 service and extract pricing info.
pub async fn handle_x402_check(client: &mut TaskApiClient, endpoint: &str, agent_id: Option<&str>) -> Result<()> {
    use super::x402_flow;

    let check = x402_flow::check_x402_endpoint(endpoint).await?;

    if !check.valid {
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
            eprintln!("⚠ token resolution failed (does not affect validity): {e}");
            data["tokenResolveError"] = serde_json::json!(e.to_string());
        }
    }

    crate::output::success(data);
    Ok(())
}
