//! Integration tests for the wallet-side of the insufficient-balance top-up
//! recovery + universal QR feature (WWINFRA-3798).
//!
//! Source: `oli-docs/xs62wtjxti4ge1kjbtul49d5goc/integration-plan.csv` rows
//! IT-004, IT-007, IT-008, IT-009, IT-012, IT-013.
//!
//! ── Scope & login dependency ─────────────────────────────────────────────────
//!
//! `wallet balance` and `wallet send` are auth-gated: on a fresh sandbox the auth
//! gate (`ensure_tokens_refreshed`) fires FIRST and bails with
//! `"session expired, please login again"` / `"not logged in"` (exit 1) before any
//! scene can be produced (see `cli_wallet_testnet.rs` for the same reasoning). The
//! recovery scenes therefore only materialise when a logged-in wallet is present:
//!   - `transfer_insufficient_balance` fires on backend `code=10004`, or after a
//!     simulation failure when a fresh balance query proves the shortfall;
//!   - the balance-recovery consumer (`wallet balance --force`) needs a logged-in
//!     wallet to report `rawBalance` (spec §2.6).
//!
//! These are `live` rows, so they run through the shared `common::run_with_retry`
//! helper in the ambient `ONCHAINOS_HOME` — a provisioned login fixture is picked
//! up when the environment supplies one. Each test validates the scene / contract
//! precisely when it is reachable and otherwise asserts the login-required /
//! structured `ok:false` path, so it is deterministic (never a crash, never a
//! wrong exit code) whether or not a wallet fixture is present.
//!
//! IT-013 (the removed `wallet qrcode` subcommand) is offline and deterministic —
//! a clap `unrecognized subcommand` usage error (exit 2), matching spec §1.2 /
//! §10.1.

mod common;

use common::{assert_ok_and_extract_data, onchainos, run_with_retry};
use predicates::prelude::*;
use serde_json::Value;

/// True when stdout/stderr carry a recognised "wallet not logged in / session
/// expired" marker. Used to accept the auth-gate path when no login fixture is
/// provisioned in the test environment. `ERR_NOT_LOGGED_IN` is `"not logged in"`
/// and the session-expiry bail is `"session expired, please login again"`.
fn login_required(stdout: &str, stderr: &str) -> bool {
    let hay = format!("{stdout}{stderr}").to_lowercase();
    [
        "not logged in",
        "session expired",
        "please login",
        "login again",
        "log in again",
        "wallet login",
    ]
    .iter()
    .any(|m| hay.contains(m))
}

// ── IT-004 — `wallet balance --force` recovery consumer reports rawBalance ────

