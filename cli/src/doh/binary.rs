use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Result};

use crate::home::onchainos_home;

use super::types::{DohBinaryResponse, DohChecksum, DohNode};

const BINARY_NAME: &str = "okx-pilot";

/// CDN sources tried in order. Each entry is a full base URL (no trailing slash).
/// Append `/{platform}/checksum.json` or `/{platform}/{binary}` to form the full URL.
///
/// Priority (mirrors okx-trade-mcp installer.ts CDN_SOURCES):
///   1. static.jingyunyilian.com  — primary CDN
///   2. static.okx.com            — OKX CDN
///   3. static.coinall.ltd        — Coinall CDN
///   4. okg-pub-hk OSS            — Aliyun OSS fallback (different path prefix)
const CDN_SOURCES: &[&str] = &[
    "https://static.jingyunyilian.com/upgradeapp/tools/pilot",
    "https://static.okx.com/upgradeapp/tools/pilot",
    "https://static.coinall.ltd/upgradeapp/tools/pilot",
    "https://okg-pub-hk.oss-cn-hongkong.aliyuncs.com/upgradeapp/tools/pilot",
];

/// Returns the platform-specific binary filename (adds .exe on Windows).
fn binary_filename() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        return "okx-pilot.exe";
    }
    #[cfg(not(target_os = "windows"))]
    {
        BINARY_NAME
    }
}

/// Returns the path to `~/.onchainos/bin/okx-pilot` (or `okx-pilot.exe` on Windows).
/// Overridable via `OKX_DOH_BINARY_PATH` env var.
pub fn binary_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("OKX_DOH_BINARY_PATH") {
        return Some(PathBuf::from(p));
    }
    onchainos_home()
        .ok()
        .map(|h| h.join("bin").join(binary_filename()))
}

/// Maps Rust compile target to CDN platform string.
#[allow(unreachable_code)]
fn cdn_platform() -> Option<&'static str> {
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    {
        return Some("darwin-arm64");
    }
    #[cfg(all(target_arch = "x86_64", target_os = "macos"))]
    {
        return Some("darwin-x64");
    }
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    {
        return Some("linux-x64");
    }
    #[cfg(all(target_arch = "aarch64", target_os = "linux"))]
    {
        return Some("linux-arm64");
    }
    #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
    {
        return Some("win32-x64");
    }
    None
}

/// Downloads and verifies the okx-pilot binary from CDN.
/// Tries each CDN source in order. For each source: fetches checksum.json,
/// downloads the binary, verifies sha256, then writes atomically.
pub async fn download_binary() -> Result<()> {
    let platform =
        cdn_platform().ok_or_else(|| anyhow::anyhow!("unsupported platform for doh binary"))?;

    let dest = binary_path().ok_or_else(|| anyhow::anyhow!("cannot determine binary path"))?;

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let filename = binary_filename();
    let mut last_err = String::new();

    for base in CDN_SOURCES {
        let checksum_url = format!("{base}/{platform}/checksum.json");
        let binary_url = format!("{base}/{platform}/{filename}");

        // Step 1: fetch checksum
        eprintln!("[doh] fetching checksum {checksum_url} ...");
        let checksum: DohChecksum = match client.get(&checksum_url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.json().await {
                Ok(c) => c,
                Err(e) => {
                    last_err = format!("{base}: checksum parse failed: {e}");
                    continue;
                }
            },
            Ok(resp) => {
                last_err = format!("{base}: checksum HTTP {}", resp.status());
                continue;
            }
            Err(e) => {
                last_err = format!("{base}: checksum fetch failed: {e}");
                continue;
            }
        };

        // Step 2: download binary
        eprintln!("[doh] downloading {binary_url} ...");
        let bytes = match client.get(&binary_url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                Ok(b) => b,
                Err(e) => {
                    last_err = format!("{base}: binary read failed: {e}");
                    continue;
                }
            },
            Ok(resp) => {
                last_err = format!("{base}: binary HTTP {}", resp.status());
                continue;
            }
            Err(e) => {
                last_err = format!("{base}: binary fetch failed: {e}");
                continue;
            }
        };

        // Step 3: verify sha256
        let actual = sha256_hex(&bytes);
        if actual != checksum.sha256 {
            last_err = format!(
                "{base}: sha256 mismatch: expected {}, got {}",
                checksum.sha256, actual
            );
            continue;
        }

        // Step 4: write atomically
        let tmp_path = dest.with_extension("tmp");
        std::fs::write(&tmp_path, &bytes)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))?;
        }

        std::fs::rename(&tmp_path, &dest)?;
        return Ok(());
    }

    bail!("all CDN sources failed, last error: {last_err}")
}

/// SHA-256 of bytes, returned as lowercase hex string.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(data);
    format!("{:x}", hash)
}

/// Executes the okx-pilot binary and parses the result.
/// Returns `None` on any error (binary missing, timeout, bad JSON, code != 0, empty ip).
pub async fn exec_doh_binary(
    domain: &str,
    exclude: &[String],
    user_agent: Option<&str>,
) -> Option<DohNode> {
    let bin = binary_path()?;
    if !bin.exists() {
        return None;
    }

    let domain = domain.to_string();
    let exclude = exclude.to_vec();
    let user_agent = user_agent.map(|s| s.to_string());

    let output = tokio::time::timeout(Duration::from_secs(30), async {
        let bin = bin.clone();
        let domain = domain.clone();
        let exclude = exclude.clone();
        let user_agent = user_agent.clone();
        tokio::task::spawn_blocking(move || {
            let mut cmd = std::process::Command::new(&bin);
            cmd.arg("--domain").arg(&domain);
            if !exclude.is_empty() {
                cmd.arg("--exclude").arg(exclude.join(","));
            }
            if let Some(ua) = &user_agent {
                cmd.arg("--user-agent").arg(ua);
            }
            cmd.output()
        })
        .await
    })
    .await
    .ok()?
    .ok()?
    .ok()?;

    if !output.status.success() {
        return None;
    }

    let resp: DohBinaryResponse = serde_json::from_slice(&output.stdout).ok()?;
    if resp.code != 0 || resp.data.ip.is_empty() {
        return None;
    }

    Some(DohNode {
        ip: resp.data.ip,
        host: resp.data.host,
        ttl: resp.data.ttl,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::TEST_ENV_MUTEX;

    #[test]
    fn binary_path_respects_env_override() {
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        std::env::set_var("OKX_DOH_BINARY_PATH", "/tmp/custom-doh");
        let path = binary_path().expect("should return Some");
        assert_eq!(path, PathBuf::from("/tmp/custom-doh"));
        std::env::remove_var("OKX_DOH_BINARY_PATH");
    }

    #[test]
    fn binary_path_default_under_onchainos_home() {
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        std::env::remove_var("OKX_DOH_BINARY_PATH");
        std::env::set_var("ONCHAINOS_HOME", "/tmp/test_onchainos_doh");
        let path = binary_path().expect("should return Some");
        assert_eq!(
            path,
            PathBuf::from(format!("/tmp/test_onchainos_doh/bin/{}", binary_filename()))
        );
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn cdn_platform_returns_some() {
        let platform = cdn_platform();
        assert!(
            platform.is_some(),
            "cdn_platform() should return Some on supported platforms"
        );
    }

    #[test]
    fn sha256_hex_known_value() {
        // echo -n "" | sha256sum → e3b0c44...
        let hash = sha256_hex(b"");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn cdn_sources_not_empty() {
        assert!(!CDN_SOURCES.is_empty());
    }
}
