//! 确认接单 + Fund
//!
//! 买家动作：
//! - set-payment-mode: 设置支付方式（独立命令，单签上链 → 等待 job_payment_mode_changed）
//! - confirm-accept: 确认接受卖家（setPaymentMode 已完成后执行实际支付）
//!    - escrow:      providerConfirmStatus → sign_escrow → accept → broadcast
//!    - non_escrow:  a2a_pay::pay() → direct/accept → broadcast
//!    - x402:        不走此命令（用 task-402-pay）
//! - direct-accept: x402 阶段 2b
//! - task-402-pay: x402 阶段 2（签名 + direct/accept + 重放 endpoint）
//!
//! 接口文档：https://okg-block.sg.larksuite.com/wiki/UumqwSyM5i1AuakBNLClJo9igIb
//! 支付设计：https://okg-block.sg.larksuite.com/docx/CwWbd6eCOopgq6x6VwTlWEivgrc

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::util::{
    json_str, json_u64, fetch_token_detail, resolve_payment_mode,
    resolve_x402_params, fetch_provider_address,
};
use crate::commands::agent_commerce::task::common::{
    self, PAYMENT_MODE_ESCROW, PAYMENT_MODE_NON_ESCROW, PAYMENT_MODE_X402,
    XLAYER_CHAIN_ID,
};
use crate::commands::agent_commerce::task::signing;
use crate::commands::payment::a2a_pay::{self, PayParams};
use super::negotiate;

/// 从 CLI flag / 本地协商记录解析 (symbol, amount)
fn resolve_symbol_and_amount(
    token_symbol: Option<&str>,
    token_amount: Option<&str>,
    job_id: &str,
    mode_label: &str,
) -> Result<(String, String)> {
    let agreed = negotiate::load_agreed(job_id)?;
    let symbol = match token_symbol {
        Some(s) => s.to_string(),
        None => match &agreed {
            Some((sym, _)) => {
                eprintln!("ℹ --token-symbol 未传入，使用本地协商记录: {sym}");
                sym.clone()
            }
            None => bail!("{mode_label} 需要 --token-symbol 或先执行 save-agreed 保存协商结果"),
        },
    };
    let amount = match token_amount {
        Some(a) => a.to_string(),
        None => match &agreed {
            Some((_, amt)) => {
                eprintln!("ℹ --token-amount 未传入，使用本地协商记录: {amt}");
                amt.clone()
            }
            None => bail!("{mode_label} 需要 --token-amount 或先执行 save-agreed 保存协商结果"),
        },
    };
    Ok((symbol, amount))
}

/// 查询 Provider 是否已 apply 及报价（escrow 参数）。
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
        .map_err(|e| anyhow::anyhow!("providerConfirmStatus 查询失败: {e}"))
}

/// set-payment-mode — 独立设置支付方式（从 confirm-accept 拆分）
///
/// 对所有支付方式统一执行：POST setPaymentMode → sign_uop → broadcast
/// 然后返回 confirming（exit code 2），等待 job_payment_mode_changed 系统通知。
#[allow(clippy::too_many_arguments)]
pub async fn handle_set_payment_mode(
    client: &mut TaskApiClient,
    job_id: &str,
    provider: &str,
    payment_mode: Option<&str>,
    token_symbol: Option<&str>,
    token_amount: Option<&str>,
    endpoint: Option<&str>,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id).await?;

    let payment_mode = resolve_payment_mode(client, payment_mode, job_id, &agent_id).await?;

    // x402: 解析服务参数 + 余额预检
    let x402_resolved = if payment_mode.as_str() == PAYMENT_MODE_X402 {
        let resolved = resolve_x402_params(job_id, provider, endpoint, token_symbol, token_amount).await?;
        if resolved.fee_amount > 0.0 && !resolved.fee_token_symbol.is_empty() {
            common::ensure_sufficient_balance(resolved.fee_amount, &resolved.fee_token_symbol).await?;
        }
        Some(resolved)
    } else {
        // escrow / non_escrow: 余额预检
        let (sym, amt_str) = resolve_symbol_and_amount(token_symbol, token_amount, job_id, "set-payment-mode")?;
        let amt: f64 = amt_str.parse().unwrap_or(0.0);
        if amt > 0.0 {
            common::ensure_sufficient_balance(amt, &sym).await?;
        }
        None
    };

    // POST setPaymentMode → sign → broadcast
    let mode_int = common::payment_mode_to_int(&payment_mode);
    let resp = client.post_with_identity(
        &client.endpoint(job_id, "setPaymentMode"),
        &serde_json::json!({ "paymentMode": mode_int }),
        &agent_id,
    ).await?;

    signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::BizContext::JobSetPaymentMode, &agent_id,
    ).await?;
    println!("✓ 支付方式已设置: {payment_mode} ({mode_int})，等待链上确认...");

    if payment_mode.as_str() == PAYMENT_MODE_X402 {
        let ep = x402_resolved.as_ref().map(|x| x.endpoint.as_str()).unwrap_or("");
        let sym = x402_resolved.as_ref().map(|x| x.fee_token_symbol.as_str()).unwrap_or("");
        let amt = x402_resolved.as_ref().map(|x| x.fee_amount).unwrap_or(0.0);
        let msg = format!(
            "x402 setPaymentMode 完成。provider={provider}, endpoint={ep}, fee={amt} {sym}",
        );
        let next = "等待 job_payment_mode_changed 系统通知 → Agent 执行 x402 endpoint 交互 → onchainos agent direct-accept".to_string();
        crate::output::confirming(&msg, &next);
        return Ok(());
    }

    let msg = format!(
        "setPaymentMode({payment_mode}) 完成。provider={provider}",
    );
    let next = format!(
        "等待 job_payment_mode_changed 系统通知 → onchainos agent confirm-accept {job_id} --provider {provider} --payment-mode {payment_mode}"
    );
    crate::output::confirming(&msg, &next);
    Ok(())
}

