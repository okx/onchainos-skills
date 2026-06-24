//! Synchronous wrappers around the external `okx-a2a` CLI binary.
//!
//! Shared by buyer / provider / evaluator sub-session flows that need to
//! probe session state (sessionKey, jobId, agentId, etc.) without going
//! through the MCP host's `xmtp_*` tools. All calls are blocking
//! (std::process::Command); spawn cost is ~100-150ms per invocation, so
//! consumers should minimize calls in hot paths.

use anyhow::Result;
use std::process::Command;

// ── User-facing notifications ──────────────────────────────────────────────

/// Bridge equivalent: `xmtp_dispatch_user '{"content": "..."}'`
/// Fire-and-forget. Uses `.output()` (not `.status()`) so the child's stdout
/// is captured and discarded — otherwise the `--json` payload would leak into
/// our parent's stdout and contaminate the playbook that `agent next-action`
/// prints to its caller (host runtime / LLM).
/// `--job-id` / `--session-key` are not passed — the CLI falls back to env vars.
///
/// `content` literal `\n` sequences are converted to real newlines so callers
/// can pass shell-safe single-line strings.
/// On success, prints `OK` to stdout for CLI callers.
pub fn user_notify(content: &str) -> Result<()> {
    let content = content.replace("\\n", "\n");
    let out = Command::new("okx-a2a")
        .args(["user", "notify", "--content", &content, "--json"])
        .output()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("okx-a2a user notify exit {status}: {stderr}", status = out.status);
    }
    println!("OK");
    Ok(())
}

