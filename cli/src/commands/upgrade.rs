use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::output;

const REPO: &str = "okx/onchainos-skills";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Known locations where onchainos-skills may be installed as a git checkout.
/// Paths are relative to the user's home directory.
const SKILL_INSTALL_PATHS: &[&str] = &[
    ".codex/onchainos-skills",
    ".openclaw/onchainos-skills",
    ".cursor/onchainos-skills",
    ".opencode/onchainos-skills",
];

#[derive(clap::Args)]
pub struct UpgradeArgs {
    /// Include pre-release (beta) versions
    #[arg(long)]
    pub beta: bool,

    /// Upgrade even if already on the latest version
    #[arg(long)]
    pub force: bool,

    /// Only check for a newer version, do not install
    #[arg(long)]
    pub check: bool,

    /// Skip skill checkout updates (only refresh the CLI binary)
    #[arg(long)]
    pub skip_skills: bool,
}

// ── Version comparison ──────────────────────────────────────────────────

/// Returns true if `a` is strictly newer than `b` (semver, with pre-release support).
fn semver_gt(a: &str, b: &str) -> bool {
    fn parse(s: &str) -> (u32, u32, u32, Option<u32>) {
        let (base, pre) = match s.splitn(2, '-').collect::<Vec<_>>()[..] {
            [b, p] => (b, Some(p)),
            [b] => (b, None),
            _ => return (0, 0, 0, None),
        };
        let parts: Vec<u32> = base.split('.').map(|x| x.parse().unwrap_or(0)).collect();
        let pre_num = pre.and_then(|p| {
            p.chars()
                .filter(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()
        });
        (
            parts.first().copied().unwrap_or(0),
            parts.get(1).copied().unwrap_or(0),
            parts.get(2).copied().unwrap_or(0),
            pre_num,
        )
    }

    let (maj_a, min_a, pat_a, pre_a) = parse(a);
    let (maj_b, min_b, pat_b, pre_b) = parse(b);

    if maj_a != maj_b {
        return maj_a > maj_b;
    }
    if min_a != min_b {
        return min_a > min_b;
    }
    if pat_a != pat_b {
        return pat_a > pat_b;
    }

    match (pre_a, pre_b) {
        (None, None) => false,           // equal
        (None, Some(_)) => true,         // stable > pre-release
        (Some(_), None) => false,        // pre-release < stable
        (Some(na), Some(nb)) => na > nb, // higher pre-release number wins
    }
}

// ── GitHub API ──────────────────────────────────────────────────────────

async fn get_latest_stable(client: &Client) -> Result<String> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", REPO);
    let resp: Value = client
        .get(&url)
        .header("User-Agent", "onchainos-cli")
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("failed to fetch latest release from GitHub")?
        .json()
        .await
        .context("failed to parse GitHub release response")?;

    resp["tag_name"]
        .as_str()
        .map(|t| t.trim_start_matches('v').to_string())
        .context("missing tag_name in GitHub release response")
}

async fn get_latest_with_beta(client: &Client) -> Result<String> {
    let url = format!("https://api.github.com/repos/{}/tags?per_page=100", REPO);
    let resp: Value = client
        .get(&url)
        .header("User-Agent", "onchainos-cli")
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("failed to fetch tags from GitHub")?
        .json()
        .await
        .context("failed to parse GitHub tags response")?;

    let tags = resp.as_array().context("expected array from tags API")?;
    let mut best: Option<String> = None;

    for tag in tags {
        let name = tag["name"]
            .as_str()
            .unwrap_or("")
            .trim_start_matches('v')
            .to_string();
        if name.is_empty() {
            continue;
        }
        match &best {
            None => best = Some(name),
            Some(b) if semver_gt(&name, b) => best = Some(name),
            _ => {}
        }
    }

    best.context("no valid versions found in GitHub tags")
}

// ── Platform detection ──────────────────────────────────────────────────

