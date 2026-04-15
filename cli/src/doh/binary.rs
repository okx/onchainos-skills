use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Result};

use crate::home::onchainos_home;

use super::types::{DohBinaryResponse, DohNode};

const BINARY_NAME: &str = "okx-pilot";

const CDN_SOURCES: &[&str] = &[
    "https://static.okx.com/upgradeapp/doh",
    "https://pcdoh.qcxex.com/upgradeapp/doh",
    "https://static.coinall.ltd/upgradeapp/doh",
];

/// Returns the path to `~/.onchainos/bin/okx-pilot`.
/// Overridable via `OKX_DOH_BINARY_PATH` env var.
pub fn binary_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("OKX_DOH_BINARY_PATH") {
        return Some(PathBuf::from(p));
    }
    onchainos_home()
        .ok()
        .map(|h| h.join("bin").join(BINARY_NAME))
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
    #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
    {
        return Some("win32-x64");
    }
    None
}

/// Downloads the okx-pilot binary from CDN.
/// Tries multiple CDN sources in order.
pub async fn download_binary() -> Result<()> {
    let platform = cdn_platform()
        .ok_or_else(|| anyhow::anyhow!("unsupported platform for doh binary"))?;

    let dest = binary_path()
        .ok_or_else(|| anyhow::anyhow!("cannot determine binary path"))?;

    // Ensure parent directory exists
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let mut last_err = None;
    for base in CDN_SOURCES {
        let url = format!("{base}/{platform}/{BINARY_NAME}");
        eprintln!("[doh] downloading {url} ...");

        match client.get(&url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    let bytes = resp.bytes().await?;
                    let tmp_path = dest.with_extension("tmp");
                    std::fs::write(&tmp_path, &bytes)?;

                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(
                            &tmp_path,
                            std::fs::Permissions::from_mode(0o755),
                        )?;
                    }

                    std::fs::rename(&tmp_path, &dest)?;
                    return Ok(());
                }
                last_err = Some(anyhow::anyhow!(
                    "CDN returned status {} for {url}",
                    resp.status()
                ));
            }
            Err(e) => {
                last_err = Some(anyhow::anyhow!("request failed for {url}: {e}"));
            }
        }
    }

    match last_err {
        Some(e) => bail!("all CDN sources failed, last error: {e}"),
        None => bail!("all CDN sources failed"),
    }
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
            PathBuf::from("/tmp/test_onchainos_doh/bin/okx-pilot")
        );
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn cdn_platform_returns_some() {
        // This test runs on a supported CI/dev platform (macOS or Linux x86_64/aarch64).
        let platform = cdn_platform();
        assert!(
            platform.is_some(),
            "cdn_platform() should return Some on supported platforms"
        );
    }
}
