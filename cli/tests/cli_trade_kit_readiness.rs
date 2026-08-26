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

if [ "$1 $2" != "list-tools --json" ]; then
  printf '%s\n' 'private or unexpected Trade Kit invocation' >&2
  exit 70
fi

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

printf '%s\n' "{\"version\":\"$version\",\"modules\":[{\"commands\":[{\"toolName\":\"market_get_ticker\"},{\"toolName\":\"spot_place_order\"},{\"toolName\":\"market_get_instruments\"},{\"toolName\":\"account_get_config\"},{\"toolName\":\"swap_get_leverage\"},{\"toolName\":\"swap_set_leverage\"},{\"toolName\":\"swap_place_order\"},{\"toolName\":\"swap_close_position\"},{\"toolName\":\"futures_get_leverage\"},{\"toolName\":\"futures_set_leverage\"},{\"toolName\":\"futures_place_order\"},{\"toolName\":\"futures_close_position\"},{\"toolName\":\"event_browse\"},{\"toolName\":\"event_get_series\"},{\"toolName\":\"event_get_events\"},{\"toolName\":\"event_get_markets\"},{\"toolName\":\"event_place_order\"},{\"toolName\":\"option_get_instruments\"},{\"toolName\":\"option_get_greeks\"},{\"toolName\":\"option_place_order\"}]}]}"
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
    environment: &str,
) -> std::process::Output {
    let mut cmd = common::onchainos();
    common::scrubbed(&mut cmd, home)
        .env("HOME", home)
        .env("PATH", bin)
        .env("FAKE_TRADE_KIT_MODE", mode)
        .env("FAKE_TRADE_KIT_LOG", log)
        .args(["agent", "trade-kit-readiness", "--environment", environment]);
    for class in classes {
        cmd.args(["--asset-class", class]);
    }
    cmd.output().expect("run trade-kit-readiness")
}

fn data_from_success(output: &std::process::Output) -> Value {
    common::assert_ok_and_extract_data(output)
}

fn assert_v3_local_shape(data: &Value) {
    assert_eq!(data["schemaVersion"], 3);
    assert_eq!(data["scope"], "local_compatibility");
    assert_eq!(data["authenticationChecked"], false);
    assert!(data["checkedAt"].as_str().is_some());
    assert!(data["assetClasses"].is_array());
    assert!(data["assetChecks"].is_array());
    assert!(matches!(
        data["environment"].as_str(),
        Some("configured" | "live" | "demo")
    ));
    assert_eq!(data["ready"], data["readiness"] == "ready");
}

#[cfg(unix)]
#[test]
fn missing_cli_returns_local_missing_without_spawning() {
    let (_guard, home) = common::fresh_home("trade-kit-readiness-missing");
    let bin = home.join("empty-bin");
    fs::create_dir_all(&bin).unwrap();
    let log = home.join("must-not-exist.log");

    let data = data_from_success(&run_readiness(
        &home,
        &bin,
        &log,
        "ready",
        &["spot"],
        "live",
    ));
    assert_v3_local_shape(&data);
    assert_eq!(data["readiness"], "missing");
    assert_eq!(data["reason"], "cli_missing");
    assert_eq!(
        data["remediation"]["install"],
        "npm install -g @okx_ai/okx-trade-cli@latest"
    );
    assert!(!log.exists());
}

#[cfg(unix)]
#[test]
fn compatible_runtime_is_ready_without_any_private_or_auth_probe() {
    for environment in ["configured", "live", "demo"] {
        let (_guard, home) = common::fresh_home(&format!("trade-kit-local-ready-{environment}"));
        let (bin, log) = install_fake_trade_kit(&home);
        let data = data_from_success(&run_readiness(
            &home,
            &bin,
            &log,
            "ready",
            &["spot", "perp", "spot"],
            environment,
        ));
        assert_v3_local_shape(&data);
        assert_eq!(data["assetClasses"], serde_json::json!(["spot", "perp"]));
        assert_eq!(data["readiness"], "ready");
        assert_eq!(data["reason"], "ready");
        assert_eq!(data["version"], "1.4.2");
        assert!(data["remediation"].is_null());
        assert!(data["assetChecks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|check| check["ready"] == true));

        let calls = fs::read_to_string(&log).unwrap();
        assert_eq!(calls, "update=false args=list-tools --json\n");
        assert!(!calls.contains("account config"));
        assert!(!calls.contains("auth status"));
    }
}

#[cfg(unix)]
#[test]
fn old_or_capability_incomplete_runtime_is_locally_incompatible() {
    for (mode, classes, reason) in [
        ("old", vec!["spot"], "upgrade_required"),
        (
            "missing-capability",
            vec!["spot", "perp"],
            "capability_missing",
        ),
    ] {
        let (_guard, home) = common::fresh_home(&format!("trade-kit-local-{mode}"));
        let (bin, log) = install_fake_trade_kit(&home);
        let data = data_from_success(&run_readiness(&home, &bin, &log, mode, &classes, "demo"));
        assert_v3_local_shape(&data);
        assert_eq!(data["readiness"], "incompatible");
        assert_eq!(data["reason"], reason);
        assert_eq!(
            data["remediation"]["upgrade"],
            "npm install -g @okx_ai/okx-trade-cli@latest"
        );
        assert_eq!(fs::read_to_string(log).unwrap().lines().count(), 1);
    }
}

#[cfg(unix)]
#[test]
fn inconclusive_local_discovery_never_claims_auth_failure_or_blocks() {
    let (_guard, home) = common::fresh_home("trade-kit-local-malformed");
    let (bin, log) = install_fake_trade_kit(&home);
    let data = data_from_success(&run_readiness(
        &home,
        &bin,
        &log,
        "malformed",
        &["spot"],
        "live",
    ));
    assert_v3_local_shape(&data);
    assert_eq!(data["readiness"], "verification_unknown");
    assert_eq!(data["reason"], "discovery_failed");
    assert!(data["remediation"].is_null());
    let encoded = serde_json::to_string(&data).unwrap();
    assert!(!encoded.contains("auth_probe"));
    assert!(!encoded.contains("auth_required"));
    assert_eq!(fs::read_to_string(log).unwrap().lines().count(), 1);
}

#[test]
fn asset_class_is_required_and_aliases_are_rejected_before_probe() {
    let (_guard, home) = common::fresh_home("trade-kit-readiness-empty");
    let output = common::scrubbed(&mut common::onchainos(), &home)
        .args(["agent", "trade-kit-readiness"])
        .output()
        .unwrap();
    common::assert_error_contains(&output, &["--asset-class"]);

    for class in ["defi", "futures", "options", "SPOT", "unknown"] {
        let (_guard, home) = common::fresh_home(&format!("trade-kit-invalid-{class}"));
        let output = common::scrubbed(&mut common::onchainos(), &home)
            .args(["agent", "trade-kit-readiness", "--asset-class", class])
            .output()
            .unwrap();
        common::assert_error_contains(
            &output,
            &["asset class must be spot, perp, prediction, or option"],
        );
    }
}

#[test]
fn signal_playbook_uses_final_command_as_the_only_authentication_authority() {
    let playbook = include_str!("../../skills/okx-ai/references/task-subscription-signal.md");
    assert!(playbook.contains("local compatibility only"));
    assert!(playbook.contains("never checks authentication"));
    assert!(playbook.contains("do not run it on every delivery"));
    assert!(playbook.contains("exactly one authority"));
    assert!(playbook.contains("never automatically retry or replay"));
    assert!(!playbook.contains("auth_probe_unavailable"));
}