/// IT-004 (live, golden): after a top-up the recovery flow refreshes balances via
/// `wallet balance --force`, and each token object carries `rawBalance` (spec
/// §2.6). Validated when a logged-in wallet is present; otherwise the auth gate
/// fires (exit 1) and the login-required path is confirmed. `--force` is an
/// existing, unchanged flag (spec §1.3).
#[test]
fn wallet_balance_force_reports_raw_balance_or_login_required() {
    let output = run_with_retry(&["wallet", "balance", "--force", "--chain", "ethereum"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        let data = assert_ok_and_extract_data(&output);
        assert!(
            data.to_string().contains("rawBalance"),
            "logged-in `wallet balance --force` must report rawBalance: {data}"
        );
    } else {
        assert!(
            login_required(&stdout, &stderr),
            "expected rawBalance output or a login-required failure\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}

// ── IT-007 — QR degradation still surfaces the deposit address ────────────────

/// IT-007 (live, edge): when the `transfer_insufficient_balance` scene is reached,
/// the deposit address is always present even if QR
/// generation degrades — the `qr` sub-object fields are simply absent, never an
/// error, and the scene still exits 1 (spec §3.2 / FR-6). Validated when the scene
/// fires; otherwise the auth gate is confirmed. Either path exits 1.
#[test]
fn wallet_send_insufficient_balance_carries_deposit_address_or_login_required() {
    let output = run_with_retry(&[
        "wallet",
        "send",
        "--readable-amount",
        "100",
        "--recipient",
        "0x1234567890abcdef1234567890abcdef12345678",
        "--chain",
        "ethereum",
    ]);
    assert!(
        !output.status.success(),
        "wallet send must fail here (exit 1), got {:?}",
        output.status.code()
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let json: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);

    if json.pointer("/data/phase").and_then(|s| s.as_str()) == Some("funding_required")
        && json.pointer("/data/payload/operation").and_then(|s| s.as_str()) == Some("transfer")
    {
        assert_eq!(json["ok"], Value::Bool(false), "scene must be ok:false: {json}");
        assert!(
            json.pointer("/data/payload/fundingTarget/receiveAddress").is_some(),
            "transfer_insufficient_balance must carry fundingTarget.receiveAddress even when QR degrades: {json}"
        );
    } else {
        assert!(
            login_required(&stdout, &stderr),
            "expected transfer_insufficient_balance scene or a login-required failure\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}

// ── IT-008 — simulation shortfall requires balance verification ──────────────

/// IT-008 (live, edge): a transfer that fails simulation enters the structured
/// funding scene only when a fresh chain-and-token balance query confirms the
/// shortfall. The scene has no fabricated backend code. If balance verification
/// is unavailable, the original simulation failure remains valid.
#[test]
fn wallet_send_simulation_shortfall_is_balance_verified_or_preserved() {
    let output = run_with_retry(&[
        "wallet",
        "send",
        "--readable-amount",
        "999999999",
        "--recipient",
        "0x1234567890abcdef1234567890abcdef12345678",
        "--chain",
        "ethereum",
    ]);
    assert!(
        !output.status.success(),
        "wallet send must fail here (exit 1), got {:?}",
        output.status.code()
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let json: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);

    if json.pointer("/data/phase").and_then(|s| s.as_str()) == Some("funding_required")
        && json.pointer("/data/payload/operation").and_then(|s| s.as_str()) == Some("transfer")
    {
        assert!(json["data"]["payload"]["error"].get("code").is_none(), "must not fabricate code=10004: {json}");
        assert!(!json["data"]["payload"]["fundingNeed"]["balance"].is_null(), "verified scene requires balance: {json}");
        assert!(!json["data"]["payload"]["fundingNeed"]["shortfall"].is_null(), "verified scene requires shortfall: {json}");
        assert_eq!(json["data"]["nextAction"], serde_json::json!([]));
    } else {
        assert!(
            login_required(&stdout, &stderr) || stdout.contains("executeErrorMsg"),
            "expected verified funding, preserved simulation failure, or login-required\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}

// ── IT-009 — transfer_insufficient_balance top-up prompt ──────────────────────

/// IT-009 (live, error): an under-funded logged-in wallet surfaces the structured
/// scene from backend `code=10004` or balance-verified simulation failure.
#[test]
fn wallet_send_insufficient_balance_scene_or_login_required() {
    let output = run_with_retry(&[
        "wallet",
        "send",
        "--readable-amount",
        "10",
        "--recipient",
        "0x1234567890abcdef1234567890abcdef12345678",
        "--chain",
        "xlayer",
    ]);
    assert!(
        !output.status.success(),
        "wallet send must fail here (exit 1), got {:?}",
        output.status.code()
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let json: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);

    if json.pointer("/data/phase").and_then(|s| s.as_str()) == Some("funding_required")
        && json.pointer("/data/payload/operation").and_then(|s| s.as_str()) == Some("transfer")
    {
        assert_eq!(json["ok"], Value::Bool(false), "scene must be ok:false: {json}");
        assert!(
            json["data"]["payload"]["error"]["code"] == "10004"
                || json["data"]["payload"]["error"].get("code").is_none(),
            "scene must preserve code=10004 or leave absent backend code null: {json}"
        );
    } else {
        assert!(
            login_required(&stdout, &stderr),
            "expected transfer_insufficient_balance scene or a login-required failure\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}

// ── IT-009b — contract-token transfer_insufficient_balance scene ──────────────

/// IT-009b (live, error): an under-funded contract-token `wallet send` surfaces
/// the `transfer_insufficient_balance` scene with a
/// contract-token asset — `fundingNeed.tokenAddress` is the CA verbatim and
/// `fundingNeed.asset` is a non-empty symbol or the full CA fallback. The scene
/// also carries `sameNetworkRequired` / `gasFree` in `fundingTarget` (spec §2.5).
/// Validated when the scene fires; otherwise the auth gate is
/// confirmed. Example uses USDC on Ethereum (chain 1, a TBC-1 covered chain).
#[test]
fn wallet_send_contract_token_insufficient_balance_scene_or_login_required() {
    const USDC_ETH_CA: &str = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
    let output = run_with_retry(&[
        "wallet",
        "send",
        "--readable-amount",
        "10",
        "--recipient",
        "0x1234567890abcdef1234567890abcdef12345678",
        "--chain",
        "ethereum",
        "--contract-token",
        USDC_ETH_CA,
    ]);
    assert!(
        !output.status.success(),
        "wallet send must fail here (exit 1), got {:?}",
        output.status.code()
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let json: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);

    if json.pointer("/data/phase").and_then(|s| s.as_str()) == Some("funding_required")
        && json.pointer("/data/payload/operation").and_then(|s| s.as_str()) == Some("transfer")
    {
        assert_eq!(json["ok"], Value::Bool(false), "scene must be ok:false: {json}");
        assert!(
            json["data"]["payload"]["error"]["code"] == "10004"
                || json["data"]["payload"]["error"].get("code").is_none(),
            "contract-token scene must preserve code=10004 or leave absent backend code null: {json}"
        );
        // The token stays identified by its CA regardless of balance availability.
        assert_eq!(
            json["data"]["payload"]["fundingNeed"]["tokenAddress"]
                .as_str()
                .map(str::to_lowercase),
            Some(USDC_ETH_CA.to_string()),
            "contract-token asset.tokenAddress must echo the CA: {json}"
        );
        // Asset identifier is never empty — symbol when known, otherwise full CA.
        let symbol = &json["data"]["payload"]["fundingNeed"]["asset"];
        assert!(
            symbol.as_str().is_some_and(|s| !s.is_empty()),
            "contract-token fundingNeed.asset must be non-empty: {json}"
        );
        // Consumable scene flags are present as booleans (§2.5).
        assert!(
            json["data"]["payload"]["fundingTarget"]["sameNetworkRequired"].is_boolean(),
            "scene must project sameNetworkRequired: {json}"
        );
        assert!(
            json["data"]["payload"]["fundingTarget"]["gasFree"].is_boolean(),
            "scene must project gasFree: {json}"
        );
    } else {
        assert!(
            login_required(&stdout, &stderr),
            "expected transfer_insufficient_balance scene or a login-required failure\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}

// ── IT-012 — address-resolution failure fails cleanly (no partial contract) ───

/// IT-012 (live, error): when the target chain has no resolvable address for the
/// current account, Send fails with a clear error rather than emitting a partial
/// funding contract (spec §3.2 address-resolution-failure class). Drives a Solana
/// scene against an EVM-only wallet fixture. The CSV contract is `$.ok == false`,
/// exit 1 — asserted here; without a logged-in wallet the auth gate produces the
/// same structured `ok:false` / exit-1 shape.
#[test]
fn wallet_send_solana_address_resolution_failure_is_ok_false() {
    let output = run_with_retry(&[
        "wallet",
        "send",
        "--readable-amount",
        "5",
        "--recipient",
        "5FHwkrdxntdK24hgQU8qM9jXihqLc9C8bZF7pcaKFTdG",
        "--chain",
        "solana",
    ]);
    assert!(
        !output.status.success(),
        "wallet send must fail here (exit 1), got {:?}",
        output.status.code()
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let json: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);

    // Must be a structured ok:false envelope (address-resolution / auth failure),
    // and MUST NOT masquerade as a successful top-up funding contract.
    let ok_false = json.get("ok").and_then(|v| v.as_bool()) == Some(false);
    assert!(
        ok_false || login_required(&stdout, &stderr),
        "expected a structured ok:false failure (no partial funding contract)\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert_ne!(
        json.pointer("/data/phase").and_then(|s| s.as_str()),
        Some("funding_required"),
        "an address-resolution failure must not be turned into a top-up scene: {json}"
    );
}

// ── IT-013 — the standalone `wallet qrcode` subcommand is gone ────────────────

/// IT-013 (offline, error): the standalone `wallet qrcode` command was removed
/// entirely (spec §1.2 / §10.1) — the QR is now embedded in each business scene.
/// Invoking it now fails with a clap `unrecognized subcommand` usage error
/// (exit 2), matching the exit-2 usage class in spec §3.1. Deterministic, no
/// network, no login. (The generic unknown-subcommand guard lives in
/// `cli_command_layout.rs::wallet_unrecognized_subcommand_errors`; this row pins
/// the specific, previously-valid `qrcode` action.)
#[test]
fn wallet_qrcode_subcommand_removed_unrecognized() {
    onchainos()
        .args([
            "wallet",
            "qrcode",
            "--address",
            "0x1234567890abcdef1234567890abcdef12345678",
            "--format",
            "unicode",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unrecognized subcommand"));
}
