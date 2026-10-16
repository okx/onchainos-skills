//! ASP broadcasts an Active-subscription signal to all Shared Groups of a Service (V2).
//!
//! Publish semantics (fail-closed, no retry):
//!   1. Validate agentId / serviceId / content BEFORE any HTTP or transport.
//!   2. Fetch the Service's Shared Groups via `listGroupsByService` (JWT + agenticId).
//!      `serviceId` is used ONLY for this query (plus local audit tags) — it is never passed to
//!      the A2A transport and never enters the XMTP envelope.
//!   3. Keep only conversations that are addressable (non-empty conversationId, status
//!      Active/Full), isolate the rest, and dedup by conversationId. Every isolated or
//!      duplicate entry increments `skippedCount` and is never sent to.
//!   4. If there are zero unique targets, return a zero-target success WITHOUT calling A2A.
//!      Otherwise call `okx-a2a subscription-push` EXACTLY ONCE for the whole batch — every
//!      deduped conversationId is passed as a repeated `--conversation-id` flag. OnchainOS does
//!      NOT fan out per-conversationId itself; the A2A binary owns the Group-only check and the
//!      parallel send.
//!   5. Emit a machine-readable summary. A partial failure (`payload.failedCount > 0`) is a
//!      COMPLETED publish — there is NO fallback, NO resend, and NO retry. A process-level
//!      failure (nonzero exit / non-JSON stdout / top-level `ok != true` / malformed payload) is
//!      fail-closed: the whole command returns `Err`.
//!
//! Unlike `okx_a2a::xmtp_send` (a peer `--to-agent-id` message), this addresses one or more
//! Shared-Group conversations via the dedicated `okx-a2a subscription-push` command. OnchainOS
//! does not construct the XMTP envelope (`msgType=a2a-subscription-push`, …) — that is entirely
//! the A2A binary's responsibility.

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

/// Fail-closed input validation. Extracted so it is exercised BEFORE any client use and can be
/// unit-tested without constructing a `TaskApiClient`.
fn validate_inputs(agent_id: &str, service_id: &str, content: &str) -> Result<()> {
    if agent_id.trim().is_empty() {
        bail!("--agent-id is required (ASP agentId; empty agenticId header is rejected)");
    }
    if service_id.trim().is_empty() {
        bail!("--service-id is required");
    }
    if content.trim().is_empty() {
        bail!("--content is required");
    }
    Ok(())
}

/// Percent-encode a query-string component. SMALL inline encoder (no new crate): keeps the
/// RFC 3986 unreserved set verbatim and `%XX`-escapes every other byte, so the serviceId is safe
/// as the sole `serviceId=` query value.
fn encode_query_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `listGroupsByService` path with the trimmed serviceId as the only (percent-encoded) query param.
fn list_groups_path(service_id: &str) -> String {
    let encoded = encode_query_component(service_id.trim());
    format!("/priapi/v1/aieco/im/xmtp/subscription/list-groups-by-service?serviceId={encoded}")
}

