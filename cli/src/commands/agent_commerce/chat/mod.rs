use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::commands::Context as CliContext;
use crate::output;
use crate::wallet_api::{ApiCodeError, WalletApiClient};

const HEARTBEAT_PATH: &str = "/priapi/v5/wallet/agentic/agent-heartbeat";
const UPLOAD_PATH: &str = "/priapi/v1/aieco/im/attachments/xmtp/encrypted/upload";
const DOWNLOAD_PATH: &str = "/priapi/v1/aieco/im/attachments/xmtp/encrypted/download";
const SENSITIVE_WORDS_PATH: &str = "/priapi/v1/aieco/im/risk/a2a/sensitive/word/list";
const MESSAGE_ELIGIBLE_PATH: &str = "/priapi/v1/aieco/im/message/eligible";
const SYSTEM_CONFIG_PATH: &str = "/priapi/v1/aieco/im/xmtp/system-config";
const WAKEUP_NOTIFY_PATH: &str = "/priapi/v1/aieco/task/wakeupNotify";

/// Build the agenticId extra header slice from an agent ID string.
fn agent_commerce_headers(agent_id: &str) -> [(&str, &str); 1] {
    [("agenticId", agent_id)]
}

fn wallet_client(ctx: &CliContext) -> Result<WalletApiClient> {
    WalletApiClient::with_base_url(ctx.base_url_override.as_deref())
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
        client_communication_address: String,
        provider_communication_address: String,
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
            let access_token = ensure_tokens_refreshed().await?;
            let mut client = wallet_client(ctx)?;
            output::success(fetch_sensitive_words(&mut client, &access_token).await?);
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
            client_communication_address,
            provider_communication_address,
        } => {
            let access_token = ensure_tokens_refreshed().await?;
            let mut client = wallet_client(ctx)?;
            output::success(
                fetch_message_eligible(
                    &mut client,
                    &access_token,
                    &agent_id,
                    &client_agent_id,
                    &provider_agent_id,
                    &job_id,
                    &group_id,
                    &direction,
                    &provider_security_rate,
                    &client_communication_address,
                    &provider_communication_address,
                )
                .await?,
            );
            Ok(())
        }
        ChatCommand::SystemConfig => {
            let access_token = ensure_tokens_refreshed().await?;
            let mut client = wallet_client(ctx)?;
            output::success(fetch_system_config(&mut client, &access_token).await?);
            Ok(())
        }
        ChatCommand::Heartbeat { chain_index } => {
            let access_token = ensure_tokens_refreshed().await?;
            let mut client = wallet_client(ctx)?;
            output::success(fetch_heartbeat(&mut client, &access_token, chain_index).await?);
            Ok(())
        }
        ChatCommand::WakeupNotify { agent_ids } => {
            if agent_ids.is_empty() {
                bail!("--agent-ids must contain at least one agent ID");
            }
            let access_token = ensure_tokens_refreshed().await?;
            let mut client = wallet_client(ctx)?;
            output::success(fetch_wakeup_notify(&mut client, &access_token, &agent_ids).await?);
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
    client: &WalletApiClient,
    access_token: &str,
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
    client
        .post_authed_multipart_with_headers(UPLOAD_PATH, access_token, form, Some(&headers))
        .await
}

async fn cmd_upload(ctx: &CliContext, file_path: &str, agent_id: &str, job_id: &str) -> Result<()> {
    // 1. Validate file exists and is readable
    let metadata =
        std::fs::metadata(file_path).with_context(|| format!("file not found: {}", file_path))?;
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

    // 3. Upload through the wallet client auth path.
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;
    let result = fetch_upload(&client, &access_token, &file_name, data, agent_id, job_id).await?;

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
    client: &mut WalletApiClient,
    access_token: &str,
    file_key: &str,
    agent_id: &str,
) -> Result<Vec<u8>> {
    let query = [("fileKey", file_key)];
    let headers = agent_commerce_headers(agent_id);
    client
        .get_authed_bytes_with_headers(DOWNLOAD_PATH, access_token, &query, Some(&headers))
        .await
}

