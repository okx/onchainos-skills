mod common;

use common::{fresh_home, onchainos, parse_stdout_json, scrubbed};
use serde_json::json;
use std::fs;

#[cfg(unix)]
fn write_fake_okx(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(
        path,
        r#"#!/bin/sh
case "$FAKE_OKX_MODE" in
  nonzero)
    echo '{"ok":false,"error":"post-run failure"}'
    exit 7
    ;;
  status_only)
    echo '{"ok":true,"status":"success"}'
    ;;
  receipt)
    echo '{"ok":true,"data":{"ordId":"42"}}'
    ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn write_json(path: &std::path::Path, value: &serde_json::Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn write_context(home: &std::path::Path, delivery_id: &str) -> serde_json::Value {
    let context = json!({
        "version": 1,
        "jobId": "job1",
        "agentId": "8315",
        "providerAgentId": "8779",
        "originSessionKey": "job:job1:my:8315:to:8779",
        "deliveryId": delivery_id,
        "savedPath": "/tmp/signal.txt",
        "deliverableType": "text",
        "receivedAtMs": 1
    });
    write_json(
        &home.join(format!(
            "autotrade/delivery-context/job1/{delivery_id}.json"
        )),
        &context,
    );
    context
}

#[test]
fn later_consent_delivery_is_queued_without_a_terminal_skip() {
    let (_guard, home) = fresh_home("cli_autotrade_delivery_fifo");
    write_context(&home, "delivery-first");
    write_context(&home, "delivery-second");
    let empty_path = home.join("empty-path");
    fs::create_dir_all(&empty_path).unwrap();

    let request = |delivery_id: &str| {
        let mut command = onchainos();
        scrubbed(&mut command, &home)
            .env("PATH", &empty_path)
            .args([
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
            ])
            .output()
            .unwrap()
    };

    let first = request("delivery-first");
    assert!(first.status.success());
    let second = request("delivery-second");
    assert!(second.status.success());
    let result = parse_stdout_json(&second);
    assert_eq!(result["data"]["status"], "queued");
    assert_eq!(result["data"]["activeDeliveryId"], "delivery-first");
    assert_eq!(result["data"]["queuePosition"], 2);
    assert_eq!(result["data"]["terminal"], false);
    assert!(!home
        .join("autotrade/outcomes/job1/delivery-second.json")
        .exists());
    assert!(!home
        .join("autotrade/execution-latch/job1/delivery-second")
        .exists());
}

#[test]
fn preflight_amount_mismatch_is_persisted_without_spawning_trade() {
    let (_guard, home) = fresh_home("cli_autotrade_execution");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    write_json(
        &home.join("autotrade/delivery-context/job1/delivery-1.json"),
        &json!({
            "version": 1,
            "jobId": "job1",
            "agentId": "8315",
            "providerAgentId": "8779",
            "originSessionKey": "job:job1:my:8315:to:8779",
            "deliveryId": "delivery-1",
            "savedPath": "/tmp/signal.txt",
            "deliverableType": "text",
            "receivedAtMs": 1
        }),
    );
    write_json(
        &home.join("autotrade/consent/job1.json"),
        &json!({
            "version": 1,
            "jobId": "job1",
            "mode": "auto",
            "capU": "10",
            "tradeAmountU": "1",
            "quoteToken": "usdt",
            "createdAt": now,
            "expiresAt": now + 3600
        }),
    );
    write_json(
        &home.join("autotrade/grants/job1.json"),
        &json!({
            "version": 1,
            "jobId": "job1",
            "grants": {"dex": {"maxBuy": "10", "maxSell": "10"}},
            "createdAt": now,
            "expiresAt": now + 3600
        }),
    );
    let empty_path = home.join("empty-path");
    fs::create_dir_all(&empty_path).unwrap();
    let mut command = onchainos();
    let output = scrubbed(&mut command, &home)
        .env("PATH", &empty_path)
        .args([
            "agent",
            "autotrade-execute",
            "--job-id",
            "job1",
            "--delivery-id",
            "delivery-1",
            "--venue",
            "dex",
            "--action",
            "buy",
            "--amount",
            "1",
            "--command-json",
            r#"["swap","execute","--readable-amount","2"]"#,
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result = parse_stdout_json(&output);
    assert_eq!(result["ok"], true);
    assert_eq!(result["data"]["status"], "failed_before_submit");
    assert_eq!(result["data"]["notificationPending"], true);
    assert_eq!(result["data"]["notificationAttempts"], 1);
    assert!(result["data"]["nextNotificationAttemptAt"]
        .as_u64()
        .unwrap_or_default()
        > 0);
    assert!(result["data"]["reason"]
        .as_str()
        .unwrap_or_default()
        .contains("does not match"));
    assert!(home
        .join("autotrade/outcomes/job1/delivery-1.json")
        .exists());
}

#[test]
fn manual_policy_uses_the_same_persisted_result_bridge() {
    let (_guard, home) = fresh_home("cli_autotrade_manual_execution");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    write_context(&home, "delivery-manual");
    write_json(
        &home.join("autotrade/consent/job1.json"),
        &json!({
            "version": 1,
            "jobId": "job1",
            "mode": "manual",
            "tradeAmountU": "1",
            "quoteToken": "usdt",
            "createdAt": now,
            "expiresAt": now + 3600
        }),
    );
    let empty_path = home.join("empty-path");
    fs::create_dir_all(&empty_path).unwrap();
    let mut command = onchainos();
    let output = scrubbed(&mut command, &home)
        .env("PATH", &empty_path)
        .args([
            "agent",
            "autotrade-execute",
            "--job-id",
            "job1",
            "--delivery-id",
            "delivery-manual",
            "--venue",
            "dex",
            "--action",
            "buy",
            "--amount",
            "1",
            "--execution-mode",
            "manual",
            "--command-json",
            r#"["swap","execute","--readable-amount","2"]"#,
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let result = parse_stdout_json(&output);
    assert_eq!(result["data"]["status"], "failed_before_submit");
    assert_eq!(result["data"]["executionMode"], "manual");
    assert!(result["data"]["reason"]
        .as_str()
        .unwrap_or_default()
        .contains("dex readable amount"));
}

#[test]
fn terminal_delivery_report_is_idempotent_and_blocks_later_execution() {
    let (_guard, home) = fresh_home("cli_autotrade_delivery_report");
    let context = write_context(&home, "delivery-report");
    write_json(&home.join("autotrade/pending/job1.json"), &context);
    let empty_path = home.join("empty-path");
    fs::create_dir_all(&empty_path).unwrap();

    let mut report = onchainos();
    let report_output = scrubbed(&mut report, &home)
        .env("PATH", &empty_path)
        .args([
            "agent",
            "autotrade-delivery-report",
            "--job-id",
            "job1",
            "--delivery-id",
            "delivery-report",
            "--status",
            "skipped",
            "--reason",
            "signal expired before execution",
        ])
        .output()
        .unwrap();
    assert!(report_output.status.success());
    let first = parse_stdout_json(&report_output);
    assert_eq!(first["data"]["status"], "skipped");
    assert!(!home.join("autotrade/pending/job1.json").exists());

    let mut execute = onchainos();
    let execute_output = scrubbed(&mut execute, &home)
        .env("PATH", &empty_path)
        .args([
            "agent",
            "autotrade-execute",
            "--job-id",
            "job1",
            "--delivery-id",
            "delivery-report",
            "--venue",
            "dex",
            "--action",
            "buy",
            "--amount",
            "1",
            "--command-json",
            r#"["swap","execute","--readable-amount","1"]"#,
        ])
        .output()
        .unwrap();
    assert!(execute_output.status.success());
    let duplicate = parse_stdout_json(&execute_output);
    assert_eq!(duplicate["data"]["status"], "skipped");
    assert_eq!(duplicate["data"]["reason"], "signal expired before execution");
}

#[test]
fn over_cap_one_time_permit_is_exact_and_consumed_by_the_result_bridge() {
    let (_guard, home) = fresh_home("cli_autotrade_one_time_execution");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let context = write_context(&home, "delivery-over-cap");
    write_json(&home.join("autotrade/pending/job1.json"), &context);
    write_json(
        &home.join("autotrade/consent/job1.json"),
        &json!({
            "version": 1,
            "jobId": "job1",
            "mode": "auto",
            "capU": "1",
            "tradeAmountU": "1",
            "quoteToken": "usdt",
            "createdAt": now,
            "expiresAt": now + 3600
        }),
    );
    let empty_path = home.join("empty-path");
    fs::create_dir_all(&empty_path).unwrap();

    let mut under_cap = onchainos();
    let under_cap_output = scrubbed(&mut under_cap, &home)
        .env("PATH", &empty_path)
        .args([
            "agent",
            "autotrade-once-authorize",
            "--job-id",
            "job1",
            "--delivery-id",
            "delivery-over-cap",
            "--amount",
            "1",
        ])
        .output()
        .unwrap();
    assert!(!under_cap_output.status.success());
    assert!(String::from_utf8_lossy(&under_cap_output.stdout)
        .contains("only valid for an amount above the current cap"));

    let mut authorize = onchainos();
    let authorize_output = scrubbed(&mut authorize, &home)
        .env("PATH", &empty_path)
        .args([
            "agent",
            "autotrade-once-authorize",
            "--job-id",
            "job1",
            "--delivery-id",
            "delivery-over-cap",
            "--amount",
            "2",
        ])
        .output()
        .unwrap();
    assert!(
        authorize_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&authorize_output.stderr)
    );
    let permit = parse_stdout_json(&authorize_output);
    assert_eq!(permit["data"]["amount"], "2");
    assert!(home
        .join("autotrade/one-time-permits/job1/delivery-over-cap.json")
        .exists());

    let mut execute = onchainos();
    let execute_output = scrubbed(&mut execute, &home)
        .env("PATH", &empty_path)
        .args([
            "agent",
            "autotrade-execute",
            "--job-id",
            "job1",
            "--delivery-id",
            "delivery-over-cap",
            "--venue",
            "dex",
            "--action",
            "buy",
            "--amount",
            "2",
            "--execution-mode",
            "one_time",
            "--command-json",
            r#"["swap","execute","--readable-amount","3"]"#,
        ])
        .output()
        .unwrap();
    assert!(execute_output.status.success());
    let result = parse_stdout_json(&execute_output);
    assert_eq!(result["data"]["status"], "failed_before_submit");
    assert_eq!(result["data"]["executionMode"], "one_time");
    assert!(result["data"]["reason"]
        .as_str()
        .unwrap_or_default()
        .contains("dex readable amount"));
    assert!(!home
        .join("autotrade/one-time-permits/job1/delivery-over-cap.json")
        .exists());
    assert!(!home.join("autotrade/pending/job1.json").exists());
}

#[cfg(unix)]
#[test]
fn started_command_failures_and_status_only_output_never_claim_submission() {
    let (_guard, home) = fresh_home("cli_autotrade_conservative_receipt");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    for delivery in ["delivery-nonzero", "delivery-status", "delivery-receipt"] {
        write_context(&home, delivery);
    }
    write_json(
        &home.join("autotrade/consent/job1.json"),
        &json!({
            "version": 1,
            "jobId": "job1",
            "mode": "auto",
            "capU": "10",
            "tradeAmountU": "1",
            "quoteToken": "usdt",
            "createdAt": now,
            "expiresAt": now + 3600
        }),
    );
    write_json(
        &home.join("autotrade/grants/job1.json"),
        &json!({
            "version": 1,
            "jobId": "job1",
            "grants": {"trade_kit": {"maxBuy": "10", "maxSell": "10"}},
            "createdAt": now,
            "expiresAt": now + 3600
        }),
    );
    let bin = home.join("bin");
    fs::create_dir_all(&bin).unwrap();
    write_fake_okx(&bin.join("okx"));

    let run = |delivery: &str, mode: &str| {
        let mut command = onchainos();
        let output = scrubbed(&mut command, &home)
            .env("PATH", &bin)
            .env("FAKE_OKX_MODE", mode)
            .args([
                "agent",
                "autotrade-execute",
                "--job-id",
                "job1",
                "--delivery-id",
                delivery,
                "--venue",
                "trade_kit",
                "--action",
                "buy",
                "--amount",
                "1",
                "--command-json",
                r#"["order","--sz","1","--side","buy"]"#,
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
        parse_stdout_json(&output)
    };

    assert_eq!(
        run("delivery-nonzero", "nonzero")["data"]["status"],
        "unknown_after_submit"
    );
    assert_eq!(
        run("delivery-status", "status_only")["data"]["status"],
        "unknown_after_submit"
    );
    assert_eq!(
        run("delivery-receipt", "receipt")["data"]["status"],
        "submitted"
    );
}
