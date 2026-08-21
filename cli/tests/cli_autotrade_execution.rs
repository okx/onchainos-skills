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
if [ "$1 $2" = "list-tools --json" ]; then
  printf '%s\n' '{"version":"1.4.2","modules":[{"commands":[{"toolName":"market_get_ticker"},{"toolName":"market_get_instruments"},{"toolName":"account_get_config"},{"toolName":"spot_place_order"},{"toolName":"swap_get_leverage"},{"toolName":"swap_set_leverage"},{"toolName":"swap_place_order"},{"toolName":"swap_close_position"},{"toolName":"futures_get_leverage"},{"toolName":"futures_set_leverage"},{"toolName":"futures_place_order"},{"toolName":"futures_close_position"},{"toolName":"event_browse"},{"toolName":"event_get_series"},{"toolName":"event_get_events"},{"toolName":"event_get_markets"},{"toolName":"event_place_order"},{"toolName":"option_get_instruments"},{"toolName":"option_get_greeks"},{"toolName":"option_place_order"}]}]}'
  exit 0
fi
if [ "$1 $2 $3" = "account config --json" ]; then
  if [ "$FAKE_OKX_MODE" = "not_ready" ]; then
    printf '%s\n' '[{"perm":"read_only"}]'
  else
    printf '%s\n' '[{"perm":"trade"}]'
  fi
  exit 0
fi
case "$FAKE_OKX_MODE" in
  nonzero_structured)
    echo '{"ok":false,"error":"post-run failure"}'
    exit 7
    ;;
  nonzero_opaque)
    echo 'opaque transport failure' >&2
    exit 9
    ;;
  status_only)
    echo '{"ok":true,"status":"success"}'
    ;;
  receipt)
    echo '{"ok":true,"data":{"ordId":"42"}}'
    ;;
  normalized_sentinels)
    saw_tp=false
    saw_sl=false
    for arg in "$@"; do
      [ "$arg" = "--tpOrdPx=-1" ] && saw_tp=true
      [ "$arg" = "--slOrdPx=-1" ] && saw_sl=true
    done
    if [ "$saw_tp" = true ] && [ "$saw_sl" = true ]; then
      echo '{"ok":true,"data":{"ordId":"normalized-42"}}'
    else
      echo "Error: negative TP/SL market sentinels were not normalized" >&2
      exit 2
    fi
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
            "tradeEnvironment": "live",
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

