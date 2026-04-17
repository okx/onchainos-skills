use anyhow::{bail, Context, Result};
use clap::Subcommand;
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

/// Build the agenticId extra header slice from an agent ID string.
fn agent_commerce_headers(agent_id: &str) -> [(&str, &str); 2] {
    [("agenticId", agent_id), ("User-Agent", "onchainos-cli")]
}

#[derive(Subcommand)]
pub enum ChatCommand {
    /// Upload an encrypted file attachment and receive a file key
    #[command(name = "file-upload")]
    FileUpload {
        /// Path to the local file to upload
        #[arg(long)]
        file: String,

        /// Agent ID (sent as agenticId header)
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

        /// Agent ID (sent as agenticId header)
        #[arg(long)]
        agent_id: String,

        /// Output file path to write the downloaded bytes
        #[arg(long)]
        output: String,
    },
    /// Get sensitive word list for A2A risk filtering
    #[command(name = "sensitive-words")]
    SensitiveWords {
        /// Agent ID (sent as agenticId header)
        #[arg(long)]
        agent_id: String,
    },
    /// Check if a message is eligible to be sent
    #[command(name = "message-eligible")]
    MessageEligible {
        /// Agent ID (sent as agenticId header)
        #[arg(long)]
        agent_id: String,

        /// Client agent ID
        #[arg(long)]
        client_agent_id: String,

        /// Provider agent ID
        #[arg(long)]
        provider_agent_id: String,

        /// Job ID
        #[arg(long)]
        job_id: String,

        /// Group ID
        #[arg(long)]
        group_id: String,

        /// Direction: client_to_provider or provider_to_client
        #[arg(long)]
        direction: String,
    },
    /// Get XMTP system config (system account addresses)
    #[command(name = "system-config")]
    SystemConfig {
        /// Agent ID (sent as agenticId header)
        #[arg(long)]
        agent_id: String,
    },
    /// Send agent heartbeat to report online status
    #[command(name = "heartbeat")]
    Heartbeat {
        /// Chain index (e.g. 1 for Ethereum, 501 for Solana)
        #[arg(long)]
        chain_index: u64,
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
        ChatCommand::SensitiveWords { agent_id } => {
            let client = ctx.client_async().await?;
            output::success(fetch_sensitive_words(&client, &agent_id).await?);
            Ok(())
        }
        ChatCommand::MessageEligible {
            agent_id,
            client_agent_id,
            provider_agent_id,
            job_id,
            group_id,
            direction,
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
                )
                .await?,
            );
            Ok(())
        }
        ChatCommand::SystemConfig { agent_id } => {
            let client = ctx.client_async().await?;
            output::success(fetch_system_config(&client, &agent_id).await?);
            Ok(())
        }
        ChatCommand::Heartbeat { chain_index } => {
            let client = ctx.client_async().await?;
            output::success(fetch_heartbeat(&client, chain_index).await?);
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
/// agenticId sent as header.
pub async fn fetch_sensitive_words(client: &ApiClient, agent_id: &str) -> Result<Value> {
    let headers = agent_commerce_headers(agent_id);
    let resp = client
        .get_with_headers_raw(SENSITIVE_WORDS_PATH, &[], Some(&headers))
        .await?;
    crate::client::handle_agent_commerce_response(resp).await
}

// ── Message Eligible ─────────────────────────────────────────────────

/// GET /priapi/v1/aieco/im/message/eligible
///
/// Checks whether a message is eligible to be sent between agents.
/// agenticId sent as header.
#[allow(clippy::too_many_arguments)]
pub async fn fetch_message_eligible(
    client: &ApiClient,
    agent_id: &str,
    client_agent_id: &str,
    provider_agent_id: &str,
    job_id: &str,
    group_id: &str,
    direction: &str,
) -> Result<Value> {
    let headers = agent_commerce_headers(agent_id);
    let resp = client
        .get_with_headers_raw(
            MESSAGE_ELIGIBLE_PATH,
            &[
                ("clientAgentId", client_agent_id),
                ("providerAgentId", provider_agent_id),
                ("jobId", job_id),
                ("groupId", group_id),
                ("direction", direction),
            ],
            Some(&headers),
        )
        .await?;
    crate::client::handle_agent_commerce_response(resp).await
}

// ── System Config ────────────────────────────────────────────────────

/// GET /priapi/v1/aieco/im/xmtp/system-config
///
/// Returns XMTP system config including system account sender addresses.
/// agenticId sent as header.
pub async fn fetch_system_config(client: &ApiClient, agent_id: &str) -> Result<Value> {
    let headers = agent_commerce_headers(agent_id);
    let resp = client
        .get_with_headers_raw(SYSTEM_CONFIG_PATH, &[], Some(&headers))
        .await?;
    crate::client::handle_agent_commerce_response(resp).await
}

// ── Heartbeat ────────────────────────────────────────────────────────
// TODO: Confirm with backend team:
//   1. Is this endpoint ready on beta?
//   2. Should it also accept agenticId header like other chat commands?
//   3. Is chainIndex really the only param needed?
//   4. This endpoint is under /priapi/v5/wallet/agentic/ (wallet namespace),
//      unlike other chat commands which use /priapi/v1/aieco/im/.

/// POST /priapi/v5/wallet/agentic/agent-heartbeat
///
/// Reports agent online status. Server updates lastOnlineTime for the
/// agent matching the JWT's ownerAddress + chainIndex.
pub async fn fetch_heartbeat(client: &ApiClient, chain_index: u64) -> Result<Value> {
    let body = serde_json::json!({ "chainIndex": chain_index });
    client.post(HEARTBEAT_PATH, &body).await
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

    // ── Sensitive Words CLI parsing ──────────────────────────────────

    #[test]
    fn cli_sensitive_words_required_args() {
        let cli = TestCli::parse_from([
            "test", "sensitive-words",
            "--agent-id", "agent_123",
        ]);
        match cli.command {
            ChatCommand::SensitiveWords { agent_id } => {
                assert_eq!(agent_id, "agent_123");
            }
            _ => panic!("expected SensitiveWords"),
        }
    }

    #[test]
    fn cli_sensitive_words_missing_agent_id() {
        let result = TestCli::try_parse_from(["test", "sensitive-words"]);
        assert!(result.is_err());
    }

    // ── Message Eligible CLI parsing ─────────────────────────────────

    #[test]
    fn cli_message_eligible_all_required_args() {
        let cli = TestCli::parse_from([
            "test", "message-eligible",
            "--agent-id", "agent_1",
            "--client-agent-id", "client_1",
            "--provider-agent-id", "provider_1",
            "--job-id", "task_001",
            "--group-id", "group_1",
            "--direction", "client_to_provider",
        ]);
        match cli.command {
            ChatCommand::MessageEligible {
                agent_id,
                client_agent_id,
                provider_agent_id,
                job_id,
                group_id,
                direction,
            } => {
                assert_eq!(agent_id, "agent_1");
                assert_eq!(client_agent_id, "client_1");
                assert_eq!(provider_agent_id, "provider_1");
                assert_eq!(job_id, "task_001");
                assert_eq!(group_id, "group_1");
                assert_eq!(direction, "client_to_provider");
            }
            _ => panic!("expected MessageEligible"),
        }
    }

    #[test]
    fn cli_message_eligible_missing_direction() {
        let result = TestCli::try_parse_from([
            "test", "message-eligible",
            "--agent-id", "a",
            "--client-agent-id", "c",
            "--provider-agent-id", "p",
            "--job-id", "j",
            "--group-id", "g",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_message_eligible_missing_agent_id() {
        let result = TestCli::try_parse_from([
            "test", "message-eligible",
            "--client-agent-id", "c",
            "--provider-agent-id", "p",
            "--job-id", "j",
            "--group-id", "g",
            "--direction", "client_to_provider",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_message_eligible_missing_client_agent_id() {
        let result = TestCli::try_parse_from([
            "test", "message-eligible",
            "--agent-id", "a",
            "--provider-agent-id", "p",
            "--job-id", "j",
            "--group-id", "g",
            "--direction", "client_to_provider",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_message_eligible_missing_provider_agent_id() {
        let result = TestCli::try_parse_from([
            "test", "message-eligible",
            "--agent-id", "a",
            "--client-agent-id", "c",
            "--job-id", "j",
            "--group-id", "g",
            "--direction", "client_to_provider",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_message_eligible_missing_job_id() {
        let result = TestCli::try_parse_from([
            "test", "message-eligible",
            "--agent-id", "a",
            "--client-agent-id", "c",
            "--provider-agent-id", "p",
            "--group-id", "g",
            "--direction", "client_to_provider",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_message_eligible_missing_group_id() {
        let result = TestCli::try_parse_from([
            "test", "message-eligible",
            "--agent-id", "a",
            "--client-agent-id", "c",
            "--provider-agent-id", "p",
            "--job-id", "j",
            "--direction", "client_to_provider",
        ]);
        assert!(result.is_err());
    }

    // ── System Config CLI parsing ────────────────────────────────────

    #[test]
    fn cli_system_config_required_args() {
        let cli = TestCli::parse_from([
            "test", "system-config",
            "--agent-id", "agent_123",
        ]);
        match cli.command {
            ChatCommand::SystemConfig { agent_id } => {
                assert_eq!(agent_id, "agent_123");
            }
            _ => panic!("expected SystemConfig"),
        }
    }

    #[test]
    fn cli_system_config_missing_agent_id() {
        let result = TestCli::try_parse_from(["test", "system-config"]);
        assert!(result.is_err());
    }

    // ── Heartbeat CLI parsing ────────────────────────────────────────

    #[test]
    fn cli_heartbeat_required_args() {
        let cli = TestCli::parse_from([
            "test", "heartbeat",
            "--chain-index", "1",
        ]);
        match cli.command {
            ChatCommand::Heartbeat { chain_index } => {
                assert_eq!(chain_index, 1);
            }
            _ => panic!("expected Heartbeat"),
        }
    }

    #[test]
    fn cli_heartbeat_solana_chain() {
        let cli = TestCli::parse_from([
            "test", "heartbeat",
            "--chain-index", "501",
        ]);
        match cli.command {
            ChatCommand::Heartbeat { chain_index } => {
                assert_eq!(chain_index, 501);
            }
            _ => panic!("expected Heartbeat"),
        }
    }

    #[test]
    fn cli_heartbeat_missing_chain_index() {
        let result = TestCli::try_parse_from(["test", "heartbeat"]);
        assert!(result.is_err());
    }
}
