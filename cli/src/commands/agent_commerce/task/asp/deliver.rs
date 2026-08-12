//! ASP submits deliverable.
//!
//! ASP action: deliver — onchainos agent deliver
//!
//! Two shapes, resolved by which endpoint owns the job (one-shot tasks and subscriptions live
//! in disjoint registries — see `resolve_precondition`):
//!   - One-shot task — found under `GET /task/{jobId}` (Accepted + Escrow gate): the unchanged
//!     pipeline — prepare → A2A send → on-chain submit (flows the task to `submitted`) → local save.
//!   - Subscription task — NOT in `/task` (that 404s: `task not found` / code 1001); its detail
//!     lives under `GET /subscribe/{jobId}` and it is gated by subStatus liveness, not the
//!     one-shot Accepted/paymentMode check. Continuous delivery — prepare → A2A send, but NEVER
//!     an on-chain submit (closing/settlement is backend-automatic). A live (Active, incl. trial)
//!     subscription sends; an ended one short-circuits to `subscriptionExpired`. Output is a
//!     machine-readable four-state JSON the resident dispatch script consumes.

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::okx_a2a;
use crate::commands::agent_commerce::task::common::payment_mode::PaymentMode;
use crate::commands::agent_commerce::task::common::state_machine::Status;
use crate::commands::agent_commerce::task::common::DEBUG_LOG;
use crate::commands::agent_commerce::task::signing;
use super::subscription::{self, Routing};

const LONG_TEXT_THRESHOLD: usize = 200;

/// Machine-readable outcome of a subscription-task delivery. The resident dispatch script
/// consumes this JSON to decide continue / drop-from-list / retry. Emitted only for
/// subscription tasks (jobType 1); one-shot deliveries keep the human-readable output.
///
/// Four-state contract (design doc §开发项 5):
///   {"ok":true, "delivered":true,  "jobId":…, "deliveryId":…}     // sent → continue
///   {"ok":true, "delivered":false, "reason":"alreadyDelivered"}   // idempotent skip
///   {"ok":false,"reason":"subscriptionExpired","backendCode":…}   // drop from list
///   {"ok":false,"reason":"sendFailed","message":…}                // retry same signal
enum DeliverOutcome {
    /// Continuous delivery sent (subscription stays `accepted`; no on-chain submit).
    /// `delivery_id` is carried when the delivery bore an autotrade signal.
    Delivered { delivery_id: Option<String> },
    /// This jobId×deliveryId signal was already delivered by a prior successful send —
    /// idempotent skip, do NOT resend (offline-backlog / restart-replay / retry).
    AlreadyDelivered { delivery_id: String },
    /// The subscription has ended — rejected; a stale signal must never escalate into a
    /// submit. `backend_code` carries the subStatus indicator.
    SubscriptionExpired { backend_code: String },
    /// A2A send failed — the script should retry the same signal next round.
    SendFailed(String),
}

