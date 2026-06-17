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

    /// Exit immediately if the last check ran within 12 hours (stamp at
    /// `<ONCHAINOS_HOME>/last_check`); refresh the stamp after a real check
    #[arg(long)]
    pub throttle: bool,

    /// Version from the active skill's frontmatter. A `-beta` value routes a
    /// stable CLI onto the beta channel; a stable value or beta CLI is ignored,
    /// so it is always safe to pass the skill's version verbatim.
    #[arg(long, value_name = "VERSION")]
    pub skill_version: Option<String>,
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
        // A digit-less suffix like "-beta" still marks a pre-release (tags use
        // this style), so it must compare below the same-base stable.
        let pre_num = pre.map(|p| {
            p.chars()
                .filter(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0)
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

fn is_prerelease(version: &str) -> bool {
    version.contains('-')
}

// ── Channel routing from the active skill ───────────────────────────────

/// True when the active skill is on the beta line but the CLI is stable —
/// the upgrade must then consult the beta channel (same as `--beta`). A beta
/// CLI already consults the beta line via the prerelease fallback.
fn skill_requests_beta(current: &str, skill_version: Option<&str>) -> bool {
    !is_prerelease(current) && skill_version.is_some_and(is_prerelease)
}

// ── Upgrade throttle ────────────────────────────────────────────────────

/// Seconds a previous check stays fresh for `--throttle` (12 hours).
const THROTTLE_WINDOW_SECS: u64 = 12 * 60 * 60;

fn last_check_path(home: &Path) -> PathBuf {
    home.join("last_check")
}

/// True when `<home>/last_check` holds a unix timestamp within the throttle
/// window. Missing, unreadable, garbage, or future stamps all count as stale.
fn is_throttled(home: &Path, now_secs: u64) -> bool {
    let Ok(raw) = std::fs::read_to_string(last_check_path(home)) else {
        return false;
    };
    let Ok(last) = raw.trim().parse::<u64>() else {
        return false;
    };
    last <= now_secs && now_secs - last < THROTTLE_WINDOW_SECS
}

/// Record `now_secs` into `<home>/last_check`.
fn record_check(home: &Path, now_secs: u64) -> std::io::Result<()> {
    std::fs::write(last_check_path(home), now_secs.to_string())
}

// ── Graduation action hint ──────────────────────────────────────────────

/// On beta→stable graduation, mark the payload and attach a machine-readable
/// `action` telling the agent how to refresh npx-installed skills.
fn decorate_graduation(payload: &mut Value, graduated: bool) {
    if !graduated {
        return;
    }
    payload["graduated"] = json!(true);
    payload["action"] = json!(
        "Beta CLI graduated to stable. If skills were installed via npx, re-run \
         `npx skills add okx/onchainos-skills --yes`, then re-read SKILL.md before continuing."
    );
}

/// Outcome of upgrade channel resolution.
struct UpgradeTarget {
    version: String,
    /// True when this upgrade moves a beta install back onto the stable channel.
    graduated: bool,
}

/// Pick the version to install. `preferred` wins as soon as it is newer than
/// `current`; otherwise `fallback` (the beta line, when the install is a beta)
/// gets a chance. Stable installs pass `fallback: None` and therefore never
/// see the beta channel.
fn decide_target(current: &str, preferred: &str, fallback: Option<&str>) -> UpgradeTarget {
    let version = if semver_gt(preferred, current) {
        preferred.to_string()
    } else {
        match fallback {
            Some(fb) if semver_gt(fb, current) => fb.to_string(),
            _ => current.to_string(),
        }
    };
    let graduated = is_prerelease(current) && !is_prerelease(&version);
    UpgradeTarget { version, graduated }
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

/// Current branch name of a checkout (`HEAD` when detached).
fn current_branch(path: &Path) -> Option<String> {
    let out = run_git(path, &["rev-parse", "--abbrev-ref", "HEAD"]).ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Default (stable) branch of the checkout's origin remote. Prefers the local
/// `origin/HEAD` symref (offline); falls back to asking the remote.
fn default_branch(path: &Path) -> Option<String> {
    if let Ok(out) = run_git(
        path,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) {
        if out.status.success() {
            let symref = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Some(branch) = symref.strip_prefix("origin/") {
                return Some(branch.to_string());
            }
        }
    }
    let out = run_git(path, &["ls-remote", "--symref", "origin", "HEAD"]).ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().find_map(|line| {
        line.strip_prefix("ref: refs/heads/")
            .and_then(|rest| rest.split_whitespace().next())
            .map(str::to_string)
    })
}

/// Switch a beta-branch checkout to the remote's default (stable) branch and
/// fast-forward it.
fn graduate_checkout(path: &Path, path_str: &str) -> Value {
    eprintln!(
        "Switching skills at {} from the beta branch to the stable branch...",
        path_str
    );
    let _ = run_git(path, &["fetch", "origin", "--prune"]);

    let Some(branch) = default_branch(path) else {
        return json!({
            "path": path_str,
            "status": "skipped",
            "reason": "could not resolve the default branch for this beta checkout",
        });
    };

    let fail = |reason: String| {
        eprintln!(
            "Warning: could not switch {} to {} — {}",
            path_str, branch, reason
        );
        json!({ "path": path_str, "status": "failed", "reason": reason })
    };

    match run_git(path, &["checkout", &branch]) {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return fail(format!("git checkout {} failed: {}", branch, stderr));
        }
        Err(e) => return fail(e.to_string()),
    }

    match run_git(path, &["pull", "--ff-only"]) {
        Ok(out) if out.status.success() => json!({
            "path": path_str,
            "status": "updated",
            "branch": branch,
        }),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            fail(format!(
                "git pull failed after switching to {}: {}",
                branch, stderr
            ))
        }
        Err(e) => fail(e.to_string()),
    }
}

/// Update a single skill checkout. `graduated` marks a beta→stable CLI
/// upgrade, in which case a checkout sitting on the beta branch is switched
/// to the remote's default (stable) branch before pulling.
fn update_one_checkout(path: &Path, graduated: bool) -> Value {
    let path_str = path.display().to_string();

    if !path.join(".git").exists() {
        return json!({
            "path": path_str,
            "status": "skipped",
            "reason": "not a git checkout — update via your package manager",
        });
    }

    if !remote_is_trusted_okx(path) {
        return json!({
            "path": path_str,
            "status": "skipped",
            "reason": "remote 'origin' is not an okx-owned repo",
        });
    }

    // On graduation a beta-branch checkout must move to the stable branch —
    // a plain pull would keep tracking the beta line.
    if graduated && current_branch(path).as_deref() == Some("beta") {
        return graduate_checkout(path, &path_str);
    }

    eprintln!("Updating skills at {}...", path_str);
    match run_git(path, &["pull", "--ff-only"]) {
        Ok(out) if out.status.success() => json!({
            "path": path_str,
            "status": "updated",
        }),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            eprintln!(
                "Warning: git pull failed at {} — {}",
                path_str,
                if stderr.is_empty() {
                    "non-zero exit"
                } else {
                    &stderr
                }
            );
            json!({
                "path": path_str,
                "status": "failed",
                "reason": stderr,
            })
        }
        Err(e) => json!({
            "path": path_str,
            "status": "failed",
            "reason": e.to_string(),
        }),
    }
}

/// Update each detected skill checkout via `git pull --ff-only`. Returns one
/// result per path describing the outcome.
fn update_skill_checkouts(graduated: bool) -> Vec<Value> {
    if Command::new("git").arg("--version").output().is_err() {
        return discover_skill_paths()
            .iter()
            .map(|path| {
                json!({
                    "path": path.display().to_string(),
                    "status": "skipped",
                    "reason": "git not found on PATH",
                })
            })
            .collect();
    }

    discover_skill_paths()
        .iter()
        .map(|path| update_one_checkout(path, graduated))
        .collect()
}

// ── Command entry point ─────────────────────────────────────────────────

pub async fn execute(args: UpgradeArgs) -> Result<()> {
    let current = CURRENT_VERSION;

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // With --throttle, a check within the last 12 hours short-circuits the
    // whole command — no network, no skill pulls. --force and --check always
    // get a real answer.
    if args.throttle && !args.force && !args.check {
        if let Ok(home) = crate::home::onchainos_home() {
            if is_throttled(&home, now_secs) {
                output::success(json!({
                    "status": "fresh",
                    "currentVersion": current,
                }));
                return Ok(());
            }
        }
    }

    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;

    // Resolve the upgrade target. Stable installs only ever see the stable
    // channel; beta installs graduate to stable the moment it passes them,
    // otherwise they advance within the beta line. `--beta` explicitly opts
    // into the beta channel, as does a beta skill driving a stable CLI.
    let use_beta = args.beta || skill_requests_beta(current, args.skill_version.as_deref());
    let resolution: Result<UpgradeTarget> = if use_beta {
        get_latest_with_beta(&client)
            .await
            .map(|latest| decide_target(current, &latest, None))
    } else if is_prerelease(current) {
        match (
            get_latest_stable(&client).await,
            get_latest_with_beta(&client).await,
        ) {
            (Ok(stable), Ok(beta)) => Ok(decide_target(current, &stable, Some(&beta))),
            (Err(e), _) | (_, Err(e)) => Err(e),
        }
    } else {
        get_latest_stable(&client)
            .await
            .map(|stable| decide_target(current, &stable, None))
    };

    // --check requires a resolved target; bail if unreachable.
    if args.check {
        let target = resolution?;
        let update_available = semver_gt(&target.version, current);
        output::success(json!({
            "currentVersion": current,
            "latestVersion": target.version,
            "updateAvailable": update_available,
            "channel": if is_prerelease(&target.version) { "beta" } else { "stable" },
            "graduated": target.graduated,
        }));
        return Ok(());
    }

    // For a full upgrade, degrade gracefully when the GitHub API is down —
    // skill checkouts can still fast-forward via their own git remotes.
    let (installed_version, binary_status, graduated) = match resolution {
        Ok(target) => {
            let needs_upgrade = args.force || semver_gt(&target.version, current);
            if needs_upgrade {
                if target.graduated {
                    eprintln!(
                        "Stable {} has superseded beta {} — switching to the stable channel.",
                        target.version, current
                    );
                }
                eprintln!("Upgrading onchainos: {} → {}", current, target.version);
                download_and_install(&client, &target.version).await?;
                (target.version, "upgraded", target.graduated)
            } else {
                (target.version, "already_latest", false)
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
            (current.to_string(), "binary_check_failed", false)
        }
    };

    let skills = if args.skip_skills {
        Vec::new()
    } else {
        update_skill_checkouts(graduated)
    };

    let mut payload = json!({
        "currentVersion": current,
        "status": binary_status,
        "skills": skills,
    });
    if binary_status != "binary_check_failed" {
        payload["latestVersion"] = json!(installed_version);
        payload["channel"] = json!(if is_prerelease(&installed_version) {
            "beta"
        } else {
            "stable"
        });
    }
    if binary_status == "upgraded" {
        payload["previousVersion"] = json!(current);
        payload["installedVersion"] = json!(installed_version);
        decorate_graduation(&mut payload, graduated);
    }

    // A completed check (even "already_latest") restarts the throttle window;
    // a failed one must not, so the next run retries immediately.
    if args.throttle && binary_status != "binary_check_failed" {
        if let Ok(home) = crate::home::ensure_onchainos_home() {
            let _ = record_check(&home, now_secs);
        }
    }

    output::success(payload);

    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{
        current_branch, decide_target, decorate_graduation, discover_skill_paths_in,
        highest_version, is_throttled, parse_ls_remote_versions, parse_release_tag_url,
        record_check, remote_is_trusted_okx, semver_gt, skill_requests_beta, update_one_checkout,
    };
    use serde_json::json;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use tempfile::TempDir;

    fn init_git_repo(dir: &Path, origin_url: &str) {
        Command::new("git").arg("init").arg(dir).output().unwrap();
        Command::new("git")
            .args([
                "-C",
                dir.to_str().unwrap(),
                "remote",
                "add",
                "origin",
                origin_url,
            ])
            .output()
            .unwrap();
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=test", "-c", "user.email=test@test"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Upstream repo named "onchainos-skills" (so the trust check passes) with
    /// a `main` default branch and a `beta` branch one commit ahead.
    fn make_upstream(tmp: &Path) -> PathBuf {
        let upstream = tmp.join("onchainos-skills");
        std::fs::create_dir_all(&upstream).unwrap();
        git(&upstream, &["init", "-b", "main"]);
        std::fs::write(upstream.join("README.md"), "stable").unwrap();
        git(&upstream, &["add", "."]);
        git(&upstream, &["commit", "-m", "stable base"]);
        git(&upstream, &["checkout", "-b", "beta"]);
        std::fs::write(upstream.join("README.md"), "beta").unwrap();
        git(&upstream, &["add", "."]);
        git(&upstream, &["commit", "-m", "beta work"]);
        git(&upstream, &["checkout", "main"]);
        upstream
    }

    fn git_clone(upstream: &Path, dest: &Path, branch: Option<&str>) {
        let mut cmd = Command::new("git");
        cmd.args(["clone", "-q"]);
        if let Some(b) = branch {
            cmd.args(["-b", b]);
        }
        let out = cmd.arg(upstream).arg(dest).output().unwrap();
        assert!(
            out.status.success(),
            "git clone failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn graduation_switches_beta_checkout_to_default_branch() {
        let tmp = TempDir::new().unwrap();
        let upstream = make_upstream(tmp.path());
        let clone = tmp.path().join("checkout");
        git_clone(&upstream, &clone, Some("beta"));

        let result = update_one_checkout(&clone, true);

        assert_eq!(result["status"], "updated", "result: {}", result);
        assert_eq!(result["branch"], "main", "result: {}", result);
        assert_eq!(current_branch(&clone).as_deref(), Some("main"));
    }

    #[test]
    fn no_graduation_keeps_beta_checkout_on_beta() {
        let tmp = TempDir::new().unwrap();
        let upstream = make_upstream(tmp.path());
        let clone = tmp.path().join("checkout");
        git_clone(&upstream, &clone, Some("beta"));

        let result = update_one_checkout(&clone, false);

        assert_eq!(result["status"], "updated", "result: {}", result);
        assert_eq!(current_branch(&clone).as_deref(), Some("beta"));
    }

    #[test]
    fn graduation_leaves_stable_checkout_untouched() {
        let tmp = TempDir::new().unwrap();
        let upstream = make_upstream(tmp.path());
        let clone = tmp.path().join("checkout");
        git_clone(&upstream, &clone, None);

        let result = update_one_checkout(&clone, true);

        assert_eq!(result["status"], "updated", "result: {}", result);
        assert!(
            result.get("branch").is_none(),
            "plain pull expected: {}",
            result
        );
        assert_eq!(current_branch(&clone).as_deref(), Some("main"));
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
        init_git_repo(
            tmp.path(),
            "git@github.com:okx/dapp-connect-agenticwallet.git",
        );
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
        assert_eq!(
            canon.len(),
            paths.len(),
            "duplicate entries after canonicalize"
        );
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
    fn stable_newer_than_digitless_prerelease() {
        // Release tags use the digit-less "-beta" suffix (e.g. v3.4.8-beta), so a
        // same-base stable graduation (3.4.8-beta → 3.4.8) must register as newer.
        assert!(semver_gt("3.4.8", "3.4.8-beta"));
        assert!(!semver_gt("3.4.8-beta", "3.4.8"));
    }

    #[test]
    fn numbered_prerelease_newer_than_digitless() {
        assert!(semver_gt("3.4.8-beta.1", "3.4.8-beta"));
        assert!(!semver_gt("3.4.8-beta", "3.4.8-beta.1"));
    }

    #[test]
    fn equal_versions_not_gt() {
        assert!(!semver_gt("2.0.0", "2.0.0"));
        assert!(!semver_gt("2.0.0-beta.0", "2.0.0-beta.0"));
    }

    // ── decide_target: channel resolution ──────────────────────────────

    #[test]
    fn beta_install_graduates_when_stable_passes_it() {
        let t = decide_target("3.4.8-beta", "3.4.9", Some("3.4.8-beta"));
        assert_eq!(t.version, "3.4.9");
        assert!(t.graduated);
    }

    #[test]
    fn beta_install_graduates_on_same_base_stable() {
        let t = decide_target("3.4.8-beta", "3.4.8", Some("3.4.8-beta"));
        assert_eq!(t.version, "3.4.8");
        assert!(t.graduated);
    }

    #[test]
    fn stable_outranks_newer_beta_once_it_passes_current() {
        // Graduation has priority even if the beta line has moved further ahead.
        let t = decide_target("3.4.8-beta", "3.4.9", Some("3.5.0-beta"));
        assert_eq!(t.version, "3.4.9");
        assert!(t.graduated);
    }

    #[test]
    fn beta_install_advances_within_beta_line_while_stable_is_behind() {
        let t = decide_target("3.4.7-beta", "3.3.11", Some("3.4.8-beta"));
        assert_eq!(t.version, "3.4.8-beta");
        assert!(!t.graduated);
    }

    #[test]
    fn beta_install_already_latest_stays_put() {
        let t = decide_target("3.4.8-beta", "3.3.11", Some("3.4.8-beta"));
        assert_eq!(t.version, "3.4.8-beta");
        assert!(!t.graduated);
    }

    #[test]
    fn stable_install_upgrades_to_newer_stable_only() {
        let t = decide_target("3.3.10", "3.3.11", None);
        assert_eq!(t.version, "3.3.11");
        assert!(!t.graduated);
    }

    #[test]
    fn stable_install_already_latest_stays_put() {
        let t = decide_target("3.3.11", "3.3.11", None);
        assert_eq!(t.version, "3.3.11");
        assert!(!t.graduated);
    }

    // ── skill_requests_beta: channel routing from skill frontmatter ────

    #[test]
    fn beta_skill_routes_stable_cli_to_beta() {
        assert!(skill_requests_beta("3.4.9", Some("3.4.9-beta")));
    }

    #[test]
    fn stable_skill_keeps_stable_cli_on_stable() {
        assert!(!skill_requests_beta("3.4.9", Some("3.4.9")));
    }

    #[test]
    fn beta_cli_ignores_skill_channel_hint() {
        assert!(!skill_requests_beta("3.4.9-beta", Some("3.4.9-beta")));
    }

    #[test]
    fn missing_skill_version_defaults_to_stable() {
        assert!(!skill_requests_beta("3.4.9", None));
    }

    // ── throttle: last_check freshness ──────────────────────────────────

    #[test]
    fn throttled_within_window() {
        let tmp = TempDir::new().unwrap();
        record_check(tmp.path(), 1_000).unwrap();
        assert!(is_throttled(tmp.path(), 1_000 + 11 * 3600));
    }

    #[test]
    fn stale_at_window_boundary() {
        let tmp = TempDir::new().unwrap();
        record_check(tmp.path(), 1_000).unwrap();
        assert!(!is_throttled(tmp.path(), 1_000 + 12 * 3600));
    }

    #[test]
    fn missing_stamp_is_stale() {
        let tmp = TempDir::new().unwrap();
        assert!(!is_throttled(tmp.path(), 5_000));
    }

    #[test]
    fn garbage_stamp_is_stale() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("last_check"), "not-a-number").unwrap();
        assert!(!is_throttled(tmp.path(), 5_000));
    }

    #[test]
    fn future_stamp_is_stale() {
        // A stamp ahead of the clock (corrupt write or clock skew) must not
        // pin the throttle shut until the clock catches up.
        let tmp = TempDir::new().unwrap();
        record_check(tmp.path(), 1_000_000).unwrap();
        assert!(!is_throttled(tmp.path(), 1_000));
    }

    #[test]
    fn record_check_overwrites_previous_stamp() {
        let tmp = TempDir::new().unwrap();
        record_check(tmp.path(), 1_000).unwrap();
        record_check(tmp.path(), 200_000).unwrap();
        assert!(is_throttled(tmp.path(), 200_000 + 100));
        assert!(!is_throttled(tmp.path(), 200_000 + 13 * 3600));
    }

    // ── decorate_graduation: machine-readable action hint ───────────────

    #[test]
    fn graduation_adds_action_hint() {
        let mut payload = json!({"status": "upgraded"});
        decorate_graduation(&mut payload, true);
        assert_eq!(payload["graduated"], json!(true));
        let action = payload["action"].as_str().expect("action must be a string");
        assert!(action.contains("npx skills add okx/onchainos-skills --yes"));
        assert!(action.contains("SKILL.md"));
    }

    #[test]
    fn no_graduation_leaves_payload_untouched() {
        let mut payload = json!({"status": "upgraded"});
        decorate_graduation(&mut payload, false);
        assert!(payload.get("graduated").is_none());
        assert!(payload.get("action").is_none());
    }

    // ── release discovery: tag URL / ls-remote parsing ──────────────────

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
