//! 确认接单 + Fund
//!
//! 买家动作：确认接单（担保 / 非担保 / x402）— onchainos task confirm-accept
//!
//! 流程：
//! 1. setPaymentMode（单签上链）
//! 2. 按支付方式分支：
//!    - escrow:      providerConfirmStatus → a2a_pay::sign_escrow → accept(signatureData) → sign uop → broadcast
//!    - non_escrow:  a2a_pay::pay() → direct/accept → sign uop → broadcast
//!    - x402:        setPaymentMode 后 return（事件驱动：job_payment_mode_changed → Agent 做 x402 endpoint → direct-accept → job_accepted → complete）
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


/// x402 三级 fallback 解析结果
struct X402ServiceParams {
    endpoint: String,
    fee_amount: f64,
    fee_token_symbol: String,
}

/// 解析 x402 服务参数：CLI flag > recommend 缓存 > identity service-list API > 报错
async fn resolve_x402_params(
    job_id: &str,
    provider_agent_id: &str,
    cli_endpoint: Option<&str>,
    cli_token_symbol: Option<&str>,
    cli_token_amount: Option<&str>,
) -> Result<X402ServiceParams> {
    // Tier 1: CLI flags 全部提供
    if let (Some(ep), Some(sym), Some(amt_str)) = (cli_endpoint, cli_token_symbol, cli_token_amount) {
        let amt: f64 = amt_str.parse()
            .map_err(|_| anyhow::anyhow!("--token-amount 格式错误: {amt_str}"))?;
        eprintln!("ℹ x402: 使用 CLI 参数 endpoint={ep}, token={sym}, amount={amt}");
        return Ok(X402ServiceParams {
            endpoint: ep.to_string(),
            fee_amount: amt,
            fee_token_symbol: sym.to_string(),
        });
    }

    // Tier 2: recommend 缓存
    match super::negotiate::current(job_id) {
        Ok(Some(pi)) => {
            if let Some(svc) = pi.services.first() {
                if !svc.endpoint.is_empty() && svc.fee_amount > 0.0 && !svc.fee_token_symbol.is_empty() {
                    eprintln!("ℹ x402: 使用 recommend 缓存 endpoint={}, token={}, amount={}",
                        svc.endpoint, svc.fee_token_symbol, svc.fee_amount);
                    return Ok(X402ServiceParams {
                        endpoint: cli_endpoint.unwrap_or(&svc.endpoint).to_string(),
                        fee_amount: cli_token_amount
                            .and_then(|a| a.parse().ok())
                            .unwrap_or(svc.fee_amount),
                        fee_token_symbol: cli_token_symbol
                            .unwrap_or(&svc.fee_token_symbol)
                            .to_string(),
                    });
                }
            }
            eprintln!("⚠ x402: recommend 缓存中 services 为空或字段缺失，尝试 service-list API");
        }
        Ok(None) => eprintln!("⚠ x402: recommend 缓存无当前 provider，尝试 service-list API"),
        Err(e) => eprintln!("⚠ x402: 读取 recommend 缓存失败 ({e})，尝试 service-list API"),
    }

    // Tier 3: identity service-list API
    let params = fetch_x402_service_from_identity(provider_agent_id).await?;
    Ok(X402ServiceParams {
        endpoint: cli_endpoint.unwrap_or(&params.endpoint).to_string(),
        fee_amount: cli_token_amount
            .and_then(|a| a.parse().ok())
            .unwrap_or(params.fee_amount),
        fee_token_symbol: cli_token_symbol
            .unwrap_or(&params.fee_token_symbol)
            .to_string(),
    })
}

