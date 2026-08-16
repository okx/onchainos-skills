//! Synchronous wrappers around the external `okx-a2a` CLI binary.
//!
//! Shared by user / asp / evaluator sub-session flows that need to
//! probe session state (sessionKey, jobId, agentId, etc.) without going
//! through the MCP host's `xmtp_*` tools. All calls are blocking
//! (std::process::Command); spawn cost is ~100-150ms per invocation, so
//! consumers should minimize calls in hot paths.

use anyhow::Result;
use std::process::Command;

// ── Communication readiness preflight ──────────────────────────────────────

/// Env flag that disables the readiness preflight entirely (tests / CI /
/// power users who manage the a2a environment themselves).
pub const SKIP_A2A_PREFLIGHT_ENV: &str = "ONCHAINOS_SKIP_A2A_PREFLIGHT";

/// Build a command that resolves npm-installed CLIs on every platform.
/// Windows CreateProcess only resolves `.exe` for a bare name, but npm lays
/// down `okx-a2a.cmd` / `npm.cmd` shims — route through `cmd /C` there so the
/// shell applies PATHEXT. Args here are fixed flag tokens (no spaces), so no
/// extra quoting is needed.
fn npm_cli_command(program: &str, args: &[&str]) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(program).args(args);
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd
    }
}

fn run_silently(program: &str, args: &[&str]) -> Option<std::process::Output> {
    npm_cli_command(program, args).output().ok()
}

fn version_probe_failure_hint(details: Option<&str>) -> String {
    let mut hint = concat!(
        "okx-a2a may not be installed, or the active Node environment may differ from the one ",
        "used to install it. Switch to the correct Node environment and retry. If it is not ",
        "installed, run `npm i -g @okxweb3/a2a-node` in a compatible Node environment."
    )
    .to_string();
    if let Some(details) = details.map(str::trim).filter(|value| !value.is_empty()) {
        hint.push_str(" Details: ");
        hint.push_str(details);
    }
    hint
}

/// Result of the read-only A2A readiness probe.
enum CommReadiness {
    /// A definitive positive verdict (or the probe was skipped via env).
    Ready,
    /// A DEFINITIVE negative: the version probe failed (missing command or wrong
    /// Node environment), or doctor returned a valid false readiness verdict.
    NotReady(String),
    /// No usable verdict could be obtained even though okx-a2a is installed
    /// (doctor crashed / produced no JSON / a build too old to have `doctor` /
    /// a report without a readiness field). We must NOT hold user operations
    /// hostage to a broken checker — callers proceed, emitting this warning.
    Unverifiable(String),
}