async fn cmd_download(
    ctx: &CliContext,
    file_key: &str,
    agent_id: &str,
    output_path: &str,
) -> Result<()> {
    // 1. Download bytes
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;
    let bytes = fetch_download(&mut client, &access_token, file_key, agent_id).await?;

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
pub async fn fetch_sensitive_words(
    client: &mut WalletApiClient,
    access_token: &str,
) -> Result<Value> {
    client.get_authed(SENSITIVE_WORDS_PATH, access_token, &[]).await
}

// ── Message Eligible ─────────────────────────────────────────────────

/// GET /priapi/v1/aieco/im/message/eligible
///
/// Checks whether a message is eligible to be sent between agents.
/// agenticId sent as header.
///
/// Only a genuine backend verdict — HTTP 2xx with a non-zero business
/// code — is reshaped into `{ eligible: false, reason: <msg> }` (`ok: true`).
/// Technical failures (auth code 50114, non-2xx statuses, rate limits,
/// transport errors) propagate as CLI errors (`ok: false`) so the caller
/// treats the check as unavailable and lets communication proceed, instead
/// of mistaking an expired token for a messaging ban.
#[allow(clippy::too_many_arguments)]
pub async fn fetch_message_eligible(
    client: &mut WalletApiClient,
    access_token: &str,
    agent_id: &str,
    client_agent_id: &str,
    provider_agent_id: &str,
    job_id: &str,
    group_id: &str,
    direction: &str,
    provider_security_rate: &str,
    client_communication_address: &str,
    provider_communication_address: &str,
) -> Result<Value> {
    let headers = agent_commerce_headers(agent_id);
    let result = client
        .get_authed_with_headers(
            MESSAGE_ELIGIBLE_PATH,
            access_token,
            &[
                ("clientAgentId", client_agent_id),
                ("providerAgentId", provider_agent_id),
                ("jobId", job_id),
                ("groupId", group_id),
                ("direction", direction),
                ("providerSecurityRate", provider_security_rate),
                ("clientCommunicationAddress", client_communication_address),
                (
                    "providerCommunicationAddress",
                    provider_communication_address,
                ),
            ],
            Some(&headers),
        )
        .await;

    match result {
        Ok(data) => Ok(data),
        Err(err) => {
            if let Some(api_err) = err.downcast_ref::<ApiCodeError>() {
                if is_business_rejection(api_err) {
                    return Ok(serde_json::json!({
                        "eligible": false,
                        "reason": api_err.msg,
                    }));
                }
            }
            Err(err)
        }
    }
}

/// True only when the backend actually evaluated the eligibility question
/// and rejected it: HTTP 2xx carrying a non-zero business code. Auth
/// failures (code 50114 — not logged in / token expired) and non-2xx
/// responses are infrastructure problems, not verdicts.
fn is_business_rejection(api_err: &ApiCodeError) -> bool {
    (200..300).contains(&api_err.http_status) && api_err.code != "50114"
}

// ── System Config ────────────────────────────────────────────────────

/// GET /priapi/v1/aieco/im/xmtp/system-config
///
/// Returns XMTP system config including system account sender addresses.
/// No agenticId header — endpoint is agent-agnostic.
pub async fn fetch_system_config(
    client: &mut WalletApiClient,
    access_token: &str,
) -> Result<Value> {
    client.get_authed(SYSTEM_CONFIG_PATH, access_token, &[]).await
}

// ── Heartbeat ────────────────────────────────────────────────────────

/// POST /priapi/v5/wallet/agentic/agent-heartbeat
///
/// Reports online status for all agents owned by the current user on the
/// given chain. Server resolves userId from JWT, finds all addresses and
/// their agents, and batch-updates lastOnlineTime. Always returns success
/// even if the user has no addresses or agents.
pub async fn fetch_heartbeat(
    client: &mut WalletApiClient,
    access_token: &str,
    chain_index: u64,
) -> Result<Value> {
    let body = serde_json::json!({ "chainIndex": chain_index });
    client
        .post_authed(HEARTBEAT_PATH, access_token, &body)
        .await
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
pub async fn fetch_wakeup_notify(
    client: &mut WalletApiClient,
    access_token: &str,
    agent_ids: &[String],
) -> Result<Value> {
    let primary_agent_id = agent_ids
        .first()
        .context("agent_ids must contain at least one agent ID")?;
    let headers = agent_commerce_headers(primary_agent_id);
    let body = serde_json::json!({ "agentIds": agent_ids });
    client
        .post_authed_with_headers(WAKEUP_NOTIFY_PATH, access_token, &body, Some(&headers))
        .await
}
