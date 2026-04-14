use anyhow::{bail, Context, Result};
use clap::Subcommand;
use serde_json::Value;

use crate::client::ApiClient;
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

pub async fn execute(ctx: &super::Context, cmd: FileCommand) -> Result<()> {
    match cmd {
        FileCommand::Upload {
            file,
            agent_id,
            job_id,
        } => cmd_upload(ctx, &file, &agent_id, &job_id).await,
    }
}

/// POST /priapi/v1/aieco/im/attachments/xmtp/encrypted/upload — multipart form
///
/// Sends file bytes, agentId, and jobId.
/// Returns the response data containing `fileKey`, `attachmentUrl`, and `fileSize`.
pub async fn fetch_upload(
    client: &ApiClient,
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

    client.post_multipart(UPLOAD_PATH, form).await
}

async fn cmd_upload(
    ctx: &super::Context,
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

    // 4. Output result (fileKey, attachmentUrl, fileSize)
    output::success(&result);

    Ok(())
}