/// confirm-accept — 确认接受卖家（setPaymentMode 已通过 set-payment-mode 独立执行）
#[allow(clippy::too_many_arguments)]
pub async fn handle_confirm_accept(
    client: &mut TaskApiClient,
    job_id: &str,
    provider: &str,
    payment_mode: Option<&str>,
    payment_id: Option<&str>,
    token_symbol: Option<&str>,
    token_amount: Option<&str>,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id).await?;

    let payment_mode = resolve_payment_mode(client, payment_mode, job_id, &agent_id).await?;

    if payment_mode.as_str() == PAYMENT_MODE_X402 {
        bail!("x402 流程请用 onchainos agent set-payment-mode 设置支付方式，再用 onchainos agent task-402-pay 执行阶段 2");
    }

    // 余额预检
    let (sym, amt_str) = resolve_symbol_and_amount(token_symbol, token_amount, job_id, &payment_mode)?;
    let amt: f64 = amt_str.parse().unwrap_or(0.0);
    if amt > 0.0 {
        common::ensure_sufficient_balance(amt, &sym).await?;
    }

    eprintln!("[debug] payment_mode 最终值: '{payment_mode}'");
    match payment_mode.as_str() {
        PAYMENT_MODE_ESCROW => {
            confirm_accept_escrow(
                client, job_id, provider, token_symbol, token_amount,
                &account_id, &address, &agent_id,
            ).await?;
        }
        PAYMENT_MODE_NON_ESCROW | "direct" => {
            confirm_accept_non_escrow(
                client, job_id, provider, payment_id, token_symbol, token_amount,
                &account_id, &address, &agent_id,
            ).await?;
        }
        PAYMENT_MODE_X402 => {
            bail!("x402 流程在 setPaymentMode 后结束，不应到达此分支；请用 onchainos agent task-402-pay 执行阶段 2");
        }
        other => {
            bail!("不支持的支付方式: {other}，可选: escrow / non_escrow / x402");
        }
    }

    if let Err(e) = negotiate::cleanup(job_id) {
        eprintln!("⚠ 清理协商状态失败（可忽略）: {e}");
    }
    Ok(())
}