#[cfg(unix)]
#[test]
fn trade_kit_execution_requires_and_matches_persisted_environment() {
    let (_guard, home) = fresh_home("cli_autotrade_trade_environment_binding");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    for delivery in ["delivery-missing-env", "delivery-mismatch-env"] {
        write_context(&home, delivery);
    }
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

    let write_policy = |environment: Option<&str>| {
        let mut policy = json!({
            "version": 2,
            "jobId": "job1",
            "mode": "auto",
            "capU": "10",
            "tradeAmountU": "1",
            "quoteToken": "usdt",
            "createdAt": now,
            "expiresAt": now + 3600
        });
        if let Some(environment) = environment {
            policy["tradeEnvironment"] = json!(environment);
        }
        policy["orderPolicy"] = json!("market");
        write_json(&home.join("autotrade/consent/job1.json"), &policy);
    };
    let run = |delivery: &str| {
        let mut command = onchainos();
        let output = scrubbed(&mut command, &home)
            .env("PATH", &bin)
            .env("FAKE_OKX_MODE", "receipt")
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
                r#"["spot","place","--sz","1","--side","buy","--ordType","market","--live"]"#,
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        parse_stdout_json(&output)
    };

    write_policy(None);
    let missing = run("delivery-missing-env");
    assert_eq!(missing["data"]["status"], "failed_before_submit");
    assert!(missing["data"]["reason"]
        .as_str()
        .unwrap_or_default()
        .contains("persisted live or demo environment"));

    write_policy(Some("demo"));
    let mismatch = run("delivery-mismatch-env");
    assert_eq!(mismatch["data"]["status"], "failed_before_submit");
    assert!(mismatch["data"]["reason"]
        .as_str()
        .unwrap_or_default()
        .contains("does not match persisted consent"));
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
fn completed_trade_kit_commands_preserve_receipts_and_safe_failure_details() {
    let (_guard, home) = fresh_home("cli_autotrade_conservative_receipt");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    for delivery in [
        "delivery-nonzero",
        "delivery-opaque",
        "delivery-status",
        "delivery-receipt",
        "delivery-normalized",
        "delivery-futures-place",
        "delivery-option",
        "delivery-event",
        "delivery-close",
        "delivery-not-ready",
    ] {
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
            "tradeEnvironment": "live",
            "marginMode": "cross",
            "orderPolicy": "market",
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

    let run = |delivery: &str, mode: &str, command_json: &str| {
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
                command_json,
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
        parse_stdout_json(&output)
    };

    let structured = run(
        "delivery-nonzero",
        "nonzero_structured",
        r#"["spot","place","--sz","1","--side","buy","--ordType","market","--live"]"#,
    );
    assert_eq!(structured["data"]["status"], "failed_before_submit");
    assert!(structured["data"]["reason"]
        .as_str()
        .unwrap_or_default()
        .contains("post-run failure"));

    let opaque = run(
        "delivery-opaque",
        "nonzero_opaque",
        r#"["spot","place","--sz","1","--side","buy","--ordType","market","--live"]"#,
    );
    assert_eq!(opaque["data"]["status"], "unknown_after_submit");
    assert!(opaque["data"]["reason"]
        .as_str()
        .unwrap_or_default()
        .contains("opaque transport failure"));
    assert_eq!(
        run(
            "delivery-status",
            "status_only",
            r#"["spot","place","--sz","1","--side","buy","--ordType","market","--live"]"#,
        )["data"]["status"],
        "unknown_after_submit"
    );
    assert_eq!(
        run(
            "delivery-receipt",
            "receipt",
            r#"["spot","place","--sz","1","--side","buy","--ordType","market","--live"]"#,
        )["data"]["status"],
        "submitted"
    );

    let normalized = run(
        "delivery-normalized",
        "normalized_sentinels",
        r#"["--live","--json","swap","place","--sz","1","--side","buy","--tdMode","cross","--ordType","market","--tpTriggerPx","999999","--tpOrdPx","-1","--slTriggerPx","1","--slOrdPx","-1"]"#,
    );
    assert_eq!(normalized["data"]["status"], "submitted");
    assert_eq!(normalized["data"]["receipt"]["ordId"], "normalized-42");

    let futures_place = run(
        "delivery-futures-place",
        "receipt",
        r#"["--live","futures","place","--instId","BTC-USDT-260925","--tdMode","cross","--side","buy","--ordType","market","--sz","1","--json"]"#,
    );
    assert_eq!(futures_place["data"]["status"], "submitted");

    let option = run(
        "delivery-option",
        "receipt",
        r#"["--live","option","place","--instId","BTC-USD-260925-100000-C","--tdMode","cross","--side","buy","--ordType","market","--sz","1","--json"]"#,
    );
    assert_eq!(option["data"]["status"], "submitted");

    let event = run(
        "delivery-event",
        "receipt",
        r#"["--live","event","place","BTC-ABOVE","buy","yes","1","--ordType","market","--json"]"#,
    );
    assert_eq!(event["data"]["status"], "submitted", "event={event}");
    assert_eq!(event["data"]["receipt"]["ordId"], "42");

    let mut close = onchainos();
    let close_output = scrubbed(&mut close, &home)
        .env("PATH", &bin)
        .env("FAKE_OKX_MODE", "receipt")
        .args([
            "agent",
            "autotrade-execute",
            "--job-id",
            "job1",
            "--delivery-id",
            "delivery-close",
            "--venue",
            "trade_kit",
            "--action",
            "sell",
            "--amount",
            "1",
            "--command-json",
            r#"["--live","--json","futures","close","--instId","BTC-USDT-260925","--mgnMode","cross","--posSide","long"]"#,
        ])
        .output()
        .unwrap();
    assert!(
        close_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&close_output.stderr)
    );
    let close_result = parse_stdout_json(&close_output);
    assert_eq!(close_result["data"]["status"], "submitted");
    assert_eq!(close_result["data"]["receipt"]["ordId"], "42");

    let not_ready = run(
        "delivery-not-ready",
        "not_ready",
        r#"["spot","place","--sz","1","--side","buy","--ordType","market","--live"]"#,
    );
    assert_eq!(not_ready["data"]["status"], "failed_before_submit");
    assert!(not_ready["data"]["reason"]
        .as_str()
        .unwrap_or_default()
        .contains("trade_permission_required"));
}
