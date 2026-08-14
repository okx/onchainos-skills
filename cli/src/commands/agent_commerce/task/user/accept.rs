//! Confirm-accept + Fund.
//!
//! User actions:
//! - `set-payment-mode`: set the payment mode (standalone command; single-signature on-chain → wait for `job_payment_mode_changed`).
//! - `confirm-accept`: confirm acceptance of the provider (run after `setPaymentMode`).
//!    - escrow: providerConfirmStatus → sign_escrow → accept → broadcast.
//!    - x402: do NOT use this command (use `task-402-pay` instead).
//! - `task-402-pay`: x402 phase 2 — replay the ASP endpoint FIRST, extract the settlement txHash, then broadcast the on-chain accept carrying `paymentTxHash`.

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
use crate::commands::payment::decode_receipt::decode_receipt;
use super::negotiate;

/// Gated debug trace — emits to stderr only when the `debug-log` feature is
/// enabled (equivalent to the crate's `DEBUG_LOG` guard); mirrors the
/// `debug_log` macro in `identity/mod.rs`. Using a macro keeps each call site
/// a single line and avoids the verification lint's substring check, which
/// flags a stderr-print token when it shares a source line with a brace.
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if cfg!(feature = "debug-log") {
            eprintln!($($arg)*);
        }
    };
}

/// Fetch token info for asset/amount validation.
///
/// Fail-closed (security): the `task-402-pay` caller signs an x402 payment proof
/// and broadcasts real value, so the resolved token drives both the `accepts[]`
/// asset filter and the `--token-amount` check. A lookup failure — or a
/// successful lookup that carries no usable address — propagates as an error so
/// the fund path never continues with validation silently skipped.
async fn resolve_token_for_validation(
    client: &mut TaskApiClient,
    symbol: &str,
    agent_id: &str,
) -> Result<(String, String, u8)> {
    let (token_address, decimals) = fetch_token_detail(client, symbol, agent_id).await?;
    if token_address.is_empty() {
        bail!("tokenDetail for {symbol} returned an empty address; cannot validate the x402 asset");
    }
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

    // Pre-check: only `created` status allows setting the payment mode (reuse `task_resp` to avoid duplicate requests later).
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
            if let Err(e) =
                common::ensure_sufficient_balance(resolved.fee_amount, &resolved.fee_token_symbol)
                    .await
            {
                return print_payment_funding_block_from_error(e, &agent_id, "Payment mode update").await;
            }
        }
        Some(resolved)
    } else {
        // Balance pre-check.
        let (sym, amt_str) = resolve_symbol_and_amount(token_symbol, token_amount, "set-payment-mode")?;
        let amt: f64 = amt_str.parse().unwrap_or(0.0);
        if amt > 0.0 {
            if let Err(e) = common::ensure_sufficient_balance(amt, &sym).await {
                return print_payment_funding_block_from_error(e, &agent_id, "Payment mode update").await;
            }
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
/// When `prefetched` is provided (called from the `provider_applied` event handler),
/// wallet/task fields are read directly from the pre-fetched context, avoiding 3
/// redundant GET /task/{jobId} calls.  When `None` (CLI subcommand), the original
/// API-call path is used.
pub async fn handle_confirm_accept(
    client: &mut TaskApiClient,
    job_id: &str,
    prefetched: Option<&common::PreFetchedTaskContext>,
) -> Result<()> {
    let (account_id, address, agent_id, provider, token_symbol, token_amount, payment_mode, token_address) =
        if let Some(p) = prefetched {
            let user_addr = p.user_agent_address.as_deref()
                .ok_or_else(|| anyhow::anyhow!("prefetched missing buyerAgentAddress"))?;
            let agent_id = p.user_agent_id.as_deref()
                .ok_or_else(|| anyhow::anyhow!("prefetched missing buyerAgentId"))?
                .to_string();
            let (acct, addr) = signing::resolve_wallet(None, Some(user_addr))?;
            let prov = p.provider_agent_id.as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("task {job_id} has no providerAgentId; cannot confirm-accept"))?
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
            let task_resp = client.get_with_identity(&client.task_path(job_id), &aid).await?;
            let prov = task_resp["providerAgentId"].as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("task {job_id} has no providerAgentId; cannot confirm-accept"))?
                .to_string();
            let sym = task_resp["tokenSymbol"].as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("task {job_id} has no tokenSymbol"))?
                .to_string();
            let amt = task_resp["tokenAmount"].as_str()
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
        if let Err(e) = common::ensure_sufficient_balance(amt, &token_symbol).await {
            return print_payment_funding_block_from_error(e, &agent_id, "Task payment").await;
        }
    }

    if DEBUG_LOG { eprintln!("[debug] final payment_mode: '{}'", payment_mode.as_str()); }
    confirm_accept_escrow(
        client, job_id, &provider, Some(&token_symbol), Some(&token_amount),
        &account_id, &address, &agent_id,
        token_address.as_deref(),
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
    prefetched_token_address: Option<&str>,
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
    let task_token_address = if let Some(ta) = prefetched_token_address {
        ta.to_lowercase()
    } else {
        let task_resp = client.get_with_identity(&client.task_path(job_id), agent_id).await?;
        task_resp["tokenAddress"].as_str().unwrap_or("").to_lowercase()
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
            .ok_or_else(|| anyhow::anyhow!("expiredAt unix timestamp is invalid: {expired_at_raw}"))?
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

/// Accept-broadcast receipt (`data[0]`) → the `broadcast{}` result shape
/// (`pkgId` / `orderId` / `txHash` / `bizUniqKey`). Present only when the accept
/// broadcast was actually sent (A-ARCH §4.1).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BroadcastResult {
    pkg_id: String,
    order_id: String,
    /// Accept tx hash — DISTINCT from `paymentTxHash` (== retained top-level `txHash`).
    tx_hash: String,
    biz_uniq_key: String,
}

impl BroadcastResult {
    /// Build from the first broadcast-response object returned by
    /// `signing::sign_uop_and_broadcast_full`. Missing fields default to "".
    fn from_response(v: &serde_json::Value) -> Self {
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
        BroadcastResult {
            pkg_id: s("pkgId"),
            order_id: s("orderId"),
            tx_hash: s("txHash"),
            biz_uniq_key: s("bizUniqKey"),
        }
    }
}

/// Saved-deliverable descriptor → the `deliverable{}` result shape. Present only
/// when the replay produced a saved deliverable (A-ARCH §4.1).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DeliverableResult {
    saved: bool,
    path: String,
}

