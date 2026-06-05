//! Evaluator staking / arbitration-config domain types + API wrappers.
//!
//! Moved out of `common::network::task_api_client` — `task_api_client` only
//! handles the low-level transport (HTTP / auth headers); business-level data
//! structures and field parsing belong in the evaluator domain module.
//!
//! - `StakingConfig`  ← GET /priapi/v1/aieco/task/staking/config
//! - `MyStake`        ← GET /priapi/v1/aieco/task/staking/myStake (amount fields are already in OKB units from the backend)

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

/// Deserialize a string-encoded number (e.g. `"604800"`) from the backend into `u64`.
/// The backend uses strings for `*Seconds` fields to avoid JS large-integer
/// precision loss.
fn de_str_u64<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let raw = String::deserialize(d)?;
    raw.parse::<u64>()
        .map_err(|e| serde::de::Error::custom(format!("expected u64 string, got {raw:?}: {e}")))
}

/// Platform staking & arbitration config (response shape of
/// GET /priapi/v1/aieco/task/staking/config). Backend reads from Apollo
/// `aitask.platform.*`; changes take effect after restart.
///
/// Sample response (fields sorted alphabetically; `rename_all=camelCase`
/// auto-maps snake_case ↔ camelCase):
/// ```json
/// {
///   "arbitrationFeeBps":         "5%",
///   "commitPhaseSeconds":        "64800",
///   "minCumulativeStakeOkb":     "0.001",
///   "partialUnstakeMinRetainOkb":"0.001",
///   "revealPhaseSeconds":        "21600",
///   "slashMinorityBps":          "1%",
///   "slashTimeoutBps":           "0.3%",
///   "slashedCooldownSeconds":    "86400",
///   "unstakeCooldownSeconds":    "604800"
/// }
/// ```
///
/// OKB amounts are decimal strings, bps fields are display strings with `%`
/// (e.g. `"5%"`) — preserved as-is. `*Seconds` fields use string transport on
/// the backend; `de_str_u64` converts them to `u64` on deserialize.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StakingConfig {
    pub min_cumulative_stake_okb: String,
    pub partial_unstake_min_retain_okb: String,
    #[serde(deserialize_with = "de_str_u64")]
    pub unstake_cooldown_seconds: u64,
    pub arbitration_fee_bps: String,
    #[serde(deserialize_with = "de_str_u64")]
    pub commit_phase_seconds: u64,
    #[serde(deserialize_with = "de_str_u64")]
    pub reveal_phase_seconds: u64,
    pub slash_minority_bps: String,
    pub slash_timeout_bps: String,
    #[serde(deserialize_with = "de_str_u64")]
    pub slashed_cooldown_seconds: u64,
}

