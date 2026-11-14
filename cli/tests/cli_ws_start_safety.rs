//! Offline regressions for start/stop channel-path consistency.
mod common;

use common::{
    assert_error_contains, assert_ok_and_extract_data, fresh_home, onchainos, parse_stdout_json,
    scrubbed,
};
use serde_json::json;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn unsafe_start_channels_fail_before_credentials_or_session_creation() {
    for dev in [false, true] {
        let (_guard, home) = fresh_home("ws_start_invalid_channel");
        for channel in [
            "foo/bar",
            "foo\\bar",
            ".",
            "..",
            "/absolute",
            "C:channel",
            "trailing.",
            "trailing ",
            "dex-token-candle/../foo",
        ] {
            let mut cmd = onchainos();
            scrubbed(&mut cmd, &home);
            for name in [
                "OKX_PRE_API_KEY",
                "OKX_PRE_SECRET_KEY",
                "OKX_PRE_PASSPHRASE",
                "OKX_PROD_API_KEY",
                "OKX_PROD_SECRET_KEY",
                "OKX_PROD_PASSPHRASE",
            ] {
                cmd.env_remove(name);
            }
            if dev {
                cmd.arg("--dev");
            }
            let output = cmd
                .args(["ws", "start"])
                .arg(format!("--channel={channel}"))
                .output()
                .unwrap();
            assert_error_contains(&output, &["invalid channel"]);
            let data = parse_stdout_json(&output);
            assert_eq!(data["ok"], false);
            assert!(data["data"]["id"].is_null());
            assert!(
                !home.join("watch").exists(),
                "start created a session for {channel:?}"
            );
        }
    }
}

#[test]
fn normal_and_default_channels_reuse_a_session_that_stop_can_clean() {
    let channel = "kol_smartmoney-tracker-activity";
    for dev in [false, true] {
        for explicit in [false, true] {
            let (_guard, home) = fresh_home("ws_start_valid_channel");
            let dir = home.join("watch/ws_abc123");
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("config.json"),
                json!({"channels":[channel], "env":if dev {"pre"} else {"prod"}, "created_at":0})
                    .to_string(),
            )
            .unwrap();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis();
            fs::write(dir.join("status"), format!("running|{now}")).unwrap();
            let mut cmd = onchainos();
            scrubbed(&mut cmd, &home);
            for name in [
                "OKX_PRE_API_KEY",
                "OKX_PRE_SECRET_KEY",
                "OKX_PRE_PASSPHRASE",
                "OKX_PROD_API_KEY",
                "OKX_PROD_SECRET_KEY",
                "OKX_PROD_PASSPHRASE",
            ] {
                cmd.env_remove(name);
            }
            if dev {
                cmd.arg("--dev");
            }
            cmd.args(["ws", "start"]);
            if explicit {
                cmd.args(["--channel", channel, "--channel", channel]);
            }
            let data = assert_ok_and_extract_data(&cmd.output().unwrap());
            assert_eq!(data["id"], "ws_abc123");
            assert_eq!(data["status"], "already_running");
            assert_eq!(data["channels"], json!([channel]));
            assert_eq!(fs::read_dir(home.join("watch")).unwrap().count(), 1);
            let output = scrubbed(&mut onchainos(), &home)
                .args(["ws", "stop", "--id", "ws_abc123"])
                .output()
                .unwrap();
            assert_ok_and_extract_data(&output);
            assert!(!dir.exists());
        }
    }
}

#[test]
fn stop_preserves_existing_unsafe_channel_configs() {
    let (_guard, home) = fresh_home("ws_stop_invalid_channel_config");
    let dir = home.join("watch/ws_abc123");
    fs::create_dir_all(&dir).unwrap();
    let config = json!({"channels":["foo/bar"], "created_at":0}).to_string();
    fs::write(dir.join("config.json"), &config).unwrap();
    fs::write(dir.join("status"), "stopped|0").unwrap();
    for flush in [false, true] {
        let mut cmd = onchainos();
        scrubbed(&mut cmd, &home).args(["ws", "stop", "--id", "ws_abc123"]);
        if flush {
            cmd.arg("--flush");
        }
        let output = cmd.output().unwrap();
        assert_error_contains(&output, &["unsafe channel", "invalid channel"]);
        assert_eq!(fs::read_to_string(dir.join("config.json")).unwrap(), config);
        assert_eq!(fs::read_to_string(dir.join("status")).unwrap(), "stopped|0");
    }
}
