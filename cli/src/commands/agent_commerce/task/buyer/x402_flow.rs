//! x402 支付流程
//!
//! 买家 accept 后调用 x402 完成支付。通过子进程调用 `onchainos payment x402-pay`
//! 复用 agentic_wallet 中的签名逻辑，本模块负责：
//! - 请求 Provider endpoint → 解码 HTTP 402
//! - 调用 CLI 签名
//! - 组装 payment header → 重放请求

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use serde_json::json;

// ─── 公共类型 ────────────────────────────────────────────────────────────

/// 解码后的 402 响应
#[derive(Debug, Clone)]
pub struct X402Payload {
    pub x402_version: i64,
    pub accepts: Vec<serde_json::Value>,
    pub resource: Option<serde_json::Value>,
    pub raw: serde_json::Value,
}

/// CLI 签名输出
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct X402PaymentProof {
    pub signature: String,
    pub authorization: serde_json::Value,
    #[serde(default, rename = "sessionCert")]
    pub session_cert: Option<String>,
}

/// 完整 x402 流程结果
#[derive(Debug)]
pub struct X402FlowResult {
    pub proof: X402PaymentProof,
    pub response_body: serde_json::Value,
    pub response_status: u16,
}

// ─── 402 解码 ────────────────────────────────────────────────────────────

/// 解码 HTTP 402 响应，提取 accepts 数组
///
/// - v2: `PAYMENT-REQUIRED` header (base64 JSON)
/// - v1: response body (直接 JSON)
pub fn decode_402_response(
    headers: &reqwest::header::HeaderMap,
    body: &str,
) -> Result<X402Payload> {
    // v2: PAYMENT-REQUIRED header
    if let Some(header_val) = headers.get("PAYMENT-REQUIRED") {
        let header_str = header_val
            .to_str()
            .context("PAYMENT-REQUIRED header is not valid UTF-8")?;
        let decoded_bytes = B64
            .decode(header_str)
            .context("PAYMENT-REQUIRED header is not valid base64")?;
        let parsed: serde_json::Value = serde_json::from_slice(&decoded_bytes)
            .context("PAYMENT-REQUIRED decoded content is not valid JSON")?;
        let version = parsed["x402Version"].as_i64().unwrap_or(2);
        let accepts = parsed["accepts"].as_array().cloned().unwrap_or_default();
        let resource = parsed.get("resource").cloned();
        return Ok(X402Payload {
            x402_version: version,
            accepts,
            resource,
            raw: parsed,
        });
    }

    // v1: response body
    let parsed: serde_json::Value =
        serde_json::from_str(body).context("402 response body is not valid JSON")?;
    let version = parsed["x402Version"].as_i64().unwrap_or(1);
    let accepts = parsed["accepts"].as_array().cloned().unwrap_or_default();
    Ok(X402Payload {
        x402_version: version,
        accepts,
        resource: None,
        raw: parsed,
    })
}

// ─── CLI 子进程签名 ──────────────────────────────────────────────────────