/// 通过 `onchainos agent service-list` 查询 provider 的 A2MCP 服务信息
async fn fetch_x402_service_from_identity(provider_agent_id: &str) -> Result<X402ServiceParams> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("无法获取可执行文件路径: {e}"))?;
    let output = tokio::process::Command::new(&exe)
        .args(["agent", "service-list", "--agent-id", provider_agent_id])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("调用 agent service-list --agent-id {provider_agent_id} 失败: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("x402 service-list 查询失败 (exit {}): {stderr}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let body: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| anyhow::anyhow!("解析 service-list 输出失败: {e}"))?;

    let services = body["data"].as_array()
        .or_else(|| body["data"]["services"].as_array())
        .or_else(|| body["data"]["list"].as_array())
        .ok_or_else(|| anyhow::anyhow!(
            "x402: service-list 响应中未找到 services 数组，provider={provider_agent_id}"
        ))?;

    let svc = services.iter()
        .find(|s| {
            let stype = s["servicetype"].as_str()
                .or_else(|| s["serviceType"].as_str())
                .unwrap_or("");
            stype.eq_ignore_ascii_case("A2MCP")
        })
        .ok_or_else(|| anyhow::anyhow!(
            "x402: provider {provider_agent_id} 无 A2MCP 类型服务"
        ))?;

    let endpoint = svc["endpoint"].as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("x402: service-list 中 A2MCP 服务 endpoint 为空"))?
        .to_string();

    let (fee_amount, fee_token_symbol) = if let Some(amt) = svc["feeAmount"].as_f64() {
        let sym = svc["feeTokenSymbol"].as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("x402: service-list 中 feeAmount 存在但 feeTokenSymbol 缺失，无法确定支付代币"))?
            .to_string();
        (amt, sym)
    } else {
        let fee_str = svc["fee"].as_str().unwrap_or("");
        parse_composite_fee(fee_str)?
    };

    eprintln!("ℹ x402: 从 service-list API 获取 endpoint={endpoint}, token={fee_token_symbol}, amount={fee_amount}");
    Ok(X402ServiceParams { endpoint, fee_amount, fee_token_symbol })
}

/// 解析复合 fee 字符串（如 "0.01 USDT"）→ (amount, symbol)
fn parse_composite_fee(fee: &str) -> Result<(f64, String)> {
    let fee = fee.trim();
    if fee.is_empty() {
        bail!("x402: service fee 字段为空");
    }
    let parts: Vec<&str> = fee.split_whitespace().collect();
    match parts.len() {
        2 => {
            let amt: f64 = parts[0].parse()
                .map_err(|_| anyhow::anyhow!("x402: fee 金额解析失败: {}", parts[0]))?;
            Ok((amt, parts[1].to_string()))
        }
        1 => {
            let numeric_end = fee.find(|c: char| c.is_alphabetic()).unwrap_or(fee.len());
            if numeric_end >= fee.len() {
                bail!("x402: fee 字段只有金额没有币种: {fee}，无法确定支付代币");
            }
            let amt: f64 = fee[..numeric_end].parse()
                .map_err(|_| anyhow::anyhow!("x402: fee 金额解析失败: {fee}"))?;
            let sym = fee[numeric_end..].to_string();
            Ok((amt, sym))
        }
        _ => bail!("x402: fee 格式无法解析: {fee}"),
    }
}

