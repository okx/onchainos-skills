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

/// Known locations where onchainos-skills may be installed as a single
/// monorepo git checkout. Paths are relative to the user's home directory.
const SKILL_INSTALL_PATHS: &[&str] = &[
    ".codex/onchainos-skills",
    ".openclaw/onchainos-skills",
    ".cursor/onchainos-skills",
    ".config/opencode/onchainos-skills",
    ".claude/onchainos-skills",
];

/// Parent directories where individual skills may be installed as their own
/// git checkouts (per-skill installer topology). For each entry, every
/// immediate child directory is treated as a candidate skill checkout.
const SKILL_HOME_DIRS: &[&str] = &[
    ".agents/skills",
    ".claude/skills",
    ".codex/skills",
    ".openclaw/skills",
    ".cursor/skills",
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

// Both lookups below avoid api.github.com on the primary path (the 60/hr
// unauthenticated limit) and only fall back to it — honoring $GITHUB_TOKEN —
// when the primary path fails. This mirrors the install.sh / install.ps1 logic.

/// Fetch the latest stable version.
///
/// Primary path follows the `releases/latest` redirect, served by the
/// github.com website backend, which does NOT count against the api.github.com
/// rate limit. Falls back to the releases API if the redirect can't be resolved
/// to a `/releases/tag/v<semver>` page.
async fn get_latest_stable(client: &Client) -> Result<String> {
    if let Some(ver) = latest_stable_via_redirect(client).await {
        return Ok(ver);
    }

    let url = format!("https://api.github.com/repos/{}/releases/latest", REPO);
    let resp: Value = with_github_token(
        client
            .get(&url)
            .header("User-Agent", "onchainos-cli")
            .timeout(Duration::from_secs(10)),
    )
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

/// HEAD the `releases/latest` URL and read the final (post-redirect) URL.
/// Returns `None` — so the caller falls back to the API — on any network error
/// or if the final URL is not a `/releases/tag/v<semver>` page.
async fn latest_stable_via_redirect(client: &Client) -> Option<String> {
    let url = format!("https://github.com/{}/releases/latest", REPO);
    let resp = client
        .head(&url)
        .header("User-Agent", "onchainos-cli")
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    parse_release_tag_url(resp.url().as_str())
}

/// Fetch the latest version including betas.
///
/// Primary path lists tags via `git ls-remote` (git smart-http), which does NOT
/// count against the api.github.com rate limit. Falls back to the tags API if
/// git is unavailable or fails. Returns the highest by semver (pre-releases rank
/// below their base version).
async fn get_latest_with_beta(client: &Client) -> Result<String> {
    let mut versions = ls_remote_tag_versions();
    if versions.is_empty() {
        versions = api_tag_versions(client).await?;
    }
    highest_version(versions).context("no valid versions found in GitHub tags")
}

/// List tag versions via `git ls-remote`. Returns an empty Vec if git is
/// unavailable or the call fails, so the caller can fall back to the API.
fn ls_remote_tag_versions() -> Vec<String> {
    let url = format!("https://github.com/{}.git", REPO);
    // GIT_HTTP_LOW_SPEED_* aborts a stalled transfer (proxy/firewall) so the API
    // fallback can run; GIT_TERMINAL_PROMPT=0 prevents a hang on an auth prompt.
    let output = Command::new("git")
        .args(["ls-remote", "--tags", &url])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_HTTP_LOW_SPEED_LIMIT", "1000")
        .env("GIT_HTTP_LOW_SPEED_TIME", "15")
        .output();
    match output {
        Ok(o) if o.status.success() => {
            parse_ls_remote_versions(&String::from_utf8_lossy(&o.stdout))
        }
        _ => Vec::new(),
    }
}

/// Fetch tag names from the tags API (fallback path), honoring `$GITHUB_TOKEN`.
async fn api_tag_versions(client: &Client) -> Result<Vec<String>> {
    let url = format!("https://api.github.com/repos/{}/tags?per_page=100", REPO);
    let resp: Value = with_github_token(
        client
            .get(&url)
            .header("User-Agent", "onchainos-cli")
            .timeout(Duration::from_secs(10)),
    )
    .send()
    .await
    .context("failed to fetch tags from GitHub")?
    .json()
    .await
    .context("failed to parse GitHub tags response")?;

    let tags = resp.as_array().context("expected array from tags API")?;
    Ok(tags
        .iter()
        .filter_map(|tag| tag["name"].as_str())
        .map(|name| name.trim_start_matches('v'))
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .collect())
}

/// Parse a version from a resolved `releases/latest` redirect URL. Returns the
/// bare semver (no `v` prefix) only when the URL is a `/releases/tag/v<digit>…`
/// page, otherwise `None` (e.g. it stayed on `/releases/latest` or points at a
/// non-semver tag like `nightly`).
fn parse_release_tag_url(final_url: &str) -> Option<String> {
    let after = final_url.split("/releases/tag/v").nth(1)?;
    let ver: String = after
        .chars()
        .take_while(|c| !matches!(c, '/' | '?' | '#'))
        .collect();
    let ver = ver.trim();
    if ver.starts_with(|c: char| c.is_ascii_digit()) {
        Some(ver.to_string())
    } else {
        None
    }
}

/// Parse `git ls-remote --tags` stdout into a deduped list of `v`-prefixed
/// semver tags (with the `v` stripped). Drops peeled-tag refs (`^{}`) and any
/// ref that isn't a `v<digit>…` tag (branches, non-semver tags).
fn parse_ls_remote_versions(stdout: &str) -> Vec<String> {
    let mut versions: Vec<String> = stdout
        .lines()
        .filter_map(|line| line.rsplit('/').next())
        .map(|r| r.trim_end_matches("^{}"))
        .filter_map(|r| r.strip_prefix('v'))
        .filter(|v| v.starts_with(|c: char| c.is_ascii_digit()))
        .map(str::to_string)
        .collect();
    versions.sort();
    versions.dedup();
    versions
}

/// Return the highest version by semver (pre-releases rank below their base).
fn highest_version(versions: Vec<String>) -> Option<String> {
    versions
        .into_iter()
        .reduce(|best, v| if semver_gt(&v, &best) { v } else { best })
}

/// Attach an `Authorization: Bearer` header when `$GITHUB_TOKEN` is set, raising
/// the api.github.com rate limit from 60/hr to 5000/hr. Used only on the API
/// fallback paths — the primary paths above avoid api.github.com entirely. The
/// token lives only in the request header, never in a process argument list.
fn with_github_token(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    match std::env::var("GITHUB_TOKEN") {
        Ok(token) if !token.is_empty() => req.bearer_auth(token),
        _ => req,
    }
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

/// Returns true if the origin URL points to a repo we trust to fast-forward.
/// Accepts:
///   • any onchainos-skills checkout (monorepo topology), or
///   • any github.com/okx/* or github.com:okx/* checkout (per-skill topology).
fn remote_is_trusted_okx(path: &Path) -> bool {
    let Ok(out) = run_git(path, &["remote", "get-url", "origin"]) else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    let url = String::from_utf8_lossy(&out.stdout)
        .trim()
        .to_ascii_lowercase();
    url.contains("onchainos-skills")
        || url.contains("github.com/okx/")
        || url.contains("github.com:okx/")
}

/// Resolve the set of candidate skill checkout paths under $HOME.
fn discover_skill_paths() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    discover_skill_paths_in(&home)
}

/// Testable variant of [`discover_skill_paths`] that accepts an explicit home
/// directory. Walks both topologies and dedupes via `canonicalize` to avoid
/// double-pulling when one path symlinks to another (common when
/// `~/.claude/skills/X` is a symlink to `~/.agents/skills/X`).
pub(super) fn discover_skill_paths_in(home: &Path) -> Vec<PathBuf> {
    // Topology A: single monorepo checkout at a fixed path.
    let monorepo = SKILL_INSTALL_PATHS
        .iter()
        .map(|rel| home.join(rel))
        .filter(|p| p.exists());

    // Topology B: per-skill checkouts under a skills-home directory. Walk
    // each home dir's immediate children; later filters in
    // update_skill_checkouts will skip anything that isn't a git checkout
    // pointing at an okx-owned remote.
    let per_skill = SKILL_HOME_DIRS
        .iter()
        .map(|rel| home.join(rel))
        .filter(|p| p.is_dir())
        .flat_map(|home_dir| {
            std::fs::read_dir(&home_dir)
                .ok()
                .into_iter()
                .flatten()
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|p| p.is_dir())
                .collect::<Vec<_>>()
        });

    let mut seen = std::collections::HashSet::new();
    monorepo
        .chain(per_skill)
        .filter(|p| {
            let key = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
            seen.insert(key)
        })
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

        if !remote_is_trusted_okx(&path) {
            results.push(json!({
                "path": path_str,
                "status": "skipped",
                "reason": "remote 'origin' is not an okx-owned repo",
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
            if args.force {
                return Err(e.context(
                    "--force requires a reachable GitHub API to resolve the target version",
                ));
            }
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

    let mut payload = json!({
        "currentVersion": current,
        "status": binary_status,
        "skills": skills,
    });
    if binary_status != "binary_check_failed" {
        payload["latestVersion"] = json!(installed_version);
    }
    if binary_status == "upgraded" {
        payload["previousVersion"] = json!(current);
        payload["installedVersion"] = json!(installed_version);
    }
    output::success(payload);

    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{
        discover_skill_paths_in, highest_version, parse_ls_remote_versions, parse_release_tag_url,
        remote_is_trusted_okx, semver_gt,
    };
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    fn init_git_repo(dir: &Path, origin_url: &str) {
        Command::new("git").arg("init").arg(dir).output().unwrap();
        Command::new("git")
            .args(["-C", dir.to_str().unwrap(), "remote", "add", "origin", origin_url])
            .output()
            .unwrap();
    }

    #[test]
    fn discovers_monorepo_checkout_for_claude() {
        let tmp = TempDir::new().unwrap();
        let claude_mono = tmp.path().join(".claude/onchainos-skills");
        std::fs::create_dir_all(&claude_mono).unwrap();
        init_git_repo(&claude_mono, "https://github.com/okx/onchainos-skills.git");

        let paths = discover_skill_paths_in(tmp.path());
        let canon_target = std::fs::canonicalize(&claude_mono).unwrap();
        assert!(
            paths
                .iter()
                .any(|p| std::fs::canonicalize(p).unwrap() == canon_target),
            "expected to discover {} in {:?}",
            claude_mono.display(),
            paths
        );
    }

    #[test]
    fn discovers_per_skill_checkouts() {
        let tmp = TempDir::new().unwrap();
        let agents = tmp.path().join(".agents/skills");
        std::fs::create_dir_all(agents.join("okx-dex-swap")).unwrap();
        std::fs::create_dir_all(agents.join("onchainos-dapp-scaffold")).unwrap();

        let paths = discover_skill_paths_in(tmp.path());
        assert!(paths.iter().any(|p| p.ends_with("okx-dex-swap")));
        assert!(paths.iter().any(|p| p.ends_with("onchainos-dapp-scaffold")));
    }

    #[test]
    fn trusts_onchainos_skills_origin() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path(), "https://github.com/okx/onchainos-skills.git");
        assert!(remote_is_trusted_okx(tmp.path()));
    }

    #[test]
    fn trusts_other_okx_owned_origin() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path(), "git@github.com:okx/dapp-connect-agenticwallet.git");
        assert!(remote_is_trusted_okx(tmp.path()));
    }

    #[test]
    fn rejects_unrelated_origin() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path(), "https://github.com/random/other.git");
        assert!(!remote_is_trusted_okx(tmp.path()));
    }

    #[test]
    fn dedupes_symlinked_paths() {
        let tmp = TempDir::new().unwrap();
        let real = tmp.path().join(".agents/skills/okx-dex-swap");
        std::fs::create_dir_all(&real).unwrap();
        init_git_repo(&real, "https://github.com/okx/okx-dex-swap.git");
        let link_parent = tmp.path().join(".claude/skills");
        std::fs::create_dir_all(&link_parent).unwrap();
        let link = link_parent.join("okx-dex-swap");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let paths = discover_skill_paths_in(tmp.path());
        let canon: std::collections::HashSet<_> = paths
            .iter()
            .map(|p| std::fs::canonicalize(p).unwrap())
            .collect();
        assert_eq!(canon.len(), paths.len(), "duplicate entries after canonicalize");
    }

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

    #[test]
    fn parse_release_tag_url_extracts_stable_and_beta() {
        let base = "https://github.com/okx/onchainos-skills/releases/tag";
        assert_eq!(
            parse_release_tag_url(&format!("{base}/v3.3.8")).as_deref(),
            Some("3.3.8")
        );
        assert_eq!(
            parse_release_tag_url(&format!("{base}/v3.4.0-beta.1")).as_deref(),
            Some("3.4.0-beta.1")
        );
    }

    #[test]
    fn parse_release_tag_url_rejects_non_tag_pages() {
        // No redirect happened — still on /releases/latest → force API fallback.
        assert_eq!(
            parse_release_tag_url("https://github.com/okx/onchainos-skills/releases/latest"),
            None
        );
        // Non-semver tag (e.g. a named release) is not a version we can use.
        assert_eq!(
            parse_release_tag_url("https://github.com/okx/onchainos-skills/releases/tag/nightly"),
            None
        );
    }

    #[test]
    fn parse_ls_remote_versions_strips_peeled_and_v_prefix() {
        let stdout = "\
abc123\trefs/tags/v3.3.8
def456\trefs/tags/v3.3.8^{}
aaa111\trefs/tags/v3.4.0-beta.1
bbb222\trefs/tags/not-a-version
ccc333\trefs/heads/main
";
        // v3.3.8 + its peeled ref dedupe to one; non-v / branch refs are dropped.
        assert_eq!(
            parse_ls_remote_versions(stdout),
            vec!["3.3.8".to_string(), "3.4.0-beta.1".to_string()]
        );
    }

    #[test]
    fn highest_version_picks_max_semver() {
        assert_eq!(
            highest_version(vec!["3.3.8".into(), "3.4.0-beta.1".into(), "3.3.9".into()]).as_deref(),
            Some("3.4.0-beta.1")
        );
        // A stable release outranks its own pre-release.
        assert_eq!(
            highest_version(vec!["3.4.0-beta.1".into(), "3.4.0".into()]).as_deref(),
            Some("3.4.0")
        );
        assert_eq!(highest_version(vec![]), None);
    }
}
