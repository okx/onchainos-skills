//! Offline integration coverage for the session-start preflight throttle.

mod common;

use common::{fresh_home, onchainos, parse_stdout_json, scrubbed};

#[test]
fn fresh_last_check_skips_remote_preflight_work() {
    let (_guard, home) = fresh_home("preflight-throttle");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock must be after unix epoch")
        .as_secs();
    std::fs::write(home.join("last_check"), now.to_string()).expect("write last_check");

    let output = scrubbed(&mut onchainos(), &home)
        .args(["preflight", "--skill-version", env!("CARGO_PKG_VERSION")])
        .output()
        .expect("run onchainos preflight");

    assert!(
        output.status.success(),
        "preflight failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["status"], "fresh");
    assert_eq!(json["data"]["throttled"], true);
    assert_eq!(json["data"]["updated"], false);
    assert_eq!(json["data"]["action"], serde_json::Value::Null);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("GitHub"),
        "unexpected network attempt: {stderr}"
    );
    assert!(
        !stderr.contains("Updating skills"),
        "unexpected skill checkout update: {stderr}"
    );
    assert!(
        !stderr.contains("Removing deprecated skills"),
        "unexpected package-manager cleanup: {stderr}"
    );
}
