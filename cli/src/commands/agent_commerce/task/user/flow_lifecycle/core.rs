//! Core happy-path lifecycle prompt generators.

use super::super::flow::FlowContext;

// ── A2A deliver content parser ──────────────────────────────────────────

/// Parsed deliverable from the A2A message `content` field.
enum DeliverPayload {
    File {
        file_key: String,
        digest: String,
        salt: String,
        nonce: String,
        secret: String,
        filename: Option<String>,
    },
    Text(String),
}

/// Parse the `content` field of an `[intent:deliver]` A2A message.
///
/// File format:
/// ```text
/// jobId: 0x...
/// deliverableType: file
/// fileKey: ...
/// digest: ...
/// salt: ...
/// nonce: ...
/// secret: ...
/// filename: ...
/// [intent:deliver]
/// ```
///
/// Text format:
/// ```text
/// jobId: 0x...
/// deliverableType: text
/// - - -
/// <content>
/// - - -
/// [intent:deliver]
/// ```
fn parse_deliver_content(content: &str) -> Option<DeliverPayload> {
    if !content.contains("[intent:deliver]") {
        return None;
    }

    let kv = |key: &str| -> Option<String> {
        content.lines()
            .find(|line| {
                let trimmed = line.trim();
                trimmed.starts_with(key) && trimmed[key.len()..].starts_with(':')
            })
            .map(|line| line.trim()[key.len() + 1..].trim().to_string())
    };

    let dtype = kv("deliverableType")?;

    match dtype.as_str() {
        "file" => {
            let file_key = kv("fileKey").filter(|s| !s.is_empty())?;
            let digest = kv("digest").filter(|s| !s.is_empty())?;
            let salt = kv("salt").filter(|s| !s.is_empty())?;
            let nonce = kv("nonce").filter(|s| !s.is_empty())?;
            let secret = kv("secret").filter(|s| !s.is_empty())?;
            let filename = kv("filename").filter(|s| !s.is_empty());
            Some(DeliverPayload::File { file_key, digest, salt, nonce, secret, filename })
        }
        "text" => {
            let start = content.find("- - -")?;
            let after = start + 5;
            let body = if let Some(rel_end) = content[after..].rfind("- - -") {
                &content[after..after + rel_end]
            } else {
                &content[after..]
            };
            let trimmed = body.trim();
            if trimmed.is_empty() { return None; }
            Some(DeliverPayload::Text(trimmed.to_string()))
        }
        _ => None,
    }
}

fn is_safe_temp_path(fp: &std::path::Path) -> bool {
    let tmp_dir = std::env::temp_dir();
    if fp.starts_with(&tmp_dir) {
        return true;
    }
    #[cfg(unix)]
    {
        if fp.starts_with("/tmp/") {
            return true;
        }
    }
    if let (Ok(c_fp), Ok(c_tmp)) = (fp.canonicalize(), tmp_dir.canonicalize()) {
        return c_fp.starts_with(&c_tmp);
    }
    false
}

/// Read A2A JSON from a temp file and extract the deliver payload from `content`.
fn parse_a2a_file(path: &str) -> Option<DeliverPayload> {
    let fp = std::path::Path::new(path);
    if !is_safe_temp_path(fp) {
        return None;
    }
    let raw = std::fs::read_to_string(fp).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let content = json.get("content").and_then(|v| v.as_str())?;
    parse_deliver_content(content)
}

/// Extract the FR-2 `autotrade: <json>` line from a delivery `content` block.
fn extract_autotrade_line(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        line.trim()
            .strip_prefix("autotrade:")
            .map(|rest| rest.trim().to_string())
            .filter(|s| !s.is_empty())
    })
}

