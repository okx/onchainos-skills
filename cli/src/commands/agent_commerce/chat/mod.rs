use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::client::ApiClient;
use crate::commands::Context as CliContext;
use crate::output;

const HEARTBEAT_PATH: &str = "/priapi/v5/wallet/agentic/agent-heartbeat";
const UPLOAD_PATH: &str = "/priapi/v1/aieco/im/attachments/xmtp/encrypted/upload";
const DOWNLOAD_PATH: &str = "/priapi/v1/aieco/im/attachments/xmtp/encrypted/download";
const SENSITIVE_WORDS_PATH: &str = "/priapi/v1/aieco/im/risk/a2a/sensitive/word/list";
const MESSAGE_ELIGIBLE_PATH: &str = "/priapi/v1/aieco/im/message/eligible";
const SYSTEM_CONFIG_PATH: &str = "/priapi/v1/aieco/im/xmtp/system-config";
const WAKEUP_NOTIFY_PATH: &str = "/priapi/v1/aieco/task/wakeupNotify";

/// Build the agenticId extra header slice from an agent ID string.
fn agent_commerce_headers(agent_id: &str) -> [(&str, &str); 2] {
    [("agenticId", agent_id), ("User-Agent", "onchainos-cli")]
}

/// Internal dispatch enum for chat commands — reshaped from `AgentCommand` variants.
pub enum ChatCommand {
    FileUpload {
        file: String,
        agent_id: String,
        job_id: String,
    },
    FileDownload {
        file_key: String,
        agent_id: String,
        output: String,
    },
    SensitiveWords,
    MessageEligible {
        agent_id: String,
        client_agent_id: String,
        provider_agent_id: String,
        job_id: String,
        group_id: String,
        direction: String,
        provider_security_rate: String,
    },
    SystemConfig,
    Heartbeat {
        chain_index: u64,
    },
    WakeupNotify {
        agent_ids: Vec<String>,
    },
}

pub async fn run(cmd: ChatCommand, ctx: &CliContext) -> Result<()> {
    match cmd {
        ChatCommand::FileUpload {
            file,
            agent_id,
            job_id,
        } => cmd_upload(ctx, &file, &agent_id, &job_id).await,
        ChatCommand::FileDownload {
            file_key,
            agent_id,
            output: output_path,
        } => cmd_download(ctx, &file_key, &agent_id, &output_path).await,
        ChatCommand::SensitiveWords => {
            let client = ctx.client_async().await?;
            output::success(fetch_sensitive_words(&client).await?);
            Ok(())
        }
        ChatCommand::MessageEligible {
            agent_id,
            client_agent_id,
            provider_agent_id,
            job_id,
            group_id,
            direction,
            provider_security_rate,
        } => {
            let client = ctx.client_async().await?;
            output::success(
                fetch_message_eligible(
                    &client,
                    &agent_id,
                    &client_agent_id,
                    &provider_agent_id,
                    &job_id,
                    &group_id,
                    &direction,
                    &provider_security_rate,
                )
                .await?,
            );
            Ok(())
        }
        ChatCommand::SystemConfig => {
            let client = ctx.client_async().await?;
            output::success(fetch_system_config(&client).await?);
            Ok(())
        }
        ChatCommand::Heartbeat { chain_index } => {
            let mut client = ctx.client_async().await?;
            output::success(fetch_heartbeat(&mut client, chain_index).await?);
            Ok(())
        }
        ChatCommand::WakeupNotify { agent_ids } => {
            if agent_ids.is_empty() {
                bail!("--agent-ids must contain at least one agent ID");
            }
            let mut client = ctx.client_async().await?;
            output::success(fetch_wakeup_notify(&mut client, &agent_ids).await?);
            Ok(())
        }
    }
}

// ── Upload ───────────────────────────────────────────────────────────

/// POST /priapi/v1/aieco/im/attachments/xmtp/encrypted/upload — multipart form
///
/// Sends file bytes and jobId as form fields, agenticId as header.
/// Returns the response data containing `fileKey` and `fileSize`.
pub async fn fetch_upload(
    client: &ApiClient,
    file_name: &str,
    data: Vec<u8>,
    agent_id: &str,
    job_id: &str,
) -> Result<Value> {
    let file_part = reqwest::multipart::Part::bytes(data)
        .file_name(file_name.to_string())
        .mime_str("application/octet-stream")
        .context("failed to set MIME type")?;

    let form = reqwest::multipart::Form::new()
        .part("file", file_part)
        .text("jobId", job_id.to_string());

    let headers = agent_commerce_headers(agent_id);
    let resp = client
        .post_multipart_raw(UPLOAD_PATH, form, Some(&headers))
        .await?;
    crate::client::handle_agent_commerce_response(resp).await
}

