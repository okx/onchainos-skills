//! Side-effect-free local OKX Trade Kit discovery and shared trading environment.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Skill that documents the CEX execution surface.
pub const TRADE_SKILL_ID: &str = "okx-cex-trade";
/// Runtime binary required when the model selects Trade Kit execution.
pub const CLI_BINARY: &str = "okx";

/// Subscription-time readiness derived without executing an external program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalReadiness {
    /// The skill may be installed, but the required `okx` runtime is absent.
    Missing,
    /// The runtime exists, but authentication and account permissions have not
    /// been checked. Binary presence must never be exposed as trading readiness.
    VerificationUnknown,
}

/// Non-sensitive local inventory. `skill_installed` is advisory only and never
/// changes execution readiness: Rust calls the CLI directly and does not need an
/// LLM skill once the runtime is present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalProbe {
    pub cli_path: Option<PathBuf>,
    pub skill_installed: bool,
}

impl LocalProbe {
    pub fn readiness(&self) -> LocalReadiness {
        if self.cli_path.is_none() {
            LocalReadiness::Missing
        } else {
            LocalReadiness::VerificationUnknown
        }
    }
}

/// Detect the current process environment without launching `okx`.
pub fn probe_local() -> LocalProbe {
    let home = dirs::home_dir().unwrap_or_default();
    let path_var = std::env::var("PATH").unwrap_or_default();
    probe_local_with(&home, &path_var)
}

/// Injectable local probe for unit tests and callers that already resolved the
/// effective home/PATH. No global environment is mutated.
pub fn probe_local_with(home: &Path, path_var: &str) -> LocalProbe {
    LocalProbe {
        cli_path: find_cli(home, path_var),
        skill_installed: crate::commands::upgrade::is_skill_installed_in(home, TRADE_SKILL_ID),
    }
}

fn executable_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["okx.exe", "okx.cmd", "okx.bat", "okx"]
    } else {
        &[CLI_BINARY]
    }
}

fn find_cli(home: &Path, path_var: &str) -> Option<PathBuf> {
    for dir in std::env::split_paths(path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for name in executable_names() {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    // Bounded common global-node locations. No recursive filesystem scan.
    const NPM_BIN_DIRS: &[&str] = &[
        ".npm-global/bin",
        ".npm/bin",
        ".local/bin",
        ".yarn/bin",
        ".config/yarn/global/node_modules/.bin",
    ];
    for rel in NPM_BIN_DIRS {
        for name in executable_names() {
            let candidate = home.join(rel).join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Trading environment used by the selected Trade Kit command. `Configured`
/// preserves the CLI's profile/env resolution for compatibility; execution
/// paths should pass `Live` or `Demo` explicitly whenever that intent is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradeEnvironment {
    Configured,
    Live,
    Demo,
}

impl TradeEnvironment {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        match value {
            "configured" => Ok(Self::Configured),
            "live" => Ok(Self::Live),
            "demo" => Ok(Self::Demo),
            _ => Err("environment must be configured, live, or demo"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::Live => "live",
            Self::Demo => "demo",
        }
    }

    pub fn is_explicit(self) -> bool {
        matches!(self, Self::Live | Self::Demo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tempdir() -> tempfile::TempDir {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("trade-kit-unit");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .prefix("case-")
            .tempdir_in(base)
            .unwrap()
    }

    #[test]
    fn local_probe_readiness_depends_only_on_okx_cli() {
        let tmp = test_tempdir();
        let home = tmp.path();
        let bin = home.join("bin");
        std::fs::create_dir_all(&bin).unwrap();

        assert_eq!(
            probe_local_with(home, bin.to_str().unwrap()).readiness(),
            LocalReadiness::Missing
        );

        // MCP alone is not the deterministic CLI runtime.
        std::fs::write(bin.join("okx-trade-mcp"), b"x").unwrap();
        assert_eq!(
            probe_local_with(home, bin.to_str().unwrap()).readiness(),
            LocalReadiness::Missing
        );

        std::fs::write(bin.join(CLI_BINARY), b"x").unwrap();
        let installed = probe_local_with(home, bin.to_str().unwrap());
        assert_eq!(installed.readiness(), LocalReadiness::VerificationUnknown);
        assert!(installed.cli_path.is_some());

        std::fs::create_dir_all(home.join(".okx")).unwrap();
        std::fs::write(home.join(".okx/config.toml"), b"[profiles.live]\n").unwrap();
        assert_eq!(
            probe_local_with(home, bin.to_str().unwrap()).readiness(),
            LocalReadiness::VerificationUnknown
        );

        std::fs::write(home.join(".okx/config.toml"), b"").unwrap();
        assert_eq!(
            probe_local_with(home, bin.to_str().unwrap()).readiness(),
            LocalReadiness::VerificationUnknown
        );
    }

    #[test]
    fn skill_is_advisory_only() {
        let tmp = test_tempdir();
        let skill = tmp.path().join(".agents/skills").join(TRADE_SKILL_ID);
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), b"# trade").unwrap();

        let probe = probe_local_with(tmp.path(), "");
        assert!(probe.skill_installed);
        assert_eq!(probe.readiness(), LocalReadiness::Missing);
    }

    #[test]
    fn finds_cli_in_bounded_home_bin() {
        let tmp = test_tempdir();
        let bin = tmp.path().join(".npm-global/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(CLI_BINARY), b"x").unwrap();

        let probe = probe_local_with(tmp.path(), "");
        assert_eq!(probe.cli_path, Some(bin.join(CLI_BINARY)));
    }
}
