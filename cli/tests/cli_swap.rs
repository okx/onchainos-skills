//! Integration tests for `onchainos swap` commands.
//!
//! Only read-only endpoints are tested (chains, liquidity, quote).
//! `swap` and `approve` are skipped as they generate real transaction data
//! and would require a valid wallet address.

mod common;

use common::{assert_ok_and_extract_data, fresh_home, onchainos, run_with_retry, tokens};
use predicates::prelude::*;

const VITALIK: &str = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045";

// ─── chains ─────────────────────────────────────────────────────────

#[test]
fn swap_chains_returns_supported_chains() {
    let output = run_with_retry(&["swap", "chains"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array of chains: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one chain");
    assert!(
        arr[0].get("chainIndex").is_some(),
        "chain entry missing 'chainIndex': {}",
        arr[0]
    );
}

// ─── liquidity ──────────────────────────────────────────────────────

#[test]
fn swap_liquidity_ethereum() {
    let output = run_with_retry(&["swap", "liquidity", "--chain", "ethereum"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array of DEX sources: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one liquidity source");
}

#[test]
fn swap_liquidity_solana() {
    let output = run_with_retry(&["swap", "liquidity", "--chain", "solana"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
}

#[test]
fn swap_liquidity_missing_chain_fails() {
    onchainos()
        .args(["swap", "liquidity"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── quote ──────────────────────────────────────────────────────────

#[test]
fn swap_quote_eth_to_usdc() {
    // Quote swapping 0.01 ETH (10^16 wei) to USDC on Ethereum
    let output = run_with_retry(&[
        "swap",
        "quote",
        "--from",
        tokens::EVM_NATIVE,
        "--to",
        tokens::ETH_USDC,
        "--amount",
        "10000000000000000",
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected quote data array: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one quote route");
}

#[test]
fn swap_quote_exact_out() {
    let output = run_with_retry(&[
        "swap",
        "quote",
        "--from",
        tokens::EVM_NATIVE,
        "--to",
        tokens::ETH_USDC,
        "--amount",
        "1000000",
        "--chain",
        "ethereum",
        "--swap-mode",
        "exactOut",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected quote data array: {data}");
}

#[test]
fn swap_quote_missing_required_args_fails() {
    onchainos()
        .args(["swap", "quote", "--from", tokens::EVM_NATIVE])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── approve (read-only calldata generation) ────────────────────────

#[test]
fn swap_approve_usdc_on_ethereum() {
    let output = run_with_retry(&[
        "swap",
        "approve",
        "--token",
        tokens::ETH_USDC,
        "--amount",
        "1000000",
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected approve data: {data}");
}

// ─── swap (read-only tx generation) ─────────────────────────────────

#[test]
fn swap_swap_eth_to_usdc_generates_tx_data() {
    let output = run_with_retry(&[
        "swap",
        "swap",
        "--from",
        tokens::EVM_NATIVE,
        "--to",
        tokens::ETH_USDC,
        "--amount",
        "10000000000000000",
        "--chain",
        "ethereum",
        "--slippage",
        "1",
        "--wallet",
        VITALIK,
        "--swap-mode",
        "exactIn",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected swap data: {data}");
}

#[test]
fn swap_swap_missing_required_args_fails() {
    onchainos()
        .args(["swap", "swap", "--from", tokens::EVM_NATIVE])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── X Layer Testnet (chainIndex 1952) integration tests ────────────
//
// These tests cover IT-001..IT-005 from the WWINFRA-3305 integration plan:
// validate that ensure_supported_chain accepts X Layer Testnet via both
// the "xlayer_test" alias and the raw "1952" chainIndex on a cold start
// (no chain_cache.json present), and rejects truly unknown chainIndices.
//
// The cold-start condition is forced by pointing ONCHAINOS_HOME at a fresh
// empty tempdir so the dynamic chain list cache cannot leak between runs;
// this exercises the offline SUPPORTED_CHAIN_INDICES constant fallback in
// ensure_supported_chain (chains.rs:17).
//
// NOTE on stdout vs stderr: onchainos prints structured errors to STDOUT as
// `{"ok": false, "error": "..."}` (output.rs:42) with exit code 1, not to
// stderr. The CSV plan's "stderr contains" hint is therefore satisfied via
// stdout here.

/// IT-001 — `swap liquidity --chain xlayer_test` succeeds via the alias on
/// cold start. The const-array fallback in ensure_supported_chain must
/// recognise 1952 before any dynamic cache is populated.
#[test]
fn swap_liquidity_xlayer_test_alias_cold_start_ensures_supported_chain() {
    let home = tempfile::tempdir().expect("create tempdir");
    let output = onchainos()
        .env("ONCHAINOS_HOME", home.path())
        .args(["swap", "liquidity", "--chain", "xlayer_test"])
        .output()
        .expect("failed to execute");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // The chain-validation step must not reject xlayer_test. Whether the
    // downstream liquidity-fetch succeeds depends on backend coverage for
    // X Layer Testnet on web3pre; we only assert that ensure_supported_chain
    // did not block us.
    assert!(
        !stdout.contains("unsupported chain") && !stderr.contains("unsupported chain"),
        "ensure_supported_chain rejected xlayer_test alias on cold start\nstdout: {stdout}\nstderr: {stderr}",
    );
}

/// IT-002 — `swap liquidity --chain 1952` succeeds via the raw chainIndex on
/// cold start. Same const-array fallback path as IT-001 but bypasses the
/// alias table.
#[test]
fn swap_liquidity_xlayer_testnet_raw_chain_index_ensures_supported_chain() {
    let home = tempfile::tempdir().expect("create tempdir");
    let output = onchainos()
        .env("ONCHAINOS_HOME", home.path())
        .args(["swap", "liquidity", "--chain", "1952"])
        .output()
        .expect("failed to execute");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !stdout.contains("unsupported chain") && !stderr.contains("unsupported chain"),
        "ensure_supported_chain rejected raw chainIndex 1952 on cold start\nstdout: {stdout}\nstderr: {stderr}",
    );
}

/// IT-003 — `swap quote --chain xlayer_test` passes chain validation. We use
/// the EVM native placeholder for both `--from` and `--to`; the quote
/// pricing step may legitimately fail (no route, no liquidity, identical
/// in/out token, etc.) but the failure must NOT be an "unsupported chain"
/// rejection. Exit code is intentionally not asserted.
#[test]
fn swap_quote_xlayer_test_alias_chain_validation_passes() {
    let home = tempfile::tempdir().expect("create tempdir");
    let output = onchainos()
        .env("ONCHAINOS_HOME", home.path())
        .args([
            "swap",
            "quote",
            "--chain",
            "xlayer_test",
            "--from",
            tokens::EVM_NATIVE,
            "--to",
            tokens::EVM_NATIVE,
            "--amount",
            "1000000",
        ])
        .output()
        .expect("failed to execute");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !stdout.contains("unsupported chain") && !stderr.contains("unsupported chain"),
        "ensure_supported_chain rejected xlayer_test alias on quote\nstdout: {stdout}\nstderr: {stderr}",
    );
}

/// IT-004 — `swap liquidity --chain 9999` is rejected at the
/// ensure_supported_chain gate with exit code 1 and an "unsupported chain"
/// error on stdout. This is the negative case proving the const-array
/// fallback rejects unknown indices in cold-start mode.
#[test]
fn swap_liquidity_unknown_chain_9999_rejected_with_unsupported_chain_error() {
    let home = tempfile::tempdir().expect("create tempdir");
    let output = onchainos()
        .env("ONCHAINOS_HOME", home.path())
        .args(["swap", "liquidity", "--chain", "9999"])
        .output()
        .expect("failed to execute");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit code 1 for unsupported chain\nstdout: {stdout}\nstderr: {stderr}",
    );
    // onchainos surfaces the error as JSON on stdout (output::error in
    // output.rs:42); accept either stream to remain robust if that ever
    // changes.
    assert!(
        stdout.contains("unsupported chain") || stderr.contains("unsupported chain"),
        "expected 'unsupported chain' in output for chain 9999\nstdout: {stdout}\nstderr: {stderr}",
    );
}

// IT-005 — covered by existing `swap_chains_returns_supported_chains` above
// (lines 16–28). That test asserts ok=true, that data is a non-empty array,
// and that entries carry a `chainIndex` field — a strict superset of IT-005's
// "stdout contains ok / exit 0" requirement. No new fn added.

// ─── SW2 always-on risk classification (WWINFRA-3509) ───────────────────
//
// Integration-plan rows IT-005 … IT-007. SW2 appends `action` (ok/warn/block)
// and `reason` to every route object in the quote/swap response (spec §2.4).
// These tests assert the wiring — that the `action` field is emitted on each
// route — not the honeypot/tax matrix values, which are unit-tested in
// `risk_classify.rs` (the tax branch is a documented TBC seam). All three are
// `live` golden rows on anonymous/AK read endpoints and go through the shared
// `run_with_retry` helper.

// Solana native-token placeholder used by the OKX aggregator (system program).
const SOL_NATIVE: &str = "11111111111111111111111111111111";

/// Assert the SW2 classification appended an `action` field to every route in a
/// non-empty `data` route array. SW2 appends `action` at the entry top level for
/// the `quote` shape, but under each entry's `routerResult` object for the `swap`
/// shape (see `swap.rs::swap_routes_mut`), so accept either nesting.
#[track_caller]
fn assert_routes_have_action(data: &serde_json::Value, label: &str) {
    let routes = data
        .as_array()
        .unwrap_or_else(|| panic!("{label}: expected a route array, got: {data}"));
    assert!(!routes.is_empty(), "{label}: expected at least one route: {data}");
    let has_action = |route: &serde_json::Value| {
        route.get("action").is_some()
            || route
                .get("routerResult")
                .and_then(|rr| rr.get("action"))
                .is_some()
    };
    assert!(
        routes.iter().all(has_action),
        "{label}: every route must carry an appended 'action' field (top level or under routerResult): {data}"
    );
}

/// IT-005 — `swap quote` (ETH→USDC on ethereum) appends a risk `action` to each
/// route.
#[test]
fn swap_quote_appends_risk_action_per_route() {
    let output = run_with_retry(&[
        "swap",
        "quote",
        "--from",
        tokens::EVM_NATIVE,
        "--to",
        tokens::ETH_USDC,
        "--amount",
        "10000000000000000",
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert_routes_have_action(&data, "IT-005 swap quote");
}

/// IT-006 — `swap swap` (read-only calldata preview, ETH→USDC on ethereum)
/// appends a risk `action` to each route.
#[test]
fn swap_swap_appends_risk_action_per_route() {
    let output = run_with_retry(&[
        "swap",
        "swap",
        "--from",
        tokens::EVM_NATIVE,
        "--to",
        tokens::ETH_USDC,
        "--amount",
        "10000000000000000",
        "--chain",
        "ethereum",
        "--slippage",
        "1",
        "--wallet",
        VITALIK,
        "--swap-mode",
        "exactIn",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert_routes_have_action(&data, "IT-006 swap swap");
}

/// IT-007 — SW2 is chain-independent: a `swap quote` on solana (native→USDC)
/// carries the same appended `action` field per route as EVM routes (spec §7).
#[test]
fn swap_quote_solana_appends_risk_action_per_route() {
    let output = run_with_retry(&[
        "swap",
        "quote",
        "--from",
        SOL_NATIVE,
        "--to",
        tokens::SOL_USDC,
        "--amount",
        "100000000",
        "--chain",
        "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert_routes_have_action(&data, "IT-007 swap quote solana");
}

// ─── Insufficient-balance top-up recovery + walletBalance (WWINFRA-3798) ─────
//
// Integration-plan rows IT-001, IT-002, IT-003, IT-005, IT-006, IT-010. The
// swap quote now reports the caller's wallet balance (spec §2.3) and, when the
// wallet holds less than the requested amount, degrades to a structured
// `swap_insufficient_balance` funding scene (spec §2.2).
//
// ── walletBalance placement (Stage-7 confirmation of the IT-005 CSV note) ──
// The real aggregator quote `data` is an ARRAY of routes, and the enhancement
// attaches `walletBalance` to EVERY route object (verified in
// `swap.rs::attach_wallet_balance*` unit tests) — NOT as a single `$.data`
// sibling. So the field lives at `$.data[N].walletBalance`. It is ALWAYS present
// and is JSON `null` (never `0`, never omitted) when the balance query fails or
// the wallet is not logged in (spec §2.3 / TBC[2]).
//
// ── Login dependency ──
// A `swap quote` is a read-only endpoint that works without a logged-in wallet
// (walletBalance is then `null`). The `swap_insufficient_balance` scene, by
// contrast, can only fire when a logged-in wallet reports a balance below the
// requested amount; IT-010 therefore validates the scene contract when reachable
// and otherwise accepts the normal-quote path. All rows are `live` and go through
// `run_with_retry` (IT-005 uses a home-isolated variant — see below).

/// True when every route in an array-shaped `data` (or the object itself, for the
/// object-shaped fallback) carries a `walletBalance` field. Robust to both shapes
/// the enhancement supports without asserting a specific route count.
#[track_caller]
fn each_route_has_wallet_balance(data: &serde_json::Value) -> bool {
    match data {
        serde_json::Value::Array(routes) => {
            !routes.is_empty() && routes.iter().all(|r| r.get("walletBalance").is_some())
        }
        serde_json::Value::Object(_) => data.get("walletBalance").is_some(),
        _ => false,
    }
}

/// True when every route's `walletBalance` is present AND JSON `null` (the
/// not-logged-in / balance-unavailable representation, spec §2.3).
#[track_caller]
fn wallet_balance_all_null(data: &serde_json::Value) -> bool {
    let is_null = |v: &serde_json::Value| v.get("walletBalance").map(|b| b.is_null()) == Some(true);
    match data {
        serde_json::Value::Array(routes) => !routes.is_empty() && routes.iter().all(is_null),
        serde_json::Value::Object(_) => is_null(data),
        _ => false,
    }
}

/// Run a `swap quote` in an isolated (guaranteed logged-out) `ONCHAINOS_HOME`
/// with the same retry-on-rate-limit contract as `common::run_with_retry`.
///
/// A dedicated wrapper is required because `run_with_retry` builds its own
/// `Command` with no way to pin a per-test home, and IT-005 must prove the
/// *not-logged-in* balance representation deterministically regardless of any
/// login fixture present in the ambient home. Only `ONCHAINOS_HOME` is
/// overridden; the inherited `OKX_*` API credentials are kept so the read-only
/// quote still authenticates.
fn run_quote_isolated_home(home: &std::path::Path, args: &[&str]) -> std::process::Output {
    for attempt in 0..3 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs(attempt));
        }
        let output = onchainos()
            .env("ONCHAINOS_HOME", home)
            .args(args)
            .output()
            .expect("failed to execute");
        if output.status.success() {
            return output;
        }
        if !String::from_utf8_lossy(&output.stdout).contains("Rate limited") {
            return output;
        }
    }
    onchainos()
        .env("ONCHAINOS_HOME", home)
        .args(args)
        .output()
        .expect("failed to execute")
}

/// IT-001 — a normal ETH→USDC quote on Ethereum reports `walletBalance` on each
/// route (spec §2.3). Extends `swap_quote_eth_to_usdc` with the balance check
/// rather than duplicating the base quote assertions.
#[test]
fn swap_quote_eth_to_usdc_reports_wallet_balance() {
    let output = run_with_retry(&[
        "swap",
        "quote",
        "--from",
        tokens::EVM_NATIVE,
        "--to",
        tokens::ETH_USDC,
        "--amount",
        "10000000000000000",
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        each_route_has_wallet_balance(&data),
        "quote must report walletBalance per route: {data}"
    );
}

/// IT-002 — per-chain coverage: the same walletBalance enhancement is present on
/// a Solana (WSOL→USDC) quote, confirming it is chain-agnostic (spec §7).
#[test]
fn swap_quote_solana_reports_wallet_balance() {
    let output = run_with_retry(&[
        "swap",
        "quote",
        "--from",
        tokens::SOL_WSOL,
        "--to",
        tokens::SOL_USDC,
        "--amount",
        "100000000",
        "--chain",
        "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        each_route_has_wallet_balance(&data),
        "solana quote must report walletBalance per route: {data}"
    );
}

/// IT-003 — per-chain coverage on X Layer (the gas-free chain). The `gasFree`
/// flag is an internal display hint and is deliberately NOT asserted (spec §2.5).
/// X Layer route availability is backend-dependent, so the walletBalance contract
/// is asserted when the quote succeeds and a benign no-route / no-liquidity
/// failure is tolerated (still a structured `ok:false` envelope).
#[test]
fn swap_quote_xlayer_reports_wallet_balance() {
    // X Layer USDT.
    const XLAYER_USDT: &str = "0x1E4a5963aBFD975d8c9021ce480b42188849D41d";
    let output = run_with_retry(&[
        "swap",
        "quote",
        "--from",
        tokens::EVM_NATIVE,
        "--to",
        XLAYER_USDT,
        "--amount",
        "1000000000000000000",
        "--chain",
        "xlayer",
    ]);
    if output.status.success() {
        let data = assert_ok_and_extract_data(&output);
        assert!(
            each_route_has_wallet_balance(&data),
            "xlayer quote must report walletBalance per route: {data}"
        );
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null);
        assert_eq!(
            json.get("ok"),
            Some(&serde_json::Value::Bool(false)),
            "xlayer quote must succeed with walletBalance or fail as a structured ok:false envelope\nstdout: {stdout}"
        );
    }
}

/// IT-005 — not-logged-in edge: in a fresh, logged-out sandbox the read-only
/// quote still succeeds and every route reports `walletBalance` as JSON `null`
/// (never `0`, never omitted, never a string sentinel — spec §2.3 / TBC[2]).
#[test]
fn swap_quote_not_logged_in_wallet_balance_is_null() {
    let (_guard, home) = fresh_home("cli_swap_walletbalance_null");
    let output = run_quote_isolated_home(
        &home,
        &[
            "swap",
            "quote",
            "--from",
            tokens::ETH_USDC,
            "--to",
            tokens::EVM_NATIVE,
            "--amount",
            "1000000",
            "--chain",
            "ethereum",
        ],
    );
    let data = assert_ok_and_extract_data(&output);
    assert!(
        wallet_balance_all_null(&data),
        "not-logged-in quote must report walletBalance=null on every route: {data}"
    );
}

/// IT-006 — flag-variant edge: `--readable-amount` (instead of `--amount`) still
/// produces a normal quote that reports `walletBalance` (spec §2.2). exactIn is
/// the default swap mode and is passed explicitly here to mirror the plan row.
#[test]
fn swap_quote_readable_amount_reports_wallet_balance() {
    let output = run_with_retry(&[
        "swap",
        "quote",
        "--from",
        tokens::EVM_NATIVE,
        "--to",
        tokens::ETH_USDC,
        "--readable-amount",
        "0.01",
        "--chain",
        "ethereum",
        "--swap-mode",
        "exactIn",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        each_route_has_wallet_balance(&data),
        "readable-amount quote must report walletBalance per route: {data}"
    );
}

/// IT-010 — swap insufficient-balance scene: a quote for more than the wallet
/// holds surfaces the flat top-level `swap_insufficient_balance` object with the
/// common Funding target, QR, need, and `fund_account` action (spec §2.2,
/// exit 1). This requires a logged-in wallet whose balance
/// is below the requested amount; without one the CLI cannot detect a shortfall
/// and returns a normal quote (exit 0) whose routes carry `walletBalance`. The
/// scene contract is validated when it fires; otherwise the normal-quote /
/// structured-failure path is accepted.
#[test]
fn swap_quote_insufficient_balance_scene_or_normal_quote() {
    let output = run_with_retry(&[
        "swap",
        "quote",
        "--from",
        tokens::EVM_NATIVE,
        "--to",
        tokens::ETH_USDC,
        "--amount",
        "100000000000000000000",
        "--chain",
        "ethereum",
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null);

    if json.get("scene").and_then(|s| s.as_str()) == Some("swap_insufficient_balance") {
        // CSV IT-010 is an `error` row with exit_code=1; the scene path exits 1
        // (spec §3.1 / §3.2). Assert it here so the scene contract also pins the
        // process outcome, not just the JSON body.
        assert_eq!(
            output.status.code(),
            Some(1),
            "swap_insufficient_balance scene must exit 1: {json}"
        );
        assert_eq!(json["ok"], serde_json::Value::Bool(false), "scene must be ok:false: {json}");
        assert!(
            json.get("fundingAddress").is_some(),
            "swap_insufficient_balance must carry fundingAddress: {json}"
        );
        assert_eq!(
            json["nextAction"],
            serde_json::json!([{"id": "fund_account", "recommend": true, "params": {}}]),
            "scene must expose only the common funding action: {json}"
        );
        assert!(
            json["payload"]["fundingTarget"].is_object()
                && json["payload"]["qr"].is_object()
                && json["payload"]["fundingNeed"].is_object(),
            "scene must carry the common funding payload: {json}"
        );
        assert_eq!(json["nextAction"][0]["params"], serde_json::json!({}));
    } else if output.status.success() {
        let data = assert_ok_and_extract_data(&output);
        assert!(
            each_route_has_wallet_balance(&data),
            "normal quote must report walletBalance per route: {data}"
        );
    } else {
        assert_eq!(
            json.get("ok"),
            Some(&serde_json::Value::Bool(false)),
            "expected swap_insufficient_balance scene, a walletBalance quote, or a structured ok:false failure\nstdout: {stdout}"
        );
    }
}
