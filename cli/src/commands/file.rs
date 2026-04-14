use anyhow::{bail, Context, Result};
use clap::Subcommand;
use reqwest::header::{HeaderValue, AUTHORIZATION};
use serde_json::Value;

use crate::client::{ApiClient, DEFAULT_BASE_URL};
use crate::output;

const UPLOAD_PATH: &str = "/priapi/v1/aieco/im/attachments/xmtp/encrypted/upload";

#[derive(Subcommand)]
pub enum FileCommand {
    /// Upload an encrypted file attachment and receive a CDN URL
    Upload {
        /// Path to the local file to upload
        #[arg(long)]
        file: String,

        /// Agent ID
        #[arg(long)]
        agent_id: String,

        /// Job ID
        #[arg(long)]
        job_id: String,
    },
}

pub async fn execute(cmd: FileCommand) -> Result<()> {
    match cmd {
        FileCommand::Upload {
            file,
            agent_id,
            job_id,
        } => cmd_upload(&file, &agent_id, &job_id).await,
    }
}

// ── Data-fetching layer ──────────────────────────────────────────────
// Separated from CLI dispatch for consistency with other command modules
// (e.g. market.rs fetch_price, token.rs fetch_search) and to allow
// reuse by a future MCP tool.

/// POST /priapi/v1/aieco/im/attachments/xmtp/encrypted/upload — multipart form
///
/// Sends file bytes, agentId, and jobId.
/// Returns the response data containing `fileKey`, `attachmentUrl`, and `fileSize`.
pub async fn fetch_upload(
    access_token: &str,
    file_name: &str,
    data: Vec<u8>,
    agent_id: &str,
    job_id: &str,
) -> Result<Value> {
    let file_part = reqwest::multipart::Part::bytes(data).file_name(file_name.to_string());

    let form = reqwest::multipart::Form::new()
        .part("file", file_part)
        .text("agentId", agent_id.to_string())
        .text("jobId", job_id.to_string());

    let base_url = option_env!("OKX_BASE_URL").unwrap_or(DEFAULT_BASE_URL);
    let url = format!("{}{}", base_url.trim_end_matches('/'), UPLOAD_PATH);

    let mut headers = ApiClient::anonymous_headers();
    // Remove Content-Type — reqwest sets it automatically for multipart
    headers.remove(reqwest::header::CONTENT_TYPE);
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", access_token))
            .context("invalid access token for header")?,
    );

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let resp = http
        .post(&url)
        .headers(headers)
        .multipart(form)
        .send()
        .await
        .context("upload request failed")?;

    parse_response(resp).await
}

/// Parse and validate the backend JSON response.
///
/// Expects the standard OKX envelope: `{ "code": 0, "data": { ... } }`.
/// Returns the `data` field on success.
async fn parse_response(resp: reqwest::Response) -> Result<Value> {
    let status = resp.status();
    if status.as_u16() >= 500 {
        bail!("server error (HTTP {})", status.as_u16());
    }

    let body: Value = resp
        .json()
        .await
        .context("failed to parse upload response")?;

    let code_ok = match &body["code"] {
        Value::String(s) => s == "0",
        Value::Number(n) => n.as_i64() == Some(0),
        _ => false,
    };
    if !code_ok {
        let code_str = match &body["code"] {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            other => other.to_string(),
        };
        let msg = body["msg"].as_str().unwrap_or("unknown error");
        bail!("upload failed (code={}): {}", code_str, msg);
    }

    Ok(body["data"].clone())
}

// ── CLI command handler ──────────────────────────────────────────────

async fn cmd_upload(
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

    // 2. Get JWT (handles refresh, AK fallback, etc.)
    let access_token =
        crate::commands::agentic_wallet::auth::ensure_tokens_refreshed().await?;

    // 3. Read file bytes
    let data = tokio::fs::read(file_path)
        .await
        .with_context(|| format!("failed to read file: {}", file_path))?;

    let file_name = std::path::Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload")
        .to_string();

    // 4. Upload
    let result = fetch_upload(&access_token, &file_name, data, agent_id, job_id).await?;

    // 5. Output result (fileKey, attachmentUrl, fileSize)
    output::success(&result);

    Ok(())
}