#[allow(unreachable_code)]
fn target_triple() -> Result<&'static str> {
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return Ok("x86_64-apple-darwin");
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return Ok("aarch64-apple-darwin");
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return Ok("x86_64-unknown-linux-gnu");
    #[cfg(all(target_os = "linux", target_arch = "x86"))]
    return Ok("i686-unknown-linux-gnu");
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return Ok("aarch64-unknown-linux-gnu");
    #[cfg(all(target_os = "linux", target_arch = "arm"))]
    return Ok("armv7-unknown-linux-gnueabihf");
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return Ok("x86_64-pc-windows-msvc");
    bail!(
        "unsupported platform — please install manually from https://github.com/{}",
        REPO
    )
}

// ── Download + verify + install ─────────────────────────────────────────

async fn download_and_install(client: &Client, version: &str) -> Result<()> {
    let triple = target_triple()?;
    let binary_name = format!("onchainos-{}", triple);
    let tag = format!("v{}", version);

    let binary_url = format!(
        "https://github.com/{}/releases/download/{}/{}",
        REPO, tag, binary_name
    );
    let checksums_url = format!(
        "https://github.com/{}/releases/download/{}/checksums.txt",
        REPO, tag
    );

    eprintln!("Fetching checksums...");
    let checksums = client
        .get(&checksums_url)
        .header("User-Agent", "onchainos-cli")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .context("failed to download checksums.txt")?
        .text()
        .await?;

    let expected_hash = checksums
        .lines()
        .find(|l| l.contains(&binary_name))
        .and_then(|l| l.split_whitespace().next())
        .context("checksum not found for this platform in checksums.txt")?
        .to_string();

    eprintln!("Downloading {} {}...", binary_name, tag);
    let bytes = client
        .get(&binary_url)
        .header("User-Agent", "onchainos-cli")
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .context("failed to download binary")?
        .bytes()
        .await
        .context("failed to read binary bytes")?;

    // SHA-256 verification
    let actual_hash = hex::encode(Sha256::digest(&bytes));
    if actual_hash != expected_hash {
        bail!(
            "checksum mismatch — binary may have been tampered with\n  expected: {}\n  actual:   {}",
            expected_hash,
            actual_hash
        );
    }
    eprintln!("Checksum verified.");

    // Atomic replace: write to <exe>.tmp then rename
    let exe_path = std::env::current_exe().context("failed to resolve current executable path")?;
    // Follow symlinks to get the real binary path
    let exe_path = exe_path.canonicalize().unwrap_or(exe_path);
    let tmp_path = exe_path.with_extension("tmp");

    std::fs::write(&tmp_path, &bytes).context("failed to write temporary binary")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))
            .context("failed to set executable permission")?;
    }

    std::fs::rename(&tmp_path, &exe_path).context("failed to replace binary")?;

    Ok(())
}

// ── Skill checkout updates ──────────────────────────────────────────────

fn run_git(cwd: &Path, args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("git").arg("-C").arg(cwd).args(args).output()
}

fn remote_belongs_to_repo(path: &Path) -> bool {
    match run_git(path, &["remote", "get-url", "origin"]) {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).contains("onchainos-skills")
        }
        _ => false,
    }
}

/// Resolve the set of candidate skill checkout paths under $HOME.
fn discover_skill_paths() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return vec![];
    };
    SKILL_INSTALL_PATHS
        .iter()
        .map(|rel| home.join(rel))
        .filter(|p| p.exists())
        .collect()
}