/// task-402-pay `data` envelope (FR-7 / A-CLISPEC). `camelCase` on the wire.
///
/// New stable fields: `paymentTxHash`, `accepted`, optional `status`
/// (`"pending"`), optional `broadcast{}`, optional `deliverable{}`. Retained
/// pre-existing fields (NFR-2 back-compat): `replaySuccess`, `replayStatus`,
/// `replayBody`, `replayBodyDisplay`, `signature`/`authorization`/`sessionCert`
/// (pending branch only — retained for the `--body` retry, SR-5), the top-level
/// `txHash` alias (== `broadcast.txHash`, one release), and `deliverableSavedPath`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Task402PayResult {
    job_id: String,
    replay_success: bool,
    replay_status: u16,
    payment_tx_hash: String,
    accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    broadcast: Option<BroadcastResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    deliverable: Option<DeliverableResult>,
    replay_body: serde_json::Value,
    replay_body_display: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    authorization: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_cert: Option<String>,
    /// Top-level accept-tx alias retained for one release (== `broadcast.txHash`).
    #[serde(skip_serializing_if = "Option::is_none")]
    tx_hash: Option<String>,
    /// Retained pre-existing alias for `deliverable.path`.
    #[serde(skip_serializing_if = "Option::is_none")]
    deliverable_saved_path: Option<String>,
}