/// Print the four-state deliver JSON for a subscription-task delivery.
fn print_deliver_result(outcome: &DeliverOutcome, job_id: &str) {
    let v = match outcome {
        DeliverOutcome::Delivered { delivery_id } => {
            let mut o = serde_json::json!({ "ok": true, "delivered": true, "jobId": job_id });
            if let Some(did) = delivery_id {
                o["deliveryId"] = serde_json::Value::String(did.clone());
            }
            o
        }
        DeliverOutcome::AlreadyDelivered { delivery_id } => serde_json::json!({
            "ok": true, "delivered": false, "reason": "alreadyDelivered",
            "jobId": job_id, "deliveryId": delivery_id,
        }),
        DeliverOutcome::SubscriptionExpired { backend_code } => serde_json::json!({
            "ok": false, "reason": "subscriptionExpired", "jobId": job_id, "backendCode": backend_code,
        }),
        DeliverOutcome::SendFailed(msg) => serde_json::json!({
            "ok": false, "reason": "sendFailed", "message": msg,
        }),
    };
    println!(
        "{}",
        crate::output::to_agent_json(&v)
            .unwrap_or_else(|_| r#"{"ok":false,"reason":"sendFailed"}"#.to_string())
    );
}

/// Deliverable preparation result — carries the info needed by later stages
/// (xmtp message was already sent; this tracks what to save locally).
enum Prepared {
    File { local_path: String, file_key: String },
    Text { tmp_path: String },
}

/// Keep accepting the retired `--autotrade` argument so older ASP scripts do
/// not fail at CLI parsing, but never put that payload on the wire. Text/file
/// deliverables are now the only delivery source of truth.
fn build_outbound_deliver_message(base_message: String, _autotrade: &str) -> String {
    base_message
}

// ── Debug-only local E2E mock (ONCHAINOS_TEST_MOCK_SUBSCRIPTION=1) ───────────
// Lets the resident-script subscription flow be exercised end-to-end with NO backend,
// credentials, or XMTP — the precondition task detail is synthesized (accepted + escrow +
// jobType 1) and each send is written to a local outbox file instead of uploaded + XMTP-sent.
// Compiled OUT of release builds (`#[cfg(debug_assertions)]`), so a release ASP can never
// fake a delivery.

/// Fetch the precondition task detail, or synthesize an accepted-escrow subscription task
/// when the debug mock is on.
async fn fetch_task_detail_or_mock(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<serde_json::Value> {
    #[cfg(debug_assertions)]
    if std::env::var("ONCHAINOS_TEST_MOCK_SUBSCRIPTION").as_deref() == Ok("1") {
        eprintln!("[MOCK] ONCHAINOS_TEST_MOCK_SUBSCRIPTION=1 — synthesizing accepted escrow subscription task {job_id}");
        return Ok(mock_task_resp(job_id));
    }
    let path = client.task_path(job_id);
    client.get_with_identity(&path, agent_id).await
}

/// Read a JSON field that may be serialized as a string or a number, as an owned string.
fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|f| f.as_str().map(str::to_string).or_else(|| f.as_i64().map(|n| n.to_string())))
}

/// True when a `/task/{jobId}` lookup failed because the job is NOT a one-shot task
/// (backend code 1001 "task not found") — i.e. it is a subscription, which lives only under
/// `/subscribe/{jobId}`. The one-shot task endpoint and the subscription endpoint are disjoint.
fn is_task_not_found(err: &anyhow::Error) -> bool {
    let s = err.to_string();
    s.contains("task not found") || s.contains("code=1001")
}

/// Normalized delivery precondition, resolved from whichever endpoint owns the job.
struct Precondition {
    routing: Routing,
    /// subStatus code (subscription path) for the audit trail; 0 on the one-shot path.
    sub_status_code: i64,
    user_agent_id: String,
    title: String,
    token_symbol: Option<String>,
    token_amount: Option<String>,
}