/// READ-ONLY probe of the local A2A communication environment. Never installs,
/// fixes, or prompts — repairs are explicitly the user's (or the AI session's)
/// move via `okx-a2a doctor --fix`, which can do the interactive parts (plugin
/// install, provider login) that a silent path cannot. Outcomes:
/// - SKIP env set → Ready
/// - version probe fails → NotReady with missing-install / Node-environment guidance
/// - doctor returns a valid readiness verdict → Ready / NotReady accordingly
/// - okx-a2a present but doctor yields no usable verdict (crash / non-JSON /
///   no `doctor` command / missing readiness field) → Unverifiable (proceed)
fn probe_communication_readiness() -> CommReadiness {
    if std::env::var(SKIP_A2A_PREFLIGHT_ENV).ok().as_deref() == Some("1") {
        return CommReadiness::Ready;
    }

    match npm_cli_command("okx-a2a", &["--version"]).output() {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let details = if stderr.trim().is_empty() {
                stdout.as_ref()
            } else {
                stderr.as_ref()
            };
            return CommReadiness::NotReady(version_probe_failure_hint(Some(details)));
        }
        Err(error) => {
            return CommReadiness::NotReady(version_probe_failure_hint(Some(&error.to_string())));
        }
    }

    eprintln!("[onchainos] checking A2A communication readiness (okx-a2a doctor)...");
    let report = run_silently("okx-a2a", &["doctor", "--json"])
        .and_then(|out| serde_json::from_slice::<serde_json::Value>(&out.stdout).ok());
    let Some(report) = report else {
        return CommReadiness::Unverifiable(
            "A2A readiness could not be verified: okx-a2a doctor produced no usable report \
             (the installed build may be outdated or broken). Continuing without the check — \
             if communication fails, run `okx-a2a doctor --fix` (or reinstall with \
             `npm i -g @okxweb3/a2a-node@latest`)."
                .to_string(),
        );
    };

    // Read the readiness verdict. `ready` is the machine-readable field; fall
    // back to `ok` (its alias). A report without either boolean is NOT a valid
    // verdict — treat it as unverifiable, not as "not ready".
    let verdict = report
        .get("ready")
        .and_then(|v| v.as_bool())
        .or_else(|| report.get("ok").and_then(|v| v.as_bool()));
    match verdict {
        Some(true) => {
            eprintln!("[onchainos] A2A communication is ready");
            CommReadiness::Ready
        }
        Some(false) => {
            let message = report
                .get("userMessage")
                .and_then(|v| v.as_str())
                .unwrap_or("A2A communication is not fully ready");
            eprintln!("[onchainos] A2A communication is NOT ready: {message}");
            CommReadiness::NotReady(build_not_ready_hint(message, &report))
        }
        None => CommReadiness::Unverifiable(
            "A2A readiness could not be verified: okx-a2a doctor returned no readiness verdict. \
             Continuing without the check — if communication fails, run `okx-a2a doctor --fix`."
                .to_string(),
        ),
    }
}

/// Assemble the user-facing not-ready hint: doctor's userMessage plus each
/// blocking (non-optional) nextAction's why and command, and the repair
/// command. Context-neutral so both the blocking error path and the
/// gate-check `hint` can reuse it. Must state intent and action plainly (e.g.
/// "The Hermes okx-a2a plugin was just installed and takes effect after the
/// Hermes gateway restarts — run /restart inside Hermes").
fn build_not_ready_hint(user_message: &str, report: &serde_json::Value) -> String {
    let mut lines = vec![user_message.to_string()];
    if let Some(actions) = report.get("nextActions").and_then(|v| v.as_array()) {
        for action in actions {
            let optional = action.get("optional").and_then(|v| v.as_bool()).unwrap_or(false);
            if optional {
                continue;
            }
            let why = action.get("why").and_then(|v| v.as_str()).unwrap_or("");
            let command = action.get("command").and_then(|v| v.as_str()).unwrap_or("");
            match (why.is_empty(), command.is_empty()) {
                (false, false) => lines.push(format!("- {why} (run: {command})")),
                (false, true) => lines.push(format!("- {why}")),
                (true, false) => lines.push(format!("- run: {command}")),
                (true, true) => {}
            }
        }
    }
    lines.push("Run `okx-a2a doctor --fix` to repair the local A2A environment, then retry.".to_string());
    lines.join("\n")
}

/// READ-ONLY A2A readiness gate, run BEFORE identity mutations
/// (agent create / update / activate) and evaluator staking. Blocks ONLY on a
/// definitive not-ready verdict (missing okx-a2a, or doctor says not ready);
/// when readiness cannot be verified (broken checker), it proceeds with a
/// warning rather than holding the user operation hostage. Never repairs.
pub fn ensure_communication_ready_preflight() -> Result<()> {
    match probe_communication_readiness() {
        CommReadiness::Ready => Ok(()),
        CommReadiness::NotReady(hint) => anyhow::bail!(
            "A2A communication is not ready, so this operation was not executed. {hint}"
        ),
        CommReadiness::Unverifiable(note) => {
            eprintln!("[onchainos] {note}");
            Ok(())
        }
    }
}

