mod common;

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

#[cfg(unix)]
fn install_fake_trade_kit(home: &Path) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let bin = home.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let log = home.join("trade-kit-argv.log");
    let script = bin.join("okx");
    fs::write(
        &script,
        r#"#!/bin/sh
printf 'update=%s args=%s\n' "$OKX_UPDATE_CHECK" "$*" >> "$FAKE_TRADE_KIT_LOG"

if [ "$1 $2" = "list-tools --json" ]; then
  case "$FAKE_TRADE_KIT_MODE" in
    old)
      version="1.3.1"
      ;;
    malformed)
      printf '%s\n' '{not-json'
      exit 0
      ;;
    missing-capability)
      printf '%s\n' '{"version":"1.4.2","modules":[{"commands":[{"toolName":"market_get_ticker"}]}]}'
      exit 0
      ;;
    *)
      version="1.4.2"
      ;;
  esac
  printf '%s\n' "{\"version\":\"$version\",\"modules\":[{\"commands\":[{\"toolName\":\"market_get_ticker\"},{\"toolName\":\"spot_place_order\"},{\"toolName\":\"market_get_instruments\"},{\"toolName\":\"account_get_config\"},{\"toolName\":\"swap_get_leverage\"},{\"toolName\":\"swap_set_leverage\"},{\"toolName\":\"swap_place_order\"},{\"toolName\":\"event_browse\"},{\"toolName\":\"event_get_series\"},{\"toolName\":\"event_get_events\"},{\"toolName\":\"event_get_markets\"},{\"toolName\":\"event_place_order\"},{\"toolName\":\"option_get_instruments\"},{\"toolName\":\"option_get_greeks\"},{\"toolName\":\"option_place_order\"}]}]}"
  exit 0
fi

if [ "$1 $2 $3" = "account config --json" ]; then
  case "$FAKE_TRADE_KIT_MODE" in
    ready|oauth-ready)
      printf '%s\n' '[{"perm":"read_only,trade","uid":"AUTH_STDOUT_CANARY"}]'
      exit 0
      ;;
    api-key-ready)
      printf '%s\n' '{"data":[{"perm":"trade","uid":"AUTH_STDOUT_CANARY"}]}'
      exit 0
      ;;
    read-only)
      printf '%s\n' '[{"perm":"read_only","uid":"ACCOUNT_DATA_CANARY"}]'
      exit 0
      ;;
    near-match-permission)
      printf '%s\n' '[{"perm":"read_only,trade_history","uid":"ACCOUNT_DATA_CANARY"}]'
      exit 0
      ;;
    missing-permission)
      printf '%s\n' '[{"uid":"ACCOUNT_DATA_CANARY","acctLv":"2"}]'
      exit 0
      ;;
    auth-required)
      printf '%s\n' 'Error: No credentials found. AUTH_STDERR_CANARY' >&2
      printf '%s\n' 'Hint: Run `okx auth login` or configure API key credentials.' >&2
      exit 1
      ;;
    invalid-oauth-token)
      printf '%s\n' 'Error: HTTP 400 from OKX: Invalid token' >&2
      printf '%s\n' 'Code: 400' >&2
      printf '%s\n' 'Hint: Retry later or verify endpoint parameters.' >&2
      exit 1
      ;;
    empty-oauth-token)
      printf '%s\n' 'Error: okx-auth returned empty token.' >&2
      printf '%s\n' 'Hint: Run `okx auth login` to re-authenticate.' >&2
      exit 1
      ;;
    oauth-helper-error)
      printf '%s\n' 'Error: okx-auth token exited with code 9.' >&2
      printf '%s\n' 'Hint: Run `okx auth login` to re-authenticate.' >&2
      exit 1
      ;;
    invalid-auth-config)
      printf '%s\n' 'Error: Failed to parse config.toml: Invalid TOML document' >&2
      printf '%s\n' 'Hint: Or re-run: okx config init' >&2
      exit 1
      ;;
    network)
      printf '%s\n' 'Error: NetworkError: fetch failed ECONNREFUSED NETWORK_STDERR_CANARY' >&2
      exit 1
      ;;
  esac
fi