/// Resolve the delivery precondition. One-shot tasks live under `GET /task/{jobId}` and keep
/// the Accepted + Escrow gate; subscription tasks live under `GET /subscribe/{jobId}` (the task
/// endpoint 404s for them) and are gated by subStatus liveness instead (Active → deliver & skip
/// submit; any other status → Ended → `subscriptionExpired`). The debug mock synthesizes a
/// jobType-1 task detail, so it flows through the one-shot branch and its subscription lookup.
async fn resolve_precondition(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
    now_secs: i64,
) -> Result<Precondition> {
    match fetch_task_detail_or_mock(client, job_id, agent_id).await {
        // ── One-shot task (or the debug mock, which synthesizes jobType=1) ──
        Ok(task_resp) => {
            let status_int = task_resp["status"]
                .as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .ok_or_else(|| anyhow::anyhow!("Task detail missing status field, cannot determine delivery eligibility"))?;
            let status = Status::from_int(status_int);
            if status != Status::Accepted {
                audit::log(
                    "cli", "ASP/deliver_blocked_wrong_status", false, Duration::default(),
                    Some(vec![
                        format!("jobId={job_id}"), format!("agentId={agent_id}"),
                        format!("statusInt={status_int}"), format!("status={}", status.as_str()),
                    ]),
                    Some("status != accepted(1)"),
                );
                bail!(
                    "Deliver rejected: current task status = {} ({}), must be accepted (1) before delivery.\n\
                     If you just applied, wait for the User Agent to confirm-accept on-chain and receive the `job_accepted` system notification before delivering.\n\
                     Do NOT call `okx-a2a xmtp-send` to rush the User Agent — confirm-accept is a user decision driven by the User Agent's session.",
                    status_int, status.as_str(),
                );
            }

            let pm_int = task_resp["paymentMode"]
                .as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .unwrap_or(1);
            let pm = PaymentMode::from_int(pm_int);
            if pm != PaymentMode::Escrow {
                audit::log(
                    "cli", "ASP/deliver_blocked_wrong_payment_mode", false, Duration::default(),
                    Some(vec![
                        format!("jobId={job_id}"), format!("agentId={agent_id}"),
                        format!("paymentMode={pm_int}"), format!("paymentModeStr={}", pm.as_str()),
                    ]),
                    Some("deliver is escrow-only"),
                );
                bail!(
                    "Deliver rejected: paymentMode = {} ({}) — deliver/submit is only supported for escrow (1).\n\
                     x402 tasks skip the submit step; the User Agent obtains the deliverable by replaying the ASP's endpoint and calls /direct/complete.",
                    pm_int, pm.as_str(),
                );
            }

            // jobType is read from the precondition task detail. A one-shot task (jobType 0 /
            // absent) short-circuits to NotSubscription with no extra call. Only the debug mock
            // reaches jobType==1 here (real subscriptions 404 on /task and take the branch below);
            // it then makes the `/subscribe/{jobId}` liveness query.
            let job_type = task_resp["jobType"]
                .as_i64()
                .or_else(|| task_resp["jobType"].as_str().and_then(|s| s.trim().parse::<i64>().ok()))
                .unwrap_or(0);
            let (routing, sub_status_code) = if job_type == subscription::JOB_TYPE_SUBSCRIBE {
                match subscription::fetch_detail(client, job_id, agent_id).await {
                    Some(d) => (d.liveness(now_secs), d.status.code()),
                    None => (Routing::Active, 0),
                }
            } else {
                (Routing::NotSubscription, 0)
            };

            Ok(Precondition {
                routing,
                sub_status_code,
                user_agent_id: task_resp["buyerAgentId"].as_str().unwrap_or("").to_string(),
                title: task_resp["title"].as_str().unwrap_or("(untitled)").to_string(),
                token_symbol: task_resp["tokenSymbol"].as_str().map(str::to_string),
                token_amount: task_resp["tokenAmount"].as_str().map(str::to_string),
            })
        }
        // ── Subscription task: the authoritative detail lives under /subscribe/{jobId} ──
        // (the task endpoint 404s — one-shot and subscription registries are disjoint). There is
        // no task-status/paymentMode gate here; the subscription's delivery gate is its subStatus
        // liveness. An ended subscription routes to Ended → `subscriptionExpired`.
        Err(e) if is_task_not_found(&e) => {
            let path = client.subscribe_path(job_id);
            let raw = client.get_with_identity(&path, agent_id).await.map_err(|e2| {
                anyhow::anyhow!(
                    "job {job_id} is not a one-shot task, and its subscription detail lookup failed: {e2}"
                )
            })?;
            let d = subscription::SubscriptionDetail::from_json(&raw);
            Ok(Precondition {
                routing: d.liveness(now_secs),
                sub_status_code: d.status.code(),
                user_agent_id: json_str(&raw, "buyerAgentId").unwrap_or_default(),
                title: json_str(&raw, "title").unwrap_or_else(|| "(untitled)".to_string()),
                token_symbol: json_str(&raw, "tokenSymbol"),
                // subscriptions never submit, so token amount is informational only; prefer the
                // service fee, fall back to the payment amount.
                token_amount: json_str(&raw, "serviceTokenAmount")
                    .or_else(|| json_str(&raw, "paymentTokenAmount")),
            })
        }
        Err(e) => Err(e),
    }
}

