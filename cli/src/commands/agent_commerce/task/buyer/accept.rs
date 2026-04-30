//! 确认接单 + Fund
//!
//! 买家动作：确认接单（担保 / 非担保 / x402）— onchainos task confirm-accept
//!
//! 流程：
//! 1. setPaymentMode（单签上链）
//! 2. 按支付方式分支：
//!    - escrow:      providerConfirmStatus → a2a_pay::create_escrow(→paymentId) → a2a_pay::pay(→signature) → accept(signatureData) → sign uop → broadcast
//!    - non_escrow:  a2a_pay::pay() → direct/accept → sign uop → broadcast
//!    - x402:        direct/accept → sign uop → broadcast → x402_request_sign_replay
//!
//! 接口文档：https://okg-block.sg.larksuite.com/wiki/UumqwSyM5i1AuakBNLClJo9igIb
//! 支付设计：https://okg-block.sg.larksuite.com/docx/CwWbd6eCOopgq6x6VwTlWEivgrc

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::{
    self, PAYMENT_MODE_ESCROW, PAYMENT_MODE_NON_ESCROW, PAYMENT_MODE_X402,
    XLAYER_CHAIN_ID,
};
use crate::commands::agent_commerce::task::signing;
use crate::commands::payment::a2a_pay::{self, PayParams};
use super::negotiate;
use super::x402_flow;

/// 通过 tokenDetail API 查询 token 合约地址和精度。
/// GET /priapi/v1/aieco/task/tokenDetail?symbol=<symbol>
/// 返回 (token_address, decimals)
async fn fetch_token_detail(client: &mut TaskApiClient, symbol: &str, agent_id: &str) -> Result<(String, u32)> {
    let path = format!("/priapi/v1/aieco/task/tokenDetail?symbol={symbol}");
    let resp = client.get_with_agent_id(&path, agent_id).await
        .map_err(|e| anyhow::anyhow!("查询 tokenDetail 失败 (symbol={symbol}): {e}"))?;
    let address = resp["address"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("tokenDetail 响应缺少 address 字段"))?
        .to_string();
    let decimals = resp["decimals"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("tokenDetail 响应缺少 decimals 字段"))? as u32;
    Ok((address, decimals))
}

/// 查询 Provider 是否已 apply 及报价（escrow 参数）。
/// GET /priapi/v1/aieco/task/{jobId}/providerConfirmStatus?providerAgentId=xxx&tokenSymbol=xxx&amount=xxx
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

/// 从 JSON 对象提取字符串字段。
fn json_str(obj: &serde_json::Value, key: &str) -> Result<String> {
    obj[key]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("escrow 响应缺少 {key} 字段"))
        .map(|s| s.to_string())
}

/// 从 JSON 对象提取 u64 字段（兼容数字和字符串）。
fn json_u64(obj: &serde_json::Value, key: &str) -> Result<u64> {
    if let Some(n) = obj[key].as_u64() {
        return Ok(n);
    }
    if let Some(s) = obj[key].as_str() {
        return s
            .parse()
            .map_err(|_| anyhow::anyhow!("escrow.{key} 解析 u64 失败: {s}"));
    }
    bail!("escrow 响应缺少 {key} 字段")
}