/// Communication leg of `agent gate-check`, in the same `{ok, hint}` shape as
/// the wallet / identity gates. Read-only: reports readiness, never repairs.
/// Only a definitive not-ready verdict fails the gate; an unverifiable check
/// stays `ok: true` (with a `note`) so a broken checker never blocks the flow.
pub fn communication_gate_json() -> serde_json::Value {
    match probe_communication_readiness() {
        CommReadiness::Ready => serde_json::json!({ "ok": true }),
        CommReadiness::NotReady(hint) => serde_json::json!({ "ok": false, "hint": hint }),
        CommReadiness::Unverifiable(note) => serde_json::json!({ "ok": true, "note": note }),
    }
}

/// Silently refresh the local A2A agent identities after a successful
/// identity mutation (create / update / activate / deactivate), so the new
/// or changed agent is picked up by the daemon immediately instead of on the
/// next periodic sync. Waits for the daemon's completed refresh result (same
/// semantics the LLM-driven ensure flow used). Best-effort: any failure —
/// including okx-a2a missing or the daemon not running — degrades to a
/// single stderr note. Honors the same skip env as the preflight.
pub fn refresh_agent_identities_silently() {
    if std::env::var(SKIP_A2A_PREFLIGHT_ENV).ok().as_deref() == Some("1") {
        return;
    }
    eprintln!("[onchainos] refreshing A2A agent identities (okx-a2a agent refresh)...");
    let ok = run_silently("okx-a2a", &["agent", "refresh", "--json"])
        .map(|o| o.status.success())
        .unwrap_or(false);
    if ok {
        eprintln!("[onchainos] A2A agent identities refreshed");
    } else {
        eprintln!("[onchainos] A2A agent refresh did not complete (non-fatal); the daemon syncs periodically, or run `okx-a2a agent refresh` manually");
    }
}

// ── Offline-replay capability probe ─────────────────────────────────────────

/// Upgrade command surfaced when the comm package cannot honor an offline-replay
/// preference and offered no `fixCommands` of its own.
pub const DEFAULT_OFFLINE_REPLAY_FIX_COMMAND: &str = "npm install -g @okxweb3/a2a-node@latest";

/// Outcome of the offline-replay capability probe. Purely informational: it is
/// copied into the json success envelope of the offline-preference commands and
/// must never change whether a write request is sent or how its success is judged.
pub struct OfflineReplayCapability {
    /// Whether the local comm package can honor an offline-replay preference.
    pub supported: bool,
    /// Upgrade commands the comm package reported for the unsupported case.
    /// Empty when the probe could read none (command missing / spawn failure /
    /// unparsable output); callers substitute `DEFAULT_OFFLINE_REPLAY_FIX_COMMAND`.
    pub fix_commands: Vec<String>,
}

impl OfflineReplayCapability {
    /// The upgrade commands to surface, guaranteed non-empty: the probe's own
    /// `fixCommands` when it returned any, else the packaged default.
    pub fn fix_commands_or_default(&self) -> Vec<String> {
        if self.fix_commands.is_empty() {
            vec![DEFAULT_OFFLINE_REPLAY_FIX_COMMAND.to_string()]
        } else {
            self.fix_commands.clone()
        }
    }
}

/// Read-only probe of whether the local comm package supports the offline-replay
/// preference: run `okx-a2a capabilities --json` and read the nested
/// `messageEligibleOfflineReplay` object. The verdict is BINARY and fail-safe to
/// unsupported — a missing command, a spawn failure, unparsable output, a
/// `messageEligibleOfflineReplay` that is missing or not an object, or a nested
/// `ok` that is not `true` all mean unsupported (the `capabilities` command is not
/// released yet, so a missing command is the expected common case).
/// `ONCHAINOS_SKIP_A2A_PREFLIGHT=1` skips the spawn and treats the package as
/// supported (tests / CI / power users). Reuses the module's `npm_cli_command`
/// shim — no new spawn logic. COPY-ONLY: never gates a write.
pub fn probe_offline_replay_capability() -> OfflineReplayCapability {
    if std::env::var(SKIP_A2A_PREFLIGHT_ENV).ok().as_deref() == Some("1") {
        return OfflineReplayCapability {
            supported: true,
            fix_commands: Vec::new(),
        };
    }
    let stdout = run_silently("okx-a2a", &["capabilities", "--json"]).map(|o| o.stdout);
    interpret_capabilities_output(stdout.as_deref())
}