async fn cmd_upload(
    ctx: &CliContext,
    file_path: &str,
    agent_id: &str,
    job_id: &str,
) -> Result<()> {
    // 1. Validate file exists and is readable
    let metadata = std::fs::metadata(file_path)
        .with_context(|| format!("file not found: {}", file_path))?;
    if !metadata.is_file() {
        bail!("not a file: {}", file_path);
    }

    // 2. Read file bytes
    let data = tokio::fs::read(file_path)
        .await
        .with_context(|| format!("failed to read file: {}", file_path))?;

    let file_name = std::path::Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload")
        .to_string();

    // 3. Upload (ApiClient handles auth internally)
    let client = ctx.client_async().await?;
    let result = fetch_upload(&client, &file_name, data, agent_id, job_id).await?;

    // 4. Output result (fileKey, fileSize)
    output::success(&result);

    Ok(())
}

// ── Download ─────────────────────────────────────────────────────────

/// GET /priapi/v1/aieco/im/attachments/xmtp/encrypted/download
///
/// Downloads encrypted file bytes by fileKey, with agenticId as header.
/// Returns raw bytes (not JSON).
pub async fn fetch_download(
    client: &ApiClient,
    file_key: &str,
    agent_id: &str,
) -> Result<Vec<u8>> {
    let query = [("fileKey", file_key)];
    let headers = agent_commerce_headers(agent_id);
    client.get_bytes(DOWNLOAD_PATH, &query, Some(&headers)).await
}

async fn cmd_download(
    ctx: &CliContext,
    file_key: &str,
    agent_id: &str,
    output_path: &str,
) -> Result<()> {
    // 1. Download bytes
    let client = ctx.client_async().await?;
    let bytes = fetch_download(&client, file_key, agent_id).await?;

    // 2. Write to output file
    tokio::fs::write(output_path, &bytes)
        .await
        .with_context(|| format!("failed to write file: {}", output_path))?;

    // 3. Output result
    output::success(serde_json::json!({
        "fileKey": file_key,
        "outputPath": output_path,
        "fileSize": bytes.len()
    }));

    Ok(())
}

// ── Sensitive Words ──────────────────────────────────────────────────

/// GET /priapi/v1/aieco/im/risk/a2a/sensitive/word/list
///
/// Returns the sensitive word checklist for A2A risk filtering.
/// No agenticId header — endpoint is agent-agnostic.
pub async fn fetch_sensitive_words(client: &ApiClient) -> Result<Value> {
    let headers = [("User-Agent", "onchainos-cli")];
    let resp = client
        .get_with_headers_response(SENSITIVE_WORDS_PATH, &[], Some(&headers))
        .await?;
    crate::client::handle_agent_commerce_response(resp).await
}

// ── Message Eligible ─────────────────────────────────────────────────

/// GET /priapi/v1/aieco/im/message/eligible
///
/// Checks whether a message is eligible to be sent between agents.
/// agenticId sent as header.
///
/// Uses a command-local response handler (not the generic agent-commerce
/// handler): when the backend returns HTTP 2xx with a non-zero business
/// code, the response is reshaped into `{ eligible: false, reason: <msg> }`
/// so the caller sees a successful CLI invocation (`ok: true`) carrying an
/// explicit business rejection, rather than a generic CLI failure
/// indistinguishable from infra issues.
#[allow(clippy::too_many_arguments)]
pub async fn fetch_message_eligible(
    client: &ApiClient,
    agent_id: &str,
    client_agent_id: &str,
    provider_agent_id: &str,
    job_id: &str,
    group_id: &str,
    direction: &str,
    provider_security_rate: &str,
) -> Result<Value> {
    let headers = agent_commerce_headers(agent_id);
    let resp = client
        .get_with_headers_response(
            MESSAGE_ELIGIBLE_PATH,
            &[
                ("clientAgentId", client_agent_id),
                ("providerAgentId", provider_agent_id),
                ("jobId", job_id),
                ("groupId", group_id),
                ("direction", direction),
                ("providerSecurityRate", provider_security_rate),
            ],
            Some(&headers),
        )
        .await?;
    handle_message_eligible_response(resp).await
}

