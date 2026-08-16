//! Local OKX Agent Trade Kit discovery and machine-readable capability parsing.
//!
//! Subscription matching stays side-effect free: it checks only whether the
//! `okx` CLI exists and reports an installed runtime as unverified. Once Trade
//! Kit is selected, the bounded runtime probe verifies version, capabilities,
//! private authentication, and the account's `trade` permission.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use crate::asset_class::AssetClass;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt};

/// Skill-pack repository used by user-facing install guidance.
pub const SKILL_REPOSITORY: &str = "okx/agent-skills";
/// Skill that documents the CEX execution surface.
pub const TRADE_SKILL_ID: &str = "okx-cex-trade";
/// Runtime binary required when the model selects Trade Kit execution.
pub const CLI_BINARY: &str = "okx";
/// Official npm runtime package.
pub const CLI_PACKAGE: &str = "@okx_ai/okx-trade-cli";
/// First stable Trade Kit release whose CLI exposes OAuth login/status.
pub const MIN_OAUTH_CLI_VERSION: &str = "1.3.2";
/// User-visible commands returned by the typed readiness result.
pub const INSTALL_COMMAND: &str = "npm install -g @okx_ai/okx-trade-cli@latest";
pub const OAUTH_LOGIN_COMMAND: &str = "okx auth login --manual";
pub const API_KEY_CONFIG_COMMAND: &str = "okx config init";
pub const READINESS_SCHEMA_VERSION: u8 = 2;

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);
const AUTH_TIMEOUT: Duration = Duration::from_secs(20);
const KILL_REAP_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_DISCOVERY_STDOUT: usize = 1024 * 1024;
const MAX_AUTH_STDOUT: usize = 64 * 1024;
const MAX_CHILD_STDERR: usize = 64 * 1024;

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

/// Parse the public readiness command's deliberately narrow asset-class surface.
/// Repo-wide aliases such as `futures` and `options` are not part of this machine
/// contract because callers persist and compare the canonical tokens verbatim.
pub fn parse_runtime_asset_class(value: &str) -> Result<AssetClass, &'static str> {
    match value {
        "spot" => Ok(AssetClass::Spot),
        "perp" => Ok(AssetClass::Perp),
        "prediction" => Ok(AssetClass::Prediction),
        "option" => Ok(AssetClass::Option),
        _ => Err("asset class must be spot, perp, prediction, or option"),
    }
}

/// Parse and de-duplicate the repeatable public CLI argument while preserving
/// caller order. Empty input is rejected even if a future caller bypasses clap.
pub fn parse_runtime_asset_classes(values: &[String]) -> Result<Vec<AssetClass>, &'static str> {
    if values.is_empty() {
        return Err("at least one --asset-class is required");
    }
    let mut classes = Vec::new();
    for value in values {
        let class = parse_runtime_asset_class(value)?;
        if !classes.contains(&class) {
            classes.push(class);
        }
    }
    Ok(classes)
}

fn missing_capabilities(snapshot: &CapabilitySnapshot, class: AssetClass) -> Vec<String> {
    required_capabilities(class)
        .iter()
        .filter(|name| !snapshot.supports(name))
        .map(|name| (*name).to_string())
        .collect()
}

/// Public five-state readiness contract shared by the aggregate and each asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Ready,
    Missing,
    VerificationUnknown,
    NeedsConfiguration,
    Incompatible,
}

/// Per-asset result from one shared discovery/authentication snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetReadinessCheck {
    pub asset_class: AssetClass,
    pub readiness: RuntimeState,
    pub ready: bool,
    pub reason: RuntimeReason,
    pub missing_capabilities: Vec<String>,
}

impl AssetReadinessCheck {
    fn new(
        asset_class: AssetClass,
        readiness: RuntimeState,
        reason: RuntimeReason,
        missing_capabilities: Vec<String>,
    ) -> Self {
        Self {
            asset_class,
            readiness,
            ready: readiness == RuntimeState::Ready,
            reason,
            missing_capabilities,
        }
    }
}

