//! Legacy update command compatibility.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

const SKILL_INSTALL_PATHS: &[&str] = &[
    ".codex/onchainos-skills",
    ".openclaw/onchainos-skills",
    ".cursor/onchainos-skills",
    ".config/opencode/onchainos-skills",
    ".claude/onchainos-skills",
];

const SKILL_HOME_DIRS: &[&str] = &[
    ".agents/skills",
    ".claude/skills",
    ".codex/skills",
    ".openclaw/skills",
    ".cursor/skills",
];

#[derive(clap::Args)]
pub struct UpgradeArgs {}

/// Deprecated command shell retained only to give existing callers a clear
/// migration message.
#[derive(clap::Args)]
pub struct PreflightArgs {}

pub async fn execute(_: UpgradeArgs) -> Result<()> {
    let status = Command::new("npx")
        .args(["-y", "oc-onchainos", "install"])
        .status()
        .context("failed to start `npx -y oc-onchainos install`")?;

    if !status.success() {
        bail!("`npx -y oc-onchainos install` exited with {status}");
    }

    Ok(())
}

pub async fn preflight(_: PreflightArgs) -> Result<()> {
    bail!("`onchainos preflight` is deprecated; use `npx -y oc-onchainos install`")
}

fn discover_skill_paths_in(home: &Path) -> Vec<PathBuf> {
    let monorepo = SKILL_INSTALL_PATHS
        .iter()
        .map(|rel| home.join(rel))
        .filter(|path| path.exists());
    let per_skill = SKILL_HOME_DIRS
        .iter()
        .map(|rel| home.join(rel))
        .filter(|path| path.is_dir())
        .flat_map(|home_dir| {
            std::fs::read_dir(home_dir)
                .ok()
                .into_iter()
                .flatten()
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.is_dir())
                .collect::<Vec<_>>()
        });

    let mut seen = std::collections::HashSet::new();
    monorepo
        .chain(per_skill)
        .filter(|path| {
            let key = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            seen.insert(key)
        })
        .collect()
}

pub(crate) fn is_skill_installed_in(home: &Path, skill_id: &str) -> bool {
    discover_skill_paths_in(home).iter().any(|path| {
        path.file_name().and_then(|name| name.to_str()) == Some(skill_id)
            && path.join("SKILL.md").is_file()
    })
}
