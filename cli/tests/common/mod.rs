//! Shared test helpers for onchainos CLI integration tests.

#![allow(dead_code)]

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

pub mod tokens {
    // EVM native token placeholder used by OKX APIs
    pub const EVM_NATIVE: &str = "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    // USDC on Ethereum
    pub const ETH_USDC: &str = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
    // WETH on Ethereum
    pub const ETH_WETH: &str = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";
    // Wrapped SOL on Solana (for market data; swaps use native address)
    pub const SOL_WSOL: &str = "So11111111111111111111111111111111111111112";
    // BONK on Solana — high-volume, non-launchpad token
    pub const SOL_BONK: &str = "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263";
    // USDC on Solana
    pub const SOL_USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    // Ethereum vitalik.eth — well-known wallet for portfolio/analysis tests
    pub const ETH_VITALIK: &str = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045";
}

/// Build a `Command` for the `onchainos` binary.
pub fn onchainos() -> assert_cmd::Command {
    cargo_bin_cmd!("onchainos")
}

/// Parse stdout as JSON, assert `ok: true`, and return the `data` field.
pub fn assert_ok_and_extract_data(output: &std::process::Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "command failed (exit={:?})\nstdout: {stdout}\nstderr: {stderr}",
        output.status.code(),
    );

    let json: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("invalid JSON in stdout: {e}\nraw: {stdout}"));

    assert_eq!(
        json["ok"],
        Value::Bool(true),
        "API returned ok=false: {}",
        json
    );
    assert!(
        json.get("data").is_some(),
        "response missing 'data' field: {}",
        json
    );

    json["data"].clone()
}

/// Run a command with up to 3 retries on rate-limit (exit code 1 + "Rate limited").
pub fn run_with_retry(args: &[&str]) -> std::process::Output {
    for attempt in 0..3 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs(attempt));
        }
        let output = onchainos().args(args).output().expect("failed to execute");

        if output.status.success() {
            return output;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.contains("Rate limited") {
            return output;
        }
    }
    onchainos().args(args).output().expect("failed to execute")
}

/// Extract a list of items from either a flat array or an object whose body
/// carries the list under one of the common wrapper keys (`list` / `data` /
/// `items` / `signals`). Keeping one extractor shared across signal and token
/// tests means a new wrapper shape is a one-line change, not a sweep.
pub fn extract_items(data: &Value) -> Vec<Value> {
    if let Some(arr) = data.as_array() {
        return arr.clone();
    }
    for key in ["list", "data", "items", "signals"] {
        if let Some(arr) = data.get(key).and_then(|v| v.as_array()) {
            return arr.clone();
        }
    }
    Vec::new()
}

/// Assert that the response carries at most `limit` items, accepting either
/// a flat array or a `{ list/data/items/signals: [...] }` wrapper.
///
/// If the response is an object with no recognised list key (e.g. an empty
/// envelope), the bound is vacuously satisfied — we only require the shape
/// to be array-or-object. This keeps tests consistent across endpoints that
/// sometimes return bare arrays and sometimes return wrapped lists.
///
/// For fixtures that are known to always return data, prefer
/// `assert_limit_non_empty` so a silent backend regression (empty list under
/// `--limit N`) is a hard failure rather than a vacuous pass.
pub fn assert_limit(data: &Value, limit: usize, label: &str) {
    let items = extract_items(data);
    if items.is_empty() {
        assert!(
            data.is_array() || data.is_object(),
            "expected array or object for {label}: {data}"
        );
        return;
    }
    assert!(
        items.len() <= limit,
        "expected at most {limit} {label}, got {}",
        items.len()
    );
}

/// Like `assert_limit`, but requires the extracted list to be non-empty.
///
/// Use for fixtures that are known to always return data (e.g. USDC holders
/// on ethereum, WSOL top traders on solana, USDC cross-chain search, default
/// hot tokens). With the lenient variant, an empty list under `--limit N`
/// silently passes — a regression that ignores `--limit` would not be caught.
/// This variant hard-fails the test so the assertion actually proves the
/// page-size bound is being applied.
pub fn assert_limit_non_empty(data: &Value, limit: usize, label: &str) {
    let items = extract_items(data);
    assert!(
        !items.is_empty(),
        "expected non-empty {label} — fixture must return > 0 rows to prove --limit is applied; got: {data}"
    );
    assert!(
        items.len() <= limit,
        "expected at most {limit} {label}, got {}",
        items.len()
    );
}
