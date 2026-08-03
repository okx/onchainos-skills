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

// ── Isolated ONCHAINOS_HOME sandbox helpers ────────────────────────────────
//
// Shared by the persistence-refactor integration files (`cli_wallet_persistence`,
// `cli_ws`, and the offline `market` rows). Sandboxes live under
// `cli/target/test_tmp/<stem>/<unique>` — NOT `tempfile::tempdir()`, whose
// `/var/folders/.../T/` target is denied write by CI/agent sandboxes.

/// Per-test sandbox directory that removes itself on drop.
pub struct TestHome {
    path: std::path::PathBuf,
}

impl TestHome {
    /// Absolute path of the sandbox directory.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        // Best-effort: a test may have chmod'd the dir; restore write so the
        // recursive remove can succeed, then ignore any residual error.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o700));
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

static SANDBOX_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Build a fresh, isolated sandbox directory under
/// `cli/target/test_tmp/<stem>/<pid>-<nanos>-<counter>` and return an RAII guard
/// plus its path. The uniqueness suffix (`process::id + nanos + AtomicU64`)
/// keeps parallel tests in the same binary from colliding.
pub fn fresh_home(stem: &str) -> (TestHome, std::path::PathBuf) {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join(stem);
    std::fs::create_dir_all(&base).expect("create test_tmp base");
    let pid = std::process::id();
    let n = SANDBOX_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = base.join(format!("{pid}-{ts}-{n}"));
    std::fs::create_dir_all(&dir).expect("create per-test sandbox dir");
    (TestHome { path: dir.clone() }, dir)
}

/// Strip inherited `OKX_*` / `ONCHAINOS_HOME` / `OKX_DOH_BINARY_PATH` env so each
/// test sees a pristine environment, then pin `ONCHAINOS_HOME` to `home`
/// **per-invocation** via `Command::env` (never `std::env::set_var`, which is
/// process-global and races across parallel tests). The caller re-sets only what
/// the specific case needs after this.
pub fn scrubbed<'a>(
    cmd: &'a mut assert_cmd::Command,
    home: &std::path::Path,
) -> &'a mut assert_cmd::Command {
    cmd.env_remove("OKX_API_KEY")
        .env_remove("OKX_ACCESS_KEY")
        .env_remove("OKX_SECRET_KEY")
        .env_remove("OKX_PASSPHRASE")
        .env_remove("OKX_BASE_URL")
        .env_remove("OKX_DOH_BINARY_PATH")
        .env("ONCHAINOS_HOME", home)
}

/// Parse `stdout` as JSON. Panics with the raw stdout/stderr on parse failure so
/// a non-JSON crash is legible instead of an opaque serde error.
pub fn parse_stdout_json(output: &std::process::Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!("invalid JSON in stdout: {e}\nstdout: {stdout}\nstderr: {stderr}")
    })
}

/// Unix permission bits (`mode & 0o777`) of a file. Unix-only — permission
/// assertions are `#[cfg(unix)]` at every call site (spec §11 #3: Windows is a
/// documented no-op).
#[cfg(unix)]
pub fn file_mode(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .unwrap_or_else(|e| panic!("stat {}: {e}", path.display()))
        .permissions()
        .mode()
        & 0o777
}