/// Stable runtime result consumed by subscription and delivery playbooks. It
/// deliberately contains no raw child-process output or local filesystem path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeReadiness {
    pub schema_version: u8,
    pub tool: &'static str,
    pub asset_classes: Vec<AssetClass>,
    pub readiness: RuntimeState,
    pub ready: bool,
    pub reason: RuntimeReason,
    pub checked_at: String,
    pub version: Option<String>,
    pub missing_capabilities: Vec<String>,
    pub remediation: Option<RuntimeRemediation>,
    pub asset_checks: Vec<AssetReadinessCheck>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeReason {
    Ready,
    CliMissing,
    AuthorizationNotChecked,
    DiscoveryTimeout,
    DiscoveryFailed,
    UpgradeRequired,
    CapabilityMissing,
    AuthRequired,
    TradePermissionRequired,
    PermissionResponseInvalid,
    AuthProbeTimeout,
    AuthProbeUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRemediation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upgrade: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry: Option<String>,
}

impl RuntimeRemediation {
    fn install() -> Self {
        Self {
            install: Some(INSTALL_COMMAND),
            upgrade: None,
            oauth: None,
            api_key: None,
            retry: None,
        }
    }

    fn upgrade() -> Self {
        Self {
            install: None,
            upgrade: Some(INSTALL_COMMAND),
            oauth: None,
            api_key: None,
            retry: None,
        }
    }

    fn authenticate() -> Self {
        Self {
            install: None,
            upgrade: None,
            oauth: Some(OAUTH_LOGIN_COMMAND),
            api_key: Some(API_KEY_CONFIG_COMMAND),
            retry: None,
        }
    }

    fn retry(classes: &[AssetClass]) -> Self {
        let mut command = "onchainos agent trade-kit-readiness".to_string();
        for class in classes {
            command.push_str(" --asset-class ");
            command.push_str(class.as_str());
        }
        Self {
            install: None,
            upgrade: None,
            oauth: None,
            api_key: None,
            retry: Some(command),
        }
    }
}

impl RuntimeReadiness {
    fn all(
        classes: &[AssetClass],
        readiness: RuntimeState,
        reason: RuntimeReason,
        version: Option<String>,
    ) -> Self {
        let checks = classes
            .iter()
            .map(|class| AssetReadinessCheck::new(*class, readiness, reason, Vec::new()))
            .collect();
        Self::from_checks(classes, version, checks)
    }

    fn from_checks(
        classes: &[AssetClass],
        version: Option<String>,
        asset_checks: Vec<AssetReadinessCheck>,
    ) -> Self {
        let (readiness, reason) = aggregate_result(&asset_checks);
        let ready = readiness == RuntimeState::Ready;
        let mut missing_capabilities = Vec::new();
        for capability in asset_checks
            .iter()
            .flat_map(|check| check.missing_capabilities.iter())
        {
            if !missing_capabilities.contains(capability) {
                missing_capabilities.push(capability.clone());
            }
        }
        let remediation = remediation_for(reason, classes);
        Self {
            schema_version: READINESS_SCHEMA_VERSION,
            tool: "trade_kit",
            asset_classes: classes.to_vec(),
            readiness,
            ready,
            reason,
            checked_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            version,
            missing_capabilities,
            remediation,
            asset_checks,
        }
    }
}

fn state_priority(state: RuntimeState) -> u8 {
    match state {
        RuntimeState::Missing => 4,
        RuntimeState::Incompatible => 3,
        RuntimeState::NeedsConfiguration => 2,
        RuntimeState::VerificationUnknown => 1,
        RuntimeState::Ready => 0,
    }
}

fn aggregate_result(checks: &[AssetReadinessCheck]) -> (RuntimeState, RuntimeReason) {
    checks
        .iter()
        .max_by_key(|check| state_priority(check.readiness))
        .map(|check| (check.readiness, check.reason))
        .unwrap_or((
            RuntimeState::VerificationUnknown,
            RuntimeReason::AuthorizationNotChecked,
        ))
}