/// Send the A2A delivery message, or write it to a local outbox when the debug mock is on.
fn send_or_mock(job_id: &str, user_agent_id: &str, msg: &str) -> Result<()> {
    #[cfg(debug_assertions)]
    if std::env::var("ONCHAINOS_TEST_MOCK_SUBSCRIPTION").as_deref() == Ok("1") {
        return mock_write_outbox(job_id, msg);
    }
    okx_a2a::xmtp_send(job_id, user_agent_id, msg)
}

#[cfg(debug_assertions)]
fn mock_task_resp(job_id: &str) -> serde_json::Value {
    let buyer = std::env::var("ONCHAINOS_TEST_MOCK_BUYER_AGENT")
        .unwrap_or_else(|_| "mock-buyer-agent-0001".to_string());
    serde_json::json!({
        "jobId": job_id,
        "status": 1,        // accepted
        "paymentMode": 1,   // escrow
        "jobType": 1,       // subscription
        "buyerAgentId": buyer,
        "title": "[MOCK] subscription continuous delivery",
        "tokenSymbol": "USDC",
        "tokenAmount": "0",
    })
}

#[cfg(debug_assertions)]
fn mock_write_outbox(job_id: &str, msg: &str) -> Result<()> {
    let dir = crate::home::onchainos_home()?.join("mock_outbox").join(job_id);
    std::fs::create_dir_all(&dir)?;
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f").to_string();
    let path = dir.join(format!("delivery-{ts}.txt"));
    std::fs::write(&path, msg)?;
    eprintln!("[MOCK] delivery written to {}", path.display());
    Ok(())
}