/// escrow 担保支付：providerConfirmStatus → sign_escrow → accept → broadcast
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
    let (symbol, amount) = resolve_symbol_and_amount(token_symbol, token_amount, job_id, "escrow")?;

    // providerConfirmStatus 确认卖家已 apply 并获取 escrow 参数
    let confirm_resp = fetch_provider_confirm_status(
        client, job_id, provider, &symbol, &amount, agent_id,
    ).await?;
    let amount_minimal = confirm_resp["amount"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("providerConfirmStatus 响应缺少 amount"))?
        .to_string();
    let currency = confirm_resp["currency"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("providerConfirmStatus 响应缺少 currency"))?
        .to_string();

    // 校验 currency 与任务 tokenAddress 一致
    let task_resp = client.get_with_identity(&client.task_path(job_id), agent_id).await?;
    let task_token_address = task_resp["tokenAddress"]
        .as_str()
        .unwrap_or("")
        .to_lowercase();
    if !task_token_address.is_empty() && currency.to_lowercase() != task_token_address {
        bail!(
            "币种不匹配：providerConfirmStatus 返回 currency={currency}，\
             但任务 tokenAddress={task_token_address}。\
             请检查协商币种是否与任务发布币种一致（--token-symbol）"
        );
    }

    // 解析 escrow 参数
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
            .ok_or_else(|| anyhow::anyhow!("expiredAt unix 时间戳无效: {expired_at_raw}"))?
            .to_rfc3339()
    } else {
        expired_at_raw
    };
    let hook = json_str(escrow, "hook")?;
    let hook_data = json_str(escrow, "hookData")?;
    let salt = json_str(escrow, "salt")?;
    println!("✓ providerConfirmStatus: 卖家已 apply，escrow 参数已获取");

    // sign_escrow — TEE 签名 EIP-3009 ReceiveWithAuthorization
    eprintln!("[debug] sign_escrow 入参:");
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
    eprintln!("[debug] sign_escrow 返回: signature={}, validAfter={}, validBefore={}",
        sign_output.signature, sign_output.authorization.valid_after, sign_output.authorization.valid_before);
    println!("✓ escrow payment签名完成");

    // accept → calldata → 签名 → 广播
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
        job_id, signing::BizContext::JobAccept, agent_id,
        payment_verify,
    ).await?;
    println!("✓ 已接受卖家 {provider}（担保支付），资金已托管");
    println!("  txHash: {tx_hash}");
    Ok(())
}

/// non_escrow 非担保支付：a2a_pay::pay() → direct/accept → broadcast
#[allow(clippy::too_many_arguments)]
async fn confirm_accept_non_escrow(
    client: &mut TaskApiClient,
    job_id: &str,
    provider: &str,
    payment_id: Option<&str>,
    token_symbol: Option<&str>,
    token_amount: Option<&str>,
    account_id: &str,
    address: &str,
    agent_id: &str,
) -> Result<()> {
    let pid = payment_id.ok_or_else(|| {
        anyhow::anyhow!("非担保支付需要 --payment-id（由卖家通过 XMTP 传递）")
    })?;

    let (symbol, amount) = resolve_symbol_and_amount(token_symbol, token_amount, job_id, "non_escrow")?;
    let provider_address = fetch_provider_address(provider).await?;

    // 查询 token 合约地址和精度
    let (token_address, decimals) = fetch_token_detail(client, &symbol, agent_id).await?;
    let amount_minimal = crate::commands::swap::readable_to_minimal_str(&amount, decimals)?;

    // a2a_pay::pay() — EIP-3009 支付签名
    let pay_result = a2a_pay::pay(PayParams {
        payment_id: pid.to_string(),
        amount: amount_minimal,
        currency: token_address,
        recipient_address: provider_address.clone(),
    }).await?;
    println!("✓ a2a_pay 支付完成: payment_id={}, status={}", pay_result.payment_id, pay_result.status);
    if let Some(ref tx) = pay_result.tx_hash {
        println!("  pay txHash: {tx}");
    }

    // direct/accept → calldata(uopData) → 签名 → 广播
    let body = serde_json::json!({
        "providerAddress": provider_address,
        "providerAgentId": provider,
        "tokenSymbol": symbol,
        "tokenAmount": amount,
    });
    let resp = client.post_with_identity(
        &client.endpoint(job_id, "direct/accept"),
        &body,
        agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], account_id, address,
        job_id, signing::BizContext::JobAccept, agent_id,
    ).await?;
    println!("✓ 已接受卖家 {provider}（非担保支付），状态 → accepted");
    println!("  txHash: {tx_hash}");
    Ok(())
}