/// Extract the auto-trade signal JSON from the inbound `--message` envelope, if any.
///
/// Pure local I/O (no network): reads the `a2aFile` temp file and scans its
/// `content` for the `autotrade:` line. Returns `None` for ordinary deliveries so
/// the caller can short-circuit before any `.await` (AC-10: zero added network).
fn extract_autotrade_from_message(message: Option<&serde_json::Value>) -> Option<String> {
    let a2a_file = message
        .and_then(|m| m.get("a2aFile"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?;
    let fp = std::path::Path::new(a2a_file);
    if !is_safe_temp_path(fp) {
        return None;
    }
    let raw = std::fs::read_to_string(fp).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let content = json.get("content").and_then(|v| v.as_str())?;
    extract_autotrade_line(content)
}

/// Serialize a buyer delivery-handler payload into the same envelope shape as
/// `output::success(data)` (`{"ok":true,"data":…}`). The auto-trade branch returns
/// this string so it flows through the caller's existing `println!` path.
fn success_envelope<T: serde::Serialize>(data: &T) -> String {
    serde_json::to_string(&serde_json::json!({ "ok": true, "data": data }))
        .unwrap_or_else(|_| "{\"ok\":false,\"error\":\"failed to serialize card\"}".to_string())
}

/// Milliseconds since the Unix epoch (buyer receive time).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The directory scanned for A2A deliver spool files. Defaults to the OS temp dir
/// (`/tmp` on Linux when `TMPDIR` is unset — matching the playbook's
/// `/tmp/a2a_deliver_…` write path), and is redirectable via `TMPDIR` so tests / CI
/// / sandbox never need to touch a hardcoded `/tmp`.
fn a2a_spool_dir() -> std::path::PathBuf {
    std::env::temp_dir()
}

/// Persist a raw inbound A2A deliver message piped on stdin (`next-action
/// --a2a-stdin`) to the recovery spool — exactly where the sub-session LLM used
/// to hand-write it (one whole model turn saved). Per-delivery unique name
/// (timestamp suffix) so two messages in one round can't overwrite each other;
/// the recovery dual-scan picks up the `a2a_deliver_<jobId>_` prefix. Returns
/// the written path for injection as `a2aFile`.
pub(crate) fn persist_a2a_spool(job_id: &str, raw: &str) -> anyhow::Result<String> {
    persist_a2a_spool_in(&a2a_spool_dir(), job_id, raw)
}

/// Dir-injected core of [`persist_a2a_spool`] — unit-testable without touching
/// `TMPDIR` (the recover_* tests mutate that env concurrently).
///
/// Filename = `a2a_deliver_<jobId>_<millis>_<pid>[ _n ].json`: pid + `create_new`
/// (+ a bounded uniquifier retry) make same-millisecond collisions impossible —
/// the old hand-written scheme was collision-free by deliveryId, and a silent
/// overwrite here would lose a copy-trade signal. Written 0600 on unix: the raw
/// deliver payload can carry a file-deliverable decryption secret, and the OS
/// temp dir is shared (same rationale as `home::write_secure` for authz records;
/// the recovery scan runs as the same uid, so tighter perms are free).
fn persist_a2a_spool_in(dir: &std::path::Path, job_id: &str, raw: &str) -> anyhow::Result<String> {
    use std::io::Write;
    // Path-traversal defense: jobId lands in a filename.
    if job_id.is_empty()
        || !job_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!("--a2a-stdin: invalid jobId for the spool filename");
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let pid = std::process::id();
    for n in 0..5u32 {
        let name = if n == 0 {
            format!("a2a_deliver_{job_id}_{ts}_{pid}.json")
        } else {
            format!("a2a_deliver_{job_id}_{ts}_{pid}_{n}.json")
        };
        let path = dir.join(name);
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        match opts.open(&path) {
            Ok(mut f) => {
                f.write_all(raw.as_bytes())?;
                return Ok(path.to_string_lossy().into_owned());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    anyhow::bail!("--a2a-stdin: could not allocate a unique spool filename");
}

/// Collect the A2A spool candidates for `job_id` and return the OLDEST by mtime.
///
/// Candidates (FR-10): the fixed-name file `a2a_deliver_<jobId>.json` (old / no
/// auto-trade block) **plus** every per-delivery file matching the
/// `a2a_deliver_<jobId>_` prefix. Subscription copy-trade delivers repeatedly under
/// one `jobId`, so the write side uses per-delivery names to avoid same-round
/// overwrite; recovery must therefore dual-scan. Oldest-first preserves delivery
/// order (first-in first-out). Returns `None` when no candidate exists.
fn oldest_spool_candidate(job_id: &str) -> Option<String> {
    let dir = a2a_spool_dir();
    let fixed = dir.join(format!("a2a_deliver_{job_id}.json"));
    let prefix = format!("a2a_deliver_{job_id}_");

    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if fixed.is_file() {
        candidates.push(fixed);
    }
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&prefix) && name.ends_with(".json") {
                candidates.push(entry.path());
            }
        }
    }
    if candidates.is_empty() {
        return None;
    }
    // Oldest first (stable): sort by mtime ascending; unknown mtime sorts earliest.
    candidates.sort_by_key(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    candidates.into_iter().next().map(|p| p.display().to_string())
}

/// Read a spool file and return its FR-2 `autotrade:` signal line, if present.
/// Recovery-path analogue of [`extract_autotrade_from_message`] (which reads the
/// live inbound `a2aFile`); lets a delivery recovered from the spool run the same
/// copy-trade pipeline as the live path (FB3).
fn extract_autotrade_from_spool_file(path: &str) -> Option<String> {
    let fp = std::path::Path::new(path);
    if !is_safe_temp_path(fp) {
        return None;
    }
    let raw = std::fs::read_to_string(fp).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let content = json.get("content").and_then(|v| v.as_str())?;
    extract_autotrade_line(content)
}

/// A deliverable recovered from the A2A spool. Beyond the saved artifact, it carries
/// the FR-2 `autotrade:` signal line (when the delivery had one) so the caller can
/// run the copy-trade pipeline — recovery parity with the live
/// `deliverable_received_cli` path (FB3).
pub(crate) struct RecoveredDeliverable {
    pub saved_path: String,
    pub deliverable_type: String,
    pub text_content: Option<String>,
    pub autotrade_signal: Option<String>,
}

/// Parse one A2A spool file, download (file) or write (text) its deliverable, save
/// via `handle_save`, delete the file on success, and return a [`RecoveredDeliverable`]
/// (including any `autotrade:` signal line, extracted before deletion). On any failure
/// returns `None` and leaves the file in place (the caller quarantines it — FB2).
#[allow(clippy::too_many_arguments)]
fn process_recovered_file(
    temp_path: &str,
    job_id: &str,
    agent_id: &str,
    short_id: &str,
    title: &str,
    token_symbol: &str,
    token_amount: &str,
    provider_agent_id: Option<&str>,
) -> Option<RecoveredDeliverable> {
    use crate::commands::agent_commerce::task::common::{deliverables, okx_a2a};

    let payload = parse_a2a_file(temp_path)?;

    let result = match payload {
        DeliverPayload::File { ref file_key, ref digest, ref salt, ref nonce, ref secret, ref filename } => {
            let local_path = okx_a2a::file_download(
                file_key, agent_id, digest, salt, nonce, secret, filename.as_deref(),
            ).ok()?;
            let r = deliverables::handle_save(&deliverables::SaveParams {
                job_id,
                role: "user",
                file_path: &local_path,
                deliverable_type: "file",
                title,
                short_id,
                file_key: Some(file_key),
                token_symbol: Some(token_symbol),
                token_amount: Some(token_amount),
                counterparty_agent_id: provider_agent_id,
                counterparty_name: None,
            }).ok()?;
            (r.path, "file".to_string(), None)
        }
        DeliverPayload::Text(ref text) => {
            let tmp = std::env::temp_dir().join(format!("deliverable-text-{job_id}.txt"));
            std::fs::write(&tmp, text).ok()?;
            let r = deliverables::handle_save(&deliverables::SaveParams {
                job_id,
                role: "user",
                file_path: &tmp.display().to_string(),
                deliverable_type: "text",
                title,
                short_id,
                file_key: None,
                token_symbol: Some(token_symbol),
                token_amount: Some(token_amount),
                counterparty_agent_id: provider_agent_id,
                counterparty_name: None,
            }).ok()?;
            (r.path, "text".to_string(), Some(text.clone()))
        }
    };

    let (saved_path, deliverable_type, text_content) = result;
    // FB3: capture the autotrade signal BEFORE the file is deleted so the caller can
    // run the copy-trade pipeline on the recovered delivery.
    let autotrade_signal = extract_autotrade_from_spool_file(temp_path);
    let _ = std::fs::remove_file(temp_path);
    Some(RecoveredDeliverable {
        saved_path,
        deliverable_type,
        text_content,
        autotrade_signal,
    })
}

/// Try to recover a deliverable from an A2A spool file (FR-10 dual-scan).
///
/// Called by `check_status_freshness` when `job_submitted` finds no manifest.
/// Picks the OLDEST spool candidate for `job_id` (fixed name + per-delivery prefix;
/// see [`oldest_spool_candidate`]), processes exactly that one, and deletes it —
/// any remaining files are handled on the next `job_submitted` recovery pass so
/// high-frequency / out-of-order subscription deliveries are not silently dropped.
/// On any failure returns `None` and falls through to the "wait" path.
#[allow(clippy::too_many_arguments)]
pub(crate) fn try_recover_from_temp_file(
    job_id: &str,
    agent_id: &str,
    short_id: &str,
    title: &str,
    token_symbol: &str,
    token_amount: &str,
    provider_agent_id: Option<&str>,
) -> Option<RecoveredDeliverable> {
    // FB2: skip past poison-pill spool files instead of re-selecting the same oldest
    // one forever. `oldest_spool_candidate` re-picks the oldest-by-mtime each pass, so
    // a file that always fails (corrupt JSON, permanently un-downloadable) would block
    // every newer per-delivery file — a silent drop on high-frequency subscriptions.
    // On a processing failure we move the file aside (rename → `.failed`, which no
    // longer matches the `*.json` scan) and try the next-oldest; if it cannot even be
    // moved, stop to avoid an infinite reselect loop. Each iteration removes one
    // candidate (success deletes, failure quarantines), so this always terminates.
    loop {
        let temp_path = oldest_spool_candidate(job_id)?;
        if let Some(recovered) = process_recovered_file(
            &temp_path,
            job_id,
            agent_id,
            short_id,
            title,
            token_symbol,
            token_amount,
            provider_agent_id,
        ) {
            return Some(recovered);
        }
        if !quarantine_failed_spool_file(&temp_path) {
            return None;
        }
    }
}

/// Move a spool file that failed to process out of the scan set (FB2 poison-pill
/// guard). Renaming to `<path>.failed` drops it from [`oldest_spool_candidate`]
/// (which only matches `*.json`) WITHOUT deleting it, so it stays on disk for manual
/// inspection / recovery. Returns `true` when the file was moved aside.
fn quarantine_failed_spool_file(path: &str) -> bool {
    std::fs::rename(path, format!("{path}.failed")).is_ok()
}

/// Run the FR-3/4/5/7 copy-trade pipeline for a deliverable recovered from the A2A
/// spool (FB3: recovery parity with the live `deliverable_received_cli` path). The
/// deliverable is already saved at `saved_path`; `signal_json` is the extracted
/// `autotrade:` line. The CLI — not the model — decides execution and returns the
/// same `{ok,data}` envelope the live path emits (execution card or notify-only).
pub(crate) async fn run_recovered_autotrade(
    signal_json: &str,
    job_id: &str,
    agent_id: &str,
    saved_path: &str,
) -> String {
    use crate::audit;
    use crate::commands::agent_commerce::task::common::autotrade::pipeline;
    use crate::commands::agent_commerce::task::common::autotrade::ACTION_AUTOTRADE_DELIVER;
    use std::time::Duration;

    let base_tags = vec![
        format!("jobId={job_id}"),
        format!("agentId={agent_id}"),
        "source=recover".to_string(),
    ];
    // Entry marker: an autotrade signal was recovered and the pipeline is starting.
    audit::log(
        "cli",
        ACTION_AUTOTRADE_DELIVER,
        true,
        Duration::default(),
        Some([base_tags.clone(), vec!["phase=detected".to_string()]].concat()),
        None,
    );
    let received_at_ms = now_ms();
    let outcome = pipeline::run(pipeline::PipelineInput {
        signal_json,
        job_id,
        agent_id,
        received_at_ms,
        saved_path,
        consent_override: false,
    })
    .await;
    // Result audit: the money-moving decision (card vs. degrade reason) must be
    // traceable after the fact, exactly as on the live path.
    match &outcome {
        pipeline::PipelineOutcome::Card(card) => audit::log(
            "cli",
            ACTION_AUTOTRADE_DELIVER,
            true,
            Duration::default(),
            Some(
                [
                    base_tags.clone(),
                    vec![
                        "phase=result".to_string(),
                        "outcome=card".to_string(),
                        format!("deliveryId={}", card.delivery_id),
                        format!("signalType={}", card.signal_type),
                    ],
                ]
                .concat(),
            ),
            None,
        ),
        pipeline::PipelineOutcome::Notify(notify) => audit::log(
            "cli",
            ACTION_AUTOTRADE_DELIVER,
            false,
            Duration::default(),
            Some(
                [
                    base_tags.clone(),
                    vec![
                        "phase=result".to_string(),
                        "outcome=degrade".to_string(),
                        format!("reason={}", notify.reason),
                    ],
                ]
                .concat(),
            ),
            Some(&notify.reason),
        ),
        pipeline::PipelineOutcome::Decision(d) => audit::log(
            "cli",
            ACTION_AUTOTRADE_DELIVER,
            true,
            Duration::default(),
            Some(
                [
                    base_tags.clone(),
                    vec![
                        "phase=result".to_string(),
                        "outcome=decision".to_string(),
                        format!("deliveryId={}", d.delivery_id),
                    ],
                ]
                .concat(),
            ),
            None,
        ),
    }
    match outcome {
        pipeline::PipelineOutcome::Card(card) => success_envelope(&*card),
        pipeline::PipelineOutcome::Notify(mut notify) => {
            // Deterministic degrade notice: the CLI tells the user itself — this
            // often runs in a headless session whose reply text never reaches
            // them, and the follow-up user-notify step used to be skippable.
            crate::commands::agent_commerce::task::common::autotrade::notify::push_degrade_notice(
                &mut notify,
                job_id,
            );
            success_envelope(&notify)
        }
        pipeline::PipelineOutcome::Decision(d) => {
            // Stash the held signal (+ its receive time) so `autotrade-consent-set` can
            // replay it after the user answers (the "execute this one" options).
            let _ =
                crate::commands::agent_commerce::task::common::autotrade::consent::stash_pending_signal(
                    job_id,
                    signal_json,
                    received_at_ms,
                );
            decision_envelope(&d, agent_id)
        }
    }
}

/// Push a pipeline decision card to the user in-process and return the envelope
/// for the consuming agent. Deterministic — the first three-way / over-cap /
/// plugin card must not depend on a (possibly headless) session copying
/// `d.command` (same cure as the consent-replay path in `agent_commerce/mod.rs`).
/// On push failure, falls back to handing the agent the full payload whose
/// `command` it can run.
fn decision_envelope(
    d: &crate::commands::agent_commerce::task::common::autotrade::card::DecisionRequest,
    agent_id: &str,
) -> String {
    use crate::commands::agent_commerce::task::common::{autotrade::card, pending_v2};
    match pending_v2::push_decision_direct(
        &d.job_id,
        "user",
        agent_id,
        &d.user_content,
        &card::decision_list_label(d),
        &d.source_event,
    ) {
        Ok(()) => success_envelope(&serde_json::json!({
            "decision": true,
            "decisionPushed": true,
            "sourceEvent": d.source_event,
            "requiresPlugin": d.requires_plugin,
            "guidance": "Decision card already pushed to the user by the CLI. Do NOT run \
                         the card command or any okx-a2a user command — just end the turn.",
        })),
        Err(e) => {
            eprintln!(
                "[autotrade] decision direct-push failed, falling back to command hand-off: {e}"
            );
            success_envelope(d)
        }
    }
}

// --- Execution stage ----------------------------------------------------

pub(crate) async fn provider_applied(ctx: &FlowContext<'_>, over_most_budget: bool) -> String {
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let mut client = TaskApiClient::new();

    if over_most_budget {
        // F19: reject_apply failure → do NOT auto-advance (apply still active on-chain)
        if let Err(e) = super::super::reject_apply::handle_reject_apply(&mut client, job_id, Some(agent_id)).await {
            return format!(
                "[provider_applied/over_budget] reject-apply failed in-process: {e}\n\n\
                 See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
            );
        }

        let short_id = ctx.short_id;
        let user_content = format!(
            "[Job {short_id} — you are the User Agent] The ASP's quote exceeded the maximum budget for this task. The apply has been rejected automatically.\n\n\
             What would you like to do next?\n\
             A. Browse the ASP list\n\
             B. Designate a specific ASP by agentId\n\
             C. Close the task"
        );
        let request_block = crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
            job_id, "user", agent_id, None,
            &user_content,
            &format!("[Over budget {short_id}] next-step decision"),
            "apply_over_budget",
        );

        return format!(
        "Push the next-step decision card via `pending-decisions-v2 request`, then end turn.\n\n\
         {request_block}\n"
        );
    }

    // ── Within-budget branch: confirm-accept on-chain (escrow funded; status → accepted) ──
    match super::super::accept::handle_confirm_accept(&mut client, job_id, ctx.prefetched).await {
        Ok(()) => {
            // R14: drain remaining ASP messages for this job, notifying each ASP
            let drain_content = format!(
                "[user_rejected]:Job {} is no longer available. It was accepted by another ASP before your request was processed.",
                job_id
            );
            let _ = crate::commands::agent_commerce::task::common::okx_a2a::task_reject_by_job(job_id, Some(&drain_content));
            "**End this turn** and wait for the `job_accepted` system notification.".to_string()
        }
        Err(e) => {
            format!(
                "[provider_applied/confirm_accept] confirm-accept failed in-process: {e}\n\n\
                 See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
            )
        }
    }
}

pub(crate) fn job_accepted(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    let pm = ctx.payment_mode;

    // ── Escrow: CLI fills all values, LLM just localizes + sends ──
    if pm != Some(3) {
        let (title, desc, provider_id, amount, symbol) = match ctx.prefetched {
            Some(p) => (
                p.title.as_str(),
                if p.description.is_empty() { "<description>" } else { p.description.as_str() },
                p.provider_agent_id.as_deref().unwrap_or("<providerAgentId>"),
                p.token_amount.as_str(),
                p.token_symbol.as_str(),
            ),
            None => ("<title>", "<description>", "<providerAgentId>", "<tokenAmount>", "<tokenSymbol>"),
        };

        return format!(
            "✓ job_accepted (escrow). Notify the user:\n\
             **Localize first** — translate the template below into the user's language before sending.\n\
             ```bash\n\
             onchainos agent user-notify --content '<localized content>'\n\
             ```\n\
             Template:\n\
             \x20\x20[Job Accepted] Job `{job_id}` has been accepted; execution begins.\n\
             \x20\x20Title: {title}\n\
             \x20\x20Description: {desc}\n\
             \x20\x20ASP agentId: {provider_id}\n\
             \x20\x20Payment: escrow\n\
             \x20\x20Amount: {amount} {symbol}\n\n\
             End turn after notifying.\n"
        );
    }

    // ── x402: LLM needs to determine replaySuccess + run complete ──
    let accepted_x402_fail = super::super::content::job_accepted_x402_replay_fail_user_notify(job_id);
    let complete_failed = super::super::content::complete_failed_user_notify(job_id);

    format!(
    "[Current Status] job_accepted (x402 — funds already paid)\n\n\
     **Step 1 -- Determine replaySuccess from the previous turn's task-402-pay:**\n\
     Look up the task-402-pay output in this sub session context.\n\
     If not found (e.g. context compaction), **default to replaySuccess=true** —\n\
     skipping complete would leave the task stuck in accepted forever.\n\n\
     **Branch 1: replaySuccess=true (or default)**\n\n\
     ```bash\n\
     onchainos agent complete {job_id}\n\
     ```\n\
     broadcast ≠ on-chain confirmed. Do NOT notify user or say \"task complete\" here.\n\
     On error → notify user:\n\
     **Localize first** — translate the content below into the user's language before sending.\n\
     ```bash\n\
     onchainos agent user-notify --content '<localized content>'\n\
     ```\n\
     Content: {complete_failed}\n\
     → End turn, wait for retry or wakeup_notify.\n\n\
     **Branch 2: replaySuccess=false (explicitly found in context)**\n\n\
     Do not run complete.\n\
     Check whether a `x402_replay_input` pending decision was already pushed in the previous turn:\n\
     ▸ Yes → end turn (user will reply to the pending decision).\n\
     ▸ No → notify user:\n\
     **Localize first** — translate the content below into the user's language before sending.\n\
     ```bash\n\
     onchainos agent user-notify --content '<localized content>'\n\
     ```\n\
     Content: {accepted_x402_fail}\n\
     → Wait for `job_completed` system event.\n"
    )
}

pub(crate) fn deliverable_received(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    let (title_field, sym_field, amt_field, provider_field) = match ctx.prefetched {
        Some(p) => (
            p.title.clone(),
            p.token_symbol.clone(),
            p.token_amount.clone(),
            p.provider_agent_id.clone().unwrap_or_else(|| "<providerAgentId>".to_string()),
        ),
        None => (
            "<title>".to_string(),
            "<tokenSymbol>".to_string(),
            "<tokenAmount>".to_string(),
            "<providerAgentId>".to_string(),
        ),
    };

    // Status-based step 4: if the task is already submitted (status=2), re-trigger
    // job_submitted immediately so the review flow starts without waiting.
    let is_submitted = ctx.prefetched
        .and_then(|p| p.status)
        .map(|s| s == 2)
        .unwrap_or(false);
    let step4 = if is_submitted {
        format!(
            "**Step 4 — Re-trigger review** (task already in submitted state):\n\
             ```bash\n\
             onchainos agent next-action --role user --agentId {agent_id} --message '{{\"event\":\"job_submitted\",\"jobId\":\"{job_id}\"}}'\n\
             ```\n"
        )
    } else {
        format!(
            "**Step 4 — End turn**. Wait for `job_submitted` → `onchainos agent next-action --role user --agentId {agent_id} --message '{{\"event\":\"job_submitted\",\"jobId\":\"{job_id}\"}}'`.\n"
        )
    };

    format!(
    "[Current action] deliverable_received — download → save → notify\n\
     [Role] User\n\n\
     Determine `deliverableType` from the ASP's message, then execute all steps in one turn.\n\n\
     **Step 1 — Download / extract**\n\
     • **file** (message has fileKey/digest/salt/nonce/secret): `okx-a2a file download --file-key <fileKey> --agent-id {agent_id} --digest <digest> --salt <salt> --nonce <nonce> --secret <secret> [--filename <filename>]` → record localPath.\n\
     • **text** (content between `- - -` separators): extract full text, write to a temp .txt file → record localPath.\n\n\
     **Step 2 — Save**\n\
     ```bash\n\
     onchainos agent task-deliverable-save --job-id {job_id} --role user \\\n\
       --file \"<localPath>\" --deliverable-type <file|text> --title \"{title_field}\" \\\n\
       --short-id {short_id} \\\n\
       --counterparty-agent-id \"{provider_field}\" --counterparty-name \"<providerName>\" \\\n\
       --token-symbol \"{sym_field}\" --token-amount \"{amt_field}\"\n\
     ```\n\
     For file type only, add `--file-key \"<fileKey>\"`. Record savedPath from output.\n\n\
     **Step 3 — Notify user**\n\
     **Localize first** — translate the template below into the user's language before sending.\n\
     ```bash\n\
     onchainos agent user-notify --content '<localized content>'\n\
     ```\n\
     Template:\n\
     \x20\x20[Deliverable Received] {title_field} (`{short_id}`)\n\
     \x20\x20ASP: {provider_field}\n\
     \x20\x20Type: <file|text>\n\
     \x20\x20Saved at: <savedPath>\n\
     \x20\x20Awaiting on-chain submission confirmation; review will follow.\n\n\
     {step4}"
    )
}

/// CLI-mode fast path: download + save in-process, return a notify-only prompt.
///
/// The sub-session LLM saves the raw A2A JSON to a temp file and passes
/// `a2aFile` in `--message`. This handler reads the file, parses the
/// `content` field to determine file vs text, does the download/save
/// entirely in Rust, then returns a minimal notify-only prompt.
///
/// Legacy `--message` fields (deliverableType/fileKey/text/filePath) are
/// still accepted as fallback for backward compatibility.
pub(crate) async fn deliverable_received_cli(
    ctx: &FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    use crate::audit;
    use crate::commands::agent_commerce::task::common::{deliverables, okx_a2a};
    use std::time::Duration;

    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    // FR-3/4/5/7: detect an auto-trade signal up front (pure local I/O). When
    // absent, the rest of this function runs exactly as before with no `.await`
    // (AC-10: ordinary delivery unchanged, zero added network calls).
    let autotrade_signal = extract_autotrade_from_message(message);

    let base_tags = vec![format!("jobId={job_id}"), format!("agentId={agent_id}")];

    let msg_str = |key: &str| {
        message
            .and_then(|m| m.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or("")
    };

    // ── Resolve DeliverPayload: a2aFile → legacy fields → fallback ──
    let a2a_file = msg_str("a2aFile");
    let payload = if !a2a_file.is_empty() {
        match parse_a2a_file(a2a_file) {
            Some(p) => {
                audit::log("cli", "user/deliverable_from_a2a_file", true, Duration::default(),
                    Some([base_tags.clone(), vec![format!("path={a2a_file}")]].concat()), None);
                p
            }
            None => {
                audit::log("cli", "user/deliverable_a2a_file_parse_failed", false, Duration::default(),
                    Some([base_tags.clone(), vec![format!("path={a2a_file}")]].concat()),
                    Some("failed to parse A2A file or extract deliver content"));
                return deliverable_received(ctx);
            }
        }
    } else {
        // Legacy: LLM passed fields directly in --message JSON
        let dtype = msg_str("deliverableType");
        if dtype.is_empty() {
            audit::log("cli", "user/deliverable_received_no_type", false, Duration::default(),
                Some(base_tags.clone()), Some("no a2aFile and no deliverableType, fallback to LLM path"));
            return deliverable_received(ctx);
        }
        match dtype {
            "file" => {
                let file_key = msg_str("fileKey");
                let digest = msg_str("digest");
                let salt = msg_str("salt");
                let nonce = msg_str("nonce");
                let secret = msg_str("secret");
                let filename = message.and_then(|m| m.get("filename")).and_then(|v| v.as_str());
                if file_key.is_empty() || digest.is_empty() || salt.is_empty()
                    || nonce.is_empty() || secret.is_empty()
                {
                    audit::log("cli", "user/deliverable_file_missing_metadata", false, Duration::default(),
                        Some(base_tags.clone()), Some("encryption metadata incomplete, fallback to LLM path"));
                    return deliverable_received(ctx);
                }
                DeliverPayload::File {
                    file_key: file_key.to_string(),
                    digest: digest.to_string(),
                    salt: salt.to_string(),
                    nonce: nonce.to_string(),
                    secret: secret.to_string(),
                    filename: filename.map(|s| s.to_string()),
                }
            }
            "text" => {
                let inline_text = msg_str("text");
                let file_path = msg_str("filePath");
                if !inline_text.is_empty() {
                    DeliverPayload::Text(inline_text.to_string())
                } else if !file_path.is_empty() {
                    let fp = std::path::Path::new(file_path);
                    if !is_safe_temp_path(fp) {
                        audit::log("cli", "user/deliverable_text_path_rejected", false, Duration::default(),
                            Some(base_tags.clone()), Some("filePath not under temp dir"));
                        return deliverable_received(ctx);
                    }
                    match std::fs::read_to_string(fp) {
                        Ok(raw) => {
                            match parse_deliver_content(&raw) {
                                Some(DeliverPayload::Text(t)) => DeliverPayload::Text(t),
                                _ => {
                                    // File contains raw text without protocol framing
                                    DeliverPayload::Text(raw.trim().to_string())
                                }
                            }
                        }
                        Err(e) => {
                            audit::log("cli", "user/deliverable_text_read_failed", false, Duration::default(),
                                Some(base_tags.clone()), Some(&e.to_string()));
                            return deliverable_received(ctx);
                        }
                    }
                } else {
                    audit::log("cli", "user/deliverable_text_no_content", false, Duration::default(),
                        Some(base_tags.clone()), Some("neither a2aFile, text, nor filePath provided"));
                    return deliverable_received(ctx);
                }
            }
            _ => {
                audit::log("cli", "user/deliverable_received_unknown_type", false, Duration::default(),
                    Some([base_tags.clone(), vec![format!("type={dtype}")]].concat()), None);
                return deliverable_received(ctx);
            }
        }
    };

    let dtype_str = match &payload { DeliverPayload::File { .. } => "file", DeliverPayload::Text(_) => "text" };
    audit::log("cli", "user/deliverable_received", true, Duration::default(),
        Some([base_tags.clone(), vec![format!("type={dtype_str}")]].concat()), None);

    let (title, sym, amt, provider_id) = match ctx.prefetched {
        Some(p) => (
            p.title.as_str(),
            p.token_symbol.as_str(),
            p.token_amount.as_str(),
            p.provider_agent_id.as_deref().unwrap_or(""),
        ),
        None => ("<title>", "<tokenSymbol>", "<tokenAmount>", ""),
    };

    // ── Execute: download (file) or write tmp (text) → handle_save ──
    let (saved_path, deliverable_type, text_content) = match payload {
        DeliverPayload::File { ref file_key, ref digest, ref salt, ref nonce, ref secret, ref filename } => {
            audit::log("cli", "user/deliverable_file_download", true, Duration::default(),
                Some([base_tags.clone(), vec![format!("fileKey={file_key}")]].concat()), None);

            let local_path = match okx_a2a::file_download(
                file_key, agent_id, digest, salt, nonce, secret, filename.as_deref(),
            ) {
                Ok(p) => {
                    audit::log("cli", "user/deliverable_file_downloaded", true, Duration::default(),
                        Some([base_tags.clone(), vec![format!("localPath={p}")]].concat()), None);
                    p
                }
                Err(e) => {
                    audit::log("cli", "user/deliverable_file_download_failed", false, Duration::default(),
                        Some([base_tags.clone(), vec![format!("fileKey={file_key}")]].concat()), Some(&e.to_string()));
                    eprintln!("[deliverable_received_cli] file download failed: {e}");
                    return deliverable_received(ctx);
                }
            };

            let save_result = deliverables::handle_save(&deliverables::SaveParams {
                job_id,
                role: "user",
                file_path: &local_path,
                deliverable_type: "file",
                title,
                short_id,
                file_key: Some(file_key),
                token_symbol: Some(sym),
                token_amount: Some(amt),
                counterparty_agent_id: if provider_id.is_empty() { None } else { Some(provider_id) },
                counterparty_name: None,
            });

            match save_result {
                Ok(r) => {
                    audit::log("cli", "user/deliverable_saved", true, Duration::default(),
                        Some([base_tags.clone(), vec!["type=file".into(), format!("path={}", r.path)]].concat()), None);
                    (r.path, "file".to_string(), None)
                }
                Err(e) => {
                    audit::log("cli", "user/deliverable_save_failed", false, Duration::default(),
                        Some([base_tags.clone(), vec!["type=file".into()]].concat()), Some(&e.to_string()));
                    eprintln!("[deliverable_received_cli] save failed: {e}");
                    return deliverable_received(ctx);
                }
            }
        }
        DeliverPayload::Text(text) => {
            audit::log("cli", "user/deliverable_text_parsed", true, Duration::default(),
                Some([base_tags.clone(), vec![format!("charCount={}", text.chars().count())]].concat()), None);

            let tmp_dir = std::env::temp_dir();
            let tmp_path = tmp_dir.join(format!("deliverable-text-{job_id}.txt"));
            if let Err(e) = std::fs::write(&tmp_path, &text) {
                audit::log("cli", "user/deliverable_text_write_failed", false, Duration::default(),
                    Some(base_tags.clone()), Some(&e.to_string()));
                eprintln!("[deliverable_received_cli] write temp file failed: {e}");
                return deliverable_received(ctx);
            }

            let save_result = deliverables::handle_save(&deliverables::SaveParams {
                job_id,
                role: "user",
                file_path: &tmp_path.display().to_string(),
                deliverable_type: "text",
                title,
                short_id,
                file_key: None,
                token_symbol: Some(sym),
                token_amount: Some(amt),
                counterparty_agent_id: if provider_id.is_empty() { None } else { Some(provider_id) },
                counterparty_name: None,
            });

            match save_result {
                Ok(r) => {
                    audit::log("cli", "user/deliverable_saved", true, Duration::default(),
                        Some([base_tags.clone(), vec!["type=text".into(), format!("path={}", r.path)]].concat()), None);
                    (r.path, "text".to_string(), Some(text))
                }
                Err(e) => {
                    audit::log("cli", "user/deliverable_save_failed", false, Duration::default(),
                        Some([base_tags.clone(), vec!["type=text".into()]].concat()), Some(&e.to_string()));
                    eprintln!("[deliverable_received_cli] save failed: {e}");
                    return deliverable_received(ctx);
                }
            }
        }
    };

    // ── FR-3/4/5/7 auto-trade execution branch ──────────────────────────
    // The deliverable is now saved locally (`saved_path`). If it carried an
    // `autotrade:` signal line, the CLI — not the model — decides whether to
    // execute: run the fixed-order pipeline and emit either an execution card
    // (all checks pass) or a notify-only payload (any degrade). This is the ONLY
    // path that awaits; ordinary deliveries never reach it (AC-10).
    if let Some(signal_json) = autotrade_signal.as_deref() {
        use crate::commands::agent_commerce::task::common::autotrade::pipeline;
        use crate::commands::agent_commerce::task::common::autotrade::ACTION_AUTOTRADE_DELIVER;
        // Entry marker: an autotrade signal was detected and the pipeline is starting.
        // `phase=detected` disambiguates this from the result audit below — the old
        // single `success=true` here was misleading (it fired before any decision).
        audit::log(
            "cli",
            ACTION_AUTOTRADE_DELIVER,
            true,
            Duration::default(),
            Some([base_tags.clone(), vec!["phase=detected".to_string()]].concat()),
            None,
        );
        let received_at_ms = now_ms();
        let outcome = pipeline::run(pipeline::PipelineInput {
            signal_json,
            job_id,
            agent_id,
            received_at_ms,
            saved_path: &saved_path,
            consent_override: false,
        })
        .await;
        // Result audit: record whether the pipeline emitted an execution card or
        // degraded, and — for a degrade — the machine-readable reason. This is the
        // money-moving path, so the final decision (executed a card vs. why it
        // degraded) must be traceable after the fact, not just the entry marker.
        match &outcome {
            pipeline::PipelineOutcome::Card(card) => audit::log(
                "cli",
                ACTION_AUTOTRADE_DELIVER,
                true,
                Duration::default(),
                Some(
                    [
                        base_tags.clone(),
                        vec![
                            "phase=result".to_string(),
                            "outcome=card".to_string(),
                            format!("deliveryId={}", card.delivery_id),
                            format!("signalType={}", card.signal_type),
                        ],
                    ]
                    .concat(),
                ),
                None,
            ),
            pipeline::PipelineOutcome::Notify(notify) => audit::log(
                "cli",
                ACTION_AUTOTRADE_DELIVER,
                false,
                Duration::default(),
                Some(
                    [
                        base_tags.clone(),
                        vec![
                            "phase=result".to_string(),
                            "outcome=degrade".to_string(),
                            format!("reason={}", notify.reason),
                        ],
                    ]
                    .concat(),
                ),
                Some(&notify.reason),
            ),
            pipeline::PipelineOutcome::Decision(d) => audit::log(
                "cli",
                ACTION_AUTOTRADE_DELIVER,
                true,
                Duration::default(),
                Some(
                    [
                        base_tags.clone(),
                        vec![
                            "phase=result".to_string(),
                            "outcome=decision".to_string(),
                            format!("deliveryId={}", d.delivery_id),
                        ],
                    ]
                    .concat(),
                ),
                None,
            ),
        }
        return match outcome {
            pipeline::PipelineOutcome::Card(card) => success_envelope(&*card),
            pipeline::PipelineOutcome::Notify(mut notify) => {
                // Deterministic degrade notice — recovery runs headless; see the
                // live-path consumer above.
                crate::commands::agent_commerce::task::common::autotrade::notify::push_degrade_notice(
                    &mut notify,
                    job_id,
                );
                success_envelope(&notify)
            }
            pipeline::PipelineOutcome::Decision(d) => {
                // Stash the held signal (+ receive time) so `autotrade-consent-set` can
                // replay it after the user answers (the "execute this one" options).
                let _ =
                    crate::commands::agent_commerce::task::common::autotrade::consent::stash_pending_signal(
                        job_id,
                        signal_json,
                        received_at_ms,
                    );
                decision_envelope(&d, agent_id)
            }
        };
    }

    // Pre-decide the ASP rating + pre-translate the rating_submitted notify
    // + pre-translate the JobCompleted notify on the backup session (escrow
    // only). The future `job_completed` event then dispatches
    // `feedback-submit` + `user-notify` in-process with zero LLM decisions.
    //
    // All three artifacts are bundled into one backup turn because they share
    // the same trigger (rating decided from this deliverable) and the same
    // downstream consumer (the job_completed fast path).
    if ctx.payment_mode != Some(3) {
        // Description is the basis for the sub LLM's rating decision — if it's
        // missing (no prefetched / empty), skip the prefetch entirely and let
        // the LLM playbook handle job_completed with full context at event time.
        let task_description = ctx.prefetched
            .map(|p| p.description.as_str())
            .filter(|s| !s.is_empty());
        if let Some(task_description) = task_description {
            let rating_title = ctx.prefetched
                .map(|p| p.title.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(ctx.title_display);
            let deliverable_summary = match (deliverable_type.as_str(), text_content.as_deref()) {
                ("text", Some(t)) => format!("type: text\ncontent:\n{t}"),
                ("file", _) => format!("type: file\nsaved path: {saved_path}"),
                _ => format!("type: {deliverable_type}\nsaved path: {saved_path}"),
            };
            // JobCompleted notify — jobId + title prefilled; `<tokenAmount>` /
            // `<tokenSymbol>` kept as placeholders, filled by the `job_completed`
            // fast path with the on-chain locked values from `ctx.prefetched`.
            let canonical_job_completed = super::super::content::job_completed_escrow_user_notify(
                job_id, rating_title, "<tokenAmount>", "<tokenSymbol>",
            );
            let prefetch_batch = format!(
                "[PREFETCH — internal cache only, NOT a user-facing flow]\n\
             Pre-decide the ASP rating, then pre-translate two notifications for job `{job_id}`. \
             Execute all steps in one turn.\n\
             ⚠️ The triple-backtick fence markers are NOT part of the content — do not include them.\n\
             ⚠️ Keep EVERY angle-bracket placeholder (e.g. `<tokenAmount>`, `<tokenSymbol>`) verbatim in your translation — CLI will fill them at dispatch time.\n\
             🛑 **Output discipline (strict):** the THREE `cache-*` commands below are the ONLY commands you may run in this turn.\n\
             Task description:\n\
             ```\n\
             {task_description}\n\
             ```\n\n\
             Deliverable:\n\
             ```\n\
             {deliverable_summary}\n\
             ```\n\n\
             [Step 1] Decide score (`X.XX`, 0.00–5.00) + comment (≤100 chars). Then run:\n\
             \x20\x20onchainos agent cache-rating --job-id {job_id} --score <X.XX> --comment '<your comment>'\n\n\
             [Step 2] Fill `<score>` and `<description>` in the template below with the values you just decided, translate the filled result into the user's chat language, then run:\n\
             \x20\x20onchainos agent cache-notify --job-id {job_id} --event-key rating_submitted --content '<your translation>'\n\
             Template:\n\
             ```\n\
             [📝 Rating Submitted] {rating_title} (`{job_id}`) — rated.\n\
             Score: <score> / 5.00\n\
             💬 Comment: <description>\n\
             ```\n\n\
             [Step 3] **Localize first** — rewrite the template below in the user's language before sending. Do NOT pass the English template verbatim to a non-English user. Preserve placeholders verbatim.\n\
             \x20\x20onchainos agent cache-notify --job-id {job_id} --event-key job_completed_escrow --content '<your translation>'\n\
             Template:\n\
             ```\n\
             {canonical_job_completed}\n\
             ```"
            );
            let _ = okx_a2a::session_send(
                job_id, None, &prefetch_batch,
            );
        }
    }

    // Out-of-order handling: if the review marker exists, job_submitted already arrived
    // before this deliverable. Delete marker → directly output the review prompt so the
    // sub doesn't wait for a job_submitted that already came.
    if deliverables::has_review_marker(job_id) {
        deliverables::delete_review_marker(job_id);
        audit::log("cli", "user/deliverable_received_marker_found", true, Duration::default(),
            Some(base_tags.clone()), Some("job_submitted arrived first; merging into review flow"));

        let mut patched = ctx.prefetched.cloned().unwrap_or_else(|| {
            crate::commands::agent_commerce::task::common::PreFetchedTaskContext {
                title: title.to_string(),
                description: String::new(),
                token_symbol: sym.to_string(),
                token_amount: amt.to_string(),
                payment_mode: ctx.payment_mode,
                max_budget: None,
                provider_agent_id: if provider_id.is_empty() { None } else { Some(provider_id.to_string()) },
                user_agent_id: None,
                status: Some(2),
                deliverable: None,
                service_id: None,
                service_token_address: None,
                service_token_amount: None,
                service_params: None,
                user_agent_address: None,
                token_address: None,
                expire_time: None,
                test_flag: false,
            }
        });
        patched.deliverable = Some(crate::commands::agent_commerce::task::common::PreFetchedDeliverable {
            path: saved_path.clone(),
            deliverable_type: deliverable_type.clone(),
            original_name: String::new(),
            text_content: text_content.clone(),
        });

        let merged_ctx = super::super::flow::FlowContext {
            job_id: ctx.job_id,
            agent_id: ctx.agent_id,
            short_id: ctx.short_id,
            title_display: ctx.title_display,
            title_query_hint: ctx.title_query_hint,
            title_in_extract: ctx.title_in_extract,
            terminal_session_hint: ctx.terminal_session_hint.clone(),
            payment_mode: ctx.payment_mode,
            prefetched: Some(&patched),
            data: ctx.data,
        };
        return job_submitted_escrow(&merged_ctx);
    }

    format!(
        "✓ {deliverable_type} deliverable saved.\n\
         savedPath: {saved_path}\n\
         title: {title} | shortId: {short_id} | ASP: {provider_id}\n\n\
         Notify the user:\n\
         **Localize first** — translate the template below into the user's language before sending.\n\
         ```bash\n\
         onchainos agent user-notify --content '<localized content>'\n\
         ```\n\
         Template (path must be full absolute — never abbreviate):\n\
         \x20\x20[Deliverable Received] {title} (`{short_id}`)\n\
         \x20\x20ASP: {provider_id}\n\
         \x20\x20Type: {deliverable_type}\n\
         \x20\x20Saved at: [{saved_path}]({saved_path})\n\
         \x20\x20Awaiting on-chain submission confirmation; acceptance review will follow.\n\n\
         End turn after notifying.\n"
    )
}

/// Top-level dispatcher — picks the path-specific playbook based on `ctx.payment_mode`.
/// The two payment modes have completely different post-submit semantics:
///   - escrow (1): user must review (approve / reject) via a pending-decision card.
///   - x402   (3): funds already paid; just notify + auto-rate; flow ends here.
/// When `payment_mode` is `None` (rare; prefetch failure) we emit both branches with
/// a "verify paymentMode first" header so the LLM can disambiguate.
pub(crate) fn job_submitted(ctx: &FlowContext<'_>) -> String {
    match ctx.payment_mode {
        Some(1) => job_submitted_escrow(ctx),
        Some(3) => job_submitted_x402(ctx),
        _ => format!(
            "paymentMode could not be pre-fetched. Run `onchainos agent status {job}` first to determine paymentMode (1=escrow, 3=x402), then follow the matching branch below.\n\n\
             ━━━━━━━━━ paymentMode=1 (escrow) ━━━━━━━━━\n\n\
             {escrow}\n\n\
             ━━━━━━━━━ paymentMode=3 (x402) ━━━━━━━━━\n\n\
             {x402}",
            job = ctx.job_id,
            escrow = job_submitted_escrow(ctx),
            x402 = job_submitted_x402(ctx),
        ),
    }
}

/// Escrow path (paymentMode=1):
///   Step 1 (task ctx) → Step 2a (saved check) → Step 2b (download / extract + save)
///   → Step 3 (compose review user_content) → push pending-decisions-v2 review card.
/// User must reply A (approve) / B (reject). Auto-approve is strictly forbidden.
pub(crate) fn job_submitted_escrow(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title_display = ctx.title_display;

    // Prefetched task context + providerAgentId are required — without them we
    // cannot resolve deliverable / chat-history target / rating recipient.
    let p = match ctx.prefetched {
        Some(p) => p,
        None => return format!(
            "[job_submitted_escrow] no prefetched task context for job {job_id}; cannot run the review flow.\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    };
    let provider_field: &str = match p.provider_agent_id.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return format!(
            "[job_submitted_escrow] prefetched task context has no providerAgentId for job {job_id}; cannot run the review flow.\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    };
    // Fallback: prefetch didn't include local deliverable info.
    // Check manifest → temp file → wait.
    if p.deliverable.is_none() {
        use crate::commands::agent_commerce::task::common::deliverables;
        if let Ok(Some(manifest)) = deliverables::read_manifest("user", job_id) {
            if let Some(entry) = manifest.entries.last() {
                let saved_path = deliverables::deliverables_dir("user", job_id)
                    .map(|d| d.join(&entry.filename))
                    .unwrap_or_default();
                let text_content = if entry.deliverable_type == "text" {
                    std::fs::read_to_string(&saved_path).ok()
                } else {
                    None
                };
                let mut patched = p.clone();
                patched.deliverable = Some(crate::commands::agent_commerce::task::common::PreFetchedDeliverable {
                    path: saved_path.display().to_string(),
                    deliverable_type: entry.deliverable_type.clone(),
                    original_name: entry.original_name.clone(),
                    text_content,
                });
                let patched_ctx = super::super::flow::FlowContext {
                    job_id: ctx.job_id,
                    agent_id: ctx.agent_id,
                    short_id: ctx.short_id,
                    title_display: ctx.title_display,
                    title_query_hint: ctx.title_query_hint,
                    title_in_extract: ctx.title_in_extract,
                    terminal_session_hint: ctx.terminal_session_hint.clone(),
                    payment_mode: ctx.payment_mode,
                    prefetched: Some(&patched),
                    data: ctx.data,
                };
                return job_submitted_escrow(&patched_ctx);
            }
        }
        if let Some(recovered) = try_recover_from_temp_file(
            job_id, agent_id, short_id, &p.title,
            &p.token_symbol, &p.token_amount,
            p.provider_agent_id.as_deref(),
        ) {
            // NOTE: this sync fallback only archives the deliverable into the review
            // flow. Autotrade execution on a recovered delivery runs in the async
            // `check_status_freshness` recovery path (FB3), which consumes the spool
            // file first — so by the time this fallback runs the file is already gone
            // and `recovered.autotrade_signal` is not actionable here (sync context).
            let mut patched = p.clone();
            patched.deliverable = Some(crate::commands::agent_commerce::task::common::PreFetchedDeliverable {
                path: recovered.saved_path,
                deliverable_type: recovered.deliverable_type,
                original_name: String::new(),
                text_content: recovered.text_content,
            });
            let patched_ctx = super::super::flow::FlowContext {
                job_id: ctx.job_id,
                agent_id: ctx.agent_id,
                short_id: ctx.short_id,
                title_display: ctx.title_display,
                title_query_hint: ctx.title_query_hint,
                title_in_extract: ctx.title_in_extract,
                terminal_session_hint: ctx.terminal_session_hint.clone(),
                payment_mode: ctx.payment_mode,
                prefetched: Some(&patched),
                data: ctx.data,
            };
            return job_submitted_escrow(&patched_ctx);
        }
        let _ = deliverables::write_review_marker(job_id);
        // FB1: point the LLM at the SAME directory recovery actually scans
        // (`a2a_spool_dir()` == `env::temp_dir()`). Hardcoding `/tmp` broke macOS:
        // launchd sets `TMPDIR` to `/var/folders/…`, so `temp_dir() != /tmp` — the
        // file was written to `/tmp` while recovery scanned `/var/folders`, so
        // `oldest_spool_candidate` always came up empty (Linux CI never reproduced it
        // because unset `TMPDIR` makes `temp_dir()` == `/tmp`). Emitting the resolved
        // spool dir keeps write-dir == scan-dir on every platform.
        let spool_dir = a2a_spool_dir();
        let spool_dir = spool_dir.display();
        return format!(
            "[System] job_submitted received but deliverable has not arrived yet (XMTP [intent:deliver] pending).\n\
             If your conversation context contains an `[intent:deliver]` message, process it FIRST with the one-command stdin intake — pipe its full raw JSON via a quoted heredoc whose delimiter you invent fresh (A2A_EOF_ + 6 random characters):\n\
             `onchainos agent next-action --role user --agentId {agent_id} --message '{{\"event\":\"deliverable_received\",\"jobId\":\"{job_id}\"}}' --a2a-stdin <<'A2A_EOF_<random>'`\n\
             (Runtimes without heredoc support may instead write the raw JSON to `{spool_dir}/a2a_deliver_{job_id}_<deliveryId>.json` and pass it as \"a2aFile\" in --message.)\n\
             Then re-trigger: `onchainos agent next-action --role user --agentId {agent_id} --message '{{\"event\":\"job_submitted\",\"jobId\":\"{job_id}\"}}'`\n\
             Otherwise, end this turn and wait.\n"
        );
    }

    // Inline-from-prefetched values used in Step 2b's task-deliverable-save commands.
    let title = p.title.as_str();
    let token_symbol = p.token_symbol.as_str();
    let token_amount = p.token_amount.as_str();

    let step2 = if let Some(d) = p.deliverable.as_ref() {
        if d.deliverable_type == "text" {
            let content = d.text_content.as_deref().unwrap_or("<content unavailable>");
            format!("\
     **Step 2 — Deliverable already saved**:\n\
     \x20\x20- localPath: {path}\n\
     \x20\x20- deliverableType: text\n\
     \x20\x20- deliverableText:\n\
     ```\n\
     {content}\n\
     ```\n\n",
                path = d.path,
            )
        } else {
            format!("\
     **Step 2 — Deliverable already saved**:\n\
     \x20\x20- localPath: {path}\n\
     \x20\x20- deliverableType: file\n\n",
                path = d.path,
            )
        }
    } else {
        format!("\
     **Step 2a — Check saved deliverable:**\n\
     ```bash\n\
     onchainos agent task-deliverable-list --job-id {job_id} --role user\n\
     ```\n\
     Non-empty `deliverables` → use first entry's `path` as localPath, `deliverableType`; skip Step 2b.\n\
     Empty → fall through to Step 2b.\n\n\
     **Step 2b — Fallback: fetch from chat history:**\n\
     ```bash\n\
     okx-a2a session history --job-id {job_id} --to-agent-id {provider_field} --json\n\
     ```\n\
     Find the ASP message with `[intent:deliver]` suffix (newest first).\n\n\
     ▸ Case A (file — message has fileKey/digest/salt/nonce/secret):\n\
     ```bash\n\
     okx-a2a file download --file-key <fileKey> --agent-id {agent_id} --digest <digest> --salt <salt> --nonce <nonce> --secret <secret> [--filename <filename>]\n\
     ```\n\
     stdout = localPath (must be full absolute path). Then persist:\n\
     ```bash\n\
     onchainos agent task-deliverable-save --job-id {job_id} --role user \\\n\
       --file \"<localPath>\" --deliverable-type file --title \"{title}\" \\\n\
       --short-id {short_id} --file-key \"<fileKey>\" \\\n\
       --counterparty-agent-id \"{provider_field}\" --counterparty-name \"<providerName>\" \\\n\
       --token-symbol \"{token_symbol}\" --token-amount \"{token_amount}\"\n\
     ```\n\n\
     ▸ Case B (text — body between `- - -` separators):\n\
     Extract full text → write to temp .txt → persist:\n\
     ```bash\n\
     onchainos agent task-deliverable-save --job-id {job_id} --role user \\\n\
       --file \"<temp .txt path>\" --deliverable-type text --title \"{title}\" \\\n\
       --short-id {short_id} --counterparty-agent-id \"{provider_field}\" \\\n\
       --counterparty-name \"<providerName>\" --token-symbol \"{token_symbol}\" --token-amount \"{token_amount}\"\n\
     ```\n\
     After save, update localPath from save command output.\n\n")
    };

    // Step 3 — compose review card user_content + push via pending-decisions-v2.
    let request_block = crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
        job_id,
        "user",
        agent_id,
        ctx.prefetched.and_then(|p| p.provider_agent_id.as_deref()),
        "<composed in Step 3a from the deliverableType template above — paste the localized result here verbatim, including the A. and B. option lines>",
        &format!("[Decision {short_id}] {title_display} acceptance decision"),
        "job_submitted",
    );

    // FR-2: append the review-deadline reminder to the acceptance card. `None`
    // (no expireTime / expireConfig, or not representable) ⇒ empty string, so the
    // card renders exactly as before (backward compatible, FR-5).
    use crate::commands::agent_commerce::task::common::deadline::{self, DeadlineKind};
    let review_deadline_line = deadline::deadline_reminder_line(
        p.expire_time,
        chrono::Local::now().timestamp(),
        DeadlineKind::Review,
    )
    .map(|l| format!("{l}\n"))
    .unwrap_or_default();

    format!(
    "MUST use `pending-decisions-v2 request` — NOT `onchainos agent user-notify` (one-way = no relay = deadlock). Auto-approval forbidden.\n\n\
     [Your next actions (strict order)]\n\n\
     {step2}\
     **Step 3 — Compose `--user-content` and push decision card:**\n\n\
     Compose `--user-content` from Step 2's deliverable variables (fill placeholders from runtime values):\n\n\
     `<localPath>` must be the full absolute path (e.g. /Users/xxx/…). Never abbreviate or shorten.\n\n\
     ▸ deliverableType=file:\n\
     ```\n\
     [Job {short_id}] The ASP has submitted the deliverable (file).\n\
     File path: [<localPath>](<localPath>)\n\
     Payment: escrow\n\
     A. Approve → reply 'A'\n\
     B. Reject (state reason; used as evidence if disputed) → reply 'B reason: …'\n\
     {review_deadline_line}\
     ```\n\n\
     ▸ deliverableType=text:\n\
     ```\n\
     [Job {short_id}] The ASP has submitted the deliverable (text).\n\
     Saved at: [<localPath>](<localPath>)\n\
     ---Deliverable---\n\
     <deliverableText from Step 2 — full content, no truncation>\n\
     ---End of deliverable---\n\
     Payment: escrow\n\
     A. Approve → reply 'A'\n\
     B. Reject (state reason; used as evidence if disputed) → reply 'B reason: …'\n\
     {review_deadline_line}\
     ```\n\n\
     Push to user (localize `--user-content` and `--list-label` to user's language first):\n\n\
     {request_block}\n"
    )
}

/// x402 path (paymentMode=3):
///   Step 1 (task ctx) → Step 2a (saved check) → Step 2b (recover deliverable from
///   task-402-pay's replayBody if not already saved) → B-1 (notify user, NO review)
///   → B-2 (auto-rate ASP, mandatory) → B-2.5 (notify rating) → B-3 (sub session
///   wrap-up). Funds were paid at job_accepted; user cannot reject.
pub(crate) fn job_submitted_x402(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let terminal_session_hint = &ctx.terminal_session_hint;
    let rating_notify = super::super::content::rating_submitted_user_notify(job_id, title_display);

    // Prefetched task context + providerAgentId are required — without them we
    // cannot resolve deliverable / rating recipient.
    let p = match ctx.prefetched {
        Some(p) => p,
        None => return format!(
            "[job_submitted_x402] no prefetched task context for job {job_id}; cannot run the x402 notify+rate flow.\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    };
    let provider_field: &str = match p.provider_agent_id.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return format!(
            "[job_submitted_x402] prefetched task context has no providerAgentId for job {job_id}; cannot run the x402 notify+rate flow.\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    };

    let step2 = if let Some(d) = p.deliverable.as_ref() {
        if d.deliverable_type == "text" {
            let content = d.text_content.as_deref().unwrap_or("<content unavailable>");
            format!("\
     **Step 2 — Deliverable already saved**:\n\
     \x20\x20- localPath: {path}\n\
     \x20\x20- deliverableType: text\n\
     \x20\x20- deliverableText:\n\
     ```\n\
     {content}\n\
     ```\n\n",
                path = d.path,
            )
        } else {
            format!("\
     **Step 2 — Deliverable already saved**:\n\
     \x20\x20- localPath: {path}\n\
     \x20\x20- deliverableType: file\n\n",
                path = d.path,
            )
        }
    } else {
        format!("\
     **Step 2a — Check saved deliverable:**\n\
     ```bash\n\
     onchainos agent task-deliverable-list --job-id {job_id} --role user\n\
     ```\n\
     Non-empty `deliverables` → use first entry's `path`/`deliverableType`; skip Step 2b.\n\
     Empty → fall through to Step 2b.\n\n\
     **Step 2b — Recover from earlier task-402-pay output:**\n\
     The deliverable was the `replayBody` from `task-402-pay` (auto-saved by CLI).\n\
     Look for `replayBodyDisplay` in this sub session's context.\n\
     Set: deliverableType=text, deliverableText=<replayBodyDisplay>, localPath=<path from Step 2a if available>.\n\n")
    };

    format!(
    "x402: funds already paid; user cannot reject — notify + auto-rate only.\n\n\
     [Your next actions (strict order)]\n\n\
     {step2}\
     **Step 3 — Auto-rate ASP, then notify user:**\n\n\
     **3a — Rate the ASP (mandatory, before notify):**\n\
     Score 0.00–5.00 based on deliverable vs description. Comment ≤100 chars.\n\
     ```bash\n\
     onchainos agent feedback-submit --agent-id {provider_field} --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment>\"\n\
     ```\n\
     `--agent-id` = ASP being rated; `--creator-id` = user's agent id.\n\n\
     **3b — Notify user (deliverable + rating in one message):**\n\
     **Localize first** — translate the composed content into the user's language before sending.\n\
     ```bash\n\
     onchainos agent user-notify --content '<localized content>'\n\
     ```\n\
     Compose from two halves (concatenate with two blank lines):\n\
     \x20\x20▸ Deliverable (always; pick template):\n\
     \x20\x20\x20\x20file: `[Deliverable Received] Job {job_id} — x402, payment settled. File: [<localPath>](<localPath>)`\n\
     \x20\x20\x20\x20text (localPath available): `[Deliverable Received] Job {job_id} — x402, payment settled. Saved at: [<localPath>](<localPath>)` + deliverableText from Step 2\n\
     \x20\x20\x20\x20text (no localPath): `[Deliverable Received] Job {job_id} — x402, payment settled.` + deliverableText from Step 2 inline\n\
     \x20\x20▸ Rating (include ONLY if feedback-submit succeeded; if it failed or errored, **omit this entire half**):\n\
     \x20\x20\x20\x20{rating_notify}\n\
     \x20\x20\x20\x20(fill `<score>` with the X.XX value used in 3a, `<description>` with the comment from 3a)\n\n\
     **3c — Terminal wrap-up:**\n\
     {terminal_session_hint}\n"
    )
}

/// Directly runs `onchainos agent complete` in-process. The single-arg bash
/// command provides no LLM decision-making value — Rust just broadcasts and
/// returns. Iron rules from the previous LLM-driven version ("don't notify
/// user via onchainos agent user-notify / don't auto-rate / don't say funds released
/// before job_completed") all become moot — Rust cannot misbehave.
///
/// Failure path: the playbook emitted on error directs the LLM into the
/// standard cli_failed 5-substep protocol (push a decision to the user).
pub(crate) async fn approve_review(ctx: &FlowContext<'_>) -> String {
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
    let job_id = ctx.job_id;
    let mut client = TaskApiClient::new();
    match super::super::complete::handle_complete(&mut client, job_id).await {
        Ok(()) => "**End this turn** and wait for the `job_completed` system notification.".to_string(),
        Err(e) => format!(
            "[approve_review] `onchainos agent complete {job_id}` failed in-process: {e}\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    }
}

/// Directly runs `onchainos agent reject` in-process. The rejection reason
/// is expected on `ctx.data` (forwarded from `next-action --data` by the
/// `user_decision_job_submitted` router after the LLM extracts it from
/// the relayed user reply); falls back to "did not meet acceptance
/// criteria" when absent. Iron rules from the previous LLM-driven version
/// ("don't send a message to the ASP about the rejection") become moot —
/// Rust just broadcasts and returns.
///
/// Failure path: standard cli_failed instruction (push decision to user).
pub(crate) async fn reject_review(ctx: &FlowContext<'_>) -> String {
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
    let job_id = ctx.job_id;

    let reason = ctx
        .data
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("did not meet acceptance criteria");

    let mut client = TaskApiClient::new();
    match super::super::reject::handle_reject(&mut client, job_id, reason).await {
        Ok(()) => format!(
            "[reject_review] [OK]`onchainos agent reject {job_id} --reason \"{reason}\"` broadcast in-process. End the turn now.\n\n\
             broadcast ≠ on-chain confirmed. The `job_rejected` system event will fire after on-chain confirmation; the ASP then decides whether to dispute (arbitration) or agree to a refund. The user cannot initiate arbitration.\n\
             Do NOT send any message to the ASP about the rejection — they learn via on-chain events.\n"
        ),
        Err(e) => format!(
            "[reject_review] `onchainos agent reject {job_id} --reason \"{reason}\"` failed in-process: {e}\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    }
}

// --- Terminal states ---------------------------------------------------

/// Primary `job_completed` playbook — on-chain confirmation notification.
///
/// This event fires when the blockchain confirms the `complete` transaction.
/// It is the ONLY place where "funds released" is factually true.
/// `approve_review` only broadcasts; this event confirms.
pub(crate) fn job_completed(ctx: &FlowContext<'_>, _message: Option<&serde_json::Value>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let terminal_session_hint = &ctx.terminal_session_hint;

    let provider_id = ctx.prefetched
        .and_then(|p| p.provider_agent_id.as_deref())
        .filter(|s| !s.is_empty())
        .unwrap_or("<providerAgentId>");

    let (token_amount, token_symbol) = ctx.prefetched
        .map(|p| (p.token_amount.as_str(), p.token_symbol.as_str()))
        .unwrap_or(("<tokenAmount>", "<tokenSymbol>"));

    let pm = ctx.payment_mode;

    // Fast path (escrow only): rating + both notify templates pre-cached at
    // deliverable_received time. Run feedback-submit and user-notify entirely
    // in-process; zero LLM decisions.
    //
    // The `job_completed_escrow` template is cached at deliverable_received
    // with `<tokenAmount>` / `<tokenSymbol>` placeholders — filled here with
    // the on-chain locked values from `ctx.prefetched`.
    let provider_id_opt = ctx.prefetched
        .and_then(|p| p.provider_agent_id.as_deref())
        .filter(|s| !s.is_empty());
    if pm != Some(3) {
        if let Some(real_provider_id) = provider_id_opt {
            use crate::commands::agent_commerce::task::common::{
                okx_a2a, onchainos_self, prefilled_notify, prefilled_rating, session_cleanup,
            };
            let cached_completed = prefilled_notify::get(job_id, "job_completed_escrow").ok().flatten();
            let cached_rating_notify = prefilled_notify::get(job_id, "rating_submitted").ok().flatten();
            let cached_rating = prefilled_rating::get(job_id).ok().flatten();
            let amount_ok = !token_amount.is_empty() && !token_amount.starts_with('<');
            let symbol_ok = !token_symbol.is_empty() && !token_symbol.starts_with('<');
            if let (Some(completed_tpl), Some(rating_text), Some(rating)) =
                (cached_completed, cached_rating_notify, cached_rating)
            {
                let placeholders_present = completed_tpl.contains("<tokenAmount>")
                    && completed_tpl.contains("<tokenSymbol>");
                if amount_ok && symbol_ok && placeholders_present {
                    let completed = completed_tpl
                        .replace("<tokenAmount>", token_amount)
                        .replace("<tokenSymbol>", token_symbol);
                    let feedback_ok = onchainos_self::feedback_submit(
                        real_provider_id, agent_id, &rating.score, job_id, &rating.comment,
                    ).is_ok();
                    let combined = if feedback_ok {
                        format!("{completed}\n\n{rating_text}")
                    } else {
                        completed
                    };
                    let _ = okx_a2a::user_notify(&combined, false);
                    let _ = session_cleanup::handle_session_cleanup(job_id, false);

                    return "Task is at a terminal state. User has been notified by the CLI. Do NOT run any further command.".to_string();
                }
                // Placeholder missing or amount/symbol unknown → fall through to LLM playbook.
            }
        }
    }

    let completed_notify = if pm == Some(3) {
        super::super::content::job_completed_x402_user_notify(job_id, title_display)
    } else {
        super::super::content::job_completed_escrow_user_notify(job_id, title_display, token_amount, token_symbol)
    };
    let rating_notify = super::super::content::rating_submitted_user_notify(job_id, title_display);

    format!(
        "✓ job_completed — on-chain confirmed. Rate ASP, then notify user in one message.\n\n\
         **Step 1 — Rate ASP** (0.00–5.00, comment ≤100 chars):\n\
         ```bash\n\
         onchainos agent feedback-submit --agent-id {provider_id} --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment>\"\n\
         ```\n\n\
         **Step 2 — Notify user** (completion + rating):\n\
         **Localize first** — translate the template below into the user's language before sending.\n\
         ```bash\n\
         onchainos agent user-notify --content '<localized content>'\n\
         ```\n\
         Template:\n\
         \x20\x20{completed_notify}\n\n\
         \x20\x20{rating_notify}  ← omit if Step 1 failed\n\n\
         **Step 3 — Wrap-up:**\n\
         {terminal_session_hint}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── persist_a2a_spool (next-action --a2a-stdin) ──────────────────

    #[test]
    fn persist_a2a_spool_writes_prefix_named_file_and_rejects_bad_job_ids() {
        // Dir-injected variant: no TMPDIR mutation, so this never races the
        // recover_* tests that toggle that env concurrently.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("a2a_spool");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();

        let raw = r#"{"msgType":"a2a-agent-chat","jobId":"0xabc123","content":"x"}"#;
        let path = persist_a2a_spool_in(&dir, "0xabc123", raw).unwrap();
        assert!(
            std::path::Path::new(&path)
                .file_name()
                .and_then(|f| f.to_str())
                .is_some_and(|f| f.starts_with("a2a_deliver_0xabc123_") && f.ends_with(".json")),
            "got: {path}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
        // 0600: the payload can carry a file-deliverable decryption secret.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        // A second write in the same millisecond must NOT overwrite the first.
        let path2 = persist_a2a_spool_in(&dir, "0xabc123", "second").unwrap();
        assert_ne!(path, path2);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);

        // Path-traversal defense.
        assert!(persist_a2a_spool_in(&dir, "../evil", raw).is_err());
        assert!(persist_a2a_spool_in(&dir, "", raw).is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── parse_deliver_content ────────────────────────────────────────

    #[test]
    fn parse_file_deliver() {
        let content = "\
jobId: 0x5ea81a18be490d59f88cb2258b4d902d76a1b9848f9e4b452c1266ee40d34721
deliverableType: file
fileKey: 0x5ea81a18be490d59f88cb2258b4d902d76a1b9848f9e4b452c1266ee40d34721/0x5ea81a18be490d59f88cb2258b4d902d76a1b9848f9e4b452c1266ee40d34721-54333239-3175-43b5-b455-015eb8aa0ad5
digest: 93f2c0186b237f10629873167217dfa173c3cbf5eebf4da71715871b16b31e0e
salt: 4CyqL4avwltYQoBg8rZ/luUpISvDwVq9H2AGs2i5JOQ=
nonce: 3qEw/DyUDt32EeA1
secret: 6Y350QXsL+lsk3AyPVMl3UguwaLj+Dc7yAYU8FUpb6k=
filename: argentina-wc-prediction.md
[intent:deliver]";

        let payload = parse_deliver_content(content).expect("should parse file deliver");
        match payload {
            DeliverPayload::File { file_key, digest, salt, nonce, secret, filename } => {
                assert!(file_key.starts_with("0x5ea81a18"), "fileKey: {file_key}");
                assert!(file_key.ends_with("015eb8aa0ad5"), "fileKey: {file_key}");
                assert_eq!(digest, "93f2c0186b237f10629873167217dfa173c3cbf5eebf4da71715871b16b31e0e");
                assert_eq!(salt, "4CyqL4avwltYQoBg8rZ/luUpISvDwVq9H2AGs2i5JOQ=");
                assert_eq!(nonce, "3qEw/DyUDt32EeA1");
                assert_eq!(secret, "6Y350QXsL+lsk3AyPVMl3UguwaLj+Dc7yAYU8FUpb6k=");
                assert_eq!(filename.as_deref(), Some("argentina-wc-prediction.md"));
            }
            DeliverPayload::Text(_) => panic!("expected File, got Text"),
        }
    }

    #[test]
    fn parse_text_deliver() {
        let content = "\
jobId: 0x8bad8245e68c40b0199dd49918e88b79dc21c6cfc68f69f2819570552412e185
deliverableType: text
- - -
onchain-arb 套利扫描报告
===========================
扫描时间: 2026-06-24 22:47 GMT+8
📊 各代币价差全景
LINK 🎯 | ETH | BTC
- - -
[intent:deliver]";

        let payload = parse_deliver_content(content).expect("should parse text deliver");
        match payload {
            DeliverPayload::Text(text) => {
                assert!(text.starts_with("onchain-arb"), "text starts with: {}", &text[..30]);
                assert!(text.contains("LINK 🎯"), "should preserve emoji");
                assert!(text.contains("📊"), "should preserve Unicode");
                assert!(!text.contains("[intent:deliver]"), "should not include suffix");
                assert!(!text.contains("- - -"), "should not include separators");
                assert!(!text.contains("deliverableType"), "should not include header");
            }
            DeliverPayload::File { .. } => panic!("expected Text, got File"),
        }
    }

    #[test]
    fn parse_a2a_json_file_type() {
        let a2a_json = r#"{
  "msgType": "a2a-agent-chat",
  "content": "jobId: 0x5ea8\ndeliverableType: file\nfileKey: abc123\ndigest: d1g\nsalt: s4lt\nnonce: n0nc\nsecret: s3cr\nfilename: report.md\n[intent:deliver]",
  "sender": {"agentId": "1891"}
}"#;
        let json: serde_json::Value = serde_json::from_str(a2a_json).unwrap();
        let content = json.get("content").unwrap().as_str().unwrap();
        let payload = parse_deliver_content(content).expect("should parse from A2A JSON");
        match payload {
            DeliverPayload::File { file_key, digest, salt, nonce, secret, filename } => {
                assert_eq!(file_key, "abc123");
                assert_eq!(digest, "d1g");
                assert_eq!(salt, "s4lt");
                assert_eq!(nonce, "n0nc");
                assert_eq!(secret, "s3cr");
                assert_eq!(filename.as_deref(), Some("report.md"));
            }
            DeliverPayload::Text(_) => panic!("expected File"),
        }
    }

    #[test]
    fn parse_a2a_json_text_type() {
        let a2a_json = r#"{
  "content": "jobId: 0x8bad\ndeliverableType: text\n- - -\nHello World 🌍\nLine 2\n- - -\n[intent:deliver]"
}"#;
        let json: serde_json::Value = serde_json::from_str(a2a_json).unwrap();
        let content = json.get("content").unwrap().as_str().unwrap();
        let payload = parse_deliver_content(content).expect("should parse text from A2A JSON");
        match payload {
            DeliverPayload::Text(text) => {
                assert_eq!(text, "Hello World 🌍\nLine 2");
            }
            DeliverPayload::File { .. } => panic!("expected Text"),
        }
    }

    #[test]
    fn parse_no_intent_deliver_returns_none() {
        let content = "jobId: 0xabc\ndeliverableType: text\n- - -\nsome text\n- - -\n";
        assert!(parse_deliver_content(content).is_none());
    }

    #[test]
    fn parse_missing_fields_returns_none() {
        let content = "jobId: 0xabc\ndeliverableType: file\nfileKey: k\n[intent:deliver]";
        assert!(parse_deliver_content(content).is_none(), "missing digest/salt/nonce/secret");
    }

    #[test]
    fn parse_text_with_internal_separator() {
        let content = "\
deliverableType: text
- - -
Part A
- - -
Part B continues
- - -
[intent:deliver]";
        let payload = parse_deliver_content(content).expect("should handle internal separator");
        match payload {
            DeliverPayload::Text(text) => {
                assert!(text.contains("Part A"), "should include Part A");
                assert!(text.contains("- - -"), "internal separator preserved");
                assert!(text.contains("Part B"), "should include Part B");
            }
            _ => panic!("expected Text"),
        }
    }

    // ── FR-10: recover dual-scans the spool and processes oldest → newest ──
    #[test]
    fn recover_processes_oldest_spool_file_first() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        // Redirect BOTH the spool dir (via TMPDIR → a2a_spool_dir) and ONCHAINOS_HOME
        // to isolated temp dirs so the test is hermetic and never touches a hardcoded
        // /tmp. The tempdirs are created BEFORE TMPDIR is set, so they land in the real
        // OS temp; the recover code then reads the redirected TMPDIR.
        let spool = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("TMPDIR", spool.path());
        std::env::set_var("ONCHAINOS_HOME", home.path());

        let job_id = "0xJOB";
        let a2a = |body: &str| {
            format!(
                r#"{{"content":"deliverableType: text\n- - -\n{body}\n- - -\n[intent:deliver]"}}"#
            )
        };
        let older = spool.path().join(format!("a2a_deliver_{job_id}_d1.json"));
        let newer = spool.path().join(format!("a2a_deliver_{job_id}_d2.json"));
        std::fs::write(&older, a2a("OLDEST")).unwrap();
        std::fs::write(&newer, a2a("NEWEST")).unwrap();
        // Force deterministic mtimes: older < newer (no sleep — avoids flakiness).
        std::fs::File::options()
            .write(true)
            .open(&older)
            .unwrap()
            .set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000))
            .unwrap();
        std::fs::File::options()
            .write(true)
            .open(&newer)
            .unwrap()
            .set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(2_000))
            .unwrap();

        let recovered = try_recover_from_temp_file(
            job_id, "1891", "short", "Title", "USDT", "10", Some("558"),
        )
        .expect("should recover from the oldest spool file");

        assert_eq!(recovered.deliverable_type, "text");
        assert_eq!(
            recovered.text_content.as_deref(),
            Some("OLDEST"),
            "must process the OLDEST delivery first (order-preserving)"
        );
        assert!(!older.exists(), "processed spool file must be deleted");
        assert!(
            newer.exists(),
            "the newer file must remain for the next recovery pass"
        );

        std::env::remove_var("TMPDIR");
        std::env::remove_var("ONCHAINOS_HOME");
    }

    // ── FB2: a poison-pill oldest spool file is quarantined, not re-selected ──
    #[test]
    fn recover_skips_poison_pill_and_processes_next() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let spool = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("TMPDIR", spool.path());
        std::env::set_var("ONCHAINOS_HOME", home.path());

        let job_id = "0xPOISON";
        let poison = spool.path().join(format!("a2a_deliver_{job_id}_d1.json"));
        let good = spool.path().join(format!("a2a_deliver_{job_id}_d2.json"));
        // Poison: not valid JSON → parse_a2a_file returns None → processing fails.
        std::fs::write(&poison, "not json at all").unwrap();
        std::fs::write(
            &good,
            r#"{"content":"deliverableType: text\n- - -\nGOOD\n- - -\n[intent:deliver]"}"#,
        )
        .unwrap();
        // Deterministic mtimes: poison (oldest) < good.
        std::fs::File::options()
            .write(true)
            .open(&poison)
            .unwrap()
            .set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000))
            .unwrap();
        std::fs::File::options()
            .write(true)
            .open(&good)
            .unwrap()
            .set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(2_000))
            .unwrap();

        let recovered = try_recover_from_temp_file(
            job_id, "1891", "short", "Title", "USDT", "10", Some("558"),
        )
        .expect("should skip the poison pill and recover the good file");

        assert_eq!(recovered.text_content.as_deref(), Some("GOOD"));
        assert!(!poison.exists(), "poison file must be moved aside");
        assert!(
            spool
                .path()
                .join(format!("a2a_deliver_{job_id}_d1.json.failed"))
                .exists(),
            "poison file must be quarantined as .failed, not deleted"
        );
        assert!(!good.exists(), "processed good file must be deleted");

        std::env::remove_var("TMPDIR");
        std::env::remove_var("ONCHAINOS_HOME");
    }

    // ── FB3: recovery surfaces the autotrade signal for live-path pipeline parity ──
    #[test]
    fn recover_surfaces_autotrade_signal_from_spool() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let spool = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("TMPDIR", spool.path());
        std::env::set_var("ONCHAINOS_HOME", home.path());

        let job_id = "0xFB3";
        // A text delivery whose content also carries an `autotrade:` block.
        let body = r#"{"content":"deliverableType: text\nautotrade: {\"schemaVersion\":1,\"deliveryId\":\"d1\",\"signalType\":\"dexTrade\",\"signalTime\":1,\"ttlSec\":60,\"params\":{}}\n- - -\nSIGNAL BODY\n- - -\n[intent:deliver]"}"#;
        let f = spool.path().join(format!("a2a_deliver_{job_id}_d1.json"));
        std::fs::write(&f, body).unwrap();

        let recovered = try_recover_from_temp_file(
            job_id, "1891", "short", "Title", "USDT", "10", Some("558"),
        )
        .expect("should recover the delivery");

        assert_eq!(recovered.deliverable_type, "text");
        assert!(
            recovered.autotrade_signal.is_some(),
            "recovery must surface the autotrade signal line so the caller can run the pipeline"
        );
        assert!(
            recovered
                .autotrade_signal
                .as_deref()
                .unwrap()
                .contains("dexTrade"),
            "the surfaced signal must be the raw autotrade JSON line"
        );

        // A plain delivery (no autotrade block) surfaces None — live path unchanged.
        let plain_job = "0xPLAIN";
        let plain = spool.path().join(format!("a2a_deliver_{plain_job}_d1.json"));
        std::fs::write(
            &plain,
            r#"{"content":"deliverableType: text\n- - -\nPLAIN\n- - -\n[intent:deliver]"}"#,
        )
        .unwrap();
        let plain_recovered = try_recover_from_temp_file(
            plain_job, "1891", "short", "Title", "USDT", "10", Some("558"),
        )
        .expect("should recover the plain delivery");
        assert!(
            plain_recovered.autotrade_signal.is_none(),
            "a delivery without an autotrade block must not surface a signal"
        );

        std::env::remove_var("TMPDIR");
        std::env::remove_var("ONCHAINOS_HOME");
    }

    // ── job_submitted_escrow review-deadline reminder (FR-2) ─────────────

    fn escrow_ctx_with_expire(
        expire_time: Option<i64>,
    ) -> crate::commands::agent_commerce::task::common::PreFetchedTaskContext {
        use crate::commands::agent_commerce::task::common::{
            PreFetchedDeliverable, PreFetchedTaskContext,
        };
        PreFetchedTaskContext {
            title: "Test Task".to_string(),
            description: String::new(),
            token_symbol: "USDT".to_string(),
            token_amount: "10".to_string(),
            payment_mode: Some(1),
            max_budget: None,
            provider_agent_id: Some("558".to_string()),
            user_agent_id: None,
            status: Some(2),
            deliverable: Some(PreFetchedDeliverable {
                path: "/tmp/deliverable.txt".to_string(),
                deliverable_type: "text".to_string(),
                original_name: "deliverable.txt".to_string(),
                text_content: Some("hello".to_string()),
            }),
            service_id: None,
            service_token_address: None,
            service_token_amount: None,
            service_params: None,
            user_agent_address: None,
            token_address: None,
            expire_time,
            test_flag: false,
        }
    }

    #[test]
    fn escrow_card_appends_review_line_when_expire_time_present() {
        let now = chrono::Local::now().timestamp();
        let p = escrow_ctx_with_expire(Some(now + 3 * 86_400));
        let ctx = crate::commands::agent_commerce::task::user::flow::FlowContext {
            job_id: "0xabc",
            agent_id: "426",
            short_id: "0xabc",
            title_display: "Test Task",
            title_query_hint: "",
            title_in_extract: "",
            terminal_session_hint: String::new(),
            payment_mode: Some(1),
            prefetched: Some(&p),
            data: None,
        };
        let out = job_submitted_escrow(&ctx);
        assert!(
            out.contains("⏰ Review deadline: 3 day(s)"),
            "escrow card should append the Review reminder line; got:\n{out}"
        );
    }

    #[test]
    fn escrow_card_no_reminder_when_expire_time_none() {
        let p = escrow_ctx_with_expire(None);
        let ctx = crate::commands::agent_commerce::task::user::flow::FlowContext {
            job_id: "0xabc",
            agent_id: "426",
            short_id: "0xabc",
            title_display: "Test Task",
            title_query_hint: "",
            title_in_extract: "",
            terminal_session_hint: String::new(),
            payment_mode: Some(1),
            prefetched: Some(&p),
            data: None,
        };
        let out = job_submitted_escrow(&ctx);
        assert!(
            !out.contains('⏰'),
            "no reminder line when expire_time is None; got:\n{out}"
        );
    }
}
