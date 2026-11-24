//! Integration coverage for the package-managed `upgrade` command surface.

mod common;

use common::onchainos;

// `upgrade` intentionally has no legacy flags: the installer owns its behavior.
#[test]
fn upgrade_help_has_no_legacy_flags() {
    let output = onchainos()
        .args(["upgrade", "--help"])
        .output()
        .expect("run onchainos upgrade --help");

    assert!(
        output.status.success(),
        "upgrade --help must exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("--check") && !stdout.contains("--force"),
        "upgrade --help must not expose retired flags: {stdout}"
    );
}