/// Parse the `listGroupsByService` `data`, returning `(unique kept conversationIds, skipped_count)`.
///
/// A missing `conversations` key or a non-array value is fail-closed (`Err`) — NO send. An empty
/// array is a valid zero-target success. An item is KEPT iff its conversationId is non-empty AND
/// its status (lowercased) is `active` or `full`; anything else is isolated. Kept items are then
/// deduped by conversationId (first wins). Both isolated and dropped-duplicate items count toward
/// `skipped_count`.
fn select_targets(data: &Value) -> Result<(Vec<String>, usize)> {
    let conversations = match data.get("conversations") {
        Some(Value::Array(arr)) => arr,
        _ => bail!("unparseable listGroupsByService response"),
    };
    let mut kept: Vec<String> = Vec::new();
    let mut skipped = 0usize;
    for item in conversations {
        let conv_id = item
            .get("conversationId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let status = item
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_lowercase();
        let is_target = !conv_id.is_empty() && (status == "active" || status == "full");
        if !is_target {
            skipped += 1;
            continue;
        }
        if kept.iter().any(|k| k == conv_id) {
            skipped += 1;
            continue;
        }
        kept.push(conv_id.to_string());
    }
    Ok((kept, skipped))
}

/// Build the machine-readable publish summary. `conv_count` / `sent` / `failed` come straight
/// from the A2A `payload` (zero-target callers pass `0, 0, 0`); `skipped` is OnchainOS's own
/// group-filter/dedup tally and is never known to A2A.
fn build_summary(
    conv_count: u64,
    sent: u64,
    failed: u64,
    skipped: usize,
    results: Vec<Value>,
) -> Value {
    json!({
        "ok": true,
        "conversationCount": conv_count,
        "sentCount": sent,
        "failedCount": failed,
        "skippedCount": skipped,
        "results": results,
    })
}

/// Fetch the Service's Shared Groups, or synthesize them when the debug mock is on.
///
/// Real path: GET `listGroupsByService` with JWT + agenticId (`get_with_agent_id`), called EXACTLY
/// ONCE; returns the `data` value. An Err propagates (fail-closed).
async fn fetch_groups_or_mock(
    client: &mut TaskApiClient,
    service_id: &str,
    agent_id: &str,
) -> Result<Value> {
    #[cfg(debug_assertions)]
    if std::env::var("ONCHAINOS_TEST_MOCK_SUBSCRIPTION_PUSH").as_deref() == Ok("1") {
        // Inline JSON wins over the file; a parse error in either is fail-closed (Err).
        let raw = match std::env::var("ONCHAINOS_TEST_MOCK_GROUPS_JSON") {
            Ok(inline) => Some(inline),
            Err(_) => match std::env::var("ONCHAINOS_TEST_MOCK_GROUPS_FILE") {
                Ok(path) => Some(
                    std::fs::read_to_string(&path)
                        .map_err(|e| anyhow!("mock groups file read failed: {e}"))?,
                ),
                Err(_) => None,
            },
        };
        return match raw {
            Some(s) => serde_json::from_str::<Value>(&s)
                .map_err(|e| anyhow!("mock groups JSON parse failed: {e}")),
            None => Ok(json!({ "conversations": [] })),
        };
    }
    let path = list_groups_path(service_id);
    client.get_with_agent_id(&path, agent_id.trim()).await
}

/// Build the argv for a single `okx-a2a subscription-push` invocation covering ALL targets.
/// Pure function so the exact flag shape is unit-testable without spawning a process.
///
/// MUST include (in order): `subscription-push`, `--my-agent-id <agentId>`,
/// `--message <content>`, one repeated `--conversation-id <id>` per target, `--json`.
/// MUST NOT include: `group`, `send`, `--agent-id`, `--content`, or `--service-id` — those are
/// OnchainOS's own outer-command flags, not the A2A transport's.
fn build_okx_a2a_subscription_push_args(
    agent_id: &str,
    content: &str,
    conversation_ids: &[String],
) -> Vec<String> {
    let mut args = vec![
        "subscription-push".to_string(),
        "--my-agent-id".to_string(),
        agent_id.to_string(),
        "--message".to_string(),
        content.to_string(),
    ];
    for id in conversation_ids {
        args.push("--conversation-id".to_string());
        args.push(id.clone());
    }
    args.push("--json".to_string());
    args
}

/// Parse a single `okx-a2a subscription-push` process outcome into its `payload` object,
/// enforcing fail-closed handling for every non-success shape (FR-3/NFR-3):
///   - nonzero exit → Err (stderr surfaced; no JWT/backend internals to leak here)
///   - stdout not valid JSON → Err
///   - top-level `ok != true` → Err (using the response's own `error`, when present)
///   - `payload` missing, not an object, or missing/malformed `conversationCount` /
///     `sentCount` / `failedCount` / `results` → Err (fail-closed; never fabricate success)
///   - a `results[]` entry missing `conversationId` or carrying an unrecognized `status` → Err
///
/// A `payload.failedCount > 0` (partial failure) is explicitly NOT an error here — it is
/// returned as `Ok(payload)` so the caller reports a completed partial publish. There is no
/// retry / per-group resend / deliver-fallback anywhere in this path.
fn parse_a2a_subscription_push_response(
    exit_ok: bool,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<Value> {
    if !exit_ok {
        let stderr = String::from_utf8_lossy(stderr);
        bail!("okx-a2a subscription-push exited non-zero: {stderr}");
    }
    let response: Value = serde_json::from_slice(stdout)
        .map_err(|e| anyhow!("okx-a2a subscription-push stdout not valid JSON: {e}"))?;
    if response.get("ok").and_then(Value::as_bool) != Some(true) {
        let err = response
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("okx-a2a subscription-push returned ok=false");
        bail!("okx-a2a subscription-push failed: {err}");
    }
    let payload = response
        .get("payload")
        .filter(|p| p.is_object())
        .ok_or_else(|| anyhow!("okx-a2a subscription-push response missing payload"))?;
    for field in ["conversationCount", "sentCount", "failedCount"] {
        if payload.get(field).and_then(Value::as_u64).is_none() {
            bail!(
                "okx-a2a subscription-push payload.{field} missing or not a non-negative integer"
            );
        }
    }
    let results = payload
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            anyhow!("okx-a2a subscription-push payload.results missing or not an array")
        })?;
    for entry in results {
        let has_conv_id = entry
            .get("conversationId")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty());
        let has_legal_status = matches!(
            entry.get("status").and_then(Value::as_str),
            Some("sent") | Some("failed") | Some("not_group")
        );
        if !has_conv_id || !has_legal_status {
            bail!("okx-a2a subscription-push payload.results entry missing conversationId or has an unrecognized status: {entry}");
        }
    }
    Ok(payload.clone())
}