printf '%s\n' 'unexpected fake TradeKit invocation' >&2
exit 70
"#,
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    (bin, log)
}

#[cfg(unix)]
fn run_readiness(
    home: &Path,
    bin: &Path,
    log: &Path,
    mode: &str,
    classes: &[&str],
) -> std::process::Output {
    let mut cmd = common::onchainos();
    common::scrubbed(&mut cmd, home)
        .env("PATH", bin)
        .env("FAKE_TRADE_KIT_MODE", mode)
        .env("FAKE_TRADE_KIT_LOG", log)
        .args(["agent", "trade-kit-readiness"]);
    for class in classes {
        cmd.args(["--asset-class", class]);
    }
    cmd.output().expect("run trade-kit-readiness")
}

fn data_from_success(output: &std::process::Output) -> Value {
    common::assert_ok_and_extract_data(output)
}

fn assert_not_exposed(output: &std::process::Output, home: &Path, canary: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let audit_path = home.join("audit.jsonl");
    assert!(
        audit_path.is_file(),
        "expected CLI audit log at {audit_path:?}"
    );
    let audit = fs::read_to_string(audit_path).unwrap();
    assert!(!stdout.contains(canary), "stdout leaked {canary}");
    assert!(!stderr.contains(canary), "stderr leaked {canary}");
    assert!(!audit.contains(canary), "audit log leaked {canary}");
}

fn assert_v2_shape(data: &Value) {
    assert_eq!(data["schemaVersion"], 2);
    assert!(data["checkedAt"].as_str().is_some());
    assert!(data["assetClasses"].is_array());
    assert!(data["assetChecks"].is_array());
    assert_eq!(data["ready"], data["readiness"] == "ready");
}

#[cfg(unix)]
#[test]
fn missing_cli_returns_missing_without_spawning() {
    let (_guard, home) = common::fresh_home("trade-kit-readiness-missing");
    let bin = home.join("empty-bin");
    fs::create_dir_all(&bin).unwrap();
    let log = home.join("must-not-exist.log");

    let data = data_from_success(&run_readiness(&home, &bin, &log, "ready", &["spot"]));
    assert_v2_shape(&data);
    assert_eq!(data["readiness"], "missing");
    assert_eq!(data["ready"], false);
    assert_eq!(data["reason"], "cli_missing");
    assert_eq!(data["assetChecks"][0]["readiness"], "missing");
    assert_eq!(
        data["remediation"]["install"],
        "npm install -g @okx_ai/okx-trade-cli@latest"
    );
    assert!(!log.exists(), "missing CLI must not spawn a child");
}