/// Pure verdict logic for `okx-a2a capabilities --json`, split out so unit tests can
/// inject fake output with no real binary. `None` models a missing command / spawn
/// failure; `Some(bytes)` is the child's captured stdout.
///
/// STRICT NESTED shape (comm-side contract): `ok` and `fixCommands` live INSIDE the
/// `messageEligibleOfflineReplay` object; the top level carries none of them. A
/// `messageEligibleOfflineReplay` that is missing or not an object is unsupported;
/// there is no top-level fallback and no flat-shape tolerance (a flat producer has
/// never existed).
fn interpret_capabilities_output(stdout: Option<&[u8]>) -> OfflineReplayCapability {
    // Fail-safe default: unsupported with no upgrade hints (the caller substitutes
    // DEFAULT_OFFLINE_REPLAY_FIX_COMMAND).
    let unsupported = || OfflineReplayCapability {
        supported: false,
        fix_commands: Vec::new(),
    };

    let Some(bytes) = stdout else {
        // command missing / spawn failure
        return unsupported();
    };
    let Ok(report) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        // unparsable output
        return unsupported();
    };
    // The capability signal lives inside the `messageEligibleOfflineReplay` object.
    // Missing or not-an-object => unsupported.
    let Some(eligibility) = report
        .get("messageEligibleOfflineReplay")
        .and_then(|v| v.as_object())
    else {
        return unsupported();
    };
    // Supported iff the nested `ok` is exactly `true`.
    let supported = eligibility.get("ok").and_then(|v| v.as_bool()) == Some(true);
    // Pass through the nested `fixCommands` string array when present.
    let fix_commands = eligibility
        .get("fixCommands")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    OfflineReplayCapability {
        supported,
        fix_commands,
    }
}

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
///
/// `print_ok = true` prints `OK` on success — for CLI entry handlers whose
/// stdout is consumed directly by a human / shell. In-process callers inside
/// playbook handlers (e.g. `next-action` event dispatch) must pass `false`,
/// otherwise the `OK` would prepend the playbook returned to the LLM.
pub fn compose_user_notify_content(content: &str, image_path: Option<&std::path::Path>) -> Result<String> {
    let content = content.replace("\\n", "\n");
    if content.contains("file://") || (content.contains("![") && content.contains("](")) {
        anyhow::bail!("local image links in --content are not supported; use --image-path <file>");
    }
    let Some(path) = image_path else {
        return Ok(content);
    };
    let path = path.to_string_lossy();
    if path.trim().is_empty() || path.contains('\n') || path.contains('\r') {
        anyhow::bail!("--image-path must be a non-empty single-line path");
    }
    Ok(format!("{content}\n\nMEDIA:{path}"))
}

pub fn user_notify(
    content: &str,
    image_path: Option<&std::path::Path>,
    print_output: bool,
) -> Result<()> {
    if let Some(path) = image_path {
        if !path.exists() {
            anyhow::bail!("--image-path file not found: {}", path.display());
        }
    }
    let content = compose_user_notify_content(content, image_path)?;
    let out = Command::new("okx-a2a")
        .args(["user", "notify", "--content", &content, "--json"])
        .output()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("okx-a2a user notify exit {status}: {stderr}", status = out.status);
    }
    if print_output {
        println!("OK");
    }
    Ok(())
}