/// Map one A2A `results[]` entry into the OnchainOS-facing shape: keep `conversationId` /
/// `status` / `messageId` / `error` verbatim, and add the boolean `ok` the existing OnchainOS
/// contract expects — `status == "sent"` → `true`; `"failed"` | `"not_group"` → `false`.
fn map_a2a_result(entry: &Value) -> Value {
    let conv_id = entry
        .get("conversationId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let status = entry.get("status").and_then(Value::as_str).unwrap_or("");
    let mut obj = json!({
        "conversationId": conv_id,
        "status": status,
        "ok": status == "sent",
    });
    if let Some(message_id) = entry.get("messageId") {
        obj["messageId"] = message_id.clone();
    }
    if let Some(error) = entry.get("error") {
        obj["error"] = error.clone();
    }
    obj
}

/// Broadcast to every deduped target with exactly ONE `okx-a2a subscription-push` invocation.
/// The A2A binary owns the Group-only check and the parallel per-conversation send; OnchainOS
/// never fans out per conversationId and never issues a second call for the same publish.
async fn subscription_push_send_all(
    agent_id: &str,
    content: &str,
    conversation_ids: &[String],
) -> Result<Value> {
    #[cfg(debug_assertions)]
    if std::env::var("ONCHAINOS_TEST_MOCK_SUBSCRIPTION_PUSH").as_deref() == Ok("1") {
        return Ok(mock_subscription_push_payload(
            agent_id,
            content,
            conversation_ids,
        ));
    }
    let agent_id = agent_id.to_string();
    let content = content.to_string();
    let conversation_ids = conversation_ids.to_vec();
    tokio::task::spawn_blocking(move || {
        subscription_push_send_all_blocking(&agent_id, &content, &conversation_ids)
    })
    .await
    .map_err(|e| anyhow!("okx-a2a subscription-push task join failed: {e}"))?
}

/// Debug-only mock payload — no process spawn. `ONCHAINOS_TEST_MOCK_PUSH_FAIL=<comma list>`
/// marks which conversationIds report `status: "failed"`; every other target reports `"sent"`.
///
/// The successful `messageId` embeds the exact `agent_id` this function received and the exact
/// byte length of `content`, so a black-box CLI test can observe — without a real `okx-a2a`
/// process — whether the caller already normalized `agent_id` (trimmed) while leaving `content`
/// untouched (its length would shrink if it had been trimmed).
#[cfg(debug_assertions)]
fn mock_subscription_push_payload(
    agent_id: &str,
    content: &str,
    conversation_ids: &[String],
) -> Value {
    let fail_list = std::env::var("ONCHAINOS_TEST_MOCK_PUSH_FAIL").unwrap_or_default();
    let fail_set: Vec<&str> = fail_list
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let results: Vec<Value> = conversation_ids
        .iter()
        .map(|id| {
            if fail_set.contains(&id.as_str()) {
                json!({"conversationId": id, "status": "failed", "error": "mock send failure"})
            } else {
                json!({
                    "conversationId": id,
                    "status": "sent",
                    "messageId": format!("mock-msg:{id}:agent={agent_id}:contentLen={}", content.len()),
                })
            }
        })
        .collect();
    let sent = results.iter().filter(|r| r["status"] == "sent").count() as u64;
    let failed = results.len() as u64 - sent;
    json!({
        "conversationCount": conversation_ids.len() as u64,
        "sentCount": sent,
        "failedCount": failed,
        "results": results,
    })
}

/// Blocking transport: shell `okx-a2a subscription-push` ONCE with every deduped conversationId
/// as a repeated `--conversation-id` flag, then parse the process outcome fail-closed.
fn subscription_push_send_all_blocking(
    agent_id: &str,
    content: &str,
    conversation_ids: &[String],
) -> Result<Value> {
    let args = build_okx_a2a_subscription_push_args(agent_id, content, conversation_ids);
    let out = std::process::Command::new("okx-a2a")
        .args(&args)
        .output()
        .map_err(|e| anyhow!("spawn failed: {e}"))?;
    parse_a2a_subscription_push_response(out.status.success(), &out.stdout, &out.stderr)
}

/// Print the summary via `to_agent_json`, degrading to a minimal literal if serialization fails.
fn print_summary(summary: &Value) {
    println!(
        "{}",
        crate::output::to_agent_json(summary).unwrap_or_else(|_| {
            r#"{"ok":true,"conversationCount":0,"sentCount":0,"failedCount":0,"skippedCount":0,"results":[]}"#
                .to_string()
        })
    );
}

/// subscription-push — broadcast an Active-subscription signal to all Shared Groups of a Service.
pub async fn handle_subscription_push(
    client: &mut TaskApiClient,
    agent_id: &str,
    service_id: &str,
    content: &str,
) -> Result<()> {
    // ── 1. Fail-closed validation (before any HTTP / transport) ──
    validate_inputs(agent_id, service_id, content)?;

    // Normalize identifiers ONCE, right after validation, by shadowing the raw parameters.
    // Every downstream use — the group query, the agenticId header, `--my-agent-id`, and the
    // audit tags below — consumes these trimmed bindings; the untrimmed originals go out of
    // scope here, so no call site can accidentally reach them. `content` is deliberately NOT
    // trimmed: only its emptiness is validated above; the exact caller-provided body (including
    // any legitimate surrounding whitespace/newlines) is what reaches the A2A transport.
    let agent_id = agent_id.trim();
    let service_id = service_id.trim();

    let base_tags = vec![
        format!("serviceId={service_id}"),
        format!("agentId={agent_id}"),
    ];

    // ── 2. Fetch groups (single call; Err propagates fail-closed). serviceId is used ONLY here
    //        and for the audit tags above — it never reaches the A2A transport below. ──
    let data = fetch_groups_or_mock(client, service_id, agent_id).await?;

    // ── 3. Parse / keep / isolate / dedup ──
    let (targets, skipped) = select_targets(&data)?;

    // ── 4. Zero unique targets → success with no A2A call ──
    if targets.is_empty() {
        audit::log(
            "cli",
            "ASP/subscription_push_no_targets",
            true,
            Duration::default(),
            Some([base_tags, vec![format!("skipped={skipped}")]].concat()),
            None,
        );
        print_summary(&build_summary(0, 0, 0, skipped, Vec::new()));
        return Ok(());
    }

    // ── 5. Exactly one A2A subscription-push call for the whole batch. A process-level failure
    //        (nonzero exit / bad JSON / ok!=true / malformed payload) propagates fail-closed —
    //        no fallback, no retry. ──
    let payload = subscription_push_send_all(agent_id, content, &targets).await?;

    // ── 6. Aggregate + output. conversationCount/sentCount/failedCount/results come from the
    //        A2A payload; skippedCount is OnchainOS's own group-filter/dedup tally. ──
    let conv_count = payload["conversationCount"].as_u64().unwrap_or(0);
    let sent = payload["sentCount"].as_u64().unwrap_or(0);
    let failed = payload["failedCount"].as_u64().unwrap_or(0);
    let results: Vec<Value> = payload
        .get("results")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(map_a2a_result).collect())
        .unwrap_or_default();

    let summary = build_summary(conv_count, sent, failed, skipped, results);
    audit::log(
        "cli",
        "ASP/subscription_push",
        true,
        Duration::default(),
        Some(
            [
                base_tags,
                vec![
                    format!("conversationCount={conv_count}"),
                    format!("sent={sent}"),
                    format!("failed={failed}"),
                    format!("skipped={skipped}"),
                ],
            ]
            .concat(),
        ),
        None,
    );
    print_summary(&summary);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_rejects_empty_or_whitespace_inputs_before_any_client_use() {
        assert!(validate_inputs("  ", "svc", "hello").is_err());
        assert!(validate_inputs("agent", "", "hello").is_err());
        assert!(validate_inputs("agent", "svc", "\t \n").is_err());
        assert!(validate_inputs("agent", "svc", "hello").is_ok());
    }

    #[test]
    fn select_targets_keeps_distinct_active_and_full_and_isolates_the_rest() {
        let data = json!({
            "conversations": [
                {"conversationId": "c1", "status": "active", "memberCount": 3},
                {"conversationId": "c2", "status": "Full", "createdAt": 1_700_000_000i64},
                {"conversationId": "c1", "status": "active"},          // duplicate → skipped
                {"conversationId": "", "status": "active"},            // missing id → skipped
                {"conversationId": "c3", "status": "archived"}         // illegal status → skipped
            ]
        });
        let (kept, skipped) = select_targets(&data).expect("valid array parses");
        assert_eq!(kept, vec!["c1".to_string(), "c2".to_string()]);
        assert!(skipped >= 3, "expected >=3 skipped, got {skipped}");
    }

    #[test]
    fn select_targets_empty_array_is_zero_target_success() {
        let (kept, skipped) =
            select_targets(&json!({"conversations": []})).expect("empty is valid");
        assert!(kept.is_empty());
        assert_eq!(skipped, 0);
    }

    #[test]
    fn select_targets_missing_or_non_array_conversations_is_fail_closed() {
        assert!(select_targets(&json!({})).is_err());
        assert!(select_targets(&json!({"conversations": "nope"})).is_err());
        assert!(select_targets(&json!({"conversations": {"conversationId": "c1"}})).is_err());
    }

    #[test]
    fn build_summary_merges_a2a_counts_with_onchainos_skipped_count() {
        let results = vec![
            json!({"conversationId": "c1", "status": "sent", "ok": true, "messageId": "m1"}),
            json!({"conversationId": "c2", "status": "failed", "ok": false, "error": "boom"}),
            json!({"conversationId": "c3", "status": "sent", "ok": true, "messageId": "m3"}),
        ];
        // conv_count/sent/failed simulate values taken straight from the A2A payload; skipped
        // (2) is OnchainOS's own group-filter tally and is never known to A2A.
        let summary = build_summary(3, 2, 1, 2, results);
        assert_eq!(summary["ok"], true);
        assert_eq!(summary["conversationCount"], 3);
        assert_eq!(summary["sentCount"], 2);
        assert_eq!(summary["failedCount"], 1);
        assert_eq!(summary["skippedCount"], 2);
        assert_eq!(summary["results"][0]["messageId"], "m1");
        assert_eq!(summary["results"][1]["ok"], false);
        assert_eq!(summary["results"][1]["error"], "boom");
    }

    #[test]
    fn list_groups_path_percent_encodes_service_id_as_sole_query_param() {
        let path = list_groups_path("  svc/1 &2  ");
        assert_eq!(
            path,
            "/priapi/v1/aieco/im/xmtp/subscription/list-groups-by-service?serviceId=svc%2F1%20%262"
        );
        // Exactly one query parameter, and it is serviceId.
        let (_, query) = path.split_once('?').expect("path has a query string");
        assert!(
            !query.contains('&'),
            "encoded query must not introduce extra params"
        );
        assert!(query.starts_with("serviceId="));
    }

    #[test]
    fn query_encoder_preserves_unreserved_and_escapes_reserved() {
        assert_eq!(encode_query_component("aZ0-._~"), "aZ0-._~");
        assert_eq!(encode_query_component("a/b?c=d"), "a%2Fb%3Fc%3Dd");
    }

    // ── A2A argv shape (build_okx_a2a_subscription_push_args) ─────────────────────────────

    #[test]
    fn a2a_args_use_subscription_push_shape_not_group_send() {
        let args = build_okx_a2a_subscription_push_args(
            "asp-agent-1",
            "hello subscribers",
            &["c1".to_string()],
        );
        assert_eq!(
            args,
            vec![
                "subscription-push".to_string(),
                "--my-agent-id".to_string(),
                "asp-agent-1".to_string(),
                "--message".to_string(),
                "hello subscribers".to_string(),
                "--conversation-id".to_string(),
                "c1".to_string(),
                "--json".to_string(),
            ]
        );
        for forbidden in ["group", "send", "--agent-id", "--content", "--service-id"] {
            assert!(
                !args.iter().any(|a| a == forbidden),
                "argv must not contain {forbidden:?}, got {args:?}"
            );
        }
    }

    #[test]
    fn a2a_args_repeat_conversation_id_once_per_deduped_target_in_a_single_call() {
        let ids = vec!["c1".to_string(), "c2".to_string(), "c3".to_string()];
        let args = build_okx_a2a_subscription_push_args("agent", "content", &ids);
        // One `--conversation-id <id>` pair per target, all within the SAME argv (one process).
        for id in &ids {
            let count = args
                .windows(2)
                .filter(|w| w[0] == "--conversation-id" && &w[1] == id)
                .count();
            assert_eq!(
                count, 1,
                "expected exactly one --conversation-id {id} pair, got {args:?}"
            );
        }
        assert_eq!(
            args.iter().filter(|a| *a == "--conversation-id").count(),
            ids.len(),
            "expected one --conversation-id flag per target in a single argv, got {args:?}"
        );
    }

    // ── A2A response parsing (parse_a2a_subscription_push_response) ───────────────────────

    #[test]
    fn a2a_response_full_success_returns_payload_with_zero_failed() {
        let stdout = json!({
            "ok": true,
            "commandId": "cmd-1",
            "payload": {
                "conversationCount": 2,
                "sentCount": 2,
                "failedCount": 0,
                "results": [
                    {"conversationId": "c1", "status": "sent", "messageId": "m1"},
                    {"conversationId": "c2", "status": "sent", "messageId": "m2"},
                ],
            },
            "error": null,
        })
        .to_string();
        let payload = parse_a2a_subscription_push_response(true, stdout.as_bytes(), b"")
            .expect("full success must parse");
        assert_eq!(payload["failedCount"], 0);
        assert_eq!(payload["sentCount"], 2);
    }

    #[test]
    fn a2a_response_partial_failure_is_ok_not_err() {
        let stdout = json!({
            "ok": true,
            "commandId": "cmd-2",
            "payload": {
                "conversationCount": 2,
                "sentCount": 1,
                "failedCount": 1,
                "results": [
                    {"conversationId": "c1", "status": "sent", "messageId": "m1"},
                    {"conversationId": "c2", "status": "failed", "error": "send failed"},
                ],
            },
            "error": null,
        })
        .to_string();
        let payload = parse_a2a_subscription_push_response(true, stdout.as_bytes(), b"")
            .expect("a partial failure is a completed publish, not an Err");
        assert_eq!(payload["failedCount"], 1);
    }

    #[test]
    fn a2a_response_top_level_ok_false_is_fail_closed_err() {
        let stdout = json!({
            "ok": false,
            "commandId": "cmd-3",
            "payload": null,
            "error": "agent not authorized",
        })
        .to_string();
        let err = parse_a2a_subscription_push_response(true, stdout.as_bytes(), b"")
            .expect_err("top-level ok=false must be fail-closed Err");
        assert!(err.to_string().contains("agent not authorized"));
    }

    #[test]
    fn a2a_response_nonzero_exit_is_fail_closed_err() {
        let err = parse_a2a_subscription_push_response(false, b"", b"boom")
            .expect_err("nonzero exit must be fail-closed Err");
        assert!(err.to_string().contains("non-zero"));
    }

    #[test]
    fn a2a_response_invalid_json_is_fail_closed_err() {
        let err = parse_a2a_subscription_push_response(true, b"not json", b"")
            .expect_err("non-JSON stdout must be fail-closed Err");
        assert!(err.to_string().contains("not valid JSON"));
    }

    #[test]
    fn a2a_response_missing_payload_is_fail_closed_err() {
        let stdout = json!({"ok": true, "commandId": "cmd-4", "error": null}).to_string();
        assert!(parse_a2a_subscription_push_response(true, stdout.as_bytes(), b"").is_err());
    }

    #[test]
    fn a2a_response_malformed_results_entry_is_fail_closed_err() {
        let stdout = json!({
            "ok": true,
            "payload": {
                "conversationCount": 1,
                "sentCount": 0,
                "failedCount": 0,
                "results": [{"status": "sent"}], // missing conversationId
            },
        })
        .to_string();
        assert!(parse_a2a_subscription_push_response(true, stdout.as_bytes(), b"").is_err());
    }

    // ── result mapping (map_a2a_result) ────────────────────────────────────────────────────

    #[test]
    fn map_a2a_result_maps_sent_and_failed_status_to_ok_boolean() {
        let sent =
            map_a2a_result(&json!({"conversationId": "c1", "status": "sent", "messageId": "m1"}));
        assert_eq!(sent["ok"], true);
        assert_eq!(sent["status"], "sent");
        assert_eq!(sent["messageId"], "m1");

        let failed =
            map_a2a_result(&json!({"conversationId": "c2", "status": "failed", "error": "boom"}));
        assert_eq!(failed["ok"], false);
        assert_eq!(failed["error"], "boom");

        let not_group = map_a2a_result(&json!({"conversationId": "c3", "status": "not_group"}));
        assert_eq!(not_group["ok"], false);
        assert_eq!(not_group["status"], "not_group");
    }

    // ── identifier normalization observability (mock_subscription_push_payload) ───────────

    #[test]
    fn mock_subscription_push_payload_embeds_received_agent_id_and_content_length() {
        // This mock stands in for the real `okx-a2a subscription-push` process, so embedding
        // exactly what it received is how the black-box CLI tests (push_06/push_07) observe
        // whether `handle_subscription_push` already normalized `agent_id` (trimmed) while
        // leaving `content` untouched — without needing a real subprocess.
        let payload = mock_subscription_push_payload("agent-1", "  hi  ", &["c1".to_string()]);
        let message_id = payload["results"][0]["messageId"]
            .as_str()
            .expect("mock success entry must carry a messageId");
        assert!(message_id.contains("agent=agent-1:"));
        assert!(message_id.contains(&format!("contentLen={}", "  hi  ".len())));
    }
}