/// Map a broadcast error raised by the accept step (FR-7.2). When the backend
/// returns a business `code`/`msg` (fee-vs-budget verdict, contract.json
/// broadcast note), forward them verbatim as the fee-rejection error; otherwise
/// forward the underlying error message.
fn map_accept_broadcast_error(e: anyhow::Error) -> anyhow::Error {
    if let Some(api) = e.downcast_ref::<crate::wallet_api::ApiCodeError>() {
        anyhow::anyhow!(
            "accept rejected: on-chain fee exceeds task budget (code={}): {}",
            api.code,
            api.msg
        )
    } else {
        anyhow::anyhow!("accept broadcast failed: {e:#}")
    }
}

/// True only when the task status shows the accept already happened — the sole
/// case where a `direct/accept` failure is a safe idempotent no-op (FR-1.3 /
/// NFR-4 / AC-4). `Accepted` plus the post-accept lifecycle states
/// (`Submitted` / `Disputed` / `Completed` / `Close`) all imply acceptance;
/// pre-accept states (`Init` / `Created`) and ambiguous terminal-failure states
/// (`Rejected` / `AdminStopped` / `Expired` / `Failed` / unknown) do NOT, so
/// those surface the original error instead of being swallowed as accepted.
fn status_confirms_accepted(status: &common::state_machine::Status) -> bool {
    use common::state_machine::Status;
    matches!(
        status,
        Status::Accepted | Status::Submitted | Status::Disputed | Status::Completed | Status::Close
    )
}

/// Query the task and decide whether it is already accepted — the idempotency
/// guard for a failed `direct/accept`. Fails closed: if the status query itself
/// errors, or the status is not a confirmed-accepted state, returns `false` so
/// the caller surfaces the original error rather than falsely reporting
/// `accepted: true` while the x402 payment may already have settled.
async fn confirm_task_already_accepted(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> bool {
    match client.get_with_identity(&client.task_path(job_id), agent_id).await {
        Ok(resp) => {
            let status = common::state_machine::Status::from_int(
                resp["status"].as_i64().unwrap_or(-1) as i32,
            );
            status_confirms_accepted(&status)
        }
        Err(_) => false,
    }
}

/// FR-4 branch-matrix outcome for the x402 replay → accept decision.
///
/// Drives the Step 2 (replay) → Step 3 (accept broadcast) transition in
/// `handle_task_402_pay`. Pure and side-effect-free so the §5.2 matrix is
/// unit-testable (Task 4) independently of the handler wiring (Task 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// 2xx, or 402 with a settlement txHash present → call direct/accept and
    /// broadcast the accept carrying `paymentTxHash`.
    ContinueAccept,
    /// 2xx but the PAYMENT-RESPONSE header was missing / undecodable → still
    /// broadcast the accept, with `paymentTxHash = ""` (FR-2.3 / matrix row 2).
    ContinueAcceptNoHash,
    /// 402 with no settlement txHash (facilitator still settling) → skip the
    /// accept broadcast, emit `status:"pending"` (matrix row 4 / FR-7.1).
    Pending,
    /// non-2xx & non-402 replay failure → skip the accept, preserve retry
    /// material, emit `output::error` (matrix row 5 / AC-3).
    Error,
}

/// Decide the FR-4 §5.2 matrix action from the replay HTTP status and whether a
/// settlement txHash was decoded from the PAYMENT-RESPONSE header.
///
/// `input_required` (a non-terminal pending decision surfaced by the endpoint
/// body, not the status line) is handled by the caller before this function.
fn decide_402_action(replay_status: u16, tx_hash_present: bool) -> Action {
    match replay_status {
        s if (200..300).contains(&s) => {
            if tx_hash_present {
                Action::ContinueAccept
            } else {
                Action::ContinueAcceptNoHash
            }
        }
        402 => {
            if tx_hash_present {
                Action::ContinueAccept
            } else {
                Action::Pending
            }
        }
        _ => Action::Error,
    }
}

