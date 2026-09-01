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
            "version": 2,
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
    let source = "Extract the declared instrument and amount from each incoming signal.";
    let source_hash = sha256_hex(source.as_bytes());
    let semantics = json!({
        "consentFields": [
            {"key": "strategyArmed", "required": true, "type": "boolean"}
        ],
        "signalFields": [
            {"key": "instId", "required": true, "type": "string"},
            {"key": "amount", "required": true, "type": "decimal"}
        ],
        "execution": {
            "toolId": "onchainos",
            "operation": "swap",
            "authorizationParameter": "amount",
            "conditions": [
                {"source": "consent.strategyArmed", "equals": true}
            ],
            "bindings": [
                {"parameter": "instrument", "source": "signal.instId"},
                {"parameter": "amount", "source": "signal.amount"}
            ]
        }
    });
    let guide_hash = sha256_hex(
        &serde_jcs::to_vec(&json!({
            "sourceHash": source_hash,
            "semantics": semantics,
        }))
        .unwrap(),
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    write_markdown(
        &home.join("autotrade/guide/job1.md"),
        "guide",
        &json!({
            "version": 1,
            "jobId": "job1",
            "serviceId": "svc-guide",
            "sourceHash": source_hash,
            "semantics": semantics,
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

fn resolve_guide_signal(home: &std::path::Path, delivery_id: &str) -> Value {
    let output = run(
        home,
        &[
            "agent",
            "autotrade-guide-intent-resolve",
            "--job-id",
            "job1",
            "--delivery-id",
            delivery_id,
            "--signal-values-json",
            r#"{"instId":"BTC-USDT","amount":"2.5"}"#,
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    parse_stdout_json(&output)["data"].clone()
}

#[test]
fn guide_direct_claim_is_bound_to_resolved_plain_text_signal_and_finalizes_once() {
    let (_guard, home) = fresh_home("cli_autotrade_direct_execution");
    let delivery_id = "delivery-direct";
    let saved_path = write_guide_direct_fixture(&home, delivery_id);
    assert!(!fs::read_to_string(&saved_path)
        .unwrap()
        .trim_start()
        .starts_with('{'));

    let intent = resolve_guide_signal(&home, delivery_id);
    assert_eq!(intent["toolId"], "onchainos");
    assert_eq!(intent["operation"], "swap");
    assert_eq!(intent["authorizationAmount"], "2.5");
    let intent_hash = intent["intentHash"].as_str().unwrap().to_string();

    let claimed = run(
        &home,
        &[
            "agent",
            "autotrade-direct-claim",
            "--job-id",
            "job1",
            "--delivery-id",
            delivery_id,
            "--guide-intent-hash",
            &intent_hash,
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
            "--guide-intent-hash",
            &intent_hash,
            "--amount",
            "2.5",
        ],
    );
    assert!(duplicate.status.success());
    assert_eq!(parse_stdout_json(&duplicate)["data"]["status"], "terminal");
}

#[test]
fn guide_direct_claim_revalidates_signal_and_active_consent() {
    let (_guard, home) = fresh_home("cli_autotrade_direct_revalidation");
    let delivery_id = "delivery-revalidate";
    let saved_path = write_guide_direct_fixture(&home, delivery_id);
    let intent_hash = resolve_guide_signal(&home, delivery_id)["intentHash"]
        .as_str()
        .unwrap()
        .to_string();

    fs::write(
        &saved_path,
        "The provider changed this signal after resolution.",
    )
    .unwrap();
    let changed_signal_claim = run(
        &home,
        &[
            "agent",
            "autotrade-direct-claim",
            "--job-id",
            "job1",
            "--delivery-id",
            delivery_id,
            "--guide-intent-hash",
            &intent_hash,
            "--amount",
            "2.5",
        ],
    );
    assert!(!changed_signal_claim.status.success());
    let changed_signal_output = format!(
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&changed_signal_claim.stdout),
        String::from_utf8_lossy(&changed_signal_claim.stderr)
    );
    assert!(
        changed_signal_output.contains("saved Signal changed after Guide resolution"),
        "{changed_signal_output}"
    );

    let paused_delivery_id = "delivery-paused";
    write_guide_direct_fixture(&home, paused_delivery_id);
    let paused_intent_hash = resolve_guide_signal(&home, paused_delivery_id)["intentHash"]
        .as_str()
        .unwrap()
        .to_string();
    let pause = run(
        &home,
        &[
            "agent",
            "autotrade-consent-set",
            "--job-id",
            "job1",
            "--mode",
            "pause",
        ],
    );
    assert!(pause.status.success());
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
            "--guide-intent-hash",
            &paused_intent_hash,
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
        paused_output.contains("active Guide Consent is not available locally"),
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
fn guide_draft_validation_accepts_only_a_local_projection_of_the_exact_guide() {
    let (_guard, home) = fresh_home("cli_autotrade_guide_draft_validation");
    let source = "Use the incoming instrument and amount only when strategy is armed.";
    let source_hash = sha256_hex(source.as_bytes());
    let semantics = json!({
        "consentFields": [
            {"key": "strategyArmed", "required": true, "type": "boolean"}
        ],
        "signalFields": [
            {"key": "instId", "required": true, "type": "string"},
            {"key": "amount", "required": true, "type": "decimal"}
        ],
        "execution": {
            "toolId": "onchainos",
            "operation": "swap",
            "authorizationParameter": "amount",
            "conditions": [
                {"source": "consent.strategyArmed", "equals": true}
            ],
            "bindings": [
                {"parameter": "instrument", "source": "signal.instId"},
                {"parameter": "amount", "source": "signal.amount"}
            ]
        }
    });
    let semantics_json = serde_json::to_string(&semantics).unwrap();
    let output = run(
        &home,
        &[
            "agent",
            "autotrade-guide-draft-validate",
            "--service-guide",
            source,
            "--service-guide-hash",
            &source_hash,
            "--autotrade-guide-semantics-json",
            &semantics_json,
        ],
    );
    assert!(output.status.success());
    let data = parse_stdout_json(&output)["data"].clone();
    assert_eq!(data["sourceHash"], source_hash);
    assert_eq!(data["semantics"], semantics);
    assert_eq!(data["validation"], "local_guide_projection_valid");
    assert_eq!(data["writesLocalFiles"], false);
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