fn remediation_for(reason: RuntimeReason, classes: &[AssetClass]) -> Option<RuntimeRemediation> {
    match reason {
        RuntimeReason::Ready => None,
        RuntimeReason::CliMissing => Some(RuntimeRemediation::install()),
        RuntimeReason::UpgradeRequired | RuntimeReason::CapabilityMissing => {
            Some(RuntimeRemediation::upgrade())
        }
        RuntimeReason::AuthRequired | RuntimeReason::TradePermissionRequired => {
            Some(RuntimeRemediation::authenticate())
        }
        RuntimeReason::AuthorizationNotChecked
        | RuntimeReason::DiscoveryTimeout
        | RuntimeReason::DiscoveryFailed
        | RuntimeReason::PermissionResponseInvalid
        | RuntimeReason::AuthProbeTimeout
        | RuntimeReason::AuthProbeUnavailable => Some(RuntimeRemediation::retry(classes)),
    }
}

#[derive(Debug)]
enum CommandOutcome {
    Finished {
        success: bool,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        stdout_truncated: bool,
        stderr_truncated: bool,
    },
    TimedOut,
    Unavailable,
}

/// Re-check Trade Kit after venue selection and before every selected Trade Kit
/// delivery. One batch shares discovery and at most one private read-only call.
pub async fn probe_runtime(classes: &[AssetClass]) -> RuntimeReadiness {
    let mut unique = Vec::new();
    for class in classes {
        if *class != AssetClass::Defi && !unique.contains(class) {
            unique.push(*class);
        }
    }
    if unique.is_empty() {
        return RuntimeReadiness::all(
            &[],
            RuntimeState::VerificationUnknown,
            RuntimeReason::AuthorizationNotChecked,
            None,
        );
    }
    let local = probe_local();
    let Some(cli_path) = local.cli_path else {
        return RuntimeReadiness::all(
            &unique,
            RuntimeState::Missing,
            RuntimeReason::CliMissing,
            None,
        );
    };
    probe_runtime_with_cli(&cli_path, &unique).await
}

async fn probe_runtime_with_cli(cli_path: &Path, classes: &[AssetClass]) -> RuntimeReadiness {
    let snapshot = match evaluate_discovery(
        run_bounded(
            cli_path,
            &["list-tools", "--json"],
            DISCOVERY_TIMEOUT,
            Some(MAX_DISCOVERY_STDOUT),
        )
        .await,
    ) {
        Ok(snapshot) => snapshot,
        Err(reason) => {
            return RuntimeReadiness::all(classes, RuntimeState::VerificationUnknown, reason, None)
        }
    };

    if !version_at_least(&snapshot.version, MIN_OAUTH_CLI_VERSION) {
        return RuntimeReadiness::all(
            classes,
            RuntimeState::Incompatible,
            RuntimeReason::UpgradeRequired,
            Some(snapshot.version),
        );
    }

    let capability_checks: Vec<AssetReadinessCheck> = classes
        .iter()
        .map(|class| {
            let missing = missing_capabilities(&snapshot, *class);
            if missing.is_empty() {
                AssetReadinessCheck::new(
                    *class,
                    RuntimeState::VerificationUnknown,
                    RuntimeReason::AuthorizationNotChecked,
                    Vec::new(),
                )
            } else {
                AssetReadinessCheck::new(
                    *class,
                    RuntimeState::Incompatible,
                    RuntimeReason::CapabilityMissing,
                    missing,
                )
            }
        })
        .collect();
    if capability_checks
        .iter()
        .any(|check| check.readiness == RuntimeState::Incompatible)
    {
        return RuntimeReadiness::from_checks(classes, Some(snapshot.version), capability_checks);
    }

    let version = snapshot.version;
    evaluate_auth(
        classes,
        version,
        run_bounded(
            cli_path,
            &["account", "config", "--json"],
            AUTH_TIMEOUT,
            Some(MAX_AUTH_STDOUT),
        )
        .await,
    )
}