/// deliver — submit deliverable (one-shot) OR continuously deliver (subscription).
pub async fn handle_deliver(
    client: &mut TaskApiClient,
    job_id: &str,
    file: &str,
    deliverable_text: &str,
    agent_id: &str,
    autotrade: &str,
) -> Result<()> {
    if agent_id.is_empty() {
        bail!("--agent-id is required (pass the ASP's own agentId; beta backend rejects empty agenticId header)");
    }

    // `--autotrade` is a retired compatibility argument. Deliberately do not
    // parse, validate, stamp, append, or derive execution/idempotency state from
    // it. The explicit text/file deliverable is the sole source of truth.
    let legacy_autotrade_ignored = !autotrade.trim().is_empty();
    let signal_delivery_id: Option<String> = None;

    // ── 1. Precondition checks ──────────────────────────────────────────

    // Resolve the precondition from whichever endpoint owns the job: one-shot tasks live under
    // `/task/{jobId}` (Accepted + Escrow gate), subscription tasks under `/subscribe/{jobId}`
    // (subStatus-liveness gate). See `resolve_precondition`.
    let now_secs = chrono::Local::now().timestamp();
    let pre = resolve_precondition(client, job_id, agent_id, now_secs).await?;
    let routing = pre.routing;
    let sub_status_code = pre.sub_status_code;
    let is_subscription = routing != Routing::NotSubscription;
    let user_agent_id = pre.user_agent_id.as_str();
    let title = pre.title.as_str();
    let token_symbol = pre.token_symbol.as_deref();
    let token_amount = pre.token_amount.as_deref();
    let short_id = if job_id.len() > 12 {
        format!("{}…", &job_id[..8])
    } else {
        job_id.to_string()
    };

    let base_tags = vec![format!("jobId={job_id}"), format!("agentId={agent_id}")];

    if legacy_autotrade_ignored {
        audit::log(
            "cli",
            "ASP/deliver_legacy_autotrade_ignored",
            true,
            Duration::default(),
            Some(base_tags.clone()),
            Some("retired --autotrade value ignored; text/file deliverable is the only payload"),
        );
    }

    // ── Ended-subscription short-circuit (before any send) ──
    // The ASP neither delivers nor submits — report subscriptionExpired
    // and let the resident script drop the job. A stale signal on a dead subscription is thus
    // never delivered.
    if routing == Routing::Ended {
        let backend_code = format!("subStatus={sub_status_code}");
        audit::log("cli", "ASP/deliver_subscription_expired", false, Duration::default(),
            Some([base_tags.clone(), vec![format!("subStatus={sub_status_code}"), format!("legacyAutotradeIgnored={legacy_autotrade_ignored}")]].concat()),
            Some("subscription ended → not delivered; settlement is backend-automatic"));
        print_deliver_result(&DeliverOutcome::SubscriptionExpired { backend_code }, job_id);
        return Ok(());
    }

    // ── 2. Prepare deliverable + A2A send ───────────────────────────────
    // Track the last send failure. On the subscription (Active) path a send failure aborts the
    // whole deliver (the script retries the same signal); the one-shot path only audit-logs and
    // continues to submit (legacy behavior).
    let mut send_error: Option<String> = None;

    let prepared = if !file.is_empty() {
        // ▸ Native file (image / PDF / document)
        let src = std::path::Path::new(file);
        if !src.exists() {
            bail!("file not found: {file}");
        }
        audit::log("cli", "ASP/deliver_file_upload", true, Duration::default(),
            Some([base_tags.clone(), vec![format!("path={file}")]].concat()), None);
        let upload = okx_a2a::file_upload(file, agent_id, job_id, None, None)?;
        audit::log("cli", "ASP/deliver_file_uploaded", true, Duration::default(),
            Some([base_tags.clone(), vec![format!("fileKey={}", upload.file_key)]].concat()), None);

        let msg = build_outbound_deliver_message(
            super::content::build_file_deliver_message(job_id, &upload),
            autotrade,
        );
        if !user_agent_id.is_empty() {
            match send_or_mock(job_id, user_agent_id, &msg) {
                Ok(()) => audit::log("cli", "ASP/deliver_xmtp_sent", true, Duration::default(),
                    Some([base_tags.clone(), vec!["type=file".into()]].concat()), None),
                Err(e) => {
                    audit::log("cli", "ASP/deliver_xmtp_failed", false, Duration::default(),
                        Some([base_tags.clone(), vec!["type=file".into()]].concat()), Some(&e.to_string()));
                    send_error = Some(e.to_string());
                }
            }
        }
        Prepared::File { local_path: file.to_string(), file_key: upload.file_key }
    } else if !deliverable_text.is_empty() {
        let text_len = deliverable_text.chars().count();
        let is_long = text_len > LONG_TEXT_THRESHOLD;
        audit::log("cli", "ASP/deliver_text_prepare", true, Duration::default(),
            Some([base_tags.clone(), vec![format!("charCount={text_len}"), format!("isLong={is_long}")]].concat()), None);

        if is_long {
            // ▸ Long text → write .md → file_upload → file-format xmtp
            //   Fallback: if tmp write or file_upload fails, degrade to inline text.
            let file_result = (|| -> Result<(Prepared, Option<String>)> {
                let tmp_dir = std::env::temp_dir();
                let tmp_path = tmp_dir.join(format!("deliverable_{}.md", job_id));
                std::fs::write(&tmp_path, deliverable_text)?;
                let tmp_str = tmp_path.display().to_string();
                let upload = okx_a2a::file_upload(&tmp_str, agent_id, job_id, None, None)?;
                audit::log("cli", "ASP/deliver_long_text_uploaded", true, Duration::default(),
                    Some([base_tags.clone(), vec![format!("fileKey={}", upload.file_key), format!("path={tmp_str}")]].concat()), None);

                let msg = build_outbound_deliver_message(
                    super::content::build_file_deliver_message(job_id, &upload),
                    autotrade,
                );
                let mut local_err: Option<String> = None;
                if !user_agent_id.is_empty() {
                    match send_or_mock(job_id, user_agent_id, &msg) {
                        Ok(()) => audit::log("cli", "ASP/deliver_xmtp_sent", true, Duration::default(),
                            Some([base_tags.clone(), vec!["type=file_from_long_text".into()]].concat()), None),
                        Err(e) => {
                            audit::log("cli", "ASP/deliver_xmtp_failed", false, Duration::default(),
                                Some([base_tags.clone(), vec!["type=file_from_long_text".into()]].concat()), Some(&e.to_string()));
                            local_err = Some(e.to_string());
                        }
                    }
                }
                Ok((Prepared::File { local_path: tmp_str, file_key: upload.file_key }, local_err))
            })();
            match file_result {
                Ok((p, local_err)) => {
                    send_error = local_err;
                    p
                }
                Err(e) => {
                    audit::log("cli", "ASP/deliver_long_text_fallback", false, Duration::default(),
                        Some([base_tags.clone(), vec![format!("charCount={text_len}")]].concat()), Some(&e.to_string()));

                    let msg = build_outbound_deliver_message(
                        super::content::build_text_deliver_message(job_id, deliverable_text),
                        autotrade,
                    );
                    if !user_agent_id.is_empty() {
                        match send_or_mock(job_id, user_agent_id, &msg) {
                            Ok(()) => audit::log("cli", "ASP/deliver_xmtp_sent", true, Duration::default(),
                                Some([base_tags.clone(), vec!["type=text_fallback".into()]].concat()), None),
                            Err(e) => {
                                audit::log("cli", "ASP/deliver_xmtp_failed", false, Duration::default(),
                                    Some([base_tags.clone(), vec!["type=text_fallback".into()]].concat()), Some(&e.to_string()));
                                send_error = Some(e.to_string());
                            }
                        }
                    }
                    let tmp_dir = std::env::temp_dir();
                    let tmp_path = tmp_dir.join(format!(
                        "deliverable_{}.txt", chrono::Local::now().format("%Y%m%d%H%M%S")
                    ));
                    let _ = std::fs::write(&tmp_path, deliverable_text);
                    Prepared::Text { tmp_path: tmp_path.display().to_string() }
                }
            }
        } else {
            // ▸ Short text → inline text-format xmtp
            let msg = build_outbound_deliver_message(
                super::content::build_text_deliver_message(job_id, deliverable_text),
                autotrade,
            );
            if !user_agent_id.is_empty() {
                match send_or_mock(job_id, user_agent_id, &msg) {
                    Ok(()) => audit::log("cli", "ASP/deliver_xmtp_sent", true, Duration::default(),
                        Some([base_tags.clone(), vec!["type=text".into()]].concat()), None),
                    Err(e) => {
                        audit::log("cli", "ASP/deliver_xmtp_failed", false, Duration::default(),
                            Some([base_tags.clone(), vec!["type=text".into()]].concat()), Some(&e.to_string()));
                        send_error = Some(e.to_string());
                    }
                }
            }
            let tmp_dir = std::env::temp_dir();
            let tmp_path = tmp_dir.join(format!(
                "deliverable_{}.txt", chrono::Local::now().format("%Y%m%d%H%M%S")
            ));
            let _ = std::fs::write(&tmp_path, deliverable_text);
            Prepared::Text { tmp_path: tmp_path.display().to_string() }
        }
    } else {
        bail!("Either --file or --deliverable-text must be provided");
    };

    // Continuous-delivery (Active) send failure = hard failure: emit the four-state JSON and
    // stop, so the resident script retries the same signal next round. Only the Active phase
    // reaches here (Ended short-circuited above); the one-shot path keeps legacy continue.
    if routing == Routing::Active {
        if let Some(msg) = &send_error {
            audit::log("cli", "ASP/deliver_subscription_send_failed", false, Duration::default(),
                Some(base_tags.clone()), Some(msg));
            print_deliver_result(&DeliverOutcome::SendFailed(msg.clone()), job_id);
            return Ok(());
        }
    }

    // ── 3. On-chain submit (one-shot only) ──────────────────────────────
    // A subscription task NEVER submits: an Active continuous delivery keeps the task in
    // `accepted`, and closing/settlement is backend-automatic (Ended already short-circuited).
    let tx_hash: Option<String> = if is_subscription {
        audit::log("cli", "ASP/deliver_subscription_continued", true, Duration::default(),
            Some([base_tags.clone(), vec![format!("subStatus={sub_status_code}")]].concat()), None);
        None
    } else {
        let (account_id, address) = signing::resolve_wallet_by_agent_id(agent_id).await?;
        let body = serde_json::json!({ "evidenceHash": "" });
        let resp = client.post_with_identity(&client.endpoint(job_id, "submit"), &body, agent_id).await?;
        let tx = signing::sign_uop_and_broadcast(
            client, &resp["uopData"], &account_id, &address,
            job_id, signing::extract_biz_type(&resp), agent_id, None,
        ).await?;
        audit::log("cli", "ASP/deliver_submitted", true, Duration::default(),
            Some(vec![format!("jobId={job_id}"), format!("agentId={agent_id}"), format!("txHash={tx}")]), None);
        Some(tx)
    };

    // ── 4. Local persistent save ────────────────────────────────────────

    match &prepared {
        Prepared::File { local_path, file_key } => {
            if std::path::Path::new(local_path).exists() {
                let params = super::super::common::deliverables::SaveParams {
                    job_id, role: "asp", file_path: local_path, deliverable_type: "file",
                    title, short_id: &short_id, file_key: Some(file_key),
                    token_symbol, token_amount,
                    counterparty_agent_id: Some(user_agent_id).filter(|s| !s.is_empty()),
                    counterparty_name: None,
                };
                match super::super::common::deliverables::handle_save(&params) {
                    Ok(r) => { if DEBUG_LOG { eprintln!("[deliver] deliverable auto-saved: {}", r.path); } }
                    Err(e) => { if DEBUG_LOG { eprintln!("[deliver] deliverable auto-save failed (non-blocking): {e}"); } }
                }
            }
        }
        Prepared::Text { tmp_path } => {
            if std::path::Path::new(tmp_path).exists() {
                let params = super::super::common::deliverables::SaveParams {
                    job_id, role: "asp", file_path: tmp_path, deliverable_type: "text",
                    title, short_id: &short_id, file_key: None,
                    token_symbol, token_amount,
                    counterparty_agent_id: Some(user_agent_id).filter(|s| !s.is_empty()),
                    counterparty_name: None,
                };
                match super::super::common::deliverables::handle_save(&params) {
                    Ok(r) => { if DEBUG_LOG { eprintln!("[deliver] text deliverable auto-saved: {}", r.path); } }
                    Err(e) => { if DEBUG_LOG { eprintln!("[deliver] text deliverable auto-save failed (non-blocking): {e}"); } }
                }
            }
        }
    }

    // ── 5. Output ────────────────────────────────────────────────────────
    if is_subscription {
        // Continuous delivery (Active) — sent, no submit. The resident script consumes this
        // JSON to decide whether to continue to the next task.
        let _ = tx_hash; // always None for a subscription (never submits)
        print_deliver_result(&DeliverOutcome::Delivered { delivery_id: signal_delivery_id.clone() }, job_id);
    } else {
        let tx_hash = tx_hash.expect("one-shot delivery always runs the on-chain submit");
        println!("✓ Deliverable submitted, waiting for on-chain confirmation (job_submitted)");
        println!("  txHash: {tx_hash}");
        println!();
        println!("⚠️  Next steps are driven by system notifications — do not proactively message the User Agent:");
        println!("    - You will receive a `job_submitted` system notification after on-chain confirmation");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retired_autotrade_never_changes_outbound_text() {
        let base = super::super::content::build_text_deliver_message(
            "job-1",
            "【合约信号】BTC-PERP | LONG 2x | 10分钟内有效",
        );
        let valid_legacy_json = r#"{"schemaVersion":1,"deliveryId":"old-1"}"#;

        for retired_value in ["", valid_legacy_json, "{not-json"] {
            let outbound = build_outbound_deliver_message(base.clone(), retired_value);
            assert_eq!(outbound, base);
            assert!(!outbound.contains("autotrade:"));
            assert!(!outbound.contains("schemaVersion"));
        }
    }
}