/// Update each detected skill checkout via `git pull --ff-only`. Returns one
/// result per path describing the outcome.
fn update_skill_checkouts() -> Vec<Value> {
    let mut results = Vec::new();

    if Command::new("git").arg("--version").output().is_err() {
        for path in discover_skill_paths() {
            results.push(json!({
                "path": path.display().to_string(),
                "status": "skipped",
                "reason": "git not found on PATH",
            }));
        }
        return results;
    }

    for path in discover_skill_paths() {
        let path_str = path.display().to_string();

        if !path.join(".git").exists() {
            results.push(json!({
                "path": path_str,
                "status": "skipped",
                "reason": "not a git checkout — update via your package manager",
            }));
            continue;
        }

        if !remote_belongs_to_repo(&path) {
            results.push(json!({
                "path": path_str,
                "status": "skipped",
                "reason": "remote 'origin' does not point to onchainos-skills",
            }));
            continue;
        }

        eprintln!("Updating skills at {}...", path_str);
        match run_git(&path, &["pull", "--ff-only"]) {
            Ok(out) if out.status.success() => {
                results.push(json!({
                    "path": path_str,
                    "status": "updated",
                }));
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                eprintln!(
                    "Warning: git pull failed at {} — {}",
                    path_str,
                    if stderr.is_empty() { "non-zero exit" } else { &stderr }
                );
                results.push(json!({
                    "path": path_str,
                    "status": "failed",
                    "reason": stderr,
                }));
            }
            Err(e) => {
                results.push(json!({
                    "path": path_str,
                    "status": "failed",
                    "reason": e.to_string(),
                }));
            }
        }
    }

    results
}

// ── Command entry point ─────────────────────────────────────────────────

pub async fn execute(args: UpgradeArgs) -> Result<()> {
    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;

    let current = CURRENT_VERSION;

    // --check requires the latest version; bail if unreachable.
    if args.check {
        let latest = if args.beta {
            get_latest_with_beta(&client).await?
        } else {
            get_latest_stable(&client).await?
        };
        let update_available = semver_gt(&latest, current);
        output::success(json!({
            "currentVersion": current,
            "latestVersion": latest,
            "updateAvailable": update_available,
            "channel": if args.beta { "beta" } else { "stable" },
        }));
        return Ok(());
    }

    // For a full upgrade, try the GitHub API but degrade gracefully — skill
    // checkouts can still fast-forward via their own git remotes even when
    // api.github.com is unreachable.
    let latest_result = if args.beta {
        get_latest_with_beta(&client).await
    } else {
        get_latest_stable(&client).await
    };

    let (installed_version, binary_status) = match latest_result {
        Ok(latest) => {
            let needs_upgrade = args.force || semver_gt(&latest, current);
            if needs_upgrade {
                eprintln!("Upgrading onchainos: {} → {}", current, latest);
                download_and_install(&client, &latest).await?;
                (latest, "upgraded")
            } else {
                (latest, "already_latest")
            }
        }
        Err(e) => {
            eprintln!(
                "Warning: could not check for CLI binary updates: {}",
                e.root_cause()
            );
            eprintln!("Proceeding with skill checkout updates.");
            (current.to_string(), "binary_check_failed")
        }
    };

    let skills = if args.skip_skills {
        Vec::new()
    } else {
        update_skill_checkouts()
    };

    output::success(json!({
        "previousVersion": current,
        "installedVersion": installed_version,
        "status": binary_status,
        "skills": skills,
    }));

    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::semver_gt;

    #[test]
    fn stable_newer_than_older_stable() {
        assert!(semver_gt("2.1.0", "2.0.0"));
        assert!(semver_gt("3.0.0", "2.9.9"));
        assert!(!semver_gt("2.0.0", "2.1.0"));
    }

    #[test]
    fn stable_newer_than_same_base_prerelease() {
        assert!(semver_gt("2.0.0", "2.0.0-beta.5"));
        assert!(!semver_gt("2.0.0-beta.5", "2.0.0"));
    }

    #[test]
    fn higher_prerelease_number_wins() {
        assert!(semver_gt("2.0.0-beta.1", "2.0.0-beta.0"));
        assert!(!semver_gt("2.0.0-beta.0", "2.0.0-beta.1"));
    }

    #[test]
    fn equal_versions_not_gt() {
        assert!(!semver_gt("2.0.0", "2.0.0"));
        assert!(!semver_gt("2.0.0-beta.0", "2.0.0-beta.0"));
    }
}