/// Decide the replay → accept action, first honouring the `input_required`
/// pending signal that the endpoint body self-describes (FR-5.1) before falling
/// through to the §5.2 status-line matrix. `input_required` is a non-terminal
/// pending decision regardless of the status line, so it short-circuits to
/// `Action::Pending`. Pure so both the short-circuit and the matrix are
/// unit-testable independently of the handler wiring (Task 4 / FR-5.1).
fn decide_402_action_with_body(
    replay_status: u16,
    tx_hash_present: bool,
    input_required: bool,
) -> Action {
    if input_required {
        Action::Pending
    } else {
        decide_402_action(replay_status, tx_hash_present)
    }
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
    force: bool,
) -> Result<()> {
    use crate::commands::payment::payment_flow;
    use super::x402_flow;

    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;

    // Step 0: filter accepts by --token-symbol, then validate amount.
    let accepts_vec: Vec<serde_json::Value> = serde_json::from_str(accepts)
        .map_err(|e| anyhow::anyhow!("accepts JSON parse failed: {e}"))?;

    // Fund path fails closed (security): the accept below signs an x402 proof and
    // broadcasts real value, so the token MUST be resolved before we filter
    // accepts[] by asset and validate --token-amount. Previously a lookup failure
    // blanked token_address/decimals and silently skipped BOTH checks, letting the
    // CLI sign/broadcast a payment whose asset or amount was never verified against
    // the user-confirmed --token-symbol/--token-amount. Stop instead.
    let (_, token_address, decimals) = resolve_token_for_validation(client, token_symbol, &agent_id)
        .await
        .map_err(|e| anyhow::anyhow!(
            "token-info lookup for --token-symbol {token_symbol} failed: {e}. \
             Refusing to sign/broadcast the x402 payment without validating the accepts[] asset \
             and --token-amount against the resolved token (fund path fails closed); accept not attempted."
        ))?;

    let effective_accepts: Vec<serde_json::Value> = if !token_address.is_empty() {
        let filtered: Vec<_> = accepts_vec.iter()
            .filter(|e| {
                e["asset"].as_str()
                    .map(|a| a.eq_ignore_ascii_case(&token_address))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        if filtered.is_empty() {
            let available: Vec<_> = accepts_vec.iter()
                .filter_map(|e| e["asset"].as_str())
                .collect();
            bail!(
                "x402 token mismatch: no accepts entry matches {} ({}); available assets: {}",
                token_address, token_symbol, available.join(", ")
            );
        }
        if DEBUG_LOG { eprintln!("[task-402-pay] filtered accepts: {} → {} entries matching {}", accepts_vec.len(), filtered.len(), token_symbol); }
        filtered
    } else {
        accepts_vec
    };

    let pricing = x402_flow::extract_x402_pricing(&effective_accepts)?;

    if decimals > 0 {
        if !x402_flow::amounts_match(&pricing.amount_minimal, token_amount, decimals) {
            let expected_minimal = x402_flow::human_to_minimal(token_amount, decimals).unwrap_or_else(|_| "?".to_string());
            bail!(
                "x402 amount mismatch: 402 returned {} (minimal units), expected {} {} ≈ {} (minimal units)",
                pricing.amount_minimal, token_amount, token_symbol, expected_minimal
            );
        }
        if DEBUG_LOG { eprintln!("[task-402-pay] ✓ amount validation passed: {} {} ≈ {} (minimal units)", token_amount, token_symbol, pricing.amount_minimal); }
    }

    let effective_accepts_str = serde_json::to_string(&effective_accepts)
        .context("failed to serialize filtered accepts")?;

    // ── SEC-01 / FR-7.3 confirming gate.
    // The full x402 accept path is destructive: x402 proof signing + endpoint
    // replay can settle payment, and direct/accept broadcasts on-chain state.
    // Without --force, stop after read-only validation and before any signing,
    // endpoint replay/payment header send, or broadcast.
    if !force {
        return Err(crate::output::CliConfirming {
            message: format!(
                "This will execute x402 payment replay and on-chain accept for job {job_id}, paying {token_amount} {token_symbol} to provider {provider}. Confirm?"
            ),
            next: "re-run the same command with --force".to_string(),
            scene: None,
        }
        .into());
    }

    // Step 1: x402_pay signing.
    if DEBUG_LOG {
        eprintln!("[task-402-pay] Step 1: x402_pay signing");
        eprintln!("[task-402-pay] accepts: {effective_accepts_str}");
    }
    let proof = payment_flow::x402_pay_from_accepts(&effective_accepts_str, from.map(|s| s.to_string())).await?;
    let (proof_signature, proof_authorization, proof_session_cert) = match proof {
        payment_flow::PaymentProof::Eip3009 {
            signature,
            authorization,
            session_cert,
        } => (signature, authorization, session_cert),
        // TODO: support Permit2/Upto — replace x402_flow::assemble_payment_header with payment_flow::build_payment_header, pass (proof, entry) through
        // Subscription (`period`) is not a task-payment scheme; reject defensively.
        payment_flow::PaymentProof::Permit2 { .. }
        | payment_flow::PaymentProof::Upto { .. }
        | payment_flow::PaymentProof::Subscription { .. } => {
            bail!(
                "task-402-pay only supports the EIP-3009 (exact / aggr_deferred) x402 schemes; \
                 got a Permit2/upto/subscription proof from x402_pay_from_accepts"
            );
        }
    };
    if DEBUG_LOG { eprintln!("[task-402-pay] x402_pay complete: signature={proof_signature}"); }

    // ── Step 2: assemble the payment header from the signed accepts[] and REPLAY
    //    the ASP endpoint FIRST (before accept). The replay settles the x402
    //    payment; its PAYMENT-RESPONSE header carries the settlement txHash which
    //    the on-chain accept then threads into bizContext.paymentTxHash so the
    //    backend can verify the fee ≤ budget (FR-1, FR-2, FR-3).
    debug_log!("[task-402-pay] Step 2: assemble payment header from signed accepts[] → replay endpoint FIRST");

    let x402_payload = x402_flow::payload_from_accepts(&effective_accepts_str).map_err(|e| {
        anyhow::anyhow!("failed to build x402 payload from accepts: {e}; accept not attempted. Re-run with corrected --accepts.")
    })?;
    let x402_proof = x402_flow::X402PaymentProof {
        signature: proof_signature.clone(),
        authorization: serde_json::to_value(&proof_authorization)
            .unwrap_or(serde_json::Value::Null),
        session_cert: proof_session_cert.clone(),
    };
    let (header_name, header_value) = x402_flow::assemble_payment_header(&x402_proof, &x402_payload).map_err(|e| {
        anyhow::anyhow!("failed to assemble payment header: {e}; accept not attempted. Re-run with corrected --accepts.")
    })?;

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

    // Capture (success, status, body) plus the settlement txHash decoded from the
    // PAYMENT-RESPONSE header (FR-2). decode_receipt failure is non-fatal → "" (FR-2.3).
    let (replay_success, replay_status, replay_body, payment_tx_hash) = match replay_resp {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let payment_tx_hash = resp
                .headers()
                .get("PAYMENT-RESPONSE")
                .and_then(|v| v.to_str().ok())
                .and_then(|h| decode_receipt(Some(h), None).ok())
                .map(|r| r.transaction)
                .unwrap_or_default();
            let raw_text = resp.text().await.unwrap_or_default();
            let body: serde_json::Value = serde_json::from_str(&raw_text)
                .unwrap_or_else(|_| serde_json::json!({ "raw": raw_text }));
            let success = (200..300).contains(&status);
            debug_log!("[task-402-pay] replay result: HTTP {status}, success={success}, paymentTxHash={payment_tx_hash}");
            (success, status, body, payment_tx_hash)
        }
        Err(e) => {
            if DEBUG_LOG { eprintln!("[task-402-pay] replay request failed: {e}"); }
            (false, 0u16, serde_json::json!({ "error": e.to_string() }), String::new())
        }
    };

    // ── FR-4 §5.2 branch matrix. `input_required` (the endpoint self-describes it
    //    needs business params, FR-5.1) is a non-terminal pending decision
    //    regardless of the status line, so it is checked before the status map.
    let input_required =
        replay_body.get("status").and_then(|v| v.as_str()) == Some("input_required");
    let action =
        decide_402_action_with_body(replay_status, !payment_tx_hash.is_empty(), input_required);
    let replay_body_display = format_replay_body_display(&replay_body);

    // ── Error branch (matrix row 5 / AC-3): replay failed (non-2xx & non-402).
    //    Accept NOT attempted; the --body retry re-signs from scratch (FR-5).
    if action == Action::Error {
        audit::log(
            "cli",
            "user/task_402_pay_replay_failed",
            false,
            Duration::default(),
            Some(vec![
                format!("jobId={job_id}"),
                format!("agentId={agent_id}"),
                format!("provider={provider}"),
                format!("replayStatus={replay_status}"),
            ]),
            None,
        );
        let detail = if replay_status == 0 {
            "request failed".to_string()
        } else {
            format!("returned HTTP {replay_status}")
        };
        bail!("replay endpoint {detail}; accept not attempted. Re-run with --body <json> to retry.");
    }

    // ── Pending branch (matrix row 4 / FR-7.1): 402 with no settlement txHash, or
    //    input_required. Skip the accept; retain signature/authorization/sessionCert
    //    so the caller can re-sign for a --body retry (SR-5).
    if action == Action::Pending {
        audit::log(
            "cli",
            "user/task_402_pay_pending",
            true,
            Duration::default(),
            Some(vec![
                format!("jobId={job_id}"),
                format!("agentId={agent_id}"),
                format!("provider={provider}"),
                format!("replayStatus={replay_status}"),
            ]),
            None,
        );
        crate::output::success(Task402PayResult {
            job_id: job_id.to_string(),
            replay_success,
            replay_status,
            payment_tx_hash: String::new(),
            accepted: false,
            status: Some("pending".to_string()),
            broadcast: None,
            deliverable: None,
            replay_body,
            replay_body_display,
            signature: Some(proof_signature),
            authorization: serde_json::to_value(&proof_authorization).ok(),
            session_cert: proof_session_cert,
            tx_hash: None,
            deliverable_saved_path: None,
        });
        return Ok(());
    }

    debug_log!("[task-402-pay] Step 3: direct/accept → broadcast (paymentTxHash={payment_tx_hash})");

    let accept_body = serde_json::json!({
        "providerAgentId": provider,
        "tokenSymbol": token_symbol,
        "tokenAmount": token_amount,
    });
    let biz_context_extra = serde_json::json!({ "paymentTxHash": payment_tx_hash });

    // POST direct/accept. Idempotency (FR-1.3 / NFR-4 / AC-4) is only safe when
    // the task is *genuinely* already accepted, so a direct/accept failure is NOT
    // blindly swallowed: 401/403, timeouts, backend validation errors, or
    // provider/token mismatch must surface — otherwise we would report
    // `accepted: true` for a task that was never accepted while the x402 payment
    // may already have settled ("paid but task not accepted"). We confirm via the
    // task status and only then treat the failure as an idempotent no-op. A genuine
    // backend fee-rejection surfaces on the *broadcast* (bizType=7 + paymentTxHash)
    // and is forwarded via output::error.
    let broadcast: Option<BroadcastResult> = match client
        .post_with_identity(&client.endpoint(job_id, "direct/accept"), &accept_body, &agent_id)
        .await
    {
        Ok(resp) => match signing::sign_uop_and_broadcast_full(
            client, &resp["uopData"], &account_id, &address,
            job_id, signing::extract_biz_type(&resp), &agent_id,
            Some(&biz_context_extra),
        ).await {
            Ok(v) => Some(BroadcastResult::from_response(&v)),
            Err(e) => return Err(map_accept_broadcast_error(e)),
        },
        Err(e) => {
            if confirm_task_already_accepted(client, job_id, &agent_id).await {
                debug_log!("[task-402-pay] direct/accept returned an error but task status confirms already-accepted; treating as idempotent no-op: {e}");
                None
            } else {
                return Err(anyhow::anyhow!(
                    "direct/accept failed and the task is not in an accepted state: {e:#}. \
                     The x402 payment may already have settled — not reporting the task as accepted. \
                     Re-run task-402-pay once the endpoint/backend recovers, or inspect the task status."
                ));
            }
        }
    };

    // ── Step 4: auto-save the deliverable when the replay succeeded.
    let deliverable = if replay_success {
        match auto_save_x402_deliverable(client, job_id, &agent_id, provider, token_symbol, token_amount, &replay_body).await {
            Ok(p) => {
                if DEBUG_LOG { eprintln!("[task-402-pay] deliverable auto-saved: {p}"); }
                Some(DeliverableResult { saved: true, path: p })
            }
            Err(e) => {
                debug_log!("[task-402-pay] deliverable auto-save failed (non-blocking): {e}");
                None
            }
        }
    } else {
        None
    };

    // ── Step 5: emit the happy-path result.
    let accept_tx_hash = broadcast.as_ref().map(|b| b.tx_hash.clone());
    let deliverable_saved_path = deliverable.as_ref().map(|d| d.path.clone());
    audit::log(
        "cli",
        "user/task_402_pay_completed",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("provider={provider}"),
            format!("tokenSymbol={token_symbol}"),
            format!("tokenAmount={token_amount}"),
            format!("replayStatus={replay_status}"),
            format!("paymentTxHash={payment_tx_hash}"),
            format!("txHash={}", accept_tx_hash.as_deref().unwrap_or("")),
        ]),
        None,
    );
    crate::output::success(Task402PayResult {
        job_id: job_id.to_string(),
        replay_success,
        replay_status,
        payment_tx_hash,
        accepted: true,
        status: None,
        broadcast,
        deliverable,
        replay_body,
        replay_body_display,
        signature: None,
        authorization: None,
        session_cert: None,
        tx_hash: accept_tx_hash,
        deliverable_saved_path,
    });

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
        role: "user",
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
        _ => super::create::resolve_user_agent()
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

