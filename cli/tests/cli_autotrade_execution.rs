mod common;

use common::{fresh_home, onchainos, parse_stdout_json, scrubbed};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn write_json(path: &std::path::Path, value: &Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn write_markdown(path: &std::path::Path, kind: &str, metadata: &Value, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let content = format!(
        "<!-- onchainos-autotrade:{kind}\n{}\n-->\n\n{body}",
        serde_json::to_string_pretty(metadata).unwrap()
    );
    fs::write(path, content).unwrap();
}

fn write_direct_context(home: &std::path::Path, delivery_id: &str, saved_path: &std::path::Path) {
    write_json(
        &home.join(format!(
            "autotrade/delivery-context/job1/{delivery_id}.json"
        )),
        &json!({
            "version": 1,
            "jobId": "job1",
            "agentId": "8315",
            "providerAgentId": "8779",
            "originSessionKey": "job:job1:my:8315:to:8779",
            "deliveryId": delivery_id,
            "savedPath": saved_path,
            "deliverableType": "text",
            "receivedAtMs": 1,
            "executionPath": "agent_direct"
        }),
    );
}

fn write_legacy_context(home: &std::path::Path, delivery_id: &str) {
    write_json(
        &home.join(format!(
            "autotrade/delivery-context/job1/{delivery_id}.json"
        )),
        &json!({
            "version": 1,
            "jobId": "job1",
            "agentId": "8315",
            "providerAgentId": "8779",
            "originSessionKey": "job:job1:my:8315:to:8779",
            "deliveryId": delivery_id,
            "savedPath": "/tmp/legacy-signal.txt",
            "deliverableType": "text",
            "receivedAtMs": 1
        }),
    );
}

fn write_guide_direct_fixture(home: &std::path::Path, delivery_id: &str) -> std::path::PathBuf {
    let source = "Read this saved Signal with the matching Consent before executing one documented trade.";
    let source_hash = sha256_hex(source.as_bytes());
    let guide_hash = source_hash.clone();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    write_markdown(
        &home.join("autotrade/guide/job1.md"),
        "guide",
        &json!({
            "version": 2,
            "jobId": "job1",
            "serviceId": "svc-guide",
            "sourceHash": source_hash,
            "createdAt": now
        }),
        source,
    );
    write_markdown(
        &home.join("autotrade/consent/job1.md"),
        "consent",
        &json!({
            "version": 1,
            "jobId": "job1",
            "guideHash": guide_hash,
            "lifecycle": "active",
            "values": {"strategyArmed": true},
            "createdAt": now,
            "expiresAt": now + 3600
        }),
        "# Service Consent",
    );
    let saved_path = home.join(format!("{delivery_id}-signal.txt"));
    fs::write(
        &saved_path,
        "BUY BTC-USDT with the Guide-declared amount of 2.5 units.",
    )
    .unwrap();
    write_direct_context(home, delivery_id, &saved_path);
    saved_path
}

fn run(home: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut command = onchainos();
    scrubbed(&mut command, home).args(args).output().unwrap()
}

#[test]
fn guide_direct_claim_requires_active_guide_consent_and_finalizes_once() {
    let (_guard, home) = fresh_home("cli_autotrade_direct_execution");
    let delivery_id = "delivery-direct";
    let saved_path = write_guide_direct_fixture(&home, delivery_id);
    assert!(!fs::read_to_string(&saved_path)
        .unwrap()
        .trim_start()
        .starts_with('{'));

    let claimed = run(
        &home,
        &[
            "agent",
            "autotrade-direct-claim",
            "--job-id",
            "job1",
            "--delivery-id",
            delivery_id,
            "--amount",
            "2.5",
        ],
    );
    assert!(claimed.status.success());
    assert_eq!(parse_stdout_json(&claimed)["data"]["status"], "claimed");

    let finalized = run(
        &home,
        &[
            "agent",
            "autotrade-direct-finalize",
            "--job-id",
            "job1",
            "--delivery-id",
            delivery_id,
            "--status",
            "submitted",
            "--tool-id",
            "onchainos",
            "--receipt-id",
            "tx_42",
        ],
    );
    assert!(finalized.status.success());
    assert_eq!(parse_stdout_json(&finalized)["data"]["status"], "submitted");

    let duplicate = run(
        &home,
        &[
            "agent",
            "autotrade-direct-claim",
            "--job-id",
            "job1",
            "--delivery-id",
            delivery_id,
            "--amount",
            "2.5",
        ],
    );
    assert!(duplicate.status.success());
    assert_eq!(parse_stdout_json(&duplicate)["data"]["status"], "terminal");
}

#[test]
fn guide_direct_claim_requires_saved_signal_and_active_consent() {
    let (_guard, home) = fresh_home("cli_autotrade_direct_revalidation");
    let delivery_id = "delivery-revalidate";
    let saved_path = write_guide_direct_fixture(&home, delivery_id);
    fs::remove_file(&saved_path).unwrap();
    let missing_signal_claim = run(
        &home,
        &[
            "agent",
            "autotrade-direct-claim",
            "--job-id",
            "job1",
            "--delivery-id",
            delivery_id,
            "--amount",
            "2.5",
        ],
    );
    assert!(!missing_signal_claim.status.success());
    let missing_signal_output = format!(
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&missing_signal_claim.stdout),
        String::from_utf8_lossy(&missing_signal_claim.stderr)
    );
    assert!(
        missing_signal_output.contains("saved subscription Signal is not available"),
        "{missing_signal_output}"
    );

    let paused_delivery_id = "delivery-paused";
    write_guide_direct_fixture(&home, paused_delivery_id);
    fs::remove_file(home.join("autotrade/consent/job1.md")).unwrap();
    assert!(!home.join("autotrade/consent/job1.md").exists());

    let paused_claim = run(
        &home,
        &[
            "agent",
            "autotrade-direct-claim",
            "--job-id",
            "job1",
            "--delivery-id",
            paused_delivery_id,
            "--amount",
            "2.5",
        ],
    );
    assert!(!paused_claim.status.success());
    let paused_output = format!(
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&paused_claim.stdout),
        String::from_utf8_lossy(&paused_claim.stderr)
    );
    assert!(
        paused_output.contains("active local Service Guide and Guide Consent are required"),
        "{paused_output}"
    );
}

