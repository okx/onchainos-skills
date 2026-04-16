use anyhow::{bail, Context, Result};
use clap::Subcommand;
use serde_json::Value;

use crate::client::ApiClient;
use crate::commands::Context as CliContext;
use crate::output;

const UPLOAD_PATH: &str = "/priapi/v1/aieco/im/attachments/xmtp/encrypted/upload";
const DOWNLOAD_PATH: &str = "/priapi/v1/aieco/im/attachments/xmtp/encrypted/download";

#[derive(Subcommand)]
pub enum ChatCommand {
    /// Upload an encrypted file attachment and receive a file key
    #[command(name = "file-upload")]
    FileUpload {
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
    /// Download an encrypted file attachment by file key
    #[command(name = "file-download")]
    FileDownload {
        /// File key returned from upload
        #[arg(long)]
        file_key: String,

        /// Agent ID
        #[arg(long)]
        agent_id: String,

        /// Output file path to write the downloaded bytes
        #[arg(long)]
        output: String,
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
    }
}

// ── Upload ───────────────────────────────────────────────────────────

/// POST /priapi/v1/aieco/im/attachments/xmtp/encrypted/upload — multipart form
///
/// Sends file bytes, agentId, and jobId.
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
        .text("agentId", agent_id.to_string())
        .text("jobId", job_id.to_string());

    client.post_multipart(UPLOAD_PATH, form).await
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
/// Downloads encrypted file bytes by fileKey and agentId.
/// Returns raw bytes (not JSON).
pub async fn fetch_download(
    client: &ApiClient,
    file_key: &str,
    agent_id: &str,
) -> Result<Vec<u8>> {
    let query = [("fileKey", file_key), ("agentId", agent_id)];
    client.get_bytes(DOWNLOAD_PATH, &query).await
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: ChatCommand,
    }

    // ── Upload CLI parsing ───────────────────────────────────────────

    #[test]
    fn cli_upload_all_required_args() {
        let cli = TestCli::parse_from([
            "test", "file-upload",
            "--file", "/tmp/test.bin",
            "--agent-id", "agent_123",
            "--job-id", "task_001",
        ]);
        match cli.command {
            ChatCommand::FileUpload { file, agent_id, job_id } => {
                assert_eq!(file, "/tmp/test.bin");
                assert_eq!(agent_id, "agent_123");
                assert_eq!(job_id, "task_001");
            }
            _ => panic!("expected FileUpload"),
        }
    }

    #[test]
    fn cli_upload_missing_file() {
        let result = TestCli::try_parse_from([
            "test", "file-upload",
            "--agent-id", "agent_123",
            "--job-id", "task_001",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_upload_missing_agent_id() {
        let result = TestCli::try_parse_from([
            "test", "file-upload",
            "--file", "/tmp/test.bin",
            "--job-id", "task_001",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_upload_missing_job_id() {
        let result = TestCli::try_parse_from([
            "test", "file-upload",
            "--file", "/tmp/test.bin",
            "--agent-id", "agent_123",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_upload_no_args() {
        let result = TestCli::try_parse_from(["test", "file-upload"]);
        assert!(result.is_err());
    }

    // ── Download CLI parsing ─────────────────────────────────────────

    #[test]
    fn cli_download_all_required_args() {
        let cli = TestCli::parse_from([
            "test", "file-download",
            "--file-key", "task_001-abc123",
            "--agent-id", "agent_123",
            "--output", "/tmp/downloaded.bin",
        ]);
        match cli.command {
            ChatCommand::FileDownload { file_key, agent_id, output } => {
                assert_eq!(file_key, "task_001-abc123");
                assert_eq!(agent_id, "agent_123");
                assert_eq!(output, "/tmp/downloaded.bin");
            }
            _ => panic!("expected FileDownload"),
        }
    }

    #[test]
    fn cli_download_missing_file_key() {
        let result = TestCli::try_parse_from([
            "test", "file-download",
            "--agent-id", "agent_123",
            "--output", "/tmp/out.bin",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_download_missing_agent_id() {
        let result = TestCli::try_parse_from([
            "test", "file-download",
            "--file-key", "abc",
            "--output", "/tmp/out.bin",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_download_missing_output() {
        let result = TestCli::try_parse_from([
            "test", "file-download",
            "--file-key", "abc",
            "--agent-id", "agent_123",
        ]);
        assert!(result.is_err());
    }
}