/// confirm-accept — 确认接受卖家
#[allow(clippy::too_many_lines)]
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

    // ── 解析支付方式：CLI flag > 任务详情 paymentType ──────────────────
    let payment_mode = match payment_mode {
        Some(m) => m.to_string(),
        None => {
            let task_resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;
            let payment_type = task_resp["paymentType"].as_i64().unwrap_or(0) as i32;
            let mode_str = common::payment_mode_to_str(payment_type);
            if mode_str == "none" || mode_str == "unknown" {
                eprintln!("⚠ 任务 paymentType={payment_type}，无法识别支付方式，默认使用 escrow");
                common::PAYMENT_MODE_ESCROW.to_string()
            } else {
                eprintln!("ℹ --payment-mode 未传入，使用任务详情 paymentType: {mode_str} ({payment_type})");
                mode_str.to_string()
            }
        }
    };

    // ── Step 1: setPaymentMode（单签 + 广播上链）──────────────────────
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
    println!("✓ 支付方式已设置: {payment_mode} ({mode_int})");

    // ── Step 2: 按支付方式分支处理 ──────────────────────────────────
    eprintln!("[debug] payment_mode 最终值: '{payment_mode}'");
    match payment_mode.as_str() {
        PAYMENT_MODE_ESCROW => {
            // ── 担保支付 (Escrow) ───────────────────────────────────
            // 流程：providerConfirmStatus → sign_escrow(TEE 签名) → accept → broadcast

            // Step 2a: 从协商结果获取金额和币种
            // 优先级：CLI flag > 本地协商记录(negotiate-state) > 报错
            // TODO(debug): 临时写死 USDT，调试完恢复原逻辑
            let agreed: Option<(String, String)> = negotiate::load_agreed(job_id)?;
            let symbol = {
                let _orig = match token_symbol {
                    Some(s) => s.to_string(),
                    None => match &agreed {
                        Some((sym, _)) => sym.clone(),
                        None => String::new(),
                    },
                };
                eprintln!("⚠ [debug] escrow 币种临时写死 USDT（原值: {_orig}）");
                "USDT".to_string()
            };
            let amount = match token_amount {
                Some(a) => a.to_string(),
                None => match &agreed {
                    Some((_, amt)) => {
                        eprintln!("ℹ --token-amount 未传入，使用本地协商记录: {amt}");
                        amt.clone()
                    }
                    None => bail!("escrow 模式需要 --token-amount 或先执行 save-agreed 保存协商结果"),
                },
            };

            // Step 2b: 调用 providerConfirmStatus 确认卖家已 apply 并获取 escrow 参数
            let confirm_resp = fetch_provider_confirm_status(
                client, job_id, provider, &symbol, &amount, &agent_id,
            ).await?;
            let amount_minimal = confirm_resp["amount"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("providerConfirmStatus 响应缺少 amount"))?
                .to_string();
            let currency = confirm_resp["currency"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("providerConfirmStatus 响应缺少 currency"))?
                .to_string();

            // Step 2b-verify: 校验 providerConfirmStatus.currency 与任务 tokenAddress 一致
            let task_resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;
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
            let escrow = &confirm_resp["escrow"];

            let escrow_contract = json_str(escrow, "escrowContract")?;
            let provider_addr = json_str(escrow, "provider")?;
            let arbitrator = json_str(escrow, "arbitrator")?;
            let receiver = json_str(escrow, "receiver")?;
            let submit_window = json_u64(escrow, "submitWindow")?;
            let dispute_window = json_u64(escrow, "disputeWindow")?;
            let arbitration_window = json_u64(escrow, "arbitrationWindow")?;
            let termination_window = json_u64(escrow, "terminationWindow")?;
            // expiredAt 可能是 unix 时间戳（如 "1777736662"）或 RFC 3339，统一转为 RFC 3339
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

            // Step 2c: sign_escrow — 本地 TEE 签名 EIP-3009 ReceiveWithAuthorization
            eprintln!("[debug] sign_escrow 入参:");
            eprintln!("  chain_id: {}", XLAYER_CHAIN_ID);
            eprintln!("  provider: {}", provider_addr);
            eprintln!("  receiver: {}", receiver);
            eprintln!("  arbitrator: {}", arbitrator);
            eprintln!("  currency: {}", currency);
            eprintln!("  escrow_contract: {}", escrow_contract);
            eprintln!("  amount: {}", amount_minimal);
            eprintln!("  submit_window: {}", submit_window);
            eprintln!("  dispute_window: {}", dispute_window);
            eprintln!("  arbitration_window: {}", arbitration_window);
            eprintln!("  termination_window: {}", termination_window);
            eprintln!("  hook: {}", hook);
            eprintln!("  hook_data: {}", hook_data);
            eprintln!("  salt: {}", salt);
            eprintln!("  expired_at: {}", expired_at);
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
            eprintln!("[debug] sign_escrow 返回:");
            eprintln!("  signature: {}", sign_output.signature);
            eprintln!("  validAfter: {}", sign_output.authorization.valid_after);
            eprintln!("  validBefore: {}", sign_output.authorization.valid_before);
            println!("✓ escrow payment签名完成");

            // Step 2d: accept → calldata → 签名 → 广播
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
                &agent_id,
            ).await?;

            // 构建 paymentVerify（escrow accept 专用，放入 bizContext）
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
                client, &resp["uopData"], &account_id, &address,
                job_id, signing::BizContext::JobAccept, &agent_id,
                payment_verify,
            ).await?;
            println!("✓ 已接受卖家 {provider}（担保支付），资金已托管");
            println!("  txHash: {tx_hash}");
        }
        PAYMENT_MODE_NON_ESCROW | "direct" => {
            // ── 非担保支付 (Charge / Direct) ────────────────────────
            let pid = payment_id.ok_or_else(|| {
                anyhow::anyhow!("非担保支付需要 --payment-id（由卖家通过 XMTP 传递）")
            })?;

            // 从任务详情获取金额和币种
            let task_resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;
            let task = &task_resp;
            let amount = task["tokenAmount"].as_str().unwrap_or("0").to_string();
            let symbol = task["paymentTokenSymbol"].as_str().unwrap_or("USDT").to_string();

            // 通过 `onchainos agent get --agent-ids` 查询 provider 钱包地址
            let provider_address = {
                let exe = std::env::current_exe()
                    .map_err(|e| anyhow::anyhow!("无法获取可执行文件路径: {e}"))?;
                let output = tokio::process::Command::new(&exe)
                    .args(["agent", "get", "--agent-ids", provider])
                    .output()
                    .await
                    .map_err(|e| anyhow::anyhow!("调用 agent get --agent-ids {provider} 失败: {e}"))?;
                let body: serde_json::Value = serde_json::from_slice(&output.stdout)
                    .map_err(|e| anyhow::anyhow!("解析 agent get 输出失败: {e}"))?;
                body["data"].as_array()
                    .and_then(|arr| arr.first())
                    .and_then(|x| x["list"].as_array())
                    .and_then(|list| list.iter()
                        .find(|a| a["agentId"].as_str() == Some(provider))
                        .and_then(|a| a["ownerAddress"].as_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string()))
                    .ok_or_else(|| anyhow::anyhow!(
                        "provider {provider} 的 ownerAddress 为空，无法确定收款地址"
                    ))?
            };

            // 通过 tokenDetail API 查询 token 合约地址和精度
            let (token_address, decimals) = fetch_token_detail(client, &symbol, &agent_id).await?;
            let amount_minimal = crate::commands::swap::readable_to_minimal_str(&amount, decimals)?;

            // Step 2a: a2a_pay::pay() — EIP-3009 支付签名
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

            // Step 2b: direct/accept → calldata(uopData) → 签名 → 广播
            let body = serde_json::json!({
                "providerAddress": provider_address,
                "providerAgentId": provider,
                "tokenSymbol": symbol,
                "tokenAmount": amount,
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
            println!("✓ 已接受卖家 {provider}（非担保支付），状态 → accepted");
            println!("  txHash: {tx_hash}");
        }
        PAYMENT_MODE_X402 => {
            // ── x402 支付：参数从缓存（/match 接口返回）获取 ────────
            let provider_info = super::negotiate::current(job_id)?
                .ok_or_else(|| anyhow::anyhow!("x402: 未找到当前 provider，请先执行 recommend"))?;
            let svc = provider_info.services.first()
                .ok_or_else(|| anyhow::anyhow!("x402: 当前 provider 无服务信息（services 为空）"))?;

            let sym = &svc.fee_token_symbol;
            if sym.is_empty() {
                bail!("x402: 服务信息中 feeTokenSymbol 为空");
            }
            let amt = svc.fee_amount;
            if amt <= 0.0 {
                bail!("x402: 服务信息中 feeAmount 无效 ({amt})");
            }
            let ep = &svc.endpoint;
            if ep.is_empty() {
                bail!("x402: 服务信息中 endpoint 为空");
            }

            // 检查 feeTokenSymbol 与任务创建时 currency 是否一致
            let task_resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;
            let task_currency = task_resp["paymentTokenSymbol"]
                .as_str().unwrap_or("");
            if !task_currency.is_empty() && !task_currency.eq_ignore_ascii_case(sym) {
                println!("⚠ 注意：Provider 要求的支付币种 ({sym}) 与任务发布时的币种 ({task_currency}) 不同");
                println!("  将以 Provider 要求的 {sym} 进行支付，金额: {amt} {sym}");
                println!("  如需取消，请 Ctrl+C 终止");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }

            // x402 Step 1-2：direct/accept 单签（paymentMode=2 时走 direct/accept）
            let amt_str = format!("{amt}");
            let body = serde_json::json!({
                "providerAddress": &provider_info.provider_address,
                "providerAgentId": provider,
                "tokenSymbol": sym,
                "tokenAmount": amt_str,
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
            println!("✓ 已接受卖家 {provider}（x402 支付），金额: {amt} {sym}");
            println!("  txHash: {tx_hash}");

            // x402 Step 3：调用 x402 支付（请求 endpoint → 402 → 签名 → 重放）
            println!("  x402: 开始调用 Provider endpoint 完成支付 ...");
            let flow_result = x402_flow::x402_request_sign_replay(
                client.http(),
                ep,
                Some(&address),
            ).await?;

            println!("✓ x402 支付完成");
            println!("  endpoint:  {ep}");
            println!("  HTTP 状态: {}", flow_result.response_status);
            if flow_result.response_status == 200 {
                println!("  服务响应: {}", serde_json::to_string_pretty(&flow_result.response_body)
                    .unwrap_or_else(|_| "ok".to_string()));
            }
        }
        other => {
            bail!("不支持的支付方式: {other}，可选: escrow / non_escrow / x402");
        }
    }

    // 清理本地协商状态
    if let Err(e) = super::negotiate::cleanup(job_id) {
        eprintln!("⚠ 清理协商状态失败（可忽略）: {e}");
    }

    Ok(())
}