fn evaluate_discovery(discovery: CommandOutcome) -> Result<CapabilitySnapshot, RuntimeReason> {
    match discovery {
        CommandOutcome::TimedOut => Err(RuntimeReason::DiscoveryTimeout),
        CommandOutcome::Unavailable => Err(RuntimeReason::DiscoveryFailed),
        CommandOutcome::Finished {
            success: true,
            stdout,
            stdout_truncated: false,
            ..
        } => std::str::from_utf8(&stdout)
            .ok()
            .and_then(|raw| CapabilitySnapshot::from_list_tools_json(raw).ok())
            .ok_or(RuntimeReason::DiscoveryFailed),
        CommandOutcome::Finished { .. } => Err(RuntimeReason::DiscoveryFailed),
    }
}

fn evaluate_auth(
    classes: &[AssetClass],
    version: String,
    outcome: CommandOutcome,
) -> RuntimeReadiness {
    match outcome {
        CommandOutcome::Finished {
            success: true,
            stdout,
            stdout_truncated,
            ..
        } => match parse_trade_permission(&stdout, stdout_truncated) {
            Ok(true) => RuntimeReadiness::all(
                classes,
                RuntimeState::Ready,
                RuntimeReason::Ready,
                Some(version),
            ),
            Ok(false) => RuntimeReadiness::all(
                classes,
                RuntimeState::NeedsConfiguration,
                RuntimeReason::TradePermissionRequired,
                Some(version),
            ),
            Err(()) => RuntimeReadiness::all(
                classes,
                RuntimeState::VerificationUnknown,
                RuntimeReason::PermissionResponseInvalid,
                Some(version),
            ),
        },
        CommandOutcome::TimedOut => RuntimeReadiness::all(
            classes,
            RuntimeState::VerificationUnknown,
            RuntimeReason::AuthProbeTimeout,
            Some(version),
        ),
        CommandOutcome::Unavailable => RuntimeReadiness::all(
            classes,
            RuntimeState::VerificationUnknown,
            RuntimeReason::AuthProbeUnavailable,
            Some(version),
        ),
        CommandOutcome::Finished {
            success: false,
            stderr,
            stderr_truncated,
            ..
        } => {
            let reason = classify_auth_failure(&stderr, stderr_truncated);
            let readiness = if reason == RuntimeReason::AuthRequired {
                RuntimeState::NeedsConfiguration
            } else {
                RuntimeState::VerificationUnknown
            };
            RuntimeReadiness::all(classes, readiness, reason, Some(version))
        }
    }
}

/// Parse only the documented permission field from Trade Kit's JSON output.
/// The CLI currently prints the response `data` array directly; an envelope with
/// `data` is accepted for environments that request output context. No other
/// account field survives this function.
fn parse_trade_permission(stdout: &[u8], stdout_truncated: bool) -> Result<bool, ()> {
    if stdout_truncated {
        return Err(());
    }
    let raw = std::str::from_utf8(stdout).map_err(|_| ())?;
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|_| ())?;
    let rows = if let Some(rows) = value.as_array() {
        rows
    } else {
        value
            .get("data")
            .and_then(serde_json::Value::as_array)
            .ok_or(())?
    };
    let permission = rows
        .first()
        .and_then(serde_json::Value::as_object)
        .and_then(|row| row.get("perm"))
        .and_then(serde_json::Value::as_str)
        .ok_or(())?;
    Ok(permission
        .split(',')
        .map(str::trim)
        .any(|token| token == "trade"))
}