/// direct-accept — x402 阶段 2b：收到 job_payment_mode_changed 后，Agent 完成 x402 endpoint 交互，
/// 然后调此命令执行 direct/accept 上链。
pub async fn handle_direct_accept(
    client: &mut TaskApiClient,
    job_id: &str,
    provider: &str,
    token_symbol: Option<&str>,
    token_amount: Option<&str>,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id).await?;

    let body = serde_json::json!({
        "providerAgentId": provider,
        "tokenSymbol": token_symbol.unwrap_or(""),
        "tokenAmount": token_amount.unwrap_or(""),
    });
    eprintln!("[debug] direct-accept 入参: {}", serde_json::to_string_pretty(&body).unwrap_or_default());

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "direct/accept"),
        &body,
        &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::BizContext::JobAccept, &agent_id,
    ).await?;
    println!("✓ direct/accept 完成（x402），任务状态 → accepted");
    println!("  txHash: {tx_hash}");
    println!("  等待 job_accepted 系统通知后执行 complete");

    Ok(())
}

/// task-402-pay — x402 阶段 2：签名 + direct/accept + 重放 endpoint。
#[allow(clippy::too_many_arguments)]
pub async fn handle_task_402_pay(
    client: &mut TaskApiClient,
    job_id: &str,
    provider: &str,
    accepts: &str,
    endpoint: &str,
    token_symbol: Option<&str>,
    token_amount: Option<&str>,
    from: Option<&str>,
) -> Result<()> {
    use crate::commands::agentic_wallet::payment;
    use super::x402_flow;

    // Step 1: x402_pay 签名
    eprintln!("[task-402-pay] Step 1: x402_pay 签名");
    eprintln!("[task-402-pay] accepts: {accepts}");
    let proof = payment::x402_pay_from_accepts(accepts, from.map(|s| s.to_string())).await?;
    eprintln!("[task-402-pay] x402_pay 完成: signature={}", proof.signature);

    // Step 2: direct/accept 上链
    eprintln!("[task-402-pay] Step 2: direct/accept 上链");
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id).await?;

    let body = serde_json::json!({
        "providerAgentId": provider,
        "tokenSymbol": token_symbol.unwrap_or(""),
        "tokenAmount": token_amount.unwrap_or(""),
    });
    let resp = client.post_with_identity(
        &client.endpoint(job_id, "direct/accept"),
        &body,
        &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::BizContext::JobAccept, &agent_id,
    ).await?;
    eprintln!("[task-402-pay] direct/accept 广播完成: txHash={tx_hash}");

    // Step 3: GET endpoint → 402 → 组装 header → 重放
    eprintln!("[task-402-pay] Step 3: GET endpoint {endpoint} → 获取完整 402 payload");
    let http = reqwest::Client::new();

    let initial_resp = http.get(endpoint).send().await
        .map_err(|e| anyhow::anyhow!("请求 x402 endpoint 失败: {e}"))?;
    let initial_status = initial_resp.status().as_u16();

    if initial_status != 402 {
        let body: serde_json::Value = initial_resp.json().await
            .unwrap_or_else(|_| serde_json::json!({ "raw": "non-json response" }));
        let success = (200..300).contains(&initial_status);
        eprintln!("[task-402-pay] endpoint 返回 HTTP {initial_status}（非 402），直接作为结果");
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
    let resp_body_text = initial_resp.text().await
        .map_err(|e| anyhow::anyhow!("读取 402 响应体失败: {e}"))?;
    let x402_payload = x402_flow::decode_402_response(&resp_headers, &resp_body_text)?;
    eprintln!("[task-402-pay] 402 payload: x402Version={}, accepts={} 条, resource={}",
        x402_payload.x402_version, x402_payload.accepts.len(),
        x402_payload.resource.is_some());

    let x402_proof = x402_flow::X402PaymentProof {
        signature: proof.signature.clone(),
        authorization: serde_json::to_value(&proof.authorization)
            .unwrap_or(serde_json::Value::Null),
        session_cert: proof.session_cert.clone(),
    };
    let (header_name, header_value) = x402_flow::assemble_payment_header(&x402_proof, &x402_payload)?;

    eprintln!("[task-402-pay] 重放 endpoint（{header_name}: ...）");
    let replay_resp = http
        .get(endpoint)
        .header(&header_name, &header_value)
        .send()
        .await;

    let (replay_success, replay_status, replay_body) = match replay_resp {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body: serde_json::Value = resp.json().await
                .unwrap_or_else(|_| serde_json::json!({ "raw": "non-json response" }));
            let success = (200..300).contains(&status);
            eprintln!("[task-402-pay] replay 结果: HTTP {status}, success={success}");
            (success, status, body)
        }
        Err(e) => {
            eprintln!("[task-402-pay] replay 请求失败: {e}");
            (false, 0u16, serde_json::json!({ "error": e.to_string() }))
        }
    };

    // Step 4: 输出完整结果
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