/// confirm-accept — 确认接受卖家
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub async fn handle_confirm_accept(
    client: &mut TaskApiClient,
    job_id: &str,
    provider: &str,
    payment_mode: Option<&str>,
    payment_id: Option<&str>,
    token_symbol: Option<&str>,
    token_amount: Option<&str>,
    endpoint: Option<&str>,
    resume: bool,
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

    // ── Step 0: x402 提前解析服务参数（余额预检 + 执行共用）──────────
    let x402_resolved = if payment_mode.as_str() == PAYMENT_MODE_X402 {
        Some(resolve_x402_params(job_id, provider, endpoint, token_symbol, token_amount).await?)
    } else {
        None
    };

    // ── Step 0.5: 余额预检（余额不足则阻断）──────────────────────────
    {
        let pm = payment_mode.as_str();
        if pm == PAYMENT_MODE_X402 {
            let x402 = x402_resolved.as_ref().unwrap();
            if x402.fee_amount > 0.0 && !x402.fee_token_symbol.is_empty() {
                common::ensure_sufficient_balance(x402.fee_amount, &x402.fee_token_symbol).await?;
            }
        } else {
            // escrow / non_escrow: CLI > 协商记录 > bail
            let agreed = negotiate::load_agreed(job_id)?;
            let sym = match token_symbol {
                Some(s) => s.to_string(),
                None => match &agreed {
                    Some((sym, _)) => sym.clone(),
                    None => bail!("需要 --token-symbol 或先执行 save-agreed 保存协商结果"),
                },
            };
            let amt: f64 = match token_amount {
                Some(a) => a.parse().map_err(|_| anyhow::anyhow!("--token-amount 格式错误"))?,
                None => match &agreed {
                    Some((_, amt)) => amt.parse().unwrap_or(0.0),
                    None => bail!("需要 --token-amount 或先执行 save-agreed 保存协商结果"),
                },
            };
            if amt > 0.0 {
                common::ensure_sufficient_balance(amt, &sym).await?;
            }
        }
    }

    // ── Step 1: setPaymentMode（单签 + 广播上链）──────────────────────
    // --resume 跳过 setPaymentMode，由 job_payment_mode_changed 事件触发时使用
    if resume && payment_mode.as_str() == PAYMENT_MODE_X402 {
        bail!("x402 不支持 --resume，请用 onchainos agent task-402-pay 执行 x402 阶段 2");
    }
    if !resume {
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

        // escrow / non_escrow: 事件驱动，等待 job_payment_mode_changed 通知后 --resume 继续
        let msg = format!(
            "setPaymentMode({payment_mode}) 完成。provider={provider}",
        );
        let next = format!(
            "等待 job_payment_mode_changed 系统通知 → onchainos agent confirm-accept {job_id} --provider {provider} --payment-mode {payment_mode} --resume"
        );
        crate::output::confirming(&msg, &next);
        return Ok(());
    }

    // ── Step 2: 按支付方式分支处理 ──────────────────────────────────
    eprintln!("[debug] payment_mode 最终值: '{payment_mode}'");
    match payment_mode.as_str() {
        PAYMENT_MODE_ESCROW => {
            // ── 担保支付 (Escrow) ───────────────────────────────────
            // 流程：providerConfirmStatus → sign_escrow(TEE 签名) → accept → broadcast

            // Step 2a: 从协商结果获取金额和币种
            // 优先级：CLI flag > 本地协商记录(negotiate-state) > 报错
            let agreed: Option<(String, String)> = negotiate::load_agreed(job_id)?;
            let symbol = match token_symbol {
                Some(s) => s.to_string(),
                None => match &agreed {
                    Some((sym, _)) => {
                        eprintln!("ℹ --token-symbol 未传入，使用本地协商记录: {sym}");
                        sym.clone()
                    }
                    None => bail!("escrow 模式需要 --token-symbol 或先执行 save-agreed 保存协商结果"),
                },
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

            // 获取金额和币种：CLI flag > 本地协商记录 > 报错
            let agreed: Option<(String, String)> = negotiate::load_agreed(job_id)?;
            let symbol = match token_symbol {
                Some(s) => s.to_string(),
                None => match &agreed {
                    Some((sym, _)) => {
                        eprintln!("ℹ --token-symbol 未传入，使用本地协商记录: {sym}");
                        sym.clone()
                    }
                    None => bail!("non_escrow 模式需要 --token-symbol 或先执行 save-agreed 保存协商结果"),
                },
            };
            let amount = match token_amount {
                Some(a) => a.to_string(),
                None => match &agreed {
                    Some((_, amt)) => {
                        eprintln!("ℹ --token-amount 未传入，使用本地协商记录: {amt}");
                        amt.clone()
                    }
                    None => bail!("non_escrow 模式需要 --token-amount 或先执行 save-agreed 保存协商结果"),
                },
            };

            // 通过 `onchainos agent get --agent-ids` 查询 provider 钱包地址
            let provider_address = {
                let exe = std::env::current_exe()
                    .map_err(|e| anyhow::anyhow!("无法获取可执行文件路径: {e}"))?;
                let output = tokio::process::Command::new(&exe)
                    .args(["agent", "get", "--agent-ids", provider])
                    .output()
                    .await
                    .map_err(|e| anyhow::anyhow!("调用 agent get --agent-ids {provider} 失败: {e}"))?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                let body: serde_json::Value = serde_json::from_str(&stdout)
                    .map_err(|e| anyhow::anyhow!("解析 agent get 输出失败: {e}"))?;
                body["data"].as_array()
                    .and_then(|arr| arr.first())
                    .and_then(|x| x["list"].as_array())
                    .or_else(|| body["data"]["list"].as_array())
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
            bail!("x402 流程在 setPaymentMode 后结束，不应到达此分支；请用 onchainos agent task-402-pay 执行阶段 2");
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
///
/// 流程：
/// 1. 调用 x402_pay（解析 accepts → TEE/session 签名）→ 得到 Payment Credential
/// 2. 执行 direct/accept → 签名 uopData → 广播上链
/// 3. 用 Payment Credential 组装 header，重放 endpoint 获取交付物
/// 4. 输出 replay 结果 + Payment Credential + txHash
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

    // ── Step 1: x402_pay 签名 ──────────────────────────────────────
    eprintln!("[task-402-pay] Step 1: x402_pay 签名");
    eprintln!("[task-402-pay] accepts: {accepts}");
    let proof = payment::x402_pay_from_accepts(accepts, from.map(|s| s.to_string())).await?;
    eprintln!("[task-402-pay] x402_pay 完成: signature={}", proof.signature);

    // ── Step 2: direct/accept 上链 ─────────────────────────────────
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

    // ── Step 3: GET endpoint → 402 → 组装 header → 重放 ────────────
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

    // ── Step 4: 输出完整结果 ───────────────────────────────────────
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