async fn run_bounded(
    executable: &Path,
    args: &[&str],
    timeout: Duration,
    stdout_cap: Option<usize>,
) -> CommandOutcome {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut command = trade_kit_command(executable);
    command
        .args(args)
        .env("OKX_UPDATE_CHECK", "false")
        .stdin(Stdio::null())
        .stdout(if stdout_cap.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return CommandOutcome::Unavailable,
    };
    let mut stdout_reader = child
        .stdout
        .take()
        .zip(stdout_cap)
        .map(|(reader, cap)| tokio::spawn(read_capped(reader, cap)));
    let mut stderr_reader = child
        .stderr
        .take()
        .map(|reader| tokio::spawn(read_capped(reader, MAX_CHILD_STDERR)));

    let status = match tokio::time::timeout_at(deadline, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(_)) => {
            terminate_child(&mut child).await;
            abort_readers(stdout_reader, stderr_reader).await;
            return CommandOutcome::Unavailable;
        }
        Err(_) => {
            terminate_child(&mut child).await;
            abort_readers(stdout_reader, stderr_reader).await;
            return CommandOutcome::TimedOut;
        }
    };

    let ((stdout, stdout_truncated), (stderr, stderr_truncated)) =
        match tokio::time::timeout_at(deadline, async {
            tokio::join!(
                read_result(&mut stdout_reader),
                read_result(&mut stderr_reader)
            )
        })
        .await
        {
            Ok(output) => output,
            Err(_) => {
                abort_readers(stdout_reader, stderr_reader).await;
                return CommandOutcome::TimedOut;
            }
        };
    CommandOutcome::Finished {
        success: status.success(),
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    }
}

async fn terminate_child(child: &mut tokio::process::Child) {
    let _ = child.start_kill();
    let _ = tokio::time::timeout(KILL_REAP_TIMEOUT, child.wait()).await;
}

async fn abort_readers(
    stdout: Option<tokio::task::JoinHandle<(Vec<u8>, bool)>>,
    stderr: Option<tokio::task::JoinHandle<(Vec<u8>, bool)>>,
) {
    async fn abort(reader: Option<tokio::task::JoinHandle<(Vec<u8>, bool)>>) {
        if let Some(reader) = reader {
            reader.abort();
            let _ = reader.await;
        }
    }
    tokio::join!(abort(stdout), abort(stderr));
}

fn trade_kit_command(executable: &Path) -> tokio::process::Command {
    #[cfg(windows)]
    {
        let extension = executable
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if extension == "cmd" || extension == "bat" {
            let mut command = tokio::process::Command::new("cmd");
            command.arg("/C").arg(executable);
            return command;
        }
    }
    tokio::process::Command::new(executable)
}

async fn read_capped<R: AsyncRead + Unpin>(mut reader: R, cap: usize) -> (Vec<u8>, bool) {
    let mut retained = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let read = match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let remaining = cap.saturating_sub(retained.len());
        if remaining > 0 {
            retained.extend_from_slice(&buffer[..read.min(remaining)]);
        }
        if read > remaining {
            truncated = true;
        }
    }
    (retained, truncated)
}

async fn read_result(
    reader: &mut Option<tokio::task::JoinHandle<(Vec<u8>, bool)>>,
) -> (Vec<u8>, bool) {
    match reader.as_mut() {
        Some(reader) => reader.await.unwrap_or_default(),
        None => (Vec::new(), false),
    }
}

fn version_at_least(current: &str, minimum: &str) -> bool {
    fn core(value: &str) -> Option<((u64, u64, u64), bool)> {
        let normalized = value
            .trim()
            .trim_start_matches('v')
            .split_once('+')
            .map(|(core, _)| core)
            .unwrap_or_else(|| value.trim().trim_start_matches('v'));
        let mut release_and_pre = normalized.splitn(2, '-');
        let base = release_and_pre.next()?;
        let is_prerelease = release_and_pre.next().is_some();
        let mut parts = base.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(((major, minor, patch), is_prerelease))
    }

    match (core(current), core(minimum)) {
        (Some((current_core, current_pre)), Some((minimum_core, minimum_pre))) => {
            current_core > minimum_core
                || (current_core == minimum_core && (!current_pre || minimum_pre))
        }
        _ => false,
    }
}

