//! `dispute/status` hard-gate helper — not a standalone CLI subcommand; it is the
//! preflight gate called inline by `evidence-info` (`handle_info`).
//!
//! Purpose: when the evaluator receives a `evaluator_selected` (or similar) system
//! notification, the envelope may be stale (agent restarted, network lag, commit
//! window already closed, current round re-drawn, task already settled, …).
//! Acting on a stale envelope to run commit/reveal gets the stake slashed, so
//! before downloading evidence `handle_info` runs [`precheck_round_gate`] to
//! check every stale scenario at once; if any gate fails it returns early
//! without downloading.
//!
//! API: `GET /priapi/v1/aieco/task/{jobId}/dispute/status` returns
//! `{ jobId, currentRound, selectedVoter, taskStatus, disputeStatus }`. The
//! backend personalizes by caller `agenticId` (when not selected as juror,
//! `selectedVoter=null`).
//!
//! Four hard gates (AND):
//! 1. `taskStatus` must not be a terminal status — 6 Completed / 7 Close / 8 Expired / 9 Rejected.
//! 2. Input `round_num` must equal `currentRound` (envelope lagging behind the real on-chain round = stale).
//! 3. `disputeRoundStatus` must be 1 (CommitPhase) — commit window already closed / not yet open → voting gets slashed.
//! 4. `selectedVoter` must be non-null (this account is not the selected juror for this round).
//!
//! [`precheck_round_gate`] is responsible for its own diagnostic output + stable marker lines:
//! - All pass → print `selected: yes`, return `true` (`handle_info` proceeds to download evidence).
//! - Any fail → print `reason: ...` + `selected: no`, return `false` (`handle_info` returns early).

use anyhow::{Context, Result};
use serde::de::IgnoredAny;
use serde::Deserialize;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::state_machine::{DisputeRoundStatus, Status};

/// Raw response payload for the `dispute/status` endpoint.
///
/// The `Response` suffix intentionally distinguishes this from
/// [`crate::commands::agent_commerce::task::common::state_machine::DisputeRoundStatus`]
/// — one is an HTTP DTO, the other is the arbitration sub-state-machine phase enum
/// (the `dispute_round_status: i32` field in the response maps to that enum).
///
/// **Nullable fields**: in terminal task state / when there is no active dispute,
/// the backend returns `{currentRound:null, disputeStatus:null, selectedVoter:null, taskStatus:9}`,
/// so `current_round` / `dispute_round_status` / `selected_voter` must be `Option`;
/// a bare `i64` / `i32` + `#[serde(default)]` is NOT enough — `#[serde(default)]`
/// only covers `missing`, not `null`, and will trigger
/// `invalid type: null, expected i64` deserialize failures.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisputeStatusResponse {
    pub job_id: String,
    #[serde(default)]
    pub current_round: Option<i64>,
    /// Backend personalizes by caller agentId: non-null = selected, null = not selected
    /// (including stale notification / no active dispute). The inner fields
    /// (voterAddress / voterAgentId) are guaranteed to be the caller itself when
    /// selected — zero incremental info — so we `IgnoredAny`-consume them instead
    /// of deserializing; the hard gate only needs `is_none()`.
    #[serde(default)]
    pub selected_voter: Option<IgnoredAny>,
    /// Current state of the task main state machine. The sample always carries an
    /// integer (terminal states also give a number like 9 Rejected), never null,
    /// so a bare `i32` + `default` is fine.
    #[serde(default)]
    pub task_status: i32,
    /// Current phase of the arbitration sub-state-machine
    /// (`state_machine::DisputeStatus`). Null when the task is in a terminal state
    /// or when there is no dispute.
    /// `rename` + `alias` accept both backend JSON keys: `disputeStatus` /
    /// `disputeRoundStatus`, so a mismatch on either side does not break parsing.
    #[serde(default)]
    pub dispute_round_status: Option<i32>,
}