#[test]
fn legacy_consent_request_is_terminal_skip_even_when_an_old_auto_record_exists() {
    let (_guard, home) = fresh_home("cli_autotrade_legacy_skip");
    let delivery_id = "delivery-legacy";
    write_legacy_context(&home, delivery_id);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    write_json(
        &home.join("autotrade/consent/job1.json"),
        &json!({
            "version": 1,
            "jobId": "job1",
            "mode": "auto",
            "capU": "10",
            "tradeAmountU": "1",
            "createdAt": now,
            "expiresAt": now + 3600
        }),
    );

    let output = run(
        &home,
        &[
            "agent",
            "autotrade-consent-request",
            "--job-id",
            "job1",
            "--agent-id",
            "8315",
            "--delivery-id",
            delivery_id,
            "--signal-type",
            "spot",
        ],
    );
    assert!(output.status.success());
    let data = parse_stdout_json(&output)["data"].clone();
    assert_eq!(data["status"], "skipped");
    assert_eq!(data["reason"], "guide_execution_unavailable");
    assert_eq!(data["terminal"], true);
    assert_ne!(data["status"], "policy_ready");
}

#[test]
fn guide_draft_validation_command_is_removed() {
    let (_guard, home) = fresh_home("cli_autotrade_guide_draft_validation");
    let output = run(
        &home,
        &[
            "agent",
            "autotrade-guide-draft-validate",
        ],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("autotrade-guide-draft-validate"), "{stderr}");
    assert!(!home.join("autotrade/guide").exists());
}

#[test]
fn stale_direct_delivery_never_reports_no_active_execution_consent() {
    let (_guard, home) = fresh_home("cli_autotrade_stale_direct_delivery");
    let delivery_id = "delivery-stale-direct";
    let saved_path = home.join("signal.md");
    fs::write(&saved_path, "raw Signal stays available to the user").unwrap();
    write_direct_context(&home, delivery_id, &saved_path);

    let output = run(
        &home,
        &[
            "agent",
            "autotrade-delivery-report",
            "--job-id",
            "job1",
            "--delivery-id",
            delivery_id,
            "--status",
            "failed_before_execution",
            "--reason",
            "No active execution consent",
        ],
    );
    assert!(output.status.success());
    let data = parse_stdout_json(&output)["data"].clone();
    assert_eq!(data["status"], "skipped");
    assert_eq!(data["reason"], "guide_execution_unavailable");
}