#[cfg(unix)]
#[test]
fn multi_asset_probe_deduplicates_and_calls_discovery_and_auth_once() {
    for mode in ["oauth-ready", "api-key-ready"] {
        let (_guard, home) = common::fresh_home(&format!("trade-kit-readiness-{mode}"));
        let (bin, log) = install_fake_trade_kit(&home);

        let output = run_readiness(&home, &bin, &log, mode, &["spot", "perp", "spot"]);
        let data = data_from_success(&output);
        assert_v2_shape(&data);
        assert_eq!(data["assetClasses"], serde_json::json!(["spot", "perp"]));
        assert_eq!(data["readiness"], "ready", "mode={mode}");
        assert_eq!(data["ready"], true, "mode={mode}");
        assert_eq!(data["reason"], "ready", "mode={mode}");
        assert_eq!(data["version"], "1.4.2", "mode={mode}");
        assert!(data["remediation"].is_null(), "mode={mode}");
        assert_eq!(data["assetChecks"].as_array().unwrap().len(), 2);
        assert!(data["assetChecks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|check| check["ready"] == true && check["readiness"] == "ready"));

        assert_not_exposed(&output, &home, "AUTH_STDOUT_CANARY");
        let calls = fs::read_to_string(log).unwrap();
        assert_eq!(
            calls.lines().collect::<Vec<_>>(),
            [
                "update=false args=list-tools --json",
                "update=false args=account config --json",
            ],
            "mode={mode}"
        );
    }
}

#[cfg(unix)]
#[test]
fn exit_zero_without_exact_trade_permission_is_not_ready() {
    for mode in ["read-only", "near-match-permission"] {
        let (_guard, home) = common::fresh_home(&format!("trade-kit-permission-{mode}"));
        let (bin, log) = install_fake_trade_kit(&home);
        let output = run_readiness(&home, &bin, &log, mode, &["spot"]);
        let data = data_from_success(&output);
        assert_v2_shape(&data);
        assert_eq!(data["readiness"], "needs_configuration", "mode={mode}");
        assert_eq!(data["reason"], "trade_permission_required", "mode={mode}");
        assert_eq!(data["ready"], false, "mode={mode}");
        assert_eq!(data["remediation"]["oauth"], "okx auth login --manual");
        assert_eq!(data["remediation"]["apiKey"], "okx config init");
        assert_not_exposed(&output, &home, "ACCOUNT_DATA_CANARY");
        assert_eq!(fs::read_to_string(log).unwrap().lines().count(), 2);
    }
}

#[cfg(unix)]
#[test]
fn successful_private_call_with_missing_permission_is_unknown_not_logged_out() {
    let (_guard, home) = common::fresh_home("trade-kit-missing-permission");
    let (bin, log) = install_fake_trade_kit(&home);
    let output = run_readiness(&home, &bin, &log, "missing-permission", &["spot"]);
    let data = data_from_success(&output);
    assert_v2_shape(&data);
    assert_eq!(data["readiness"], "verification_unknown");
    assert_eq!(data["reason"], "permission_response_invalid");
    assert!(data["remediation"].get("oauth").is_none());
    assert!(data["remediation"].get("apiKey").is_none());
    assert!(data["remediation"]["retry"].is_string());
    assert_not_exposed(&output, &home, "ACCOUNT_DATA_CANARY");
}

#[cfg(unix)]
#[test]
fn unauthenticated_cli_returns_oauth_and_api_key_guidance_without_leaking_stderr() {
    let (_guard, home) = common::fresh_home("trade-kit-readiness-auth-required");
    let (bin, log) = install_fake_trade_kit(&home);

    let output = run_readiness(&home, &bin, &log, "auth-required", &["spot"]);
    let data = data_from_success(&output);
    assert_v2_shape(&data);
    assert_eq!(data["readiness"], "needs_configuration");
    assert_eq!(data["ready"], false);
    assert_eq!(data["reason"], "auth_required");
    assert_eq!(data["remediation"]["oauth"], "okx auth login --manual");
    assert_eq!(data["remediation"]["apiKey"], "okx config init");
    assert_not_exposed(&output, &home, "AUTH_STDERR_CANARY");
    assert_eq!(fs::read_to_string(log).unwrap().lines().count(), 2);
}

#[cfg(unix)]
#[test]
fn invalid_auth_configuration_states_return_setup_guidance_instead_of_retry_only() {
    for mode in [
        "invalid-oauth-token",
        "empty-oauth-token",
        "oauth-helper-error",
        "invalid-auth-config",
    ] {
        let (_guard, home) = common::fresh_home(&format!("trade-kit-readiness-{mode}"));
        let (bin, log) = install_fake_trade_kit(&home);

        let output = run_readiness(&home, &bin, &log, mode, &["spot"]);
        let data = data_from_success(&output);
        assert_v2_shape(&data);
        assert_eq!(data["readiness"], "needs_configuration", "mode={mode}");
        assert_eq!(data["reason"], "auth_required", "mode={mode}");
        assert_eq!(data["remediation"]["oauth"], "okx auth login --manual");
        assert_eq!(data["remediation"]["apiKey"], "okx config init");
    }
}

#[cfg(unix)]
#[test]
fn old_cli_is_incompatible_before_auth_probe() {
    let (_guard, home) = common::fresh_home("trade-kit-readiness-old");
    let (bin, log) = install_fake_trade_kit(&home);

    let data = data_from_success(&run_readiness(&home, &bin, &log, "old", &["spot"]));
    assert_v2_shape(&data);
    assert_eq!(data["readiness"], "incompatible");
    assert_eq!(data["reason"], "upgrade_required");
    assert_eq!(data["version"], "1.3.1");
    assert_eq!(
        data["remediation"]["upgrade"],
        "npm install -g @okx_ai/okx-trade-cli@latest"
    );
    assert_eq!(fs::read_to_string(log).unwrap().lines().count(), 1);
}

#[cfg(unix)]
#[test]
fn missing_capability_is_incompatible_before_auth_probe() {
    let (_guard, home) = common::fresh_home("trade-kit-readiness-capability");
    let (bin, log) = install_fake_trade_kit(&home);

    let data = data_from_success(&run_readiness(
        &home,
        &bin,
        &log,
        "missing-capability",
        &["spot", "perp"],
    ));
    assert_v2_shape(&data);
    assert_eq!(data["readiness"], "incompatible");
    assert_eq!(data["reason"], "capability_missing");
    assert!(data["assetChecks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["readiness"] == "incompatible"));
    assert_eq!(fs::read_to_string(log).unwrap().lines().count(), 1);
}

#[cfg(unix)]
#[test]
fn malformed_discovery_is_unknown_before_auth_probe() {
    let (_guard, home) = common::fresh_home("trade-kit-readiness-malformed");
    let (bin, log) = install_fake_trade_kit(&home);

    let data = data_from_success(&run_readiness(&home, &bin, &log, "malformed", &["spot"]));
    assert_v2_shape(&data);
    assert_eq!(data["readiness"], "verification_unknown");
    assert_eq!(data["reason"], "discovery_failed");
    assert_eq!(fs::read_to_string(log).unwrap().lines().count(), 1);
}

#[cfg(unix)]
#[test]
fn network_failure_is_unknown_not_misreported_as_logged_out() {
    let (_guard, home) = common::fresh_home("trade-kit-readiness-network");
    let (bin, log) = install_fake_trade_kit(&home);

    let output = run_readiness(&home, &bin, &log, "network", &["spot"]);
    let data = data_from_success(&output);
    assert_v2_shape(&data);
    assert_eq!(data["readiness"], "verification_unknown");
    assert_eq!(data["reason"], "auth_probe_unavailable");
    assert!(data["remediation"].get("oauth").is_none());
    assert!(data["remediation"].get("apiKey").is_none());
    assert_not_exposed(&output, &home, "NETWORK_STDERR_CANARY");
}

#[test]
fn asset_class_is_required_and_aliases_are_rejected_before_probe() {
    let (_guard, home) = common::fresh_home("trade-kit-readiness-empty");
    let output = common::scrubbed(&mut common::onchainos(), &home)
        .args(["agent", "trade-kit-readiness"])
        .output()
        .expect("run readiness without a class");
    common::assert_error_contains(&output, &["--asset-class"]);

    for class in ["defi", "futures", "options", "SPOT", "unknown"] {
        let (_guard, home) = common::fresh_home(&format!("trade-kit-readiness-invalid-{class}"));
        let output = common::scrubbed(&mut common::onchainos(), &home)
            .args(["agent", "trade-kit-readiness", "--asset-class", class])
            .output()
            .expect("run unsupported readiness class");
        common::assert_error_contains(
            &output,
            &["asset class must be spot, perp, prediction, or option"],
        );
    }
}

#[test]
fn signal_playbook_gates_every_delivery_and_never_auto_replays_blocked_work() {
    let playbook = include_str!("../../skills/okx-ai/references/task-subscription-signal.md");
    let command =
        "onchainos agent trade-kit-readiness --asset-class <class> [--asset-class <class> ...]";
    let gate = playbook
        .find(command)
        .expect("signal playbook must invoke the typed Trade Kit gate");
    let route = playbook
        .find("onchainos agent subscription-route-set")
        .expect("signal playbook route-set command");
    assert!(gate < route, "readiness must run before route persistence");
    assert!(playbook.contains("Run it on **every delivery**"));
    assert!(playbook.contains("`data.readiness == \"ready\"`"));
    assert!(playbook.contains("Non-Trade-Kit routes never run this command"));
    assert!(playbook.contains("preserve and display the deliverable"));
    assert!(playbook.contains("MUST NOT automatically replay"));
}