pub async fn get_dispute_status(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<DisputeStatusResponse> {
    let path = client.endpoint(job_id, "dispute/status");
    let data = client.get_with_identity(&path, agent_id).await?;
    serde_json::from_value(data).context("failed to parse dispute/status response")
}

/// Run the 4 AND hard gates: on pass return `true` and print `selected: yes`;
/// on any fail return `false` and print `reason: ...` + `selected: no`. `agent_id`
/// is resolved by the caller (`handle_info`); this function does NOT re-resolve
/// (repeated resolution within the same evaluator flow is pointless).
pub async fn precheck_round_gate(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
    round_num: &str,
) -> Result<bool> {
    let s = get_dispute_status(client, job_id, agent_id).await?;

    // Lift the backend's bare ints into enums up front; downstream gate checks
    // and printing both work on enums, eliminating bare-number comparisons.
    // Both enums tolerate unknown values (Status::Other / DisputeStatus::Other).
    // The disputeStatus field is null in terminal / no-dispute cases, hence the
    // whole `dispute_round_status` is `Option`.
    let task_status = Status::from_int(s.task_status);
    let dispute_round_status = s.dispute_round_status.map(DisputeRoundStatus::from_int);

    // Option-field printing: render None as "null" so readers don't mistake a
    // default 0 for round 0 / a NONE state.
    let fmt_opt = |n: Option<i64>| n.map(|v| v.to_string()).unwrap_or_else(|| "null".into());
    let fmt_opt_i32 = |n: Option<i32>| n.map(|v| v.to_string()).unwrap_or_else(|| "null".into());

    println!("dispute status (jobId={})", s.job_id);
    println!("  currentRound : {}", fmt_opt(s.current_round));
    println!("  taskStatus   : {} ({})", s.task_status, task_status.as_str());
    println!(
        "  dispute_round_status: {} ({})",
        fmt_opt_i32(s.dispute_round_status),
        dispute_round_status.as_ref().map(DisputeRoundStatus::as_str).unwrap_or("null"),
    );
    println!(
        "  selectedVoter: {}",
        match &s.selected_voter {
            Some(_) => "present (this account is selected as juror for current round)",
            None => "null (not selected for current round / notification expired / no active dispute)",
        },
    );

    // Hard gates (AND): any failure is stale; print the first failing reason.
    // Order: first check whether the task is in a terminal state (strongest
    // signal — terminal states have currentRound/disputeStatus both null, so we
    // must short-circuit here, otherwise the None branches below would print
    // misleading reasons) → then verify round_num is parseable → then verify
    // on-chain currentRound is non-null → then verify req_round == currentRound
    // → then verify disputeStatus is non-null → then verify
    // disputeStatus == CommitPhase → finally verify this account was selected.
    let reason: Option<String> = if task_status.is_terminal() {
        Some(format!(
            "taskStatus={} ({}) is terminal — task finished, dispute window closed",
            s.task_status, task_status.as_str(),
        ))
    } else {
        match round_num.parse::<i64>() {
            Err(e) => Some(format!("--round-num cannot be parsed as integer: {round_num:?} ({e})")),
            Ok(req_round) => match (s.current_round, dispute_round_status.as_ref()) {
                (None, _) => Some(
                    "currentRound=null — no active dispute (task not in dispute / already ended / backend has not advanced round)".into(),
                ),
                (Some(cur), _) if req_round != cur => Some(format!(
                    "round mismatch: envelope round_num={req_round} != on-chain currentRound={cur} (stale envelope)",
                )),
                (Some(_), None) => Some(
                    "disputeStatus=null — dispute sub-state-machine not started / already settled (commit window guaranteed closed)".into(),
                ),
                (Some(_), Some(ds)) if *ds != DisputeRoundStatus::CommitPhase => Some(format!(
                    "disputeStatus={} ({}) is not {} — commit window not open / already closed",
                    fmt_opt_i32(s.dispute_round_status),
                    ds.as_str(),
                    DisputeRoundStatus::CommitPhase.as_str(),
                )),
                (Some(_), Some(_)) if s.selected_voter.is_none() => {
                    Some("selectedVoter=null — this account is not the selected juror for the current round".into())
                }
                (Some(_), Some(_)) => None,
            },
        }
    };

    // Stable marker line + reason line: flow.rs scripts dispatch on
    // `selected: yes/no`; the reason line is for diagnostics (emitted only when
    // a gate fails, printed immediately above the `selected` line).
    match reason {
        None => {
            println!("\nselected: yes");
            Ok(true)
        }
        Some(r) => {
            println!("\nreason: {r}");
            println!("selected: no");
            Ok(false)
        }
    }
}