/// Format a seconds value into a precise display string, using the supplied
/// unit (seconds-per-day, seconds-per-hour, etc.):
/// - Integer multiple → bare integer (e.g. `"7"`)
/// - Fractional       → prefer 2 decimal places; if 2 decimals round to 0, escalate to 4 (with trailing zeros stripped)
/// - 0 seconds        → `"0"`
fn format_fractional_unit(seconds: u64, unit_seconds: u64) -> String {
    if seconds == 0 {
        return "0".into();
    }
    if seconds.is_multiple_of(unit_seconds) {
        return (seconds / unit_seconds).to_string();
    }
    let value = seconds as f64 / unit_seconds as f64;
    let two = format!("{value:.2}");
    let trimmed_two = two.trim_end_matches('0').trim_end_matches('.');
    if trimmed_two != "0" {
        return trimmed_two.to_string();
    }
    let four = format!("{value:.4}");
    four.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl StakingConfig {
    /// Unstake cooldown in days (precise display, see [`format_fractional_unit`]).
    pub fn unstake_cooldown_days(&self) -> String {
        format_fractional_unit(self.unstake_cooldown_seconds, 86400)
    }

    /// Commit phase duration in hours (precise display).
    pub fn commit_phase_hours(&self) -> String {
        format_fractional_unit(self.commit_phase_seconds, 3600)
    }

    /// Reveal phase duration in hours (precise display).
    pub fn reveal_phase_hours(&self) -> String {
        format_fractional_unit(self.reveal_phase_seconds, 3600)
    }

    /// Slashed cooldown in hours (precise display).
    pub fn slashed_cooldown_hours(&self) -> String {
        format_fractional_unit(self.slashed_cooldown_seconds, 3600)
    }
}

/// On-chain staking state for the currently logged-in account (response shape
/// of GET /priapi/v1/aieco/task/staking/myStake).
///
/// Distinct from "wallet balance": the balance sits on the EOA and is
/// spendable, whereas `activeStake` has been moved out of the balance into
/// `VoterStaking` contract lockup and has historical slashes already deducted.
/// The skill's cumulative-threshold check MUST use `activeStake`; wallet
/// balance is NOT a substitute.
///
/// Sample response:
/// ```json
/// {
///   "activeDisputes":     "0",
///   "activeStake":        "0.00196520335316019",
///   "agentId":            "548",
///   "cooldownEndsAt":     0,
///   "pendingUnstake":     "0",
///   "registered":         true,
///   "unstakeAvailableAt": 0,
///   "validStake":         "0.00196520335316019",
///   "voterAddress":       "0x9b66587f0adaf2047bf925ae196e371401e429f7"
/// }
/// ```
///
/// The backend now returns `activeStake` / `pendingUnstake` / `validStake`
/// uniformly in OKB units (no longer wei); the local `_okb` suffix is just a
/// semantic hint on field names. `cooldownEndsAt` / `unstakeAvailableAt` are
/// JSON numbers (unix seconds); `0` means "not applicable".
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MyStake {
    pub voter_address: String,
    pub agent_id: String,
    #[serde(rename = "activeStake")]
    pub active_stake_okb: String,
    #[serde(rename = "pendingUnstake")]
    pub pending_unstake_okb: String,
    #[serde(rename = "validStake")]
    pub valid_stake_okb: String,
    pub active_disputes: String,
    #[serde(default)]
    pub cooldown_ends_at: i64,
    #[serde(default)]
    pub unstake_available_at: i64,
    #[serde(default)]
    pub registered: bool,
}

/// Fetch the platform staking & arbitration config
/// (GET /priapi/v1/aieco/task/staking/config).
///
/// This endpoint requires JWT + `agenticId` header (the backend interceptor
/// validates evaluator identity); no body. Returns the cumulative stake
/// threshold, unstake cooldown, arbitration deposit, commit/reveal durations,
/// slash ratios, etc. All values come from Apollo config — the backend is
/// authoritative; the CLI only uses them for UX hints and local preflight
/// (not a substitute for contract/backend validation).
pub async fn get_staking_config(
    client: &mut TaskApiClient,
    agent_id: &str,
) -> Result<StakingConfig> {
    let data = client
        .get_with_identity("/priapi/v1/aieco/task/staking/config", agent_id)
        .await?;
    serde_json::from_value(data).context("failed to parse staking config response")
}

/// Fetch the currently logged-in account's on-chain staking state
/// (GET /priapi/v1/aieco/task/staking/myStake).
///
/// The API doc says only JWT is required, but in practice a pure-JWT call is
/// rejected by the backend interceptor (code=3001) — same as
/// `/staking/config`, the `agenticId` header is required for evaluator
/// identity validation. So this aligns with `get_staking_config`: resolve the
/// evaluator agentId and call through `get_with_identity`.
///
/// The backend now returns `activeStake` / `pendingUnstake` / `validStake`
/// uniformly as decimal strings in OKB units; the fields can be displayed
/// directly with no wei → OKB conversion. In the response, `agentId` is
/// `"0"` and `registered=false` when not registered, but this endpoint must
/// only be called after evaluator registration (otherwise the interceptor
/// rejects upstream).
pub async fn get_my_stake(client: &mut TaskApiClient, agent_id: &str) -> Result<MyStake> {
    let data = client
        .get_with_identity("/priapi/v1/aieco/task/staking/myStake", agent_id)
        .await?;
    serde_json::from_value(data).context("failed to parse myStake response")
}
