//! Business-domain execution bridge for model-routed subscription trades.
//!
//! The target CLI/plugin stays unchanged. This bridge runs a fixed executable
//! for the selected venue (never a shell), captures its terminal result,
//! persists a redacted outcome, and pushes a job-scoped idempotent UI notice.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::process::Command;
use tokio::time::timeout;

use super::amount::Decimal;
use super::{consent, grants, trade_kit};
use crate::asset_class::AssetClass;
use crate::commands::agent_commerce::task::common::{okx_a2a, user_lang};

const OUTCOME_VERSION: u32 = 1;
const MAX_ARG_COUNT: usize = 96;
const MAX_ARG_LEN: usize = 4096;
const MAX_TIMEOUT_SEC: u64 = 600;
const ONE_TIME_PERMIT_VERSION: u32 = 1;
const ONE_TIME_PERMIT_TTL_SEC: u64 = 15 * 60;
const NOTICE_REF_VERSION: u32 = 1;
const EXECUTION_LATCH_VERSION: u32 = 1;
const TERMINAL_JOURNAL_VERSION: u32 = 1;
const GLOBAL_RETRY_BUDGET: Duration = Duration::from_millis(300);
const INITIAL_NOTIFY_TIMEOUT: Duration = Duration::from_secs(1);
const STALE_LEASE_SEC: u64 = 30;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum OutcomeStatus {
    Submitted,
    FailedBeforeSubmit,
    UnknownAfterSubmit,
    Skipped,
    FailedBeforeExecution,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ExecutionPhase {
    Reserved,
    Prepared,
    Spawned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExecutionLatch {
    version: u32,
    job_id: String,
    delivery_id: String,
    phase: ExecutionPhase,
    updated_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryState {
    NoExecution,
    PreSubmitInterrupted,
    SubmissionUnknown,
    TerminalOutcome,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    #[default]
    Auto,
    Manual,
    OneTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AuthorizedTradeKitSettings {
    environment: Option<trade_kit::TradeEnvironment>,
    margin_mode: Option<consent::MarginMode>,
    order_policy: Option<consent::OrderPolicy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TradeKitOperation {
    Place,
    ClosePosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TradeKitExecutionContext {
    asset_class: AssetClass,
    environment: trade_kit::TradeEnvironment,
    operation: TradeKitOperation,
}

impl ExecutionMode {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "auto" => Ok(Self::Auto),
            "manual" => Ok(Self::Manual),
            "one_time" => Ok(Self::OneTime),
            _ => bail!("execution mode must be auto, manual, or one_time"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OneTimePermit {
    version: u32,
    job_id: String,
    delivery_id: String,
    amount: String,
    created_at: u64,
    expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionOutcome {
    version: u32,
    job_id: String,
    delivery_id: String,
    venue: String,
    action: String,
    amount: String,
    #[serde(default)]
    execution_mode: ExecutionMode,
    status: OutcomeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    notification_pending: bool,
    #[serde(default)]
    notification_attempts: u32,
    #[serde(default)]
    next_notification_attempt_at: u64,
    created_at: u64,
    updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OutcomeNoticeRef {
    version: u32,
    job_id: String,
    delivery_id: String,
    next_attempt_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TerminalJournal {
    version: u32,
    outcome: ExecutionOutcome,
}

pub struct ExecuteRequest<'a> {
    pub job_id: &'a str,
    pub delivery_id: &'a str,
    pub venue: &'a str,
    pub action: &'a str,
    pub amount: &'a str,
    pub execution_mode: ExecutionMode,
    pub command_json: &'a str,
    pub timeout_sec: u64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn safe_delivery_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b':' | b'.'))
}

fn outcome_path(job_id: &str, delivery_id: &str) -> Result<PathBuf> {
    if !grants::job_id_is_safe(job_id) || !safe_delivery_id(delivery_id) {
        bail!("invalid job or delivery id");
    }
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("outcomes")
        .join(job_id)
        .join(format!("{delivery_id}.json")))
}

fn latch_path(job_id: &str, delivery_id: &str) -> Result<PathBuf> {
    if !grants::job_id_is_safe(job_id) || !safe_delivery_id(delivery_id) {
        bail!("invalid job or delivery id");
    }
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("execution-latch")
        .join(job_id)
        .join(delivery_id))
}

fn terminal_journal_path(job_id: &str, delivery_id: &str) -> Result<PathBuf> {
    if !grants::job_id_is_safe(job_id) || !safe_delivery_id(delivery_id) {
        bail!("invalid job or delivery id");
    }
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("terminal-journal")
        .join(job_id)
        .join(format!("{delivery_id}.json")))
}

fn one_time_permit_path(job_id: &str, delivery_id: &str) -> Result<PathBuf> {
    if !grants::job_id_is_safe(job_id) || !safe_delivery_id(delivery_id) {
        bail!("invalid job or delivery id");
    }
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("one-time-permits")
        .join(job_id)
        .join(format!("{delivery_id}.json")))
}

fn notice_index_root() -> Result<PathBuf> {
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("pending-outcome-notifications"))
}

fn notice_ref_path(job_id: &str, delivery_id: &str) -> Result<PathBuf> {
    if !grants::job_id_is_safe(job_id) || !safe_delivery_id(delivery_id) {
        bail!("invalid job or delivery id");
    }
    let name = hex::encode(Sha256::digest(format!("{job_id}\0{delivery_id}")));
    Ok(notice_index_root()?.join(format!("{name}.json")))
}

fn sync_notice_ref(outcome: &ExecutionOutcome) -> Result<()> {
    let path = notice_ref_path(&outcome.job_id, &outcome.delivery_id)?;
    if !outcome.notification_pending {
        let _ = std::fs::remove_file(path);
        return Ok(());
    }
    crate::home::write_secure(
        &path,
        &serde_json::to_vec_pretty(&OutcomeNoticeRef {
            version: NOTICE_REF_VERSION,
            job_id: outcome.job_id.clone(),
            delivery_id: outcome.delivery_id.clone(),
            next_attempt_at: outcome.next_notification_attempt_at,
        })?,
    )?;
    Ok(())
}

fn reserve_execution(job_id: &str, delivery_id: &str) -> Result<bool> {
    let path = latch_path(job_id, delivery_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            file.write_all(&serde_json::to_vec_pretty(&ExecutionLatch {
                version: EXECUTION_LATCH_VERSION,
                job_id: job_id.to_string(),
                delivery_id: delivery_id.to_string(),
                phase: ExecutionPhase::Reserved,
                updated_at: now_secs(),
            })?)?;
            file.sync_all()?;
            Ok(true)
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn update_execution_phase(
    job_id: &str,
    delivery_id: &str,
    phase: ExecutionPhase,
) -> Result<()> {
    crate::home::write_secure(
        &latch_path(job_id, delivery_id)?,
        &serde_json::to_vec_pretty(&ExecutionLatch {
            version: EXECUTION_LATCH_VERSION,
            job_id: job_id.to_string(),
            delivery_id: delivery_id.to_string(),
            phase,
            updated_at: now_secs(),
        })?,
    )?;
    Ok(())
}

fn read_execution_phase(job_id: &str, delivery_id: &str) -> Result<Option<ExecutionPhase>> {
    let path = latch_path(job_id, delivery_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let latch: ExecutionLatch = serde_json::from_slice(&std::fs::read(path)?)?;
    if latch.version != EXECUTION_LATCH_VERSION
        || latch.job_id != job_id
        || latch.delivery_id != delivery_id
    {
        bail!("execution latch mismatch");
    }
    Ok(Some(latch.phase))
}

pub(crate) fn recovery_state(job_id: &str, delivery_id: &str) -> Result<RecoveryState> {
    if read_outcome(&outcome_path(job_id, delivery_id)?)?.is_some() {
        return Ok(RecoveryState::TerminalOutcome);
    }
    let path = latch_path(job_id, delivery_id)?;
    if !path.exists() {
        return Ok(RecoveryState::NoExecution);
    }
    match read_execution_phase(job_id, delivery_id) {
        Ok(Some(ExecutionPhase::Reserved | ExecutionPhase::Prepared)) => {
            Ok(RecoveryState::PreSubmitInterrupted)
        }
        Ok(Some(ExecutionPhase::Spawned)) | Err(_) => Ok(RecoveryState::SubmissionUnknown),
        Ok(None) => Ok(RecoveryState::NoExecution),
    }
}

fn read_outcome(path: &Path) -> Result<Option<ExecutionOutcome>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read(path)?;
    let outcome: ExecutionOutcome = serde_json::from_slice(&raw)?;
    if outcome.version != OUTCOME_VERSION {
        bail!("unsupported automatic execution outcome version");
    }
    Ok(Some(outcome))
}

fn write_outcome(path: &Path, outcome: &ExecutionOutcome) -> Result<()> {
    crate::home::write_secure(
        path,
        &serde_json::to_vec_pretty(outcome)?,
    )?;
    if let Err(error) = sync_notice_ref(outcome) {
        // The durable execution outcome is authoritative. A secondary index
        // failure must never turn a submitted/unknown transaction into a CLI
        // error; explicit job-scoped flush can still recover from the outcome.
        eprintln!("[autotrade] pending-notification index update failed: {error}");
    }
    Ok(())
}

fn write_terminal_journal(outcome: &ExecutionOutcome) -> Result<PathBuf> {
    let path = terminal_journal_path(&outcome.job_id, &outcome.delivery_id)?;
    crate::home::write_secure(
        &path,
        &serde_json::to_vec_pretty(&TerminalJournal {
            version: TERMINAL_JOURNAL_VERSION,
            outcome: outcome.clone(),
        })?,
    )?;
    Ok(path)
}

fn read_terminal_journal(path: &Path) -> Result<TerminalJournal> {
    let journal: TerminalJournal = serde_json::from_slice(&std::fs::read(path)?)?;
    if journal.version != TERMINAL_JOURNAL_VERSION {
        bail!("unsupported terminal journal version");
    }
    Ok(journal)
}

fn terminal_reconciliation_complete(outcome: &ExecutionOutcome) -> bool {
    if sync_notice_ref(outcome).is_err() {
        return false;
    }
    super::delivery_queue::contains_delivery(&outcome.job_id, &outcome.delivery_id)
        .map(|present| !present)
        .unwrap_or(false)
}

fn parse_command(venue: &str, command_json: &str) -> Result<(PathBuf, Vec<String>)> {
    let mut args: Vec<String> = serde_json::from_str(command_json)
        .context("--command-json must be a JSON array of argument strings")?;
    if args.is_empty() || args.len() > MAX_ARG_COUNT {
        bail!("invalid automatic execution argument count");
    }
    if args.iter().any(|arg| {
        arg.is_empty()
            || arg.len() > MAX_ARG_LEN
            || arg.contains('\0')
            || arg.contains('\n')
            || arg.contains('\r')
    }) {
        bail!("invalid automatic execution argument");
    }
    if args
        .windows(2)
        .any(|pair| pair[0] == "agent" && pair[1] == "autotrade-execute")
    {
        bail!("recursive automatic execution is not allowed");
    }

    let program = match venue {
        "dex" => {
            if args.get(0).map(String::as_str) != Some("swap")
                || args.get(1).map(String::as_str) != Some("execute")
            {
                bail!("dex automatic execution requires `swap execute`");
            }
            if args.iter().any(|arg| arg == "--notify-job-id") {
                bail!("the execution bridge owns outcome notification");
            }
            std::env::current_exe()?
        }
        "defi" => {
            let operation = args.get(1).map(String::as_str);
            if args.get(0).map(String::as_str) != Some("defi")
                || !matches!(operation, Some("deposit" | "redeem" | "collect"))
            {
                bail!("defi automatic execution requires a supported write operation");
            }
            std::env::current_exe()?
        }
        "trade_kit" => PathBuf::from("okx"),
        "polymarket" => {
            if !matches!(args.first().map(String::as_str), Some("buy" | "sell")) {
                bail!("polymarket automatic execution requires buy or sell");
            }
            PathBuf::from("polymarket-plugin")
        }
        "hyperliquid" => match args.first().map(String::as_str) {
            Some("order" | "close" | "tpsl" | "spot-order" | "order-batch") => {
                PathBuf::from("hyperliquid")
            }
            Some("outcome-buy" | "outcome-sell") => PathBuf::from("hyperliquid-plugin"),
            _ => bail!("unsupported hyperliquid automatic trade operation"),
        },
        _ => bail!("unsupported automatic execution venue"),
    };
    if venue == "trade_kit" {
        normalize_trade_kit_dash_values(&mut args);
    }
    Ok((program, args))
}

/// Node's `util.parseArgs` treats a following `-1` as another option rather
/// than as the value of a string option. Trade Kit uses `-1` as the documented
/// market-order sentinel for attached TP/SL, so canonicalize only those known
/// flags to the equivalent `--flag=-1` argv representation before spawning.
fn normalize_trade_kit_dash_values(args: &mut Vec<String>) {
    const MARKET_SENTINEL_FLAGS: &[&str] = &["--tpOrdPx", "--slOrdPx"];
    let mut index = 0;
    while index + 1 < args.len() {
        if MARKET_SENTINEL_FLAGS.contains(&args[index].as_str()) && args[index + 1] == "-1" {
            args[index] = format!("{}=-1", args[index]);
            args.remove(index + 1);
        }
        index += 1;
    }
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn require_same_amount(actual: Option<&str>, expected: &str, label: &str) -> Result<()> {
    let actual = actual.with_context(|| format!("{label} is missing"))?;
    let actual = Decimal::parse(actual)
        .with_context(|| format!("{label} is invalid"))?
        .to_plain_string();
    if actual != expected {
        bail!("{label} does not match the persisted policy amount");
    }
    Ok(())
}

fn read_one_time_permit(path: &Path) -> Result<Option<OneTimePermit>> {
    if !path.exists() {
        return Ok(None);
    }
    let permit: OneTimePermit = serde_json::from_slice(&std::fs::read(path)?)?;
    if permit.version != ONE_TIME_PERMIT_VERSION {
        bail!("unsupported one-time execution permit version");
    }
    Ok(Some(permit))
}

/// Create the durable authorization used by the legacy over-cap A option.
/// The permit is bound to one admitted delivery and exact amount; the normal
/// execution latch still guarantees that it can spawn at most one command.
pub fn authorize_one_time(
    job_id: &str,
    delivery_id: &str,
    amount: &str,
) -> Result<OneTimePermit> {
    let context = consent::load_delivery_context(job_id, delivery_id)
        .context("trusted delivery context is unavailable")?;
    let pending = consent::load_pending_delivery_context(job_id)?
        .context("no delivery is awaiting a one-time execution decision")?;
    if context != pending || context.delivery_id != delivery_id {
        bail!("one-time authorization does not match the pending delivery");
    }
    if latch_path(job_id, delivery_id)?.exists() {
        bail!("delivery already has a terminal execution outcome");
    }
    let normalized = Decimal::parse(amount)
        .context("invalid one-time execution amount")?
        .to_plain_string();
    if normalized == "0" {
        bail!("one-time execution amount must be positive");
    }
    let policy = consent::load_consent(job_id)
        .map_err(|error| anyhow::anyhow!(error.0))?
        .context("auto-trade consent is missing or expired")?;
    if policy.mode != consent::ConsentMode::Auto {
        bail!("one-time over-cap authorization requires an active auto policy");
    }
    let cap = Decimal::parse(
        policy
            .cap_u
            .as_deref()
            .context("auto-trade cap is missing")?,
    )?;
    let requested = Decimal::parse(&normalized)?;
    if requested.le(&cap) {
        bail!("one-time authorization is only valid for an amount above the current cap");
    }

    let path = one_time_permit_path(job_id, delivery_id)?;
    if let Some(existing) = read_one_time_permit(&path)? {
        if existing.job_id == job_id
            && existing.delivery_id == delivery_id
            && existing.amount == normalized
            && existing.expires_at > now_secs()
        {
            return Ok(existing);
        }
        if existing.expires_at > now_secs() {
            bail!("a different live one-time permit already exists for this delivery");
        }
        let _ = std::fs::remove_file(&path);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    let created_at = now_secs();
    let permit = OneTimePermit {
        version: ONE_TIME_PERMIT_VERSION,
        job_id: job_id.to_string(),
        delivery_id: delivery_id.to_string(),
        amount: normalized,
        created_at,
        expires_at: created_at.saturating_add(ONE_TIME_PERMIT_TTL_SEC),
    };
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .context("one-time permit was concurrently replaced")?;
    file.write_all(&serde_json::to_vec_pretty(&permit)?)?;
    file.sync_all()?;
    Ok(permit)
}

fn validate_one_time_permit(job_id: &str, delivery_id: &str, amount: &str) -> Result<()> {
    let permit = read_one_time_permit(&one_time_permit_path(job_id, delivery_id)?)?
        .context("one-time execution permit is missing")?;
    if permit.job_id != job_id
        || permit.delivery_id != delivery_id
        || permit.amount != amount
        || permit.expires_at <= now_secs()
    {
        bail!("one-time execution permit is invalid or expired");
    }
    Ok(())
}

fn validate_bound_intent(
    venue: &str,
    action: &str,
    amount: &str,
    job_id: &str,
    execution_mode: ExecutionMode,
    args: &[String],
) -> Result<()> {
    match venue {
        "dex" => {
            if args.iter().any(|arg| arg == "--amount") {
                bail!("automatic dex execution requires --readable-amount");
            }
            require_same_amount(
                flag_value(args, "--readable-amount"),
                amount,
                "dex readable amount",
            )?;
        }
        "defi" => match args.get(1).map(String::as_str) {
            Some("deposit") => {
                let input = flag_value(args, "--user-input")
                    .context("defi deposit --user-input is missing")?;
                let value: Value =
                    serde_json::from_str(input).context("defi deposit --user-input is invalid")?;
                let coin_amount = value
                    .as_array()
                    .and_then(|items| items.first())
                    .and_then(|item| item.get("coinAmount"))
                    .and_then(Value::as_str);
                require_same_amount(coin_amount, amount, "defi deposit amount")?;
            }
            Some("redeem") => {
                let expected = Decimal::pct_to_ratio(&Decimal::parse(amount)?)?.to_plain_string();
                require_same_amount(flag_value(args, "--ratio"), &expected, "defi redeem ratio")?;
            }
            Some("collect") => {}
            _ => bail!("unsupported defi automatic execution operation"),
        },
        "trade_kit" => {
            let context = trade_kit_execution_context(args)?;
            match context.operation {
                TradeKitOperation::Place => {
                    let (actual_side, actual_amount) = if context.asset_class == AssetClass::Prediction {
                        let index = args
                            .windows(2)
                            .position(|pair| pair[0] == "event" && pair[1] == "place")
                            .context("Trade Kit event command shape is invalid")?;
                        (
                            args.get(index + 3).map(String::as_str),
                            args.get(index + 5).map(String::as_str),
                        )
                    } else {
                        (flag_value(args, "--side"), flag_value(args, "--sz"))
                    };
                    require_same_amount(actual_amount, amount, "Trade Kit order size")?;
                    if actual_side != Some(action) {
                        bail!("Trade Kit order side does not match the authorized action");
                    }
                }
                TradeKitOperation::ClosePosition => {
                    if flag_value(args, "--sz").is_some() || flag_value(args, "--side").is_some() {
                        bail!("Trade Kit full-position close must not carry order size or side flags");
                    }
                    let position_side = flag_value(args, "--posSide")
                        .context("Trade Kit full-position close requires an explicit position side")?;
                    match (position_side, action) {
                        ("long", "sell") | ("short", "buy") | ("net", "buy" | "sell") => {}
                        ("long", _) | ("short", _) => {
                            bail!("Trade Kit close direction does not match the authorized action")
                        }
                        _ => bail!("Trade Kit close position side must be net, long, or short"),
                    }
                }
            }
        }
        "polymarket" => {
            match execution_mode {
                ExecutionMode::Auto if flag_value(args, "--autotrade-job") != Some(job_id) => {
                    bail!("polymarket command is not bound to this auto-trade job")
                }
                ExecutionMode::Manual | ExecutionMode::OneTime
                    if flag_value(args, "--autotrade-job").is_some() =>
                {
                    bail!("user-confirmed polymarket execution must not use an automatic grant")
                }
                _ => {}
            }
            if args.first().map(String::as_str) != Some(action) {
                bail!("polymarket command side does not match the authorized action");
            }
            if action == "buy" {
                require_same_amount(
                    flag_value(args, "--amount"),
                    amount,
                    "polymarket buy amount",
                )?;
            }
        }
        "hyperliquid" => {
            match execution_mode {
                ExecutionMode::Auto if flag_value(args, "--autotrade-job") != Some(job_id) => {
                    bail!("hyperliquid command is not bound to this auto-trade job")
                }
                ExecutionMode::Manual | ExecutionMode::OneTime
                    if flag_value(args, "--autotrade-job").is_some() =>
                {
                    bail!("user-confirmed hyperliquid execution must not use an automatic grant")
                }
                _ => {}
            }
            if matches!(
                args.first().map(String::as_str),
                Some("order" | "spot-order")
            ) && flag_value(args, "--side") != Some(action)
            {
                bail!("hyperliquid order side does not match the authorized action");
            }
        }
        _ => bail!("unsupported automatic execution venue"),
    }
    Ok(())
}

fn trade_kit_execution_context(args: &[String]) -> Result<TradeKitExecutionContext> {
    let live = args.iter().any(|arg| arg == "--live");
    let demo = args.iter().any(|arg| arg == "--demo");
    let environment = match (live, demo) {
        (true, false) => trade_kit::TradeEnvironment::Live,
        (false, true) => trade_kit::TradeEnvironment::Demo,
        (true, true) => bail!("Trade Kit command cannot select both live and demo trading"),
        (false, false) => {
            bail!("Trade Kit execution requires an explicit --live or --demo flag")
        }
    };
    let (asset_class, operation) = args
        .windows(2)
        .find_map(|pair| match (pair[0].as_str(), pair[1].as_str()) {
            ("spot", "place") => Some((AssetClass::Spot, TradeKitOperation::Place)),
            ("swap" | "futures", "place") => {
                Some((AssetClass::Perp, TradeKitOperation::Place))
            }
            ("swap" | "futures", "close") => {
                Some((AssetClass::Perp, TradeKitOperation::ClosePosition))
            }
            ("option", "place") => Some((AssetClass::Option, TradeKitOperation::Place)),
            ("event", "place") => Some((AssetClass::Prediction, TradeKitOperation::Place)),
            _ => None,
        })
        .context("Trade Kit execution requires a supported place or contract-close command")?;
    Ok(TradeKitExecutionContext {
        asset_class,
        environment,
        operation,
    })
}

fn validate_trade_kit_execution_settings(
    args: &[String],
    context: TradeKitExecutionContext,
    settings: AuthorizedTradeKitSettings,
) -> Result<()> {
    let order_policy = settings
        .order_policy
        .context("Trade Kit execution requires a persisted order policy")?;
    match context.operation {
        TradeKitOperation::Place => {
            let actual = flag_value(args, "--ordType")
                .context("Trade Kit order type is missing from the command")?;
            match order_policy {
                consent::OrderPolicy::Market if actual != "market" => {
                    bail!("Trade Kit order type does not match persisted market policy")
                }
                consent::OrderPolicy::SignalPriceLimit if actual != "limit" => {
                    bail!("Trade Kit order type does not match persisted signal-price limit policy")
                }
                consent::OrderPolicy::SignalPriceLimit
                    if flag_value(args, "--px").map_or(true, str::is_empty) =>
                {
                    bail!("Trade Kit signal-price limit policy requires an explicit order price")
                }
                _ => {}
            }
        }
        TradeKitOperation::ClosePosition => {
            if order_policy != consent::OrderPolicy::Market {
                bail!("Trade Kit full-position close requires a persisted market policy");
            }
            if flag_value(args, "--ordType").is_some() || flag_value(args, "--tdMode").is_some() {
                bail!("Trade Kit full-position close must use close-position margin arguments");
            }
        }
    }

    if let Some(margin_mode) = settings.margin_mode {
        if matches!(context.asset_class, AssetClass::Perp | AssetClass::Option) {
            let margin_flag = match context.operation {
                TradeKitOperation::Place => "--tdMode",
                TradeKitOperation::ClosePosition => "--mgnMode",
            };
            let actual = flag_value(args, margin_flag)
                .context("Trade Kit margin mode is missing from the command")?;
            if actual != margin_mode.as_str() {
                bail!("Trade Kit margin mode does not match persisted consent");
            }
        }
    } else if context.asset_class == AssetClass::Perp {
        bail!("Trade Kit derivative execution requires a persisted margin mode");
    }
    Ok(())
}

fn authorize(
    job_id: &str,
    delivery_id: &str,
    venue: &str,
    action: &str,
    amount: &str,
    execution_mode: ExecutionMode,
) -> Result<(String, AuthorizedTradeKitSettings)> {
    let normalized = Decimal::parse(amount)
        .context("invalid automatic execution amount")?
        .to_plain_string();
    if normalized == "0" {
        bail!("automatic execution amount must be positive");
    }
    let policy = consent::load_consent(job_id)
        .map_err(|error| anyhow::anyhow!(error.0))?
        .context("automatic execution consent is missing or expired")?;
    match (execution_mode, policy.mode) {
        (ExecutionMode::Auto, consent::ConsentMode::Auto) => {
            let stored = policy
                .trade_amount_u
                .as_deref()
                .context("automatic execution policy amount is missing")?;
            if Decimal::parse(stored)?.to_plain_string() != normalized {
                bail!("execution amount does not match the persisted policy");
            }
            grants::check_grant(job_id, venue, action, &normalized)
                .map_err(|error| anyhow::anyhow!(error.0))?;
        }
        (ExecutionMode::Manual, consent::ConsentMode::Manual) => {
            let stored = policy
                .trade_amount_u
                .as_deref()
                .context("manual execution policy amount is missing")?;
            if Decimal::parse(stored)?.to_plain_string() != normalized {
                bail!("execution amount does not match the persisted manual policy");
            }
        }
        (ExecutionMode::OneTime, consent::ConsentMode::Auto) => {
            validate_one_time_permit(job_id, delivery_id, &normalized)?;
        }
        (ExecutionMode::Auto, consent::ConsentMode::Manual) => {
            bail!("automatic execution is not authorized by the manual policy")
        }
        (ExecutionMode::Manual, consent::ConsentMode::Auto) => {
            bail!("manual execution requires an explicit manual policy")
        }
        (ExecutionMode::OneTime, consent::ConsentMode::Manual) => {
            bail!("one-time over-cap execution requires an active auto policy")
        }
        (_, consent::ConsentMode::Decline) => bail!("copy-trade execution is declined"),
    }
    Ok((
        normalized,
        AuthorizedTradeKitSettings {
            environment: policy.trade_environment,
            margin_mode: policy.margin_mode,
            order_policy: policy.order_policy,
        },
    ))
}

fn json_from_stdout(stdout: &[u8]) -> Option<Value> {
    serde_json::from_slice(stdout).ok().or_else(|| {
        String::from_utf8_lossy(stdout)
            .lines()
            .rev()
            .find_map(|line| serde_json::from_str(line.trim()).ok())
    })
}

fn receipt_from_stdout(venue: &str, stdout: &[u8]) -> Option<Value> {
    let value = json_from_stdout(stdout)?;
    let mut receipt = serde_json::Map::new();
    collect_receipt_fields(venue, &value, &mut receipt, 0);
    (!receipt.is_empty()).then(|| Value::Object(receipt))
}

fn structured_failure(stdout: &[u8]) -> Option<String> {
    let value = json_from_stdout(stdout)?;
    structured_failure_value(&value, 0)
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn failure_code(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|value| !matches!(value.trim(), "" | "0" | "200"))
        || value
            .as_i64()
            .is_some_and(|value| !matches!(value, 0 | 200))
}

fn object_failure_message(object: &serde_json::Map<String, Value>) -> Option<String> {
    [
        "sMsg",
        "errorMessage",
        "error_message",
        "message",
        "msg",
        "error",
    ]
    .iter()
    .find_map(|key| object.get(*key).and_then(Value::as_str))
    .filter(|value| !value.trim().is_empty())
    .map(safe_child_text)
}

fn format_structured_failure(label: &str, code: Option<String>, message: Option<String>) -> String {
    match (code, message) {
        (Some(code), Some(message)) => format!("{label} (code {code}): {message}"),
        (Some(code), None) => format!("{label} (code {code})"),
        (None, Some(message)) => format!("{label}: {message}"),
        (None, None) => label.to_string(),
    }
}

fn structured_failure_value(value: &Value, depth: u8) -> Option<String> {
    if depth > 8 {
        return None;
    }
    let object = value.as_object();
    if let Some(object) = object {
        if let Some(code) = object.get("sCode").filter(|code| failure_code(code)) {
            return Some(format_structured_failure(
                "target order was rejected",
                scalar_text(code).map(|code| safe_child_text(&code)),
                object_failure_message(object),
            ));
        }
    }

    let nested = match value {
        Value::Object(map) => map
            .values()
            .find_map(|child| structured_failure_value(child, depth + 1)),
        Value::Array(values) => values
            .iter()
            .take(16)
            .find_map(|child| structured_failure_value(child, depth + 1)),
        _ => None,
    };
    if nested.is_some() {
        return nested;
    }

    if let Some(object) = object {
        let message = object_failure_message(object);
        if let Some(code) = object
            .get("errorCode")
            .or_else(|| object.get("error_code"))
            .filter(|code| failure_code(code))
        {
            return Some(format_structured_failure(
                "target command returned an error",
                scalar_text(code).map(|code| safe_child_text(&code)),
                message,
            ));
        }
        if let Some(code) = object.get("code").filter(|code| failure_code(code)) {
            return Some(format_structured_failure(
                "target command returned a failure",
                scalar_text(code).map(|code| safe_child_text(&code)),
                message,
            ));
        }
        if object.get("ok").and_then(Value::as_bool) == Some(false)
            || object.get("success").and_then(Value::as_bool) == Some(false)
        {
            return Some(format_structured_failure(
                "target command returned an explicit failure",
                None,
                message,
            ));
        }
        if object
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| {
                matches!(
                    status.to_ascii_lowercase().as_str(),
                    "failed" | "failure" | "error" | "rejected"
                )
            })
        {
            return Some(format_structured_failure(
                "target command returned a failure status",
                None,
                message,
            ));
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TextFailureDetail {
    message: String,
    code: Option<String>,
    definitely_before_submit: bool,
}

fn sensitive_label(value: &str) -> bool {
    let normalized = value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect::<String>();
    const NAMES: &[&str] = &[
        "apikey",
        "secret",
        "secretkey",
        "passphrase",
        "password",
        "authorization",
        "accesstoken",
        "refreshtoken",
        "token",
        "cookie",
        "signature",
        "privatekey",
        "mnemonic",
        "seed",
    ];
    NAMES
        .iter()
        .any(|name| normalized == *name || normalized.ends_with(name))
}

fn looks_like_jwt(value: &str) -> bool {
    let trimmed = value.trim_matches(|character: char| {
        matches!(character, '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']')
    });
    trimmed.len() >= 40
        && trimmed.bytes().filter(|byte| *byte == b'.').count() == 2
        && trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Reduce child-process output to a bounded, single-line, user-safe summary.
/// Raw stdout/stderr is never persisted. Known secret assignments, bearer
/// values, JWT-shaped values, and control characters are removed first.
fn safe_child_text(value: &str) -> String {
    let printable = value
        .chars()
        .map(|character| if character.is_control() { ' ' } else { character })
        .collect::<String>();
    let mut output = Vec::new();
    let mut redact_next = 0usize;
    for token in printable.split_whitespace() {
        if redact_next > 0 {
            output.push("[REDACTED]".to_string());
            redact_next -= 1;
            continue;
        }
        if token.eq_ignore_ascii_case("bearer") {
            output.push("Bearer".to_string());
            redact_next = 1;
            continue;
        }
        if looks_like_jwt(token) {
            output.push("[REDACTED]".to_string());
            continue;
        }
        let assignment = token
            .char_indices()
            .filter(|(_, character)| matches!(character, '=' | ':'))
            .find(|(position, _)| sensitive_label(&token[..*position]))
            .map(|(position, separator)| {
                (&token[..position], &token[position + 1..], separator)
            });
        if let Some((label, assigned, separator)) = assignment {
            output.push(format!("{label}{separator}[REDACTED]"));
            if assigned.is_empty() {
                redact_next = if label.eq_ignore_ascii_case("authorization") {
                    2
                } else {
                    1
                };
            }
            continue;
        }
        if sensitive_label(token) {
            output.push(token.to_string());
            redact_next = 1;
            continue;
        }
        output.push(token.to_string());
    }
    safe_text(&output.join(" "))
}

fn definitely_pre_submit_cli_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("argument is ambiguous")
        || message.contains("unexpected argument")
        || message.contains("unknown command")
        || message.contains("unknown option")
        || message.contains("missing required argument")
        || message.contains("did you forget to specify the option argument")
}

fn text_failure_detail(output: &[u8]) -> Option<TextFailureDetail> {
    let text = String::from_utf8_lossy(output);
    let mut message_parts = Vec::new();
    let mut code = None;
    let mut collecting_error = false;
    let mut fallback = None;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty()
            || line.starts_with("Update available for ")
            || line.starts_with("Run: npm install ")
            || line.starts_with("Version: ")
            || line.starts_with("TraceId: ")
            || line.starts_with("Hint: ")
        {
            continue;
        }
        fallback.get_or_insert_with(|| line.to_string());
        if let Some(value) = line.strip_prefix("Error:") {
            collecting_error = true;
            if !value.trim().is_empty() {
                message_parts.push(value.trim().to_string());
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("Code:") {
            code = Some(safe_child_text(value));
            collecting_error = false;
            continue;
        }
        if collecting_error {
            message_parts.push(line.to_string());
        }
    }
    let raw_message = if message_parts.is_empty() {
        fallback?
    } else {
        message_parts.join(" ")
    };
    let message = safe_child_text(&raw_message);
    Some(TextFailureDetail {
        definitely_before_submit: definitely_pre_submit_cli_error(&message),
        message,
        code: code.filter(|code| !code.is_empty()),
    })
}

fn classify_nonzero(
    venue: &str,
    exit_code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
) -> (OutcomeStatus, Option<Value>, Option<String>) {
    let receipt = receipt_from_stdout(venue, stdout);
    let exit = exit_code
        .map(|code| format!("exit code {code}"))
        .unwrap_or_else(|| "a non-zero exit status".to_string());
    if let Some(reason) = structured_failure(stdout).or_else(|| structured_failure(stderr)) {
        return if receipt.is_some() {
            (
                OutcomeStatus::UnknownAfterSubmit,
                receipt,
                Some(format!(
                    "{reason}; {exit}; a submission identifier was also returned, so final state is unknown"
                )),
            )
        } else {
            (
                OutcomeStatus::FailedBeforeSubmit,
                None,
                Some(format!("{reason}; {exit}")),
            )
        };
    }
    let text_detail = if json_from_stdout(stderr).is_none() {
        text_failure_detail(stderr)
    } else {
        None
    }
    .or_else(|| {
        if json_from_stdout(stdout).is_none() {
            text_failure_detail(stdout)
        } else {
            None
        }
    });
    if let Some(detail) = text_detail {
        let code = detail
            .code
            .map(|code| format!(" (code {code})"))
            .unwrap_or_default();
        let reason = format!("target command failed with {exit}{code}: {}", detail.message);
        if detail.definitely_before_submit && receipt.is_none() {
            return (OutcomeStatus::FailedBeforeSubmit, None, Some(reason));
        }
        return (
            OutcomeStatus::UnknownAfterSubmit,
            receipt,
            Some(format!("{reason}; submission state is unknown")),
        );
    }
    (
        OutcomeStatus::UnknownAfterSubmit,
        receipt,
        Some(format!(
            "target command started and returned {exit} without a safe diagnostic; submission state is unknown"
        )),
    )
}

fn classify_success(venue: &str, stdout: &[u8]) -> (OutcomeStatus, Option<Value>, Option<String>) {
    let receipt = receipt_from_stdout(venue, stdout);
    if let Some(reason) = structured_failure(stdout) {
        return if receipt.is_some() {
            (
                OutcomeStatus::UnknownAfterSubmit,
                receipt,
                Some(format!(
                    "{reason}; a submission identifier was also returned, so final state is unknown"
                )),
            )
        } else {
            (OutcomeStatus::FailedBeforeSubmit, None, Some(reason))
        };
    }
    match receipt {
        Some(receipt) => (OutcomeStatus::Submitted, Some(receipt), None),
        None => (
            OutcomeStatus::UnknownAfterSubmit,
            None,
            Some("target command exited successfully but returned no verifiable order or transaction receipt".to_string()),
        ),
    }
}

fn safe_reason(error: &anyhow::Error) -> String {
    safe_text(
        &error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": "),
    )
}

fn safe_text(value: &str) -> String {
    let mut reason = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if reason.is_empty() {
        reason = "unspecified terminal reason".to_string();
    }
    if reason.chars().count() > 240 {
        reason = reason.chars().take(240).collect::<String>() + "…";
    }
    reason
}

fn make_outcome(
    request: &ExecuteRequest<'_>,
    amount: String,
    status: OutcomeStatus,
    receipt: Option<Value>,
    reason: Option<String>,
    created_at: u64,
) -> ExecutionOutcome {
    ExecutionOutcome {
        version: OUTCOME_VERSION,
        job_id: request.job_id.to_string(),
        delivery_id: request.delivery_id.to_string(),
        venue: request.venue.to_string(),
        action: request.action.to_string(),
        amount,
        execution_mode: request.execution_mode,
        status,
        receipt,
        reason,
        notification_pending: true,
        notification_attempts: 0,
        next_notification_attempt_at: 0,
        created_at,
        updated_at: now_secs(),
    }
}

fn persist_and_notify(path: &Path, mut outcome: ExecutionOutcome) -> Result<ExecutionOutcome> {
    let journal_path = write_terminal_journal(&outcome)
        .map(Some)
        .unwrap_or_else(|error| {
            eprintln!("[autotrade] terminal journal write failed: {error}");
            None
        });
    write_outcome(path, &outcome)?;
    if outcome.execution_mode == ExecutionMode::OneTime {
        if let Ok(permit_path) = one_time_permit_path(&outcome.job_id, &outcome.delivery_id) {
            let _ = std::fs::remove_file(permit_path);
        }
    }
    consent::clear_pending_delivery(&outcome.job_id, &outcome.delivery_id);
    notify_and_persist(
        path,
        &mut outcome,
        false,
        Some(INITIAL_NOTIFY_TIMEOUT),
    );
    // A durable terminal result, including a user-selected skip, is the only
    // normal trigger that advances the per-subscription decision FIFO.
    if let Err(error) = super::delivery_queue::complete_and_advance(
        &outcome.job_id,
        &outcome.delivery_id,
    ) {
        eprintln!("[autotrade] queued-delivery resume failed (persisted for retry): {error}");
    }
    if terminal_reconciliation_complete(&outcome) {
        if let Some(journal_path) = journal_path {
            let _ = std::fs::remove_file(journal_path);
        }
    }
    Ok(outcome)
}

fn receipt_identifier_allowed(venue: &str, key: &str) -> bool {
    match venue {
        "dex" => matches!(
            key,
            "txHash" | "swapTxHash" | "transactionHash" | "orderId" | "swapOrderId"
        ),
        "defi" => matches!(key, "txHash" | "transactionHash" | "orderId"),
        "trade_kit" => matches!(key, "ordId" | "clOrdId" | "orderId"),
        "polymarket" => matches!(
            key,
            "orderId" | "orderID" | "order_id" | "txHash" | "transactionHash"
        ),
        "hyperliquid" => matches!(
            key,
            "oid" | "orderId" | "order_id" | "txHash" | "transactionHash"
        ),
        _ => false,
    }
}

fn collect_receipt_fields(
    venue: &str,
    value: &Value,
    receipt: &mut serde_json::Map<String, Value>,
    depth: u8,
) {
    if depth > 8 {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if receipt_identifier_allowed(venue, key)
                    && matches!(value, Value::String(_) | Value::Number(_))
                    && !value.as_str().is_some_and(str::is_empty)
                {
                    receipt.entry(key.clone()).or_insert_with(|| value.clone());
                }
                collect_receipt_fields(venue, value, receipt, depth + 1);
            }
        }
        Value::Array(values) => {
            for value in values.iter().take(16) {
                collect_receipt_fields(venue, value, receipt, depth + 1);
            }
        }
        _ => {}
    }
}

fn notification(outcome: &ExecutionOutcome) -> String {
    let receipt = outcome
        .receipt
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|map| {
            map.iter()
                .find_map(|(key, value)| value.as_str().map(|v| format!("{key}: {v}")))
        });
    let zh_label = match outcome.execution_mode {
        ExecutionMode::Auto => "[自动跟单]",
        ExecutionMode::Manual | ExecutionMode::OneTime => "[手动跟单]",
    };
    let en_label = match outcome.execution_mode {
        ExecutionMode::Auto => "[Auto Copy-Trade]",
        ExecutionMode::Manual | ExecutionMode::OneTime => "[Manual Copy-Trade]",
    };
    match (user_lang::resolve(&outcome.job_id), outcome.status) {
        (user_lang::Lang::Zh, OutcomeStatus::Submitted) => format!(
            "{zh_label} 交易已提交。类型: {},方向: {},金额: {}。{}",
            outcome.venue,
            outcome.action,
            outcome.amount,
            receipt.unwrap_or_else(|| "可在对应交易记录中查看详情".to_string())
        ),
        (user_lang::Lang::En, OutcomeStatus::Submitted) => format!(
            "{en_label} Trade submitted. Venue: {}, action: {}, amount: {}. {}",
            outcome.venue,
            outcome.action,
            outcome.amount,
            receipt.unwrap_or_else(|| "Check the venue history for details".to_string())
        ),
        (user_lang::Lang::Zh, OutcomeStatus::FailedBeforeSubmit) => format!(
            "{zh_label} 交易执行失败，未确认提交。类型: {},方向: {},金额: {}。原因: {}。系统不会自动重试。",
            outcome.venue,
            outcome.action,
            outcome.amount,
            outcome.reason.as_deref().unwrap_or("执行命令失败")
        ),
        (user_lang::Lang::En, OutcomeStatus::FailedBeforeSubmit) => format!(
            "{en_label} Trade execution failed; submission was not confirmed. Venue: {}, action: {}, amount: {}. Reason: {}. No automatic retry will occur.",
            outcome.venue,
            outcome.action,
            outcome.amount,
            outcome.reason.as_deref().unwrap_or("execution command failed")
        ),
        (user_lang::Lang::Zh, OutcomeStatus::UnknownAfterSubmit) => format!(
            "{zh_label} 交易提交状态未知。类型: {},方向: {},金额: {}。原因: {}。请先查询订单/交易记录，系统不会自动重试。",
            outcome.venue,
            outcome.action,
            outcome.amount,
            outcome.reason.as_deref().unwrap_or("未获得可验证的交易回执")
        ),
        (user_lang::Lang::En, OutcomeStatus::UnknownAfterSubmit) => format!(
            "{en_label} Trade submission status is unknown. Venue: {}, action: {}, amount: {}. Reason: {}. Check order/transaction history first; no automatic retry will occur.",
            outcome.venue,
            outcome.action,
            outcome.amount,
            outcome.reason.as_deref().unwrap_or("no verifiable transaction receipt was returned")
        ),
        (user_lang::Lang::Zh, OutcomeStatus::Skipped) => format!(
            "{zh_label} 本次交付物未执行交易。原因: {}。",
            outcome.reason.as_deref().unwrap_or("信号不满足执行条件")
        ),
        (user_lang::Lang::En, OutcomeStatus::Skipped) => format!(
            "{en_label} No trade was executed for this delivery. Reason: {}.",
            outcome.reason.as_deref().unwrap_or("the signal was not eligible for execution")
        ),
        (user_lang::Lang::Zh, OutcomeStatus::FailedBeforeExecution) => format!(
            "{zh_label} 交付物处理失败，未启动交易。原因: {}。系统不会自动下单重试。",
            outcome.reason.as_deref().unwrap_or("无法完成交易前处理")
        ),
        (user_lang::Lang::En, OutcomeStatus::FailedBeforeExecution) => format!(
            "{en_label} Delivery processing failed before a trade was started. Reason: {}. No automatic order retry will occur.",
            outcome.reason.as_deref().unwrap_or("pre-trade processing could not be completed")
        ),
    }
}

fn notify_and_persist(
    path: &Path,
    outcome: &mut ExecutionOutcome,
    force: bool,
    attempt_timeout: Option<Duration>,
) {
    if !outcome.notification_pending
        || (!force && outcome.next_notification_attempt_at > now_secs())
    {
        return;
    }
    let key = format!(
        "autotrade-outcome:{}",
        hex::encode(Sha256::digest(format!(
            "{}\0{}\0{:?}",
            outcome.job_id, outcome.delivery_id, outcome.status
        )))
    );
    let content = notification(outcome);
    let max_attempts = if force { 3 } else { 1 };
    for attempt in 0..max_attempts {
        let delivered = match attempt_timeout {
            Some(timeout) => okx_a2a::user_notify_scoped_with_timeout(
                &content,
                &outcome.job_id,
                &key,
                timeout,
            ),
            None => okx_a2a::user_notify_scoped(&content, &outcome.job_id, &key),
        };
        if delivered.is_ok() {
            outcome.notification_pending = false;
            outcome.next_notification_attempt_at = 0;
            outcome.updated_at = now_secs();
            let _ = write_outcome(path, outcome);
            return;
        }
        if attempt + 1 < max_attempts {
            std::thread::sleep(Duration::from_millis(50 * (attempt + 1)));
        }
    }
    outcome.notification_attempts = outcome
        .notification_attempts
        .saturating_add(max_attempts as u32);
    outcome.updated_at = now_secs();
    let delay = 30u64
        .saturating_mul(1u64 << outcome.notification_attempts.min(5))
        .min(15 * 60);
    outcome.next_notification_attempt_at = outcome.updated_at.saturating_add(delay);
    let _ = write_outcome(path, outcome);
}

pub async fn execute(request: ExecuteRequest<'_>) -> Result<ExecutionOutcome> {
    let context = consent::load_delivery_context(request.job_id, request.delivery_id)
        .context("trusted delivery context is unavailable")?;
    if context.job_id != request.job_id || context.delivery_id != request.delivery_id {
        bail!("trusted delivery context mismatch");
    }
    let outcome_path = outcome_path(request.job_id, request.delivery_id)?;
    if !reserve_execution(request.job_id, request.delivery_id)? {
        if let Some(mut outcome) = read_outcome(&outcome_path)? {
            if outcome.notification_pending {
                notify_and_persist(
                    &outcome_path,
                    &mut outcome,
                    false,
                    Some(INITIAL_NOTIFY_TIMEOUT),
                );
            }
            return Ok(outcome);
        }
        let (status, reason) = match recovery_state(request.job_id, request.delivery_id)? {
            RecoveryState::PreSubmitInterrupted => (
                OutcomeStatus::FailedBeforeSubmit,
                "an earlier execution stopped before the transaction command started; no automatic retry will occur",
            ),
            RecoveryState::SubmissionUnknown | RecoveryState::TerminalOutcome => (
                OutcomeStatus::UnknownAfterSubmit,
                "an earlier execution may have started but has no terminal outcome; do not retry",
            ),
            RecoveryState::NoExecution => (
                OutcomeStatus::FailedBeforeSubmit,
                "execution reservation is unavailable; no transaction command was started",
            ),
        };
        let outcome = make_outcome(
            &request,
            request.amount.chars().take(64).collect(),
            status,
            None,
            Some(reason.to_string()),
            now_secs(),
        );
        return persist_and_notify(&outcome_path, outcome);
    }

    let started = now_secs();
    let prepared = (|| -> Result<(
        String,
        PathBuf,
        Vec<String>,
        Option<TradeKitExecutionContext>,
    )> {
        let (amount, authorized_settings) = authorize(
            request.job_id,
            request.delivery_id,
            request.venue,
            request.action,
            request.amount,
            request.execution_mode,
        )?;
        let (program, args) = parse_command(request.venue, request.command_json)?;
        validate_bound_intent(
            request.venue,
            request.action,
            &amount,
            request.job_id,
            request.execution_mode,
            &args,
        )?;
        let trade_kit_context = if request.venue == "trade_kit" {
            let context = trade_kit_execution_context(&args)?;
            let expected = authorized_settings
                .environment
                .context("Trade Kit execution requires a persisted live or demo environment")?;
            if context.environment != expected {
                bail!("Trade Kit command environment does not match persisted consent");
            }
            validate_trade_kit_execution_settings(&args, context, authorized_settings)?;
            Some(context)
        } else {
            None
        };
        Ok((amount, program, args, trade_kit_context))
    })();
    let (amount, program, args, trade_kit_context) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            let amount = Decimal::parse(request.amount)
                .map(Decimal::to_plain_string)
                .unwrap_or_else(|_| request.amount.chars().take(64).collect());
            let outcome = make_outcome(
                &request,
                amount,
                OutcomeStatus::FailedBeforeSubmit,
                None,
                Some(safe_reason(&error)),
                started,
            );
            return persist_and_notify(&outcome_path, outcome);
        }
    };
    if let Some(context) = trade_kit_context {
        let readiness = trade_kit::probe_runtime(&[context.asset_class], context.environment).await;
        if !readiness.ready {
            let reason = serde_json::to_value(readiness.reason)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "verification_unknown".to_string());
            let outcome = make_outcome(
                &request,
                amount,
                OutcomeStatus::FailedBeforeSubmit,
                None,
                Some(format!("Trade Kit readiness failed: {reason}")),
                started,
            );
            return persist_and_notify(&outcome_path, outcome);
        }
    }
    if let Err(error) = update_execution_phase(
        request.job_id,
        request.delivery_id,
        ExecutionPhase::Prepared,
    ) {
        let outcome = make_outcome(
            &request,
            amount,
            OutcomeStatus::FailedBeforeSubmit,
            None,
            Some(format!("could not persist the prepared execution state: {error}")),
            started,
        );
        return persist_and_notify(&outcome_path, outcome);
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Err(error) = update_execution_phase(
        request.job_id,
        request.delivery_id,
        ExecutionPhase::Spawned,
    ) {
        let outcome = make_outcome(
            &request,
            amount,
            OutcomeStatus::FailedBeforeSubmit,
            None,
            Some(format!("could not persist the execution start state: {error}")),
            started,
        );
        return persist_and_notify(&outcome_path, outcome);
    }
    let duration = Duration::from_secs(request.timeout_sec.clamp(1, MAX_TIMEOUT_SEC));
    let result = timeout(duration, command.output()).await;
    let (status, receipt, reason) = match result {
        Ok(Ok(output)) if output.status.success() => {
            classify_success(request.venue, &output.stdout)
        }
        Ok(Ok(output)) => classify_nonzero(
            request.venue,
            output.status.code(),
            &output.stdout,
            &output.stderr,
        ),
        Ok(Err(error)) => (
            OutcomeStatus::FailedBeforeSubmit,
            None,
            Some(format!("execution command could not start: {error}")),
        ),
        Err(_) => (
            OutcomeStatus::UnknownAfterSubmit,
            None,
            Some("timeout".to_string()),
        ),
    };
    persist_and_notify(
        &outcome_path,
        make_outcome(&request, amount, status, receipt, reason, started),
    )
}

/// Persist and notify a terminal delivery result that occurs before a
/// money-moving command exists (for example, a non-actionable signal or route
/// preparation failure). Reserving the same delivery latch prevents a later
/// model turn from executing a delivery it already declared terminal.
pub fn report_delivery(
    job_id: &str,
    delivery_id: &str,
    status: &str,
    reason: &str,
) -> Result<ExecutionOutcome> {
    let context = consent::load_delivery_context(job_id, delivery_id)
        .context("trusted delivery context is unavailable")?;
    if context.job_id != job_id || context.delivery_id != delivery_id {
        bail!("trusted delivery context mismatch");
    }
    let status = match status {
        "skipped" => OutcomeStatus::Skipped,
        "failed_before_execution" => OutcomeStatus::FailedBeforeExecution,
        _ => bail!("delivery report status must be skipped or failed_before_execution"),
    };
    let outcome_path = outcome_path(job_id, delivery_id)?;
    if !reserve_execution(job_id, delivery_id)? {
        if let Some(mut outcome) = read_outcome(&outcome_path)? {
            if outcome.notification_pending {
                notify_and_persist(
                    &outcome_path,
                    &mut outcome,
                    false,
                    Some(INITIAL_NOTIFY_TIMEOUT),
                );
            }
            return Ok(outcome);
        }
        bail!("delivery already reserved without a terminal outcome");
    }
    let now = now_secs();
    persist_and_notify(
        &outcome_path,
        ExecutionOutcome {
            version: OUTCOME_VERSION,
            job_id: job_id.to_string(),
            delivery_id: delivery_id.to_string(),
            venue: String::new(),
            action: String::new(),
            amount: String::new(),
            execution_mode: ExecutionMode::Auto,
            status,
            receipt: None,
            reason: Some(safe_text(reason)),
            notification_pending: true,
            notification_attempts: 0,
            next_notification_attempt_at: 0,
            created_at: now,
            updated_at: now,
        },
    )
}

/// Convert an abandoned execution gateway invocation into a durable terminal
/// result without ever starting the target command again.
pub(crate) fn recover_incomplete(job_id: &str, delivery_id: &str) -> Result<bool> {
    let state = recovery_state(job_id, delivery_id)?;
    if state == RecoveryState::NoExecution {
        return Ok(false);
    }
    if state == RecoveryState::TerminalOutcome {
        let path = outcome_path(job_id, delivery_id)?;
        if let Some(outcome) = read_outcome(&path)? {
            write_outcome(&path, &outcome)?;
            consent::clear_pending_delivery(job_id, delivery_id);
            let _ = super::delivery_queue::reconcile_terminal(job_id, delivery_id);
        }
        return Ok(true);
    }
    let (status, reason) = match state {
        RecoveryState::PreSubmitInterrupted => (
            OutcomeStatus::FailedBeforeSubmit,
            "execution was interrupted before the transaction command started; no order was submitted and no automatic retry will occur",
        ),
        RecoveryState::SubmissionUnknown => (
            OutcomeStatus::UnknownAfterSubmit,
            "execution was interrupted after the transaction command may have started; submission state is unknown and no automatic retry will occur",
        ),
        RecoveryState::NoExecution | RecoveryState::TerminalOutcome => unreachable!(),
    };
    let now = now_secs();
    persist_and_notify(
        &outcome_path(job_id, delivery_id)?,
        ExecutionOutcome {
            version: OUTCOME_VERSION,
            job_id: job_id.to_string(),
            delivery_id: delivery_id.to_string(),
            venue: String::new(),
            action: String::new(),
            amount: String::new(),
            execution_mode: ExecutionMode::Auto,
            status,
            receipt: None,
            reason: Some(reason.to_string()),
            notification_pending: true,
            notification_attempts: 0,
            next_notification_attempt_at: 0,
            created_at: now,
            updated_at: now,
        },
    )?;
    Ok(true)
}

/// Repair terminal transitions that were interrupted between outcome, notice
/// index, pending pointer, and FIFO updates. This never invokes a trade command.
pub fn reconcile_terminal_journals(max_records: usize, budget: Duration) -> Result<usize> {
    let deadline = Instant::now() + budget;
    let root = crate::home::onchainos_home()?
        .join("autotrade")
        .join("terminal-journal");
    if !root.is_dir() {
        return Ok(0);
    }
    let mut repaired = 0;
    'jobs: for directory in std::fs::read_dir(root)? {
        let directory = directory?;
        if !directory.path().is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(directory.path())? {
            if repaired >= max_records || Instant::now() >= deadline {
                break 'jobs;
            }
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let journal = match read_terminal_journal(&path) {
                Ok(journal) => journal,
                Err(error) => {
                    eprintln!("[autotrade] unreadable terminal journal {:?}: {error}", path);
                    continue;
                }
            };
            let outcome_path = outcome_path(
                &journal.outcome.job_id,
                &journal.outcome.delivery_id,
            )?;
            let outcome = read_outcome(&outcome_path)?.unwrap_or(journal.outcome);
            write_outcome(&outcome_path, &outcome)?;
            if outcome.execution_mode == ExecutionMode::OneTime {
                if let Ok(permit_path) =
                    one_time_permit_path(&outcome.job_id, &outcome.delivery_id)
                {
                    let _ = std::fs::remove_file(permit_path);
                }
            }
            consent::clear_pending_delivery(&outcome.job_id, &outcome.delivery_id);
            let _ = super::delivery_queue::reconcile_terminal(
                &outcome.job_id,
                &outcome.delivery_id,
            );
            if terminal_reconciliation_complete(&outcome) {
                let _ = std::fs::remove_file(&path);
            }
            repaired += 1;
        }
    }
    Ok(repaired)
}

fn flush_with_policy(
    job_id: &str,
    force: bool,
    max_records: usize,
) -> Result<Vec<ExecutionOutcome>> {
    if !grants::job_id_is_safe(job_id) {
        bail!("invalid job id");
    }
    let directory = crate::home::onchainos_home()?
        .join("autotrade")
        .join("outcomes")
        .join(job_id);
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut outcomes = Vec::new();
    for entry in std::fs::read_dir(directory)?.take(max_records.max(1)) {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Some(mut outcome) = read_outcome(&path)? else {
            continue;
        };
        if outcome.notification_pending {
            notify_and_persist(&path, &mut outcome, force, None);
        }
        outcomes.push(outcome);
    }
    let _ = super::notify::flush_pending(job_id, force);
    Ok(outcomes)
}

pub fn flush(job_id: &str) -> Result<Vec<ExecutionOutcome>> {
    flush_with_policy(job_id, true, 32)
}

pub fn flush_due(job_id: &str) -> Result<Vec<ExecutionOutcome>> {
    if !grants::job_id_is_safe(job_id) {
        bail!("invalid job id");
    }
    let _ = flush_all_due(4)?;
    Ok(Vec::new())
}

/// Bounded cleanup for genuinely expiring authorization tickets. Terminal
/// outcomes and execution latches are intentionally retained as idempotency
/// tombstones; deleting those could permit an old delivery to trade again.
pub fn cleanup_expired_tickets(limit: usize) -> Result<usize> {
    let root = crate::home::onchainos_home()?
        .join("autotrade")
        .join("one-time-permits");
    if !root.is_dir() {
        return Ok(0);
    }
    let mut inspected = 0;
    let mut removed = 0;
    for directory in std::fs::read_dir(root)? {
        if inspected >= limit {
            break;
        }
        let directory = directory?;
        if !directory.path().is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(directory.path())? {
            if inspected >= limit {
                break;
            }
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            inspected += 1;
            let expired = std::fs::read(&path)
                .ok()
                .and_then(|raw| serde_json::from_slice::<OneTimePermit>(&raw).ok())
                .is_some_and(|permit| {
                    permit.version == ONE_TIME_PERMIT_VERSION && permit.expires_at <= now_secs()
                });
            if expired && std::fs::remove_file(path).is_ok() {
                removed += 1;
            }
        }
    }
    Ok(removed)
}

/// Retry due result/degrade notices across jobs. Called opportunistically on
/// Agent command startup/heartbeat; it never re-runs a transaction command.
pub fn flush_all_due(max_records: usize) -> Result<usize> {
    let deadline = Instant::now() + GLOBAL_RETRY_BUDGET;
    let root = notice_index_root()?;
    let mut pending = Vec::new();
    if root.is_dir() {
        for entry in std::fs::read_dir(&root)? {
            let path = entry?.path();
            let extension = path.extension().and_then(|value| value.to_str());
            if extension.is_some_and(|value| value.starts_with("lease-")) {
                let stale = path
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                    .is_some_and(|age| age.as_secs() >= STALE_LEASE_SEC);
                if stale {
                    let original = path.with_extension("json");
                    if original.exists() {
                        let _ = std::fs::remove_file(&path);
                    } else {
                        let _ = std::fs::rename(&path, original);
                    }
                }
                continue;
            }
            if extension != Some("json") {
                continue;
            }
            if let Some(reference) = std::fs::read(&path)
                .ok()
                .and_then(|raw| serde_json::from_slice::<OutcomeNoticeRef>(&raw).ok())
                .filter(|reference| reference.version == NOTICE_REF_VERSION)
            {
                pending.push((path, reference));
            }
        }
        pending.sort_by_key(|(_, reference)| reference.next_attempt_at);
        for (index_path, reference) in pending.into_iter().take(max_records.max(1)) {
            if reference.next_attempt_at > now_secs() {
                break;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining < Duration::from_millis(25) {
                break;
            }
            let lease = index_path.with_extension(format!("lease-{}", std::process::id()));
            if std::fs::rename(&index_path, &lease).is_err() {
                continue;
            }
            let path = match outcome_path(&reference.job_id, &reference.delivery_id) {
                Ok(path) => path,
                Err(_) => {
                    let _ = std::fs::remove_file(&lease);
                    continue;
                }
            };
            match read_outcome(&path) {
                Ok(Some(mut outcome)) if outcome.notification_pending => {
                    notify_and_persist(&path, &mut outcome, false, Some(remaining));
                }
                _ => {
                    let _ = std::fs::remove_file(&index_path);
                }
            }
            // `write_outcome` recreated the canonical pending ref on failure;
            // success removed it. The lease itself is always disposable now.
            let _ = std::fs::remove_file(&lease);
        }
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    let delivered = if remaining >= Duration::from_millis(25) {
        super::notify::flush_all_pending_bounded(max_records, remaining).unwrap_or(0)
    } else {
        0
    };
    Ok(delivered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tempdir() -> tempfile::TempDir {
        let root = std::env::current_dir()
            .expect("current directory")
            .join("target")
            .join("autotrade-executor-test-home");
        std::fs::create_dir_all(&root).expect("create executor test root");
        tempfile::tempdir_in(root).expect("create executor test home")
    }

    #[test]
    fn pending_only_notification_index_tracks_outcome_state() {
        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = test_tempdir();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        let path = outcome_path("job1", "delivery-index").unwrap();
        let mut outcome = ExecutionOutcome {
            version: OUTCOME_VERSION,
            job_id: "job1".to_string(),
            delivery_id: "delivery-index".to_string(),
            venue: "dex".to_string(),
            action: "buy".to_string(),
            amount: "1".to_string(),
            execution_mode: ExecutionMode::Auto,
            status: OutcomeStatus::FailedBeforeSubmit,
            receipt: None,
            reason: Some("test".to_string()),
            notification_pending: true,
            notification_attempts: 1,
            next_notification_attempt_at: now_secs() + 30,
            created_at: now_secs(),
            updated_at: now_secs(),
        };
        write_outcome(&path, &outcome).unwrap();
        let index = notice_ref_path("job1", "delivery-index").unwrap();
        assert!(index.exists());
        outcome.notification_pending = false;
        write_outcome(&path, &outcome).unwrap();
        assert!(!index.exists());
        assert!(path.exists(), "terminal outcome remains an idempotency tombstone");
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn command_parser_never_accepts_a_shell_or_recursive_agent_command() {
        assert!(parse_command("dex", r#"["swap","execute","--from","usdt"]"#).is_ok());
        assert!(parse_command("dex", r#"["sh","-c","anything"]"#).is_err());
        assert!(parse_command("dex", r#"["agent","autotrade-execute","--job-id","x"]"#).is_err());
        let (program, _) =
            parse_command("hyperliquid", r#"["order","--coin","BTC","--confirm"]"#).unwrap();
        assert_eq!(program, PathBuf::from("hyperliquid"));
        let (program, _) = parse_command(
            "hyperliquid",
            r#"["outcome-buy","--outcome","2","--confirm"]"#,
        )
        .unwrap();
        assert_eq!(program, PathBuf::from("hyperliquid-plugin"));
    }

    #[test]
    fn trade_kit_market_sentinels_are_canonicalized_for_node_parse_args() {
        let (_, args) = parse_command(
            "trade_kit",
            r#"["--demo","--json","swap","place","--sz","1","--side","buy","--tpOrdPx","-1","--slOrdPx","-1"]"#,
        )
        .unwrap();
        assert!(args.iter().any(|arg| arg == "--tpOrdPx=-1"));
        assert!(args.iter().any(|arg| arg == "--slOrdPx=-1"));
        assert!(!args.windows(2).any(|pair| {
            matches!(pair[0].as_str(), "--tpOrdPx" | "--slOrdPx") && pair[1] == "-1"
        }));
    }

    #[test]
    fn receipt_extraction_keeps_only_safe_identifiers() {
        let receipt = receipt_from_stdout(
            "dex",
            br#"{"ok":true,"data":{"txHash":"0xabc","secret":"do-not-copy"}}"#,
        )
        .unwrap();
        assert_eq!(receipt["txHash"], "0xabc");
        assert!(receipt.get("secret").is_none());
        assert!(receipt_from_stdout("dex", br#"{"ok":true,"status":"success"}"#).is_none());
        assert!(receipt_from_stdout("trade_kit", br#"{"data":{"ordId":"42"}}"#).is_some());
        assert!(receipt_from_stdout("dex", br#"{"data":{"ordId":"42"}}"#).is_none());
    }

    #[test]
    fn zero_exit_json_failures_are_not_classified_as_success() {
        assert!(structured_failure(br#"{"ok":false,"errorCode":"DENIED"}"#).is_some());
        assert!(structured_failure(br#"{"code":"50001","data":[]}"#).is_some());
        assert!(structured_failure(br#"{"code":"0","data":[{"sCode":"51000"}]}"#).is_some());
        assert!(structured_failure(br#"{"code":200,"data":{"ordId":"1"}}"#).is_none());
        assert!(structured_failure(br#"log line\n{"ok":true,"data":{"ordId":"1"}}"#).is_none());
        assert_eq!(
            classify_success("dex", br#"{"ok":true}"#).0,
            OutcomeStatus::UnknownAfterSubmit
        );
        assert_eq!(
            classify_success("trade_kit", br#"{"ok":true,"data":{"ordId":"1"}}"#).0,
            OutcomeStatus::Submitted
        );
        assert_eq!(
            classify_success(
                "trade_kit",
                br#"{"ok":true,"data":{"ordId":"1","status":"failed"}}"#,
            )
            .0,
            OutcomeStatus::UnknownAfterSubmit
        );
    }

    #[test]
    fn nonzero_trade_kit_parser_error_is_specific_and_pre_submit() {
        let stderr = b"Error: Option '--tpOrdPx' argument is ambiguous.\nDid you forget to specify the option argument for '--tpOrdPx'?\nTo specify an option argument starting with a dash use '--tpOrdPx=-XYZ'.\nVersion: fixture\n";
        let (status, receipt, reason) = classify_nonzero("trade_kit", Some(1), b"", stderr);
        assert_eq!(status, OutcomeStatus::FailedBeforeSubmit);
        assert!(receipt.is_none());
        let reason = reason.unwrap();
        assert!(reason.contains("argument is ambiguous"));
        assert!(reason.contains("exit code 1"));
        assert!(!reason.contains("Version:"));
    }

    #[test]
    fn nonzero_structured_rejection_preserves_safe_code_and_message() {
        let stdout = br#"{"code":"1","data":[{"sCode":"51008","sMsg":"Insufficient account balance"}]}"#;
        let (status, receipt, reason) = classify_nonzero("trade_kit", Some(7), stdout, b"");
        assert_eq!(status, OutcomeStatus::FailedBeforeSubmit);
        assert!(receipt.is_none());
        let reason = reason.unwrap();
        assert!(reason.contains("51008"));
        assert!(reason.contains("Insufficient account balance"));
        assert!(reason.contains("exit code 7"));
    }

    #[test]
    fn opaque_nonzero_output_keeps_unknown_state_but_exposes_safe_summary() {
        let (status, receipt, reason) =
            classify_nonzero("trade_kit", Some(9), b"", b"opaque transport failure");
        assert_eq!(status, OutcomeStatus::UnknownAfterSubmit);
        assert!(receipt.is_none());
        let reason = reason.unwrap();
        assert!(reason.contains("opaque transport failure"));
        assert!(reason.contains("submission state is unknown"));
    }

    #[test]
    fn child_diagnostics_redact_sensitive_assignments_and_jwt_shaped_values() {
        let jwt_shaped = format!(
            "{}.{}.{}",
            "a".repeat(16),
            "b".repeat(16),
            "c".repeat(16)
        );
        let raw = format!(
            "apiKey=fixture-api-value --secret fixture-secret-value https://invalid.local?token=fixture-query-value {jwt_shaped} Error: denied"
        );
        let safe = safe_child_text(&raw);
        assert!(!safe.contains("fixture-api-value"));
        assert!(!safe.contains("fixture-secret-value"));
        assert!(!safe.contains("fixture-query-value"));
        assert!(!safe.contains(&jwt_shaped));
        assert!(safe.contains("[REDACTED]"));
        assert!(safe.contains("denied"));
    }

    #[test]
    fn intent_binding_rejects_amount_or_job_substitution() {
        let dex = vec![
            "swap".into(),
            "execute".into(),
            "--readable-amount".into(),
            "100".into(),
        ];
        assert!(validate_bound_intent(
            "dex",
            "buy",
            "10",
            "job1",
            ExecutionMode::Auto,
            &dex
        )
        .is_err());

        let polymarket = vec![
            "buy".into(),
            "--amount".into(),
            "10".into(),
            "--autotrade-job".into(),
            "other-job".into(),
        ];
        assert!(validate_bound_intent(
            "polymarket",
            "buy",
            "10",
            "job1",
            ExecutionMode::Auto,
            &polymarket
        )
        .is_err());
    }

    #[test]
    fn trade_kit_execution_classifies_supported_commands_and_requires_environment() {
        let live_spot = vec![
            "spot".into(),
            "place".into(),
            "--sz".into(),
            "1".into(),
            "--live".into(),
        ];
        assert_eq!(
            trade_kit_execution_context(&live_spot).unwrap(),
            TradeKitExecutionContext {
                asset_class: AssetClass::Spot,
                environment: trade_kit::TradeEnvironment::Live,
                operation: TradeKitOperation::Place,
            }
        );

        let demo_perp = vec!["swap".into(), "place".into(), "--demo".into()];
        assert_eq!(
            trade_kit_execution_context(&demo_perp).unwrap(),
            TradeKitExecutionContext {
                asset_class: AssetClass::Perp,
                environment: trade_kit::TradeEnvironment::Demo,
                operation: TradeKitOperation::Place,
            }
        );

        for (module, asset_class) in [
            ("futures", AssetClass::Perp),
            ("option", AssetClass::Option),
            ("event", AssetClass::Prediction),
        ] {
            let args = vec![module.into(), "place".into(), "--demo".into()];
            assert_eq!(
                trade_kit_execution_context(&args).unwrap(),
                TradeKitExecutionContext {
                    asset_class,
                    environment: trade_kit::TradeEnvironment::Demo,
                    operation: TradeKitOperation::Place,
                }
            );
        }

        let event_place = vec![
            "event".into(),
            "place".into(),
            "BTC-ABOVE".into(),
            "buy".into(),
            "yes".into(),
            "10".into(),
            "--ordType".into(),
            "market".into(),
            "--demo".into(),
        ];
        validate_bound_intent(
            "trade_kit",
            "buy",
            "10",
            "job1",
            ExecutionMode::Auto,
            &event_place,
        )
        .unwrap();
        assert!(validate_bound_intent(
            "trade_kit",
            "sell",
            "10",
            "job1",
            ExecutionMode::Auto,
            &event_place,
        )
        .is_err());

        let futures_close = vec!["futures".into(), "close".into(), "--live".into()];
        assert_eq!(
            trade_kit_execution_context(&futures_close).unwrap(),
            TradeKitExecutionContext {
                asset_class: AssetClass::Perp,
                environment: trade_kit::TradeEnvironment::Live,
                operation: TradeKitOperation::ClosePosition,
            }
        );

        for invalid in [
            vec!["spot".into(), "place".into()],
            vec![
                "spot".into(),
                "place".into(),
                "--live".into(),
                "--demo".into(),
            ],
            vec!["order".into(), "--live".into()],
        ] {
            assert!(trade_kit_execution_context(&invalid).is_err());
        }
    }

    #[test]
    fn trade_kit_execution_settings_are_bound_to_local_consent() {
        let market_perp = vec![
            "swap".into(),
            "place".into(),
            "--tdMode".into(),
            "cross".into(),
            "--ordType".into(),
            "market".into(),
        ];
        let context = TradeKitExecutionContext {
            asset_class: AssetClass::Perp,
            environment: trade_kit::TradeEnvironment::Demo,
            operation: TradeKitOperation::Place,
        };
        validate_trade_kit_execution_settings(
            &market_perp,
            context,
            AuthorizedTradeKitSettings {
                environment: Some(trade_kit::TradeEnvironment::Demo),
                margin_mode: Some(consent::MarginMode::Cross),
                order_policy: Some(consent::OrderPolicy::Market),
            },
        )
        .unwrap();

        assert!(validate_trade_kit_execution_settings(
            &market_perp,
            context,
            AuthorizedTradeKitSettings {
                environment: Some(trade_kit::TradeEnvironment::Demo),
                margin_mode: Some(consent::MarginMode::Cross),
                order_policy: Some(consent::OrderPolicy::SignalPriceLimit),
            },
        )
        .unwrap_err()
        .to_string()
        .contains("signal-price limit policy"));

        let limit_without_price = vec![
            "spot".into(),
            "place".into(),
            "--ordType".into(),
            "limit".into(),
        ];
        assert!(validate_trade_kit_execution_settings(
            &limit_without_price,
            TradeKitExecutionContext {
                asset_class: AssetClass::Spot,
                environment: trade_kit::TradeEnvironment::Live,
                operation: TradeKitOperation::Place,
            },
            AuthorizedTradeKitSettings {
                environment: Some(trade_kit::TradeEnvironment::Live),
                margin_mode: None,
                order_policy: Some(consent::OrderPolicy::SignalPriceLimit),
            },
        )
        .unwrap_err()
        .to_string()
        .contains("explicit order price"));

        assert!(validate_trade_kit_execution_settings(
            &["swap".into(), "place".into(), "--ordType".into(), "market".into()],
            TradeKitExecutionContext {
                asset_class: AssetClass::Perp,
                environment: trade_kit::TradeEnvironment::Live,
                operation: TradeKitOperation::Place,
            },
            AuthorizedTradeKitSettings {
                environment: Some(trade_kit::TradeEnvironment::Live),
                margin_mode: None,
                order_policy: Some(consent::OrderPolicy::Market),
            },
        )
        .unwrap_err()
        .to_string()
        .contains("persisted margin mode"));
    }

    #[test]
    fn trade_kit_contract_close_is_bound_to_direction_margin_and_market_policy() {
        let close_long = vec![
            "swap".into(),
            "close".into(),
            "--instId".into(),
            "BTC-USDT-SWAP".into(),
            "--mgnMode".into(),
            "cross".into(),
            "--posSide".into(),
            "long".into(),
            "--demo".into(),
        ];
        validate_bound_intent(
            "trade_kit",
            "sell",
            "10",
            "job1",
            ExecutionMode::Auto,
            &close_long,
        )
        .unwrap();
        let context = trade_kit_execution_context(&close_long).unwrap();
        validate_trade_kit_execution_settings(
            &close_long,
            context,
            AuthorizedTradeKitSettings {
                environment: Some(trade_kit::TradeEnvironment::Demo),
                margin_mode: Some(consent::MarginMode::Cross),
                order_policy: Some(consent::OrderPolicy::Market),
            },
        )
        .unwrap();

        assert!(validate_bound_intent(
            "trade_kit",
            "buy",
            "10",
            "job1",
            ExecutionMode::Auto,
            &close_long,
        )
        .unwrap_err()
        .to_string()
        .contains("close direction"));

        assert!(validate_trade_kit_execution_settings(
            &close_long,
            context,
            AuthorizedTradeKitSettings {
                environment: Some(trade_kit::TradeEnvironment::Demo),
                margin_mode: Some(consent::MarginMode::Isolated),
                order_policy: Some(consent::OrderPolicy::Market),
            },
        )
        .unwrap_err()
        .to_string()
        .contains("margin mode"));

        assert!(validate_trade_kit_execution_settings(
            &close_long,
            context,
            AuthorizedTradeKitSettings {
                environment: Some(trade_kit::TradeEnvironment::Demo),
                margin_mode: Some(consent::MarginMode::Cross),
                order_policy: Some(consent::OrderPolicy::SignalPriceLimit),
            },
        )
        .unwrap_err()
        .to_string()
        .contains("market policy"));
    }

    #[test]
    fn execution_latch_distinguishes_pre_submit_from_spawned_recovery() {
        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = test_tempdir();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        assert_eq!(
            recovery_state("job-phase", "delivery-1").unwrap(),
            RecoveryState::NoExecution
        );
        assert!(reserve_execution("job-phase", "delivery-1").unwrap());
        assert_eq!(
            recovery_state("job-phase", "delivery-1").unwrap(),
            RecoveryState::PreSubmitInterrupted
        );
        update_execution_phase(
            "job-phase",
            "delivery-1",
            ExecutionPhase::Spawned,
        )
        .unwrap();
        assert_eq!(
            recovery_state("job-phase", "delivery-1").unwrap(),
            RecoveryState::SubmissionUnknown
        );
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn terminal_journal_repairs_outcome_notice_and_fifo_without_execution() {
        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = test_tempdir();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        consent::register_delivery_context(
            "job-journal",
            "7",
            "8",
            Some("session:test"),
            "delivery-1",
            "/tmp/signal.txt",
            "text",
            1,
        )
        .unwrap();
        super::super::delivery_queue::enqueue("job-journal", "delivery-1").unwrap();
        super::super::delivery_queue::mark_awaiting_decision("job-journal", "delivery-1")
            .unwrap();
        let now = now_secs();
        let outcome = ExecutionOutcome {
            version: OUTCOME_VERSION,
            job_id: "job-journal".to_string(),
            delivery_id: "delivery-1".to_string(),
            venue: String::new(),
            action: String::new(),
            amount: String::new(),
            execution_mode: ExecutionMode::Auto,
            status: OutcomeStatus::FailedBeforeExecution,
            receipt: None,
            reason: Some("test interruption".to_string()),
            notification_pending: true,
            notification_attempts: 0,
            next_notification_attempt_at: 0,
            created_at: now,
            updated_at: now,
        };
        let journal = write_terminal_journal(&outcome).unwrap();
        assert_eq!(
            reconcile_terminal_journals(1, Duration::from_secs(1)).unwrap(),
            1
        );
        assert!(outcome_path("job-journal", "delivery-1")
            .unwrap()
            .exists());
        assert!(notice_ref_path("job-journal", "delivery-1")
            .unwrap()
            .exists());
        assert!(!super::super::delivery_queue::contains_delivery(
            "job-journal",
            "delivery-1"
        )
        .unwrap());
        assert!(!journal.exists());
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn durable_outcome_repairs_fifo_even_when_terminal_journal_is_missing() {
        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = test_tempdir();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        for delivery_id in ["delivery-1", "delivery-2"] {
            consent::register_delivery_context(
                "job-outcome-fallback",
                "7",
                "8",
                Some("session:test"),
                delivery_id,
                "/tmp/signal.txt",
                "text",
                1,
            )
            .unwrap();
            super::super::delivery_queue::enqueue(
                "job-outcome-fallback",
                delivery_id,
            )
            .unwrap();
        }
        super::super::delivery_queue::mark_awaiting_decision(
            "job-outcome-fallback",
            "delivery-1",
        )
        .unwrap();
        let now = now_secs();
        let outcome = ExecutionOutcome {
            version: OUTCOME_VERSION,
            job_id: "job-outcome-fallback".to_string(),
            delivery_id: "delivery-1".to_string(),
            venue: String::new(),
            action: String::new(),
            amount: String::new(),
            execution_mode: ExecutionMode::Auto,
            status: OutcomeStatus::FailedBeforeExecution,
            receipt: None,
            reason: Some("journal unavailable".to_string()),
            notification_pending: true,
            notification_attempts: 0,
            next_notification_attempt_at: 0,
            created_at: now,
            updated_at: now,
        };
        write_outcome(
            &outcome_path("job-outcome-fallback", "delivery-1").unwrap(),
            &outcome,
        )
        .unwrap();
        assert!(!terminal_journal_path("job-outcome-fallback", "delivery-1")
            .unwrap()
            .exists());
        assert!(super::super::delivery_queue::reconcile_terminal_head(
            "job-outcome-fallback"
        )
        .unwrap());
        assert!(!super::super::delivery_queue::contains_delivery(
            "job-outcome-fallback",
            "delivery-1"
        )
        .unwrap());
        assert!(super::super::delivery_queue::contains_delivery(
            "job-outcome-fallback",
            "delivery-2"
        )
        .unwrap());
        std::env::remove_var("ONCHAINOS_HOME");
    }
}