/// 调用 `onchainos payment x402-pay --accepts '<json>'` 子进程完成签名
///
/// 复用 agentic_wallet/payment.rs 中的 TEE 签名逻辑，避免重复代码。
pub async fn sign_via_cli(
    accepts_json: &str,
    from: Option<&str>,
) -> Result<X402PaymentProof> {
    let exe = std::env::current_exe().context("无法获取当前可执行文件路径")?;

    let mut cmd = tokio::process::Command::new(&exe);
    cmd.arg("payment").arg("x402-pay").arg("--accepts").arg(accepts_json);
    if let Some(addr) = from {
        cmd.arg("--from").arg(addr);
    }
    // 以 JSON 模式输出
    cmd.env("ONCHAINOS_OUTPUT", "json");
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let output = cmd.output().await.context("x402-pay 子进程启动失败")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("x402-pay 签名失败: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // CLI 输出可能包含 JSON 前的非 JSON 行，找到第一个 '{' 开始解析
    let json_start = stdout.find('{').ok_or_else(|| {
        anyhow!("x402-pay 输出中未找到 JSON: {stdout}")
    })?;
    let proof: X402PaymentProof =
        serde_json::from_str(&stdout[json_start..]).context("解析 x402-pay 输出失败")?;

    Ok(proof)
}

// ─── Header 组装 ─────────────────────────────────────────────────────────

/// 根据签名结果和 402 payload 组装 payment header
///
/// 返回 `(header_name, header_value)`:
/// - v2 → `("PAYMENT-SIGNATURE", base64(...))`
/// - v1 → `("X-PAYMENT", base64(...))`
pub fn assemble_payment_header(
    proof: &X402PaymentProof,
    payload: &X402Payload,
) -> Result<(String, String)> {
    let payment_payload = if payload.x402_version >= 2 {
        // v2: 选择对应 scheme 的 accepted entry
        // 由于 CLI 自动选择 scheme（exact > aggr_deferred > first），
        // 这里尝试匹配 session_cert 来判断 scheme
        let scheme = if proof.session_cert.is_some() {
            "aggr_deferred"
        } else {
            "exact"
        };

        let mut accepted = payload
            .accepts
            .iter()
            .find(|a| a["scheme"].as_str() == Some(scheme))
            .cloned()
            .or_else(|| payload.accepts.first().cloned())
            .ok_or_else(|| anyhow!("accepts array is empty"))?;

        // aggr_deferred: merge sessionCert into extra
        if let Some(ref cert) = proof.session_cert {
            if let Some(extra) = accepted.get_mut("extra") {
                extra["sessionCert"] = json!(cert);
            } else {
                accepted["extra"] = json!({ "sessionCert": cert });
            }
        }

        json!({
            "x402Version": payload.x402_version,
            "resource": payload.resource,
            "accepted": accepted,
            "payload": {
                "signature": proof.signature,
                "authorization": proof.authorization,
            }
        })
    } else {
        // v1
        let network = payload
            .accepts
            .first()
            .and_then(|a| a["network"].as_str())
            .unwrap_or("");
        let scheme = if proof.session_cert.is_some() {
            "aggr_deferred"
        } else {
            "exact"
        };
        json!({
            "x402Version": 1,
            "scheme": scheme,
            "network": network,
            "payload": {
                "signature": proof.signature,
                "authorization": proof.authorization,
            }
        })
    };

    let header_value = B64.encode(serde_json::to_string(&payment_payload)?);
    let header_name = if payload.x402_version >= 2 {
        "PAYMENT-SIGNATURE"
    } else {
        "X-PAYMENT"
    };

    Ok((header_name.to_string(), header_value))
}

// ─── 完整流程 ────────────────────────────────────────────────────────────

/// 完整 x402 支付流程：请求 endpoint → 处理 402 → 签名 → 组装 header → 重放
///
/// accept 后调用，复用 `onchainos payment x402-pay` CLI 完成签名。
pub async fn x402_request_sign_replay(
    http: &reqwest::Client,
    endpoint: &str,
    from: Option<&str>,
) -> Result<X402FlowResult> {
    // ── Step 1: 请求 endpoint ───────────────────────────────
    println!("  x402: 请求 endpoint {endpoint} ...");
    let resp = http
        .get(endpoint)
        .send()
        .await
        .map_err(|e| anyhow!("请求 x402 endpoint 失败: {e}"))?;

    let status = resp.status().as_u16();
    if status != 402 {
        let body: serde_json::Value = resp.json().await.unwrap_or(json!({}));
        bail!(
            "endpoint 返回 HTTP {status}（期望 402）: {}",
            serde_json::to_string_pretty(&body).unwrap_or_default()
        );
    }

    let headers = resp.headers().clone();
    let body_text = resp
        .text()
        .await
        .map_err(|e| anyhow!("读取 402 响应体失败: {e}"))?;

    // ── Step 2: 解码 402 ────────────────────────────────────
    let payload = decode_402_response(&headers, &body_text)?;
    if payload.accepts.is_empty() {
        bail!("402 响应中 accepts 为空");
    }
    let accepts_json = serde_json::to_string(&payload.accepts)?;
    println!("  x402: 已获取 402 payload（{} 个 accepts entry）", payload.accepts.len());

    // ── Step 3: 签名（子进程调用 onchainos payment x402-pay）──
    println!("  x402: 签名中 ...");
    let proof = sign_via_cli(&accepts_json, from).await?;
    println!("  x402: 签名完成");

    // ── Step 4: 组装 header ─────────────────────────────────
    let (header_name, header_value) = assemble_payment_header(&proof, &payload)?;

    // ── Step 5: 重放 ────────────────────────────────────────
    println!("  x402: 重放请求 ...");
    let replay_resp = http
        .get(endpoint)
        .header(&header_name, &header_value)
        .send()
        .await
        .map_err(|e| anyhow!("x402 重放请求失败: {e}"))?;

    let replay_status = replay_resp.status().as_u16();
    let response_body: serde_json::Value = replay_resp
        .json()
        .await
        .unwrap_or_else(|_| json!({ "status": "ok" }));

    Ok(X402FlowResult {
        proof,
        response_body,
        response_status: replay_status,
    })
}
