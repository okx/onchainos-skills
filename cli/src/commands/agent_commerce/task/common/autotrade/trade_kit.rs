//! Local OKX Agent Trade Kit discovery and machine-readable capability parsing.
//!
//! Subscription preflight must stay side-effect free: it only checks whether the
//! `okx` CLI exists. It never launches the CLI, reads credential/configuration
//! state, touches the keychain, or performs network I/O.
//! Runtime authentication and account checks belong to the selected trading
//! Skill/tool and must run for every delivery.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::asset_class::AssetClass;

/// Skill-pack repository used by user-facing install guidance.
pub const SKILL_REPOSITORY: &str = "okx/agent-skills";
/// Skill that documents the CEX execution surface.
pub const TRADE_SKILL_ID: &str = "okx-cex-trade";
/// Runtime binary required when the model selects Trade Kit execution.
pub const CLI_BINARY: &str = "okx";
/// Official npm runtime package.
pub const CLI_PACKAGE: &str = "@okx_ai/okx-trade-cli";

/// Subscription-time readiness derived without executing an external program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalReadiness {
    /// The skill may be installed, but the required `okx` runtime is absent.
    Missing,
    /// The runtime exists. Authentication, account permissions, and required
    /// capabilities are intentionally re-checked by the first signal at runtime.
    Ready,
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
            LocalReadiness::Ready
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

/// Sanitized result of `okx list-tools --json`. It retains only version and tool
/// identifiers; descriptions, parameters, environment metadata, and command
/// output are deliberately discarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySnapshot {
    pub version: String,
    tool_names: BTreeSet<String>,
}

impl CapabilitySnapshot {
    pub fn supports(&self, tool_name: &str) -> bool {
        self.tool_names.contains(tool_name)
    }

    pub fn supports_all(&self, required: &[&str]) -> bool {
        required.iter().all(|name| self.supports(name))
    }

    /// Parse the official discovery envelope without executing the CLI. Unknown
    /// fields are ignored for forward compatibility; malformed required shapes
    /// fail closed.
    pub fn from_list_tools_json(raw: &str) -> Result<Self, &'static str> {
        let value: serde_json::Value =
            serde_json::from_str(raw).map_err(|_| "trade_kit_capabilities_invalid")?;
        let version = value
            .get("version")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("trade_kit_capabilities_invalid")?
            .to_string();
        let modules = value
            .get("modules")
            .and_then(serde_json::Value::as_array)
            .ok_or("trade_kit_capabilities_invalid")?;
        let mut tool_names = BTreeSet::new();
        for module in modules {
            let commands = module
                .get("commands")
                .and_then(serde_json::Value::as_array)
                .ok_or("trade_kit_capabilities_invalid")?;
            for command in commands {
                if let Some(name) = command
                    .get("toolName")
                    .and_then(serde_json::Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    tool_names.insert(name.to_string());
                }
            }
        }
        Ok(Self {
            version,
            tool_names,
        })
    }
}

/// Minimum machine-discovered tools needed by the Trade Kit adapter for each
/// supported asset class. CLI-only flags require a separate version gate because
/// the upstream registry intentionally omits them.
pub fn required_capabilities(class: AssetClass) -> &'static [&'static str] {
    match class {
        AssetClass::Spot => &["market_get_ticker", "spot_place_order"],
        AssetClass::Perp => &[
            "market_get_ticker",
            "market_get_instruments",
            "account_get_config",
            "swap_get_leverage",
            "swap_set_leverage",
            "swap_place_order",
        ],
        AssetClass::Prediction => &[
            "event_browse",
            "event_get_series",
            "event_get_events",
            "event_get_markets",
            "event_place_order",
        ],
        AssetClass::Option => &[
            "option_get_instruments",
            "option_get_greeks",
            "option_place_order",
        ],
        // DeFi is native OnchainOS and is never routed to Trade Kit.
        AssetClass::Defi => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_probe_readiness_depends_only_on_okx_cli() {
        let tmp = tempfile::tempdir().unwrap();
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
        assert_eq!(installed.readiness(), LocalReadiness::Ready);
        assert!(installed.cli_path.is_some());

        std::fs::create_dir_all(home.join(".okx")).unwrap();
        std::fs::write(home.join(".okx/config.toml"), b"[profiles.live]\n").unwrap();
        assert_eq!(
            probe_local_with(home, bin.to_str().unwrap()).readiness(),
            LocalReadiness::Ready
        );

        std::fs::write(home.join(".okx/config.toml"), b"").unwrap();
        assert_eq!(
            probe_local_with(home, bin.to_str().unwrap()).readiness(),
            LocalReadiness::Ready
        );
    }

    #[test]
    fn skill_is_advisory_only() {
        let tmp = tempfile::tempdir().unwrap();
        let skill = tmp.path().join(".agents/skills").join(TRADE_SKILL_ID);
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), b"# trade").unwrap();

        let probe = probe_local_with(tmp.path(), "");
        assert!(probe.skill_installed);
        assert_eq!(probe.readiness(), LocalReadiness::Missing);
    }

    #[test]
    fn finds_cli_in_bounded_home_bin() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join(".npm-global/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(CLI_BINARY), b"x").unwrap();

        let probe = probe_local_with(tmp.path(), "");
        assert_eq!(probe.cli_path, Some(bin.join(CLI_BINARY)));
    }

    #[test]
    fn parses_machine_readable_capabilities_without_retaining_payload() {
        let raw = r#"{
          "version":"1.4.2",
          "totalTools":3,
          "ignored":{"api_key":"must-not-be-retained"},
          "modules":[
            {"name":"market","commands":[
              {"path":"okx market ticker","toolName":"market_get_ticker","parameters":[]}
            ]},
            {"name":"spot","commands":[
              {"path":"okx spot place","toolName":"spot_place_order","parameters":[]},
              {"path":"okx spot composite","toolName":null,"parameters":[]}
            ]}
          ]
        }"#;
        let snapshot = CapabilitySnapshot::from_list_tools_json(raw).unwrap();
        assert_eq!(snapshot.version, "1.4.2");
        assert!(snapshot.supports_all(required_capabilities(AssetClass::Spot)));
        assert!(!snapshot.supports("must-not-be-retained"));
    }

    #[test]
    fn malformed_capability_envelope_fails_closed() {
        assert_eq!(
            CapabilitySnapshot::from_list_tools_json("{}"),
            Err("trade_kit_capabilities_invalid")
        );
        assert_eq!(
            CapabilitySnapshot::from_list_tools_json("not-json"),
            Err("trade_kit_capabilities_invalid")
        );
    }
}
