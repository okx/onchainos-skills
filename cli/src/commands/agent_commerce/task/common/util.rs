//! common::util — 任务系统通用工具函数
//!
//! 收敛 task 模块里被多处复用、与具体业务无关的小工具，避免散落在各 mod / flow 中。
//! 后续新增的展示格式化、字符串归一化、时间换算等通用 helper 都放这里。

use anyhow::{bail, Result};
use chrono::{TimeZone, Utc};

use super::network::task_api_client::TaskApiClient;
use super::{PaymentMode, XLAYER_CHAIN_INDEX};

/// unix 秒 → 展示字符串。0 / 负数当未设置；正常值转 RFC 3339。
pub fn fmt_unix_secs(secs: Option<i64>) -> String {
    match secs {
        Some(n) if n > 0 => Utc
            .timestamp_opt(n, 0)
            .single()
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| n.to_string()),
        _ => "—".to_string(),
    }
}

// ─── JSON 提取工具 ──────────────────────────────────────────────────────

/// 从 JSON 对象提取字符串字段。
pub fn json_str(obj: &serde_json::Value, key: &str) -> Result<String> {
    obj[key]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("响应缺少 {key} 字段"))
        .map(|s| s.to_string())
}

/// 从 JSON 对象提取 u64 字段（兼容数字和字符串）。
pub fn json_u64(obj: &serde_json::Value, key: &str) -> Result<u64> {
    if let Some(n) = obj[key].as_u64() {
        return Ok(n);
    }
    if let Some(s) = obj[key].as_str() {
        return s
            .parse()
            .map_err(|_| anyhow::anyhow!("{key} 解析 u64 失败: {s}"));
    }
    bail!("响应缺少 {key} 字段")
}

// ─── Token 查询 ─────────────────────────────────────────────────────────

/// 通过 tokenDetail API 查询 token 合约地址和精度。
/// GET /priapi/v1/aieco/task/tokenDetail?symbol=<symbol>
/// 返回 (token_address, decimals)
pub async fn fetch_token_detail(client: &mut TaskApiClient, symbol: &str, agent_id: &str) -> Result<(String, u32)> {
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

// ─── 支付方式解析 ───────────────────────────────────────────────────────

/// 解析支付方式：CLI flag > 任务详情 paymentType
pub async fn resolve_payment_mode(
    client: &mut TaskApiClient,
    payment_mode: Option<&str>,
    job_id: &str,
    agent_id: &str,
) -> Result<PaymentMode> {
    match payment_mode {
        Some(m) => Ok(PaymentMode::from_str(m)),
        None => {
            let task_resp = client.get_with_identity(&client.task_path(job_id), agent_id).await?;
            let payment_type = task_resp["paymentType"].as_i64().unwrap_or(0) as i32;
            let mode = PaymentMode::from_int(payment_type);
            if mode == PaymentMode::None {
                eprintln!("⚠ 任务 paymentType={payment_type}，无法识别支付方式，默认使用 escrow");
                Ok(PaymentMode::Escrow)
            } else {
                eprintln!("ℹ --payment-mode 未传入，使用任务详情 paymentType: {} ({payment_type})", mode.as_str());
                Ok(mode)
            }
        }
    }
}

/// 解析复合 fee 字符串（如 "0.01 USDT"）→ (amount, symbol)
pub fn parse_composite_fee(fee: &str) -> Result<(f64, String)> {
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

// ─── x402 服务参数解析 ──────────────────────────────────────────────────

/// x402 三级 fallback 解析结果
pub struct X402ServiceParams {
    pub endpoint: String,
    pub fee_amount: f64,
    pub fee_token_symbol: String,
}

/// 解析 x402 服务参数：CLI flag > recommend 缓存 > identity service-list API > 报错
pub async fn resolve_x402_params(
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
    match crate::commands::agent_commerce::task::buyer::negotiate::current(job_id) {
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

// ─── Provider 地址查询 ──────────────────────────────────────────────────

/// 通过 `onchainos agent get --agent-ids` 查询 provider 钱包地址
pub async fn fetch_provider_address(provider: &str) -> Result<String> {
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
        ))
}

// ─── 余额预检 ──────────────────────────────────────────────────────────

/// 归一化 token symbol：Unicode 货币符号 → ASCII 等价字母，然后转大写。
/// 例：`USD₮0` → `USDT0`（₮ U+20AE → T）
fn normalize_token_symbol(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '₮' => 'T',
            _ => c,
        })
        .collect::<String>()
        .to_uppercase()
}

/// 调用 `onchainos wallet balance --chain 196` 查询 XLayer 余额，
/// 若指定代币余额不足则 bail，阻断后续流程。
pub async fn ensure_sufficient_balance(required: f64, currency: &str) -> Result<()> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("无法获取可执行文件路径: {e}"))?;

    let output = tokio::process::Command::new(&exe)
        .args(["wallet", "balance", "--chain", XLAYER_CHAIN_INDEX])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("余额查询失败: {e}"))?;

    if !output.status.success() {
        bail!("余额查询失败（exit {}），请检查登录态", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| anyhow::anyhow!("解析余额查询结果失败: {e}"))?;

    let currency_norm = normalize_token_symbol(currency);
    let details = parsed["data"]["details"].as_array();
    if let Some(details) = details {
        for detail in details {
            let assets = detail["tokenAssets"]
                .as_array()
                .or_else(|| detail["assets"].as_array());
            if let Some(assets) = assets {
                for asset in assets {
                    let symbol = asset["tokenSymbol"]
                        .as_str()
                        .or_else(|| asset["symbol"].as_str())
                        .unwrap_or("");
                    let sym_norm = normalize_token_symbol(symbol);
                    if sym_norm == currency_norm || sym_norm == format!("{currency_norm}0") {
                        let balance: f64 = asset["balance"]
                            .as_str()
                            .and_then(|s| s.parse().ok())
                            .or_else(|| asset["balance"].as_f64())
                            .unwrap_or(0.0);
                        if balance < required {
                            bail!(
                                "余额不足：当前 XLayer {symbol} 余额为 {balance}，\
                                 需要 {required} {currency}。请先充值后再操作"
                            );
                        }
                        return Ok(());
                    }
                }
            }
        }
    }

    bail!(
        "未查到 XLayer 上的 {currency} 余额，请确认账户已持有该代币并充值后重试"
    );
}