/// Bridge equivalent: `xmtp_prompt_user '{"llmContent": "...", "userContent": "..."}'`
/// Sub-side replacement for the MCP `xmtp_prompt_user` tool. Pushes a
/// decision card into the okx-a2a CLI's SQLite `user_attention` table so the
/// user-session can surface it and relay the user's reply back later.
/// The job id is also passed as a first-class CLI argument so okx-a2a can
/// atomically replace an older pending decision for the same job. The remaining
/// routing fields are encoded inside `llm_content` by the caller (see
/// `resolve_llm_content_cli`).
fn user_decision_request_args<'a>(
    job_id: &'a str,
    user_content: &'a str,
    llm_content: &'a str,
) -> Vec<&'a str> {
    vec![
        "user",
        "decision-request",
        "--user-content",
        user_content,
        "--llm-content",
        llm_content,
        "--job-id",
        job_id,
        "--json",
    ]
}

pub(crate) fn validate_decision_job_id(job_id: &str) -> Result<()> {
    if job_id.trim().is_empty() {
        anyhow::bail!("job id is required for an atomic user decision request");
    }
    Ok(())
}

pub fn user_decision_request(job_id: &str, user_content: &str, llm_content: &str) -> Result<()> {
    validate_decision_job_id(job_id)?;
    let args = user_decision_request_args(job_id, user_content, llm_content);
    let out = Command::new("okx-a2a")
        .args(args)
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

/// Reject all pending ASP messages for a given job (batch drain).
/// `okx-a2a task reject --job-id <jobId> --json`
///
/// Used after successful confirm-accept (R14) to clear remaining ASP
/// messages in the queue for an already-accepted task.
pub fn task_reject_by_job(job_id: &str, content: Option<&str>) -> Result<()> {
    let mut args: Vec<String> = vec![
        "task".into(), "reject".into(),
        "--job-id".into(), job_id.into(),
    ];
    if let Some(c) = content {
        args.push("--content".into());
        args.push(c.into());
    }
    args.push("--json".into());
    let out = Command::new("okx-a2a")
        .args(&args)
        .output()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!(
            "okx-a2a task reject --job-id exit {status}: {stderr}",
            status = out.status
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_request_is_bound_to_job_for_atomic_coalescing() {
        let args = user_decision_request_args("job-123", "prompt", "route");
        assert_eq!(
            args,
            [
                "user",
                "decision-request",
                "--user-content",
                "prompt",
                "--llm-content",
                "route",
                "--job-id",
                "job-123",
                "--json",
            ]
        );
    }

    #[test]
    fn decision_request_rejects_an_empty_job_binding() {
        let err = validate_decision_job_id("  ").unwrap_err();
        assert!(err.to_string().contains("job id is required"));
    }

    #[test]
    fn user_notify_content_appends_media_path() {
        let content =
            compose_user_notify_content("line1\\nline2", Some(std::path::Path::new("/tmp/qr.png")))
                .expect("compose notify content");
        assert_eq!(content, "line1\nline2\n\nMEDIA:/tmp/qr.png");
    }

    #[test]
    fn user_notify_content_rejects_multiline_media_path() {
        let err = compose_user_notify_content("notice", Some(std::path::Path::new("/tmp/a\nb.png")))
            .expect_err("path must be rejected");
        assert!(err.to_string().contains("single-line path"));
    }

    #[test]
    fn user_notify_content_rejects_local_image_links() {
        let md_err = compose_user_notify_content(
            "1. Scan\n![QR Code](file:///tmp/deposit.png)",
            None,
        )
        .expect_err("markdown local image must be rejected");
        assert!(md_err.to_string().contains("--image-path"));

        let file_err = compose_user_notify_content("QR: file:///tmp/deposit.png", None)
            .expect_err("file url must be rejected");
        assert!(file_err.to_string().contains("--image-path"));
    }

    #[test]
    fn version_probe_hint_covers_missing_install_and_wrong_node_environment() {
        let hint = version_probe_failure_hint(None);
        assert!(hint.contains("may not be installed"));
        assert!(hint.contains("active Node environment"));
        assert!(hint.contains("npm i -g @okxweb3/a2a-node"));
    }

    #[test]
    fn version_probe_hint_preserves_command_failure_details() {
        let hint = version_probe_failure_hint(Some(
            "Detected Node 20.14.0; okx-a2a requires Node >=22.14.0",
        ));
        assert!(hint.contains("Detected Node 20.14.0"));
        assert!(hint.contains("requires Node >=22.14.0"));
    }

    // The verdict is exercised through the pure `interpret_capabilities_output` seam
    // with injected output — no real `okx-a2a` binary is spawned (the `capabilities`
    // command is not released yet). The two fixtures below are the LITERAL wire JSON
    // from the comm-side contract: `ok` / `fixCommands` / `message` live INSIDE the
    // `messageEligibleOfflineReplay` object; the top level carries none of them.

    // Contract example — unsupported comm package (nested `ok: false`). Held as a
    // UTF-8 `&str` (the verbatim `message` field is CJK, which a byte-string literal
    // cannot carry); call sites pass `.as_bytes()`.
    const NESTED_UNSUPPORTED: &str = r#"{
  "messageEligibleOfflineReplay": {
    "ok": false,
    "fixCommands": ["npm install -g @okxweb3/a2a-node@latest"],
    "message": "当前通信包不支持离线回放偏好，请升级后重试。"
  }
}"#;

    // New-package minimal variant — supported comm package (nested `ok: true`).
    const NESTED_SUPPORTED: &str = r#"{"messageEligibleOfflineReplay": {"ok": true}}"#;

    #[test]
    fn capability_unsupported_when_command_missing() {
        // None models a missing command / spawn failure.
        let cap = interpret_capabilities_output(None);
        assert!(!cap.supported);
        assert!(cap.fix_commands.is_empty());
        // The caller still gets a usable upgrade hint via the packaged default.
        assert_eq!(
            cap.fix_commands_or_default(),
            vec![DEFAULT_OFFLINE_REPLAY_FIX_COMMAND.to_string()]
        );
    }

    #[test]
    fn capability_unsupported_when_output_unparsable() {
        let cap = interpret_capabilities_output(Some(b"not json at all"));
        assert!(!cap.supported);
        assert!(cap.fix_commands.is_empty());
    }

    #[test]
    fn capability_unsupported_when_eligibility_missing_or_not_object() {
        // `messageEligibleOfflineReplay` absent entirely => unsupported.
        let absent = interpret_capabilities_output(Some(br#"{"ok": true}"#));
        assert!(!absent.supported);
        assert!(absent.fix_commands.is_empty());
        // Present but NOT an object (the never-existent flat shape, where it was a
        // bare bool) => unsupported: the flat producer is not tolerated.
        let flat = interpret_capabilities_output(Some(
            br#"{"ok": true, "messageEligibleOfflineReplay": true}"#,
        ));
        assert!(!flat.supported);
        assert!(flat.fix_commands.is_empty());
    }

    #[test]
    fn capability_unsupported_when_nested_ok_false() {
        let cap = interpret_capabilities_output(Some(NESTED_UNSUPPORTED.as_bytes()));
        assert!(!cap.supported);
        // The nested fixCommands are surfaced verbatim for the unsupported case.
        assert_eq!(
            cap.fix_commands,
            vec!["npm install -g @okxweb3/a2a-node@latest".to_string()]
        );
        assert_eq!(
            cap.fix_commands_or_default(),
            vec!["npm install -g @okxweb3/a2a-node@latest".to_string()]
        );
    }

    #[test]
    fn capability_supported_when_nested_ok_true() {
        let cap = interpret_capabilities_output(Some(NESTED_SUPPORTED.as_bytes()));
        assert!(cap.supported);
    }
}