/// Reshape backend business rejections (HTTP 2xx + non-zero `code`) into
/// `{ eligible: false, reason: <msg> }`, while leaving infrastructure
/// failures (gateway errors, HTTP 5xx, non-JSON bodies, transport errors)
/// to bail. Mirrors `handle_agent_commerce_response` for the success and
/// infra branches; only the "HTTP 2xx + code != 0" branch differs.
async fn handle_message_eligible_response(resp: reqwest::Response) -> Result<Value> {
    let status = resp.status().as_u16();

    let is_gateway = resp
        .headers()
        .get(reqwest::header::SERVER)
        .and_then(|v| v.to_str().ok())
        .map(|s| {
            let s = s.to_lowercase();
            s.contains("openresty") || s.contains("nginx")
        })
        .unwrap_or(false);

    let body_bytes = resp.bytes().await.context("failed to read response body")?;
    let parsed: Option<Value> = if body_bytes.is_empty() {
        None
    } else {
        serde_json::from_slice(&body_bytes).ok()
    };

    if (200..300).contains(&status) {
        if let Some(ref body) = parsed {
            let code_ok = matches!(&body["code"], Value::String(s) if s == "0")
                || matches!(&body["code"], Value::Number(n) if n.as_i64() == Some(0));
            if code_ok {
                return Ok(body["data"].clone());
            }
            // HTTP 2xx + non-zero business code → backend says this
            // message is not eligible. Surface as a synthetic envelope
            // so the caller can forward `msg` to the counterparty.
            let msg = body["msg"].as_str().unwrap_or("");
            let detail = body["detailMsg"].as_str().unwrap_or("");
            let reason = if !detail.is_empty() {
                format!("{} — {}", msg, detail)
            } else {
                msg.to_string()
            };
            return Ok(serde_json::json!({
                "eligible": false,
                "reason": reason,
            }));
        }
    }

    if is_gateway {
        let reason = match status {
            413 => "payload too large",
            429 => "rate limited",
            502 => "bad gateway (backend unreachable)",
            503 => "service unavailable",
            504 => "gateway timeout",
            _ => "gateway rejected request",
        };
        bail!("Gateway error (HTTP {}): {}", status, reason);
    }

    if let Some(ref body) = parsed {
        let backend_code = match &body["code"] {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => "null".into(),
        };
        let msg = body["msg"].as_str().unwrap_or("unknown error");
        bail!(
            "API error (HTTP {}, backend_code={}): {}",
            status,
            backend_code,
            msg
        );
    }

    if status == 429 {
        bail!("Rate limited — retry with backoff (HTTP 429)");
    }
    if status >= 500 {
        bail!("Server error (HTTP {})", status);
    }
    if body_bytes.is_empty() {
        bail!("Empty response body (HTTP {})", status);
    }
    let text = String::from_utf8_lossy(&body_bytes);
    let trimmed: String = text.trim().chars().take(500).collect();
    bail!("HTTP {}: {}", status, trimmed)
}

// ── System Config ────────────────────────────────────────────────────

/// GET /priapi/v1/aieco/im/xmtp/system-config
///
/// Returns XMTP system config including system account sender addresses.
/// No agenticId header — endpoint is agent-agnostic.
pub async fn fetch_system_config(client: &ApiClient) -> Result<Value> {
    let headers = [("User-Agent", "onchainos-cli")];
    let resp = client
        .get_with_headers_response(SYSTEM_CONFIG_PATH, &[], Some(&headers))
        .await?;
    crate::client::handle_agent_commerce_response(resp).await
}

// ── Heartbeat ────────────────────────────────────────────────────────
// TODO: Confirm if endpoint is ready on beta for testing.
// Note: This endpoint is under /priapi/v5/wallet/agentic/ (wallet namespace),
//       unlike other chat commands which use /priapi/v1/aieco/im/.
//       No agenticId header needed — userId is extracted from JWT server-side.

/// POST /priapi/v5/wallet/agentic/agent-heartbeat
///
/// Reports online status for all agents owned by the current user on the
/// given chain. Server resolves userId from JWT, finds all addresses and
/// their agents, and batch-updates lastOnlineTime. Always returns success
/// even if the user has no addresses or agents.
pub async fn fetch_heartbeat(client: &mut ApiClient, chain_index: u64) -> Result<Value> {
    let body = serde_json::json!({ "chainIndex": chain_index });
    client.post(HEARTBEAT_PATH, &body).await
}

// ── Wakeup Notify ────────────────────────────────────────────────────

/// POST /priapi/v1/aieco/task/wakeupNotify
///
/// Wakes up all in-flight tasks under the given agent wallets by triggering
/// system notifications. Used after IM reconnect or process restart so the
/// agent can resume any task lifecycle messages it missed while offline.
/// Returns the list of in-flight jobs (jobId / buyerAgentId / providerAgentId / status).
///
/// Sends `agenticId` header set to the first agent ID, matching the convention
/// used by other agent-commerce endpoints.
pub async fn fetch_wakeup_notify(client: &mut ApiClient, agent_ids: &[String]) -> Result<Value> {
    let primary_agent_id = agent_ids
        .first()
        .context("agent_ids must contain at least one agent ID")?;
    let headers = agent_commerce_headers(primary_agent_id);
    let body = serde_json::json!({ "agentIds": agent_ids });
    client
        .post_with_headers(WAKEUP_NOTIFY_PATH, &body, Some(&headers))
        .await
}

