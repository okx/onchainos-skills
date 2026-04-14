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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: FileCommand,
    }

    // ── CLI argument parsing ─────────────────────────────────────────

    #[test]
    fn cli_upload_all_required_args() {
        let cli = TestCli::parse_from([
            "test", "upload",
            "--file", "/tmp/test.bin",
            "--agent-id", "agent_123",
            "--job-id", "task_001",
        ]);
        match cli.command {
            FileCommand::Upload { file, agent_id, job_id } => {
                assert_eq!(file, "/tmp/test.bin");
                assert_eq!(agent_id, "agent_123");
                assert_eq!(job_id, "task_001");
            }
        }
    }

    #[test]
    fn cli_upload_missing_file() {
        let result = TestCli::try_parse_from([
            "test", "upload",
            "--agent-id", "agent_123",
            "--job-id", "task_001",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_upload_missing_agent_id() {
        let result = TestCli::try_parse_from([
            "test", "upload",
            "--file", "/tmp/test.bin",
            "--job-id", "task_001",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_upload_missing_job_id() {
        let result = TestCli::try_parse_from([
            "test", "upload",
            "--file", "/tmp/test.bin",
            "--agent-id", "agent_123",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_upload_no_args() {
        let result = TestCli::try_parse_from(["test", "upload"]);
        assert!(result.is_err());
    }
}