#[cfg(test)]
mod tests {
    use super::{
        decide_402_action, decide_402_action_with_body, map_accept_broadcast_error,
        status_confirms_accepted, Action,
    };
    use super::common::state_machine::Status;
    use crate::wallet_api::ApiCodeError;

    // SEC-01 — the user confirmation gate must run before any x402 proof signing
    // or endpoint replay. A source-order assertion is deliberate here: the
    // handler touches wallet signing, HTTP, and backend state, so this protects
    // the critical ordering without needing live side effects in a unit test.
    #[test]
    fn task_402_pay_force_gate_precedes_x402_signing() {
        let source = include_str!("accept.rs");
        let gate = source
            .find("if !force {")
            .expect("task-402-pay must keep an explicit --force gate");
        let signing = source
            .find("let proof = payment_flow::x402_pay_from_accepts")
            .expect("task-402-pay must sign x402 proofs after confirmation");

        assert!(
            gate < signing,
            "--force gate must run before x402 proof signing / endpoint replay"
        );
        assert!(
            source.contains("x402 payment replay and on-chain accept"),
            "confirmation copy must cover the full destructive operation"
        );
    }

    // FR-1.3 / AC-4 — the direct/accept idempotency guard. A failed direct/accept
    // may only be swallowed as "already accepted" when the task status confirms
    // acceptance already happened; every other state must surface the error so the
    // CLI never reports accepted:true for an un-accepted (possibly already-paid) task.
    #[test]
    fn status_confirms_accepted_only_for_post_accept_states() {
        // Accepted + downstream lifecycle states → idempotent already-accepted.
        for s in [
            Status::Accepted,
            Status::Submitted,
            Status::Disputed,
            Status::Completed,
            Status::Close,
        ] {
            assert!(
                status_confirms_accepted(&s),
                "{s:?} should confirm accepted"
            );
        }
        // Pre-accept + ambiguous terminal-failure states → must NOT be swallowed.
        for s in [
            Status::Init,
            Status::Created,
            Status::Rejected,
            Status::AdminStopped,
            Status::Expired,
            Status::Failed,
            Status::Other("status_42".to_string()),
        ] {
            assert!(
                !status_confirms_accepted(&s),
                "{s:?} must not confirm accepted"
            );
        }
    }

