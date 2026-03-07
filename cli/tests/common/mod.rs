//! Shared test helpers for onchainos CLI integration tests.

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

#[allow(dead_code)]
pub mod tokens {
    // EVM native token placeholder used by OKX APIs
    pub const EVM_NATIVE: &str = "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    // USDC on Ethereum
    pub const ETH_USDC: &str = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
    // WETH on Ethereum
    pub const ETH_WETH: &str = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";
    // Wrapped SOL on Solana (for market data; swaps use native address)
    pub const SOL_WSOL: &str = "So11111111111111111111111111111111111111112";
}

/// Build a `Command` for the `onchainos` binary.
pub fn onchainos() -> assert_cmd::Command {
    assert_cmd::Command::from(cargo_bin_cmd!("onchainos"))
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
