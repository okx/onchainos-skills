//! Compatibility coverage for the retired `preflight` subcommand.

mod common;

use common::onchainos;

#[test]
fn preflight_reports_deprecation() {
    let output = onchainos()
        .arg("preflight")
        .output()
        .expect("run onchainos preflight");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("is deprecated"));
}

#[test]
fn preflight_rejects_legacy_arguments() {
    let output = onchainos()
        .args(["preflight", "--force"])
        .output()
        .expect("run onchainos preflight --force");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument"));
}