fn classify_auth_failure(stderr: &[u8], stderr_truncated: bool) -> RuntimeReason {
    if stderr_truncated {
        return RuntimeReason::AuthProbeUnavailable;
    }
    let message = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    const AUTH_MARKERS: &[&str] = &[
        "no credentials found",
        "not logged in",
        "partial api credentials",
        "token refresh failed",
        "run `okx auth login`",
        "re-run: okx config init",
        "invalid token",
        "invalid ok-access-key",
        "invalid sign",
        "api key lacks",
        "api key expired",
        "passphrase is incorrect",
        "failed to spawn okx-auth",
        "okx-auth rejected",
        "code: 50100",
        "code: 50110",
        "code: 50111",
        "code: 50112",
        "code: 50113",
    ];
    if AUTH_MARKERS.iter().any(|marker| message.contains(marker)) {
        return RuntimeReason::AuthRequired;
    }

    const UNAVAILABLE_MARKERS: &[&str] = &[
        "networkerror",
        "network error",
        "fetch failed",
        "econn",
        "dns",
        "timed out",
        "timeout",
        "connection",
        "proxy",
    ];
    if UNAVAILABLE_MARKERS
        .iter()
        .any(|marker| message.contains(marker))
    {
        return RuntimeReason::AuthProbeUnavailable;
    }
    RuntimeReason::AuthProbeUnavailable
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

    #[test]
    fn runtime_asset_class_parser_accepts_only_canonical_trade_kit_classes() {
        for (raw, expected) in [
            ("spot", AssetClass::Spot),
            ("perp", AssetClass::Perp),
            ("prediction", AssetClass::Prediction),
            ("option", AssetClass::Option),
        ] {
            assert_eq!(parse_runtime_asset_class(raw), Ok(expected));
        }
        for raw in ["defi", "futures", "options", "SPOT", "", "unknown"] {
            assert_eq!(
                parse_runtime_asset_class(raw),
                Err("asset class must be spot, perp, prediction, or option"),
                "raw={raw}"
            );
        }

        let values = vec!["spot".to_string(), "perp".to_string(), "spot".to_string()];
        assert_eq!(
            parse_runtime_asset_classes(&values),
            Ok(vec![AssetClass::Spot, AssetClass::Perp])
        );
        assert_eq!(
            parse_runtime_asset_classes(&[]),
            Err("at least one --asset-class is required")
        );
    }

    #[test]
    fn every_supported_asset_class_has_a_fail_closed_capability_matrix() {
        let classes = [
            AssetClass::Spot,
            AssetClass::Perp,
            AssetClass::Prediction,
            AssetClass::Option,
        ];
        let all_tools: BTreeSet<String> = classes
            .iter()
            .flat_map(|class| required_capabilities(*class).iter())
            .map(|name| (*name).to_string())
            .collect();
        let full = CapabilitySnapshot {
            version: "1.4.2".to_string(),
            tool_names: all_tools.clone(),
        };

        for class in classes {
            assert!(missing_capabilities(&full, class).is_empty());

            let omitted = required_capabilities(class)
                .last()
                .expect("every Trade Kit class has an execution capability");
            let mut without_one = all_tools.clone();
            without_one.remove(*omitted);
            let incomplete = CapabilitySnapshot {
                version: "1.4.2".to_string(),
                tool_names: without_one,
            };
            assert_eq!(
                missing_capabilities(&incomplete, class),
                vec![(*omitted).to_string()],
                "class={}",
                class.as_str()
            );
        }
    }

    #[test]
    fn runtime_result_serialization_keeps_the_stable_top_level_shape() {
        let ready = serde_json::to_value(RuntimeReadiness::all(
            &[AssetClass::Spot, AssetClass::Perp],
            RuntimeState::Ready,
            RuntimeReason::Ready,
            Some("1.4.2".to_string()),
        ))
        .unwrap();
        assert_eq!(ready["schemaVersion"], 2);
        assert_eq!(ready["assetClasses"], serde_json::json!(["spot", "perp"]));
        assert_eq!(ready["readiness"], "ready");
        assert_eq!(ready["ready"], true);
        assert!(ready["checkedAt"].as_str().is_some());
        assert_eq!(ready["assetChecks"].as_array().unwrap().len(), 2);
        assert_eq!(ready["version"], "1.4.2");
        assert!(ready.get("remediation").is_some());
        assert!(ready["remediation"].is_null());

        let missing = serde_json::to_value(RuntimeReadiness::all(
            &[AssetClass::Spot],
            RuntimeState::Missing,
            RuntimeReason::CliMissing,
            None,
        ))
        .unwrap();
        assert!(missing.get("version").is_some());
        assert!(missing["version"].is_null());
        assert_eq!(missing["remediation"]["install"], INSTALL_COMMAND);
    }

    #[test]
    fn oauth_version_boundary_is_explicit() {
        assert!(!version_at_least("1.3.1", MIN_OAUTH_CLI_VERSION));
        assert!(!version_at_least("1.3.2-beta.7", MIN_OAUTH_CLI_VERSION));
        assert!(version_at_least("1.3.2", MIN_OAUTH_CLI_VERSION));
        assert!(version_at_least("v1.3.2+build.9", MIN_OAUTH_CLI_VERSION));
        assert!(version_at_least("1.4.3-beta.2", MIN_OAUTH_CLI_VERSION));
        assert!(!version_at_least("not-a-version", MIN_OAUTH_CLI_VERSION));
    }

    #[test]
    fn discovery_timeout_and_oversized_output_fail_closed_with_typed_reasons() {
        let timed_out =
            evaluate_discovery(CommandOutcome::TimedOut).expect_err("timeout cannot be ready");
        assert_eq!(timed_out, RuntimeReason::DiscoveryTimeout);
        let result = RuntimeReadiness::all(
            &[AssetClass::Spot, AssetClass::Perp],
            RuntimeState::VerificationUnknown,
            timed_out,
            None,
        );
        assert_eq!(
            result.remediation.unwrap().retry.as_deref(),
            Some("onchainos agent trade-kit-readiness --asset-class spot --asset-class perp")
        );

        let oversized = evaluate_discovery(CommandOutcome::Finished {
            success: true,
            stdout: br#"{"version":"1.4.2","modules":[]}"#.to_vec(),
            stderr: Vec::new(),
            stdout_truncated: true,
            stderr_truncated: false,
        })
        .expect_err("truncated discovery cannot be trusted");
        assert_eq!(oversized, RuntimeReason::DiscoveryFailed);

        let nonzero = evaluate_discovery(CommandOutcome::Finished {
            success: false,
            stdout: Vec::new(),
            stderr: b"opaque discovery failure".to_vec(),
            stdout_truncated: false,
            stderr_truncated: false,
        })
        .expect_err("non-zero discovery cannot be trusted");
        assert_eq!(nonzero, RuntimeReason::DiscoveryFailed);
    }

    #[test]
    fn permission_parser_requires_an_exact_trade_token_and_accepts_both_shapes() {
        assert_eq!(
            parse_trade_permission(br#"[{"perm":"read_only, trade","uid":"SECRET"}]"#, false),
            Ok(true)
        );
        assert_eq!(
            parse_trade_permission(br#"{"data":[{"perm":"trade"}]}"#, false),
            Ok(true)
        );
        for raw in [
            br#"[{"perm":"read_only"}]"#.as_slice(),
            br#"[{"perm":"trade_history"}]"#.as_slice(),
            br#"[{"perm":"read_only,trader"}]"#.as_slice(),
        ] {
            assert_eq!(parse_trade_permission(raw, false), Ok(false));
        }
        for raw in [
            br#"[{"uid":"no-perm"}]"#.as_slice(),
            br#"{"data":[]}"#.as_slice(),
            b"not-json".as_slice(),
        ] {
            assert_eq!(parse_trade_permission(raw, false), Err(()));
        }
        assert_eq!(
            parse_trade_permission(br#"[{"perm":"trade"}]"#, true),
            Err(())
        );
    }

    #[test]
    fn auth_failures_are_distinct_from_transport_failures() {
        for message in [
            "Error: No credentials found.",
            "Error: Partial API credentials detected.",
            "Error: Token refresh failed.",
            "Code: 50100 API key lacks required permissions",
            "Code: 50111 Invalid OK-ACCESS-KEY",
            "Code: 50112 Invalid Sign",
            "Code: 50113 Passphrase is incorrect",
        ] {
            assert_eq!(
                classify_auth_failure(message.as_bytes(), false),
                RuntimeReason::AuthRequired,
                "message={message}"
            );
            let readiness = evaluate_auth(
                &[AssetClass::Spot],
                "1.4.2".to_string(),
                CommandOutcome::Finished {
                    success: false,
                    stdout: Vec::new(),
                    stderr: message.as_bytes().to_vec(),
                    stdout_truncated: false,
                    stderr_truncated: false,
                },
            );
            assert_eq!(readiness.reason, RuntimeReason::AuthRequired);
            let remediation = readiness.remediation.unwrap();
            assert_eq!(remediation.oauth, Some(OAUTH_LOGIN_COMMAND));
            assert_eq!(remediation.api_key, Some(API_KEY_CONFIG_COMMAND));
        }
        assert_eq!(
            classify_auth_failure(b"NetworkError: fetch failed ECONNREFUSED", false),
            RuntimeReason::AuthProbeUnavailable
        );
        assert_eq!(
            classify_auth_failure(b"opaque upstream failure", false),
            RuntimeReason::AuthProbeUnavailable
        );
        assert_eq!(
            classify_auth_failure(b"No credentials found", true),
            RuntimeReason::AuthProbeUnavailable,
            "truncated stderr must never be treated as conclusive auth evidence"
        );
    }

    #[test]
    fn auth_timeout_and_unavailable_results_never_offer_login_guidance() {
        for outcome in [
            CommandOutcome::TimedOut,
            CommandOutcome::Unavailable,
            CommandOutcome::Finished {
                success: false,
                stdout: Vec::new(),
                stderr: b"No credentials found".to_vec(),
                stdout_truncated: false,
                stderr_truncated: true,
            },
            CommandOutcome::Finished {
                success: false,
                stdout: Vec::new(),
                stderr: b"opaque upstream failure".to_vec(),
                stdout_truncated: false,
                stderr_truncated: false,
            },
        ] {
            let readiness = evaluate_auth(&[AssetClass::Spot], "1.4.2".to_string(), outcome);
            assert!(!readiness.ready);
            assert!(matches!(
                readiness.reason,
                RuntimeReason::AuthProbeTimeout | RuntimeReason::AuthProbeUnavailable
            ));
            let remediation = readiness.remediation.unwrap();
            assert!(remediation.oauth.is_none());
            assert!(remediation.api_key.is_none());
            assert!(remediation.retry.is_some());
        }
    }

    #[tokio::test]
    async fn capped_reader_drains_but_retains_only_the_bound() {
        use tokio::io::AsyncWriteExt;

        let (reader, mut writer) = tokio::io::duplex(32);
        let write = tokio::spawn(async move {
            writer.write_all(b"0123456789").await.unwrap();
        });
        let (retained, truncated) = read_capped(reader, 4).await;
        write.await.unwrap();
        assert_eq!(retained, b"0123");
        assert!(truncated);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bounded_runner_kills_a_hung_child() {
        let started = std::time::Instant::now();
        let outcome = run_bounded(
            Path::new("/bin/sh"),
            &["-c", "exec sleep 2"],
            Duration::from_millis(25),
            None,
        )
        .await;
        assert!(matches!(outcome, CommandOutcome::TimedOut));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bounded_runner_does_not_wait_forever_for_a_descendant_held_pipe() {
        let started = std::time::Instant::now();
        let outcome = run_bounded(
            Path::new("/bin/sh"),
            &["-c", "sleep 2 >&2 & exit 0"],
            Duration::from_millis(25),
            None,
        )
        .await;
        assert!(matches!(outcome, CommandOutcome::TimedOut));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
