//! AC-level integration tests for the two-phase `payment quote` → `payment pay`
//! flow. These exercise the CLI end-to-end through the
//! built `onchainos` binary, covering the paymentId state-machine contracts that
//! the PRD acceptance criteria pin down and that unit tests cannot reach at the
//! process/exit-code level:
//!
//! - unconfirmed `pay` (no `--yes`) stops at the confirming gate with exit 2;
//! - a missing / expired paymentId → `quote_expired_or_missing` (exit 1);
//! - a paymentId owned by a different account → `cross_user_payment_id` (exit 1);
//! - `--selected-index` out of range → `invalid_input` (exit 1);
//! - malformed `--param` on `quote` → `invalid_input` (exit 1, before any probe).
//!
//! These are hermetic: they set `ONCHAINOS_HOME` to a per-test temp dir and, for
//! the state-machine cases, pre-write the persisted quote state directly, so no
//! wallet login or network is required. A full CLI-level `pay` replay stays out
//! of scope here because the sign step before the replay needs a logged-in
//! wallet + keyring + live wallet-backend calls; the merchant replay seam itself
//! (`replay_merchant`) — including the 402 → `pending` and 200 → `success`
//! status mapping — is covered hermetically against a local mock merchant in
//! `payment_flow`'s `replay_merchant_maps_*` tests.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::onchainos;
use serde_json::Value;

/// Per-test temp `ONCHAINOS_HOME` under the crate target dir (no hardcoded /tmp).
fn temp_home(sub: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join("cli_payment")
        .join(sub);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp home");
    dir
}

/// Write a persisted quote state to `<home>/payments/<id>.json`, mirroring the
/// on-disk `PaymentState` shape so `payment pay --payment-id` reads it back.
fn write_state(home: &Path, id: &str, owner: &str, expires_at: u64) {
    let payments = home.join("payments");
    fs::create_dir_all(&payments).expect("create payments dir");
    let state = serde_json::json!({
        "payment_id": id,
        "owner_wallet": owner,
        "created_at": 1_000,
        "expires_at": expires_at,
        "accepts": [{
            "index": 0,
            "scheme": "exact",
            "amount": "10000",
            "asset": "USDC",
            "network": "eip155:8453"
        }],
        "decoded_challenge": {
            "amount": "10000",
            "amountHuman": "0.01",
            "decimals": 6,
            "recipient": "0xRECIPIENT",
            "expires": 0,
            "supported": true,
            "unsupported_reason": null
        },
        "candidates": [],
        "known_params": {},
        "merchant_body": "",
        "endpoint_url": "https://merchant.example/x",
        "raw_accepts": [{
            "scheme": "exact",
            "amount": "10000",
            "network": "eip155:8453",
            "payTo": "0xRECIPIENT",
            "asset": "USDC"
        }],
        "resource": null,
        "method": "GET",
        "param_plan": []
    });
    fs::write(
        payments.join(format!("{id}.json")),
        serde_json::to_vec_pretty(&state).unwrap(),
    )
    .expect("write state file");
}

/// Parse stdout as the always-on JSON envelope.
fn stdout_json(out: &std::process::Output) -> Value {
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("invalid JSON stdout: {e}\nraw: {stdout}"))
}

/// Assert an error envelope whose machine token (leading word of `error`)
/// matches `token`, and that the process exited 1.
fn assert_error_token(out: &std::process::Output, token: &str) {
    assert_eq!(
        out.status.code(),
        Some(1),
        "expected exit 1, got {:?}\nstdout: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout)
    );
    let json = stdout_json(out);
    assert_eq!(json["ok"], Value::Bool(false), "expected ok=false: {json}");
    let err = json["error"].as_str().unwrap_or_default();
    assert!(
        err.starts_with(token),
        "expected error to start with `{token}`, got `{err}`"
    );
}

// ── paymentId TTL / ownership guards ──────────────────────────────────────

#[test]
fn pay_missing_payment_id_is_quote_expired_or_missing() {
    let home = temp_home("missing");
    let out = onchainos()
        .env("ONCHAINOS_HOME", &home)
        .args([
            "payment",
            "pay",
            "--payment-id",
            "pay_does_not_exist",
            "--yes",
        ])
        .output()
        .expect("run onchainos");
    assert_error_token(&out, "quote_expired_or_missing");
}

#[test]
fn pay_expired_state_is_quote_expired_or_missing() {
    let home = temp_home("expired");
    // expires_at in the distant past → read() bails as expired.
    write_state(&home, "pay_expired", "", 1_500);
    let out = onchainos()
        .env("ONCHAINOS_HOME", &home)
        .args(["payment", "pay", "--payment-id", "pay_expired", "--yes"])
        .output()
        .expect("run onchainos");
    assert_error_token(&out, "quote_expired_or_missing");
}

#[test]
fn pay_state_owned_by_other_account_is_cross_user() {
    let home = temp_home("cross_user");
    // owner is a different account; with no wallet logged in the current owner
    // resolves to "" → mismatch → cross_user_payment_id (checked before expiry).
    write_state(&home, "pay_other", "someone-else-account", 9_999_999_999);
    let out = onchainos()
        .env("ONCHAINOS_HOME", &home)
        .args(["payment", "pay", "--payment-id", "pay_other", "--yes"])
        .output()
        .expect("run onchainos");
    assert_error_token(&out, "cross_user_payment_id");
}

// ── confirming gate (exit 2) ──────────────────────────────────────────────

#[test]
fn pay_without_yes_stops_at_confirming_gate_exit_2() {
    let home = temp_home("confirm");
    write_state(&home, "pay_confirm", "", 9_999_999_999);
    let out = onchainos()
        .env("ONCHAINOS_HOME", &home)
        .args(["payment", "pay", "--payment-id", "pay_confirm"])
        .output()
        .expect("run onchainos");
    assert_eq!(
        out.status.code(),
        Some(2),
        "confirming gate must exit 2\nstdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let json = stdout_json(&out);
    assert_eq!(
        json["confirming"],
        Value::Bool(true),
        "expected confirming: {json}"
    );
    // The next-step hint must carry --yes so the agent knows how to proceed.
    assert!(
        json["next"].as_str().unwrap_or_default().contains("--yes"),
        "confirming next-step must include --yes: {json}"
    );
}

#[test]
fn pay_selected_index_out_of_range_is_invalid_input() {
    let home = temp_home("range");
    write_state(&home, "pay_range", "", 9_999_999_999);
    // raw_accepts has a single entry (index 0); index 9 is out of range and is
    // rejected before the confirming gate.
    let out = onchainos()
        .env("ONCHAINOS_HOME", &home)
        .args([
            "payment",
            "pay",
            "--payment-id",
            "pay_range",
            "--selected-index",
            "9",
        ])
        .output()
        .expect("run onchainos");
    assert_error_token(&out, "invalid_input");
}

// ── quote input validation (no network) ───────────────────────────────────

#[test]
fn quote_malformed_param_is_invalid_input() {
    let home = temp_home("bad_param");
    // A `--param` without `=` fails parse_params before any network probe.
    let out = onchainos()
        .env("ONCHAINOS_HOME", &home)
        .args([
            "payment",
            "quote",
            "https://merchant.example/x",
            "--param",
            "no_equals_sign",
        ])
        .output()
        .expect("run onchainos");
    assert_error_token(&out, "invalid_input");
}