/// Bridge equivalent: `xmtp_prompt_user '{"llmContent": "...", "userContent": "..."}'`
/// Sub-side replacement for the MCP `xmtp_prompt_user` tool. Pushes a
/// decision card into the okx-a2a CLI's SQLite `user_attention` table so the
/// user-session can surface it and relay the user's reply back later.
/// All routing fields (job_id / role / agent_id / to_agent_id / source_event)
/// are encoded inside `llm_content` by the caller (see `resolve_llm_content_cli`).
pub fn user_decision_request(user_content: &str, llm_content: &str) -> Result<()> {
    let out = Command::new("okx-a2a")
        .args([
            "user", "decision-request",
            "--user-content", user_content,
            "--llm-content", llm_content,
            "--json",
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("okx-a2a user decision-request exit {status}: {stderr}", status = out.status);
    }
    Ok(())
}

// ── Session management ────────────────────────────────────────────────────

/// Bridge equivalent: `xmtp_sessions_query '{jobId, myAgentId, toAgentId}'`
/// The bridge only consumes `.length` on the returned sessions array;
/// callers usually just want to know "does a session already exist?".
pub fn session_query_exists(job_id: &str, my_agent_id: &str, to_agent_id: &str) -> Result<bool> {
    let out = Command::new("okx-a2a")
        .args([
            "session", "query",
            "--job-id", job_id,
            "--my-agent-id", my_agent_id,
            "--to-agent-id", to_agent_id,
            "--json",
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("okx-a2a session query exit {status}: {stderr}", status = out.status);
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| anyhow::anyhow!("session query stdout not valid JSON: {e}"))?;
    let exists = json
        .get("sessions")
        .and_then(|v| v.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    Ok(exists)
}

/// Bridge equivalent: `xmtp_start_conversation '{myAgentId, toAgentId, jobId}'`
/// Registers the session in okx-a2a's SQLite SessionStore so downstream
/// `session send` works, and returns the `sessionKey` field from the CLI's
/// JSON response. Do not assemble the sessionKey from the IDs — the CLI is
/// the source of truth.
pub fn session_create(job_id: &str, my_agent_id: &str, to_agent_id: &str) -> Result<String> {
    let out = Command::new("okx-a2a")
        .args([
            "session", "create",
            "--job-id", job_id,
            "--my-agent-id", my_agent_id,
            "--to-agent-id", to_agent_id,
            "--json",
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("okx-a2a session create exit {status}: {stderr}", status = out.status);
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| anyhow::anyhow!("session create stdout not valid JSON: {e}"))?;
    // okx-a2a returns sessionKey in two shapes depending on mode:
    //   - queued mode: top-level `sessionKey`
    //   - sync mode:   nested under `session.sessionKey`
    // Try nested first (the canonical sync response), then fall back to top-level.
    json.get("session")
        .and_then(|s| s.get("sessionKey"))
        .and_then(|v| v.as_str())
        .or_else(|| json.get("sessionKey").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("session create response missing sessionKey (checked session.sessionKey and top-level)"))
}

/// Dispatch a session message using the new job-id based addressing.
///
/// - `to_agent_id = None`  → sends to the `backup:<jobId>` session.
/// - `to_agent_id = Some`  → sends to every session matching `jobId + toAgentId`.
///   The CLI auto-suffixes message ids to avoid duplicates across fan-out.
pub fn session_send(job_id: &str, to_agent_id: Option<&str>, content: &str) -> Result<()> {
    let mut args: Vec<&str> = vec![
        "session", "send",
        "--job-id", job_id,
        "--content", content,
        "--json",
    ];
    if let Some(to) = to_agent_id {
        args.push("--to-agent-id");
        args.push(to);
    }
    let out = Command::new("okx-a2a")
        .args(&args)
        .output()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("okx-a2a session send exit {status}: {stderr}", status = out.status);
    }
    Ok(())
}

/// Delete sessions matched by job (and optionally peer agent).
///
/// - `to_agent_id = None`  → deletes every session matching `jobId`.
/// - `to_agent_id = Some`  → deletes only sessions matching `jobId + toAgentId`.
///
/// When the daemon's lifecycle provider is `openclaw`, the CLI also asks the
/// gateway to drop the corresponding session.
pub fn session_delete(job_id: &str, to_agent_id: Option<&str>) -> Result<()> {
    let mut args: Vec<&str> = vec![
        "session", "delete",
        "--job-id", job_id,
        "--json",
    ];
    if let Some(to) = to_agent_id {
        args.push("--to-agent-id");
        args.push(to);
    }
    let out = Command::new("okx-a2a")
        .args(&args)
        .output()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("okx-a2a session delete exit {status}: {stderr}", status = out.status);
    }
    Ok(())
}

// ── XMTP wire messages ────────────────────────────────────────────────────

/// Bridge equivalent: `xmtp_send '{sessionKey, content, payload?}'`
/// Real-business XMTP message (payload is silently dropped by the bridge, so
/// we don't expose it here). Note the API divergence:
/// - CLI uses `--message` (not `--content`, unlike user_notify / session_send).
/// - `--my-agent-id` / `--from-agent-id` were removed from the CLI spec —
///   the daemon resolves the local agent from session metadata.
pub fn xmtp_send(job_id: &str, to_agent_id: &str, message: &str) -> Result<()> {
    let out = Command::new("okx-a2a")
        .args([
            "xmtp-send",
            "--job-id", job_id,
            "--to-agent-id", to_agent_id,
            "--message", message,
            "--no-wait",
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("okx-a2a xmtp-send exit {status}: {stderr}", status = out.status);
    }
    Ok(())
}

// ── XMTP conversation history ─────────────────────────────────────────────

/// Bridge equivalent: `xmtp_get_conversation_history '{jobId, toAgentId}'`
/// `okx-a2a session history --job-id <id> --to-agent-id <id> --json` — new
/// job-id based addressing; matches the session bound to `jobId + toAgentId`.
///
/// Returns the CLI's raw stdout verbatim (typically a JSON array of
/// messages). Schema evolves on the okx-a2a side faster than this CLI
/// recompiles, so we hand the bytes straight to the LLM downstream rather
/// than maintaining a brittle parser. Callers should still trim and treat
/// `""` / `"[]"` as the empty case.
pub fn session_history(job_id: &str, to_agent_id: &str) -> Result<String> {
    let out = Command::new("okx-a2a")
        .args([
            "session", "history",
            "--job-id", job_id,
            "--to-agent-id", to_agent_id,
            "--json",
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("okx-a2a session history exit {status}: {stderr}", status = out.status);
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ── Pending task requests ────────────────────────────────────────────────

/// Bridge equivalent: `xmtp_get_pending_list` / `xmtp_pending_list`
/// `okx-a2a task requests --json` — list pending XMTP task requests (ASPs
/// trying to reach the buyer). Returns the raw item array as
/// `Vec<serde_json::Value>` so callers can extract whichever fields they
/// need (typical: `agentId` / `name` / `serviceName` / `creditScore` /
/// `completedTaskCount`).
///
/// Requires daemon running. The CLI response shape may wrap items under
/// `items` / `requests` / `pending`, or use a top-level array — this helper
/// tries each in order and falls back to an empty vec.
pub fn task_requests() -> Result<Vec<serde_json::Value>> {
    let out = Command::new("okx-a2a")
        .args(["task", "requests", "--json"])
        .output()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("okx-a2a task requests exit {status}: {stderr}", status = out.status);
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| anyhow::anyhow!("task requests stdout not valid JSON: {e}"))?;
    let arr = json.get("payload").and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(arr)
}

/// Bridge equivalent: `xmtp_deny_pending_conversation` / `xmtp_deny_conversation`
/// `okx-a2a task reject --group-id <id> [--agent-id <id>] [--json]` — reject
/// a pending XMTP conversation. Note: keyed by **groupId** (the XMTP group),
/// not jobId.
///
/// Requires daemon running. Queued command — does not wait for the final
/// result.
pub fn task_reject(group_id: &str) -> Result<()> {
    let mut args: Vec<&str> = vec![
        "task", "reject",
        "--group-id", group_id,
    ];
    args.push("--json");
    let out = Command::new("okx-a2a")
        .args(&args)
        .output()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("okx-a2a task reject exit {status}: {stderr}", status = out.status);
    }
    Ok(())
}

// ── File transfer ────────────────────────────────────────────────────────

/// Result of `okx-a2a file upload`. The 5 encryption fields (digest / salt /
/// nonce / secret / fileKey) plus filename are what the receiving peer needs
/// to download and decrypt the file later — they are typically embedded in
/// the next `xmtp_send` payload so the peer can call `file_download`.
#[derive(Debug, Clone)]
pub struct FileUploadResult {
    pub file_key: String,
    pub digest: String,
    pub salt: String,
    pub nonce: String,
    pub secret: String,
    pub filename: String,
}

/// Bridge equivalent: `xmtp_file_upload '{filePath, agentId, jobId, filename?, mimeType?}'`
///
/// Uploads + encrypts the file via the okx-a2a CLI and returns the metadata
/// that the receiving peer needs to download it.
///
/// ⚠️ Note: the bridge calls the agent-id field `agentId`, NOT `myAgentId`
/// (regardless of what the CLAUDE.md top-level mapping table says — the
/// `handleFileUpload` source is the source of truth).
pub fn file_upload(
    file_path: &str,
    agent_id: &str,
    job_id: &str,
    filename: Option<&str>,
    mime_type: Option<&str>,
) -> Result<FileUploadResult> {
    let mut args: Vec<&str> = vec![
        "file", "upload",
        "--file-path", file_path,
        "--agent-id", agent_id,
        "--job-id", job_id,
    ];
    if let Some(f) = filename {
        args.push("--filename");
        args.push(f);
    }
    if let Some(m) = mime_type {
        args.push("--mime-type");
        args.push(m);
    }
    let out = Command::new("okx-a2a")
        .args(&args)
        .output()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("okx-a2a file upload exit {status}: {stderr}", status = out.status);
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| anyhow::anyhow!("file upload stdout not valid JSON: {e}"))?;
    let take = |key: &str| -> Result<String> {
        json.get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("file upload response missing field: {key}"))
    };
    Ok(FileUploadResult {
        file_key: take("fileKey")?,
        digest: take("digest")?,
        salt: take("salt")?,
        nonce: take("nonce")?,
        secret: take("secret")?,
        filename: take("filename")?,
    })
}

/// Bridge equivalent: `xmtp_file_download '{fileKey, agentId, digest, salt, nonce, secret, filename?}'`
///
/// Downloads + decrypts an attachment using the encryption metadata that the
/// sender embedded in the original attachment message. Returns the local
/// path where the decrypted file was written.
///
/// ⚠️ Note: the 4 encryption parameters (`digest` / `salt` / `nonce` /
/// `secret`) are NOT derived from `fileKey` — they are generated by the
/// uploader and shipped in-band with the attachment message. Callers must
/// extract them from the inbound message payload before invoking this helper.
pub fn file_download(
    file_key: &str,
    agent_id: &str,
    digest: &str,
    salt: &str,
    nonce: &str,
    secret: &str,
    filename: Option<&str>,
) -> Result<String> {
    let mut args: Vec<&str> = vec![
        "file", "download",
        "--file-key", file_key,
        "--agent-id", agent_id,
        "--digest", digest,
        "--salt", salt,
        "--nonce", nonce,
        "--secret", secret,
    ];
    if let Some(f) = filename {
        args.push("--filename");
        args.push(f);
    }
    let out = Command::new("okx-a2a")
        .args(&args)
        .output()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("okx-a2a file download exit {status}: {stderr}", status = out.status);
    }
    // The doc says stdout is "the local saved path" — pass-through. Some CLI
    // builds may wrap it in a JSON object (e.g. `{"path": "..."}`). Handle
    // both shapes so callers don't have to guess.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let trimmed = stdout.trim();
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(p) = json.get("path").and_then(|v| v.as_str()) {
            return Ok(p.to_string());
        }
        // JSON parsed but no `path` field — return the serialized JSON as a
        // fallback so the caller can inspect what the CLI emitted.
        return Ok(json.to_string());
    }
    Ok(trimmed.to_string())
}
