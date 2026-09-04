//! Offline integration coverage for the legacy `upgrade` command (NFR-1: kept
//! one release cycle after `preflight --force` ships). Verifies the command
//! still parses and its `--help` lists the documented flags, so existing scripts
//! calling `onchainos upgrade [--check|--force|…]` keep working.

mod common;

use common::onchainos;

// IT-007: the older update command still works so existing scripts keep running.
// The legacy `upgrade` entry is unchanged this release; `upgrade --help` still
// lists its flags, including `--check`. clap prints help to stdout and exits 0.
#[test]
fn upgrade_help_lists_check_flag() {
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
        stdout.contains("--check"),
        "upgrade --help must still list the --check flag: {stdout}"
    );
}