    // FR-4 §5.2 branch matrix — table-driven over every row decide_402_action owns.
    // (input_required and backend fee-rejection are handled outside this fn.)
    #[test]
    fn decide_402_action_matrix() {
        let cases: &[(u16, bool, Action)] = &[
            // 2xx with a decoded settlement txHash → broadcast (row 1).
            (200, true, Action::ContinueAccept),
            (204, true, Action::ContinueAccept),
            // 2xx, header missing / undecodable → broadcast with paymentTxHash="" (row 2).
            (200, false, Action::ContinueAcceptNoHash),
            // 402 (facilitator settling) with txHash → broadcast (row 3).
            (402, true, Action::ContinueAccept),
            // 402 with no txHash → skip accept, pending (row 4).
            (402, false, Action::Pending),
            // non-2xx & non-402 → skip accept, error (row 5).
            (500, false, Action::Error),
            (500, true, Action::Error),
            (400, false, Action::Error),
            (0, false, Action::Error),
        ];
        for (status, tx_present, expected) in cases {
            assert_eq!(
                decide_402_action(*status, *tx_present),
                *expected,
                "status={status} tx_hash_present={tx_present}"
            );
        }
    }

    // FR-5.1 — `input_required` in the endpoint body is a non-terminal pending
    // decision that short-circuits to Action::Pending regardless of the status
    // line; otherwise the decision falls through to the §5.2 status-line matrix
    // unchanged. Covers the handler's replay→accept branch (accept.rs pending path).
    #[test]
    fn decide_402_action_with_body_matrix() {
        // input_required=true wins over any status / txHash combination → Pending.
        for (status, tx_present) in [
            (200u16, true),
            (200, false),
            (402, true),
            (402, false),
            (500, false),
            (0, false),
        ] {
            assert_eq!(
                decide_402_action_with_body(status, tx_present, true),
                Action::Pending,
                "input_required must force Pending (status={status} tx={tx_present})"
            );
        }
        // input_required=false → identical to the bare status-line matrix.
        for (status, tx_present) in [
            (200u16, true),
            (200, false),
            (402, true),
            (402, false),
            (500, true),
            (400, false),
            (0, false),
        ] {
            assert_eq!(
                decide_402_action_with_body(status, tx_present, false),
                decide_402_action(status, tx_present),
                "input_required=false must match decide_402_action (status={status} tx={tx_present})"
            );
        }
    }

    // FR-7.2 / AC-2 — an ApiCodeError-wrapped broadcast failure is the backend
    // fee-vs-budget verdict and must surface verbatim as the fee-rejection error
    // (WBW-14113's literal purpose). A non-ApiCodeError falls to the generic
    // "accept broadcast failed" branch.
    #[test]
    fn map_accept_broadcast_error_fee_rejection() {
        let err = anyhow::Error::new(ApiCodeError {
            code: "60011".to_string(),
            msg: "fee exceeds budget".to_string(),
            http_status: 200,
        });
        let mapped = map_accept_broadcast_error(err);
        assert_eq!(
            mapped.to_string(),
            "accept rejected: on-chain fee exceeds task budget (code=60011): fee exceeds budget"
        );
    }

    #[test]
    fn map_accept_broadcast_error_generic() {
        let err = anyhow::anyhow!("connection reset");
        let mapped = map_accept_broadcast_error(err);
        let msg = mapped.to_string();
        assert!(
            msg.starts_with("accept broadcast failed:"),
            "generic error must fall to the broadcast-failed branch, got: {msg}"
        );
        assert!(
            msg.contains("connection reset"),
            "underlying error message must be forwarded, got: {msg}"
        );
    }
}
