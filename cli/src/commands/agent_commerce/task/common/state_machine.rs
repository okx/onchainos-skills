//! Task-system state machine — single source of truth.
//!
//! Centralizes string literals like `"created"` / `"provider_applied"` previously scattered across
//! `available_actions` / `provider/flow.rs` / `buyer/flow.rs` / `evaluator/flow.rs`, exposing
//! `Status` / `Event` / `Role` enums plus status<->event conversion helpers. All matches now go
//! through the enums, eliminating string-spelling drift.
//!
//! By design the **event view** and the **status view** are interconvertible:
//! - `entry_event(status)` — the entry event that drove the task into this status.
//! - `status_when_event(event)` — what status the task is in when the event fires (including
//!   "pass-through events" like `provider_applied`, which fires in the created state and does not
//!   change the status).

// ─── Role ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Buyer,
    Provider,
    Evaluator,
}

impl Role {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "buyer" | "client"            => Some(Role::Buyer),
            "provider" | "seller"         => Some(Role::Provider),
            "evaluator" | "arbitrator"    => Some(Role::Evaluator),
            _                             => None,
        }
    }

    pub fn as_canonical_str(&self) -> &'static str {
        match self {
            Role::Buyer     => "buyer",
            Role::Provider    => "provider",
            Role::Evaluator => "evaluator",
        }
    }
}

// ─── Status ─────────────────────────────────────────────────────────────

/// The task's current real state in the state machine. Backend `TaskStatusEnum` returns `status: int`;
/// derive locally via [`Status::from_int`].
///
/// Aligns with backend `TaskStatusEnum`:
/// INIT=-1, CREATED=0, ACCEPTED=1, SUBMITTED=2, REJECTED=3, DISPUTED=4,
/// ADMINSTOPPED=5, COMPLETE=6, CLOSE=7, EXPIRED=8, FAILED=9.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Init,         // -1
    Created,      // 0 (backend original is OPEN; renamed to avoid ambiguity with "public" visibility)
    Accepted,     // 1
    Submitted,    // 2
    Rejected,     // 3
    Disputed,     // 4
    AdminStopped, // 5
    Completed,    // 6
    Close,        // 7
    Expired,      // 8
    Failed,       // 9
    /// A status string returned by the backend that this enum does not recognize (tolerantly preserved as-is).
    Other(String),
}

impl Status {
    /// String parsing (for the CLI `--jobStatus` flag / event-name parsing); int fields in the spec should go through [`Self::from_int`].
    pub fn parse(s: &str) -> Self {
        match s {
            "init"                               => Status::Init,
            "created" | "open"                   => Status::Created,
            "accepted"                           => Status::Accepted,
            "submitted"                          => Status::Submitted,
            "rejected"                           => Status::Rejected,
            "disputed"                           => Status::Disputed,
            "admin_stopped" | "adminstopped"     => Status::AdminStopped,
            "completed" | "complete"             => Status::Completed,
            "close" | "closed"                   => Status::Close,
            "expired"                            => Status::Expired,
            "failed"                             => Status::Failed,
            other                                => Status::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Status::Init         => "init",
            Status::Created      => "created",
            Status::Accepted     => "accepted",
            Status::Submitted    => "submitted",
            Status::Rejected     => "rejected",
            Status::Disputed     => "disputed",
            Status::AdminStopped => "admin_stopped",
            Status::Completed    => "completed",
            Status::Close        => "close",
            Status::Expired      => "expired",
            Status::Failed       => "failed",
            Status::Other(s)     => s.as_str(),
        }
    }

    /// Backend `TaskStatusEnum` int mapping:
    /// -1=INIT / 0=CREATED / 1=ACCEPTED / 2=SUBMITTED / 3=REJECTED / 4=DISPUTED /
    /// 5=ADMINSTOPPED / 6=COMPLETE / 7=CLOSE / 8=EXPIRED / 9=FAILED.
    pub fn from_int(n: i32) -> Self {
        match n {
            -1 => Status::Init,
             0 => Status::Created,
             1 => Status::Accepted,
             2 => Status::Submitted,
             3 => Status::Rejected,
             4 => Status::Disputed,
             5 => Status::AdminStopped,
             6 => Status::Completed,
             7 => Status::Close,
             8 => Status::Expired,
             9 => Status::Failed,
            other => Status::Other(format!("status_{other}")),
        }
    }

    /// Terminal states of the main task state machine — in these statuses the task is finished and
    /// no further chain events can advance it; any dispute subflow (if it exists) is also necessarily
    /// closed, and any commit/reveal vote will be slashed.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Status::Completed | Status::Close | Status::Expired | Status::Failed,
        )
    }
}

// ─── DisputeRoundStatus ─────────────────────────────────────────────────

/// Current phase of a single dispute round's sub state machine, carried by the
/// `disputeRoundStatus: int` field on `GET /priapi/v1/aieco/task/{jobId}/dispute/status`.
/// Orthogonal to the main task status [`Status`] — a Disputed task may walk through
/// CommitPhase / RevealPhase / Completed in the dispute subflow, or drop into Invalidated
/// when the current round's votes are insufficient and wait for the next-round redraw.
///
/// Aligns with backend `RoundStatusEnum`:
/// INIT=0, COMMIT_PHASE=1, REVEAL_PHASE=2, COMPLETED=3, REJECTED=4, INVALIDATED=5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisputeRoundStatus {
    Init,         // 0 — initialized (round started, waiting to enter the commit window)
    CommitPhase,  // 1 — commit phase
    RevealPhase,  // 2 — reveal phase
    Completed,    // 3 — round completed (verdict reached)
    Rejected,     // 4 — round rejected
    Invalidated,  // 5 — round invalidated (insufficient votes / nobody revealed); wait for next-round redraw
    /// A status code returned by the backend that this enum does not recognize (tolerantly preserved as-is).
    Other(i32),
}

impl DisputeRoundStatus {
    pub fn from_int(n: i32) -> Self {
        match n {
            0 => DisputeRoundStatus::Init,
            1 => DisputeRoundStatus::CommitPhase,
            2 => DisputeRoundStatus::RevealPhase,
            3 => DisputeRoundStatus::Completed,
            4 => DisputeRoundStatus::Rejected,
            5 => DisputeRoundStatus::Invalidated,
            other => DisputeRoundStatus::Other(other),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            DisputeRoundStatus::Init         => "init",
            DisputeRoundStatus::CommitPhase  => "commit_phase",
            DisputeRoundStatus::RevealPhase  => "reveal_phase",
            DisputeRoundStatus::Completed    => "completed",
            DisputeRoundStatus::Rejected     => "rejected",
            DisputeRoundStatus::Invalidated  => "invalidated",
            DisputeRoundStatus::Other(_)     => "unknown",
        }
    }
}

// ─── Event ──────────────────────────────────────────────────────────────

/// The `event` field in system notifications — the specific action that triggered this notification.
/// Fully aligned with the backend event enum (see the task system design doc).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    // ── Main task flow ────────────────────────────────────────────────
    /// Task creation on-chain (status enters created; notifies buyer).
    JobCreated,
    /// Provider apply on-chain (status remains created; pass-through event; notifies the provider that just applied).
    ProviderApplied,
    /// Buyer confirm-accept on-chain (status enters accepted; notifies provider).
    JobAccepted,
    /// Provider deliver on-chain (status enters submitted; notifies buyer to review).
    JobSubmitted,
    /// Buyer complete on-chain / arbitration approve (status enters completed; notifies provider).
    JobCompleted,
    /// Buyer reject on-chain (status enters rejected; notifies provider to choose between arbitration / refund).
    JobRefused,
    /// Arbitration phase-1 (approve) on-chain (status remains rejected; pass-through event;
    /// notifies the initiating provider to proceed to phase-2 dispute confirm).
    DisputeApproved,
    /// Either party's dispute-raise on-chain (status enters disputed; notifies both buyer + provider to upload evidence).
    JobDisputed,
    /// Provider agrees to refund / arbitration buyer-wins refund on-chain (status enters refunded; notifies buyer + provider).
    JobRefunded,
    /// DisputeSettled arbitration verdict (status enters completed or refunded; notifies buyer/provider/voters
    /// to call /claimable + /claim to collect rewards).
    DisputeResolved,
    /// Task expired (no accept before the acceptance window, or no submit before the delivery window;
    /// notifies buyer to close and reclaim funds).
    JobExpired,
    /// TaskMarket.close on-chain / Close tx result (notifies the initiating client).
    JobClosed,
    /// TaskMarket.setVisibility on-chain (notifies the initiating client).
    JobVisibilityChanged,
    /// TaskMarket.setPaymentMode on-chain (notifies the initiating client).
    JobPaymentModeChanged,

    // ── Arbitration lifecycle (evaluator sub state machine) ─────────────
    /// VotersSelected — round evaluators selected (notifies each selected evaluator to call /vote with commit).
    EvaluatorSelected,
    /// RevealStarted on-chain (commit phase ends, reveal window opens; notifies evaluators who already committed).
    RevealStarted,
    /// Evaluator commit tx on-chain success (notifies the evaluator that initiated the commit; wait for reveal window).
    VoteCommitted,
    /// Evaluator reveal tx on-chain success (notifies the evaluator that initiated the reveal; wait for dispute_resolved).
    VoteRevealed,
    /// DisputeInvalidated — current round invalidated (insufficient votes / nobody revealed, etc.;
    /// notifies buyer/provider/round-evaluators to wait for the next round).
    RoundFailed,
    /// VoterStaking.Slashed on-chain (no user tx triggers this; notifies the slashed evaluator).
    Slashed,

    // ── Staking lifecycle (evaluator) ─────────────────────────────────
    /// VoterStaking.Staked on-chain (**both first-time stake and additional increaseStake emit this event**;
    /// the real backend does not distinguish — the event stream only has `staked`. Distinguishing first-time
    /// vs additional can only be inferred from my-stake's activeStake delta.)
    Staked,
    /// VoterStaking.UnstakeRequested on-chain (enters cooldown; notifies the evaluator that initiated unstake).
    UnstakeRequested,
    /// VoterStaking.UnstakeClaimed on-chain (cooldown finished, funds withdrawn; notifies the evaluator that initiated claim).
    UnstakeClaimed,
    /// VoterStaking.UnstakeCancelled on-chain (cancelled during cooldown; notifies the evaluator that initiated cancel).
    UnstakeCancelled,
    /// claimRewards tx on-chain result (notifies the claimer — client/provider/evaluator).
    RewardClaimed,

    // ── Timeout events ────────────────────────────────────────────────
    /// Submit timeout — no delivery (notifies buyer to call claimAutoRefund).
    SubmitExpired,
    /// After reject, the provider failed to raise arbitration in time (notifies buyer to call claimAutoRefund).
    RefuseExpired,
    /// Review timeout (after provider submit, the buyer did not confirm; notifies provider to call claimAutoComplete).
    ReviewExpired,
    // ── Auto-complete / auto-refund tx receipts ──────────────────────
    /// Provider's claimAutoComplete tx on-chain result (after review timeout the provider pulls funds; notifies provider).
    JobAutoCompleted,
    /// Buyer's claimAutoRefund tx on-chain result (after submit/refuse timeout the buyer pulls funds back; notifies buyer).
    JobAutoRefunded,

    // ── Deadline reminders (warn class, no status change) ─────────────
    /// Escrow delivery-window nearing-deadline reminder (notifies provider to submit).
    SubmitDeadlineWarn,
    /// Escrow submit→complete nearing-deadline reminder (notifies buyer to complete).
    ReviewDeadlineWarn,

    // ── Extra evaluator lifecycle ────────────────────────────────────
    /// VoterStaking.VoterStakeStopped on-chain (exits the voter pool; notifies the evaluator that initiated stop).
    StakeStopped,
    /// DisputeManager.VoterCooldownEntered on-chain (passive entry into cooldown; notifies evaluator).
    CooldownEntered,

    // ── Buyer terms-change tx receipts (no status change, stays created) ─
    /// setTokenAndBudget tx on-chain success (notifies all sub sessions on the buyer side; if none, notifies backup).
    TaskTokenBudgetChange,
    /// setProviderAndAgentId tx on-chain success (notifies all sub sessions on the buyer side; if none, notifies backup).
    TaskProviderChange,

    // ── User-session pseudo events (triggered by user command, no status change) ──
    /// After set-provider succeeds, the user session immediately kicks off the new-provider flow
    /// (without waiting for the on-chain task_provider_change confirmation).
    SwitchProvider,

    // ── Attachment relay event (buyer-local dispatch, no status change) ─
    /// User session dispatched `[ATTACHMENT_ADDED]`; sub session uploads + forwards the file to the provider.
    /// Can fire in Created (with active sub session) or Accepted — multi-status, so freshness check is skipped.
    AttachmentAdded,

    // ── Negotiation relay events (buyer-local dispatch, no status change) ─
    /// Provider's natural-language reply (no [intent:*] marker); buyer.md Route 4 → negotiate_reply.
    NegotiateReply,
    /// Provider replied with [intent:ack] (accepts PROPOSE); buyer.md Route 3 → negotiate_ack.
    NegotiateAck,
    /// Provider replied with [intent:counter] (counter-proposal); buyer.md Route 3 → negotiate_counter.
    NegotiateCounter,

    // ── Network / restart recovery events (pass-through, no status change) ─
    /// After a network / machine restart, the backend notifies the agent to resume this task's script.
    /// Envelope shape (per-task fan-out):
    /// `{ agentId, message: { event: "wakeup_notify", source: "system",
    ///                         jobId: <real jobId>, jobStatus: <real status string>,
    ///                         paymentMode, visibility, ... } }`
    /// Upon receipt the agent **must not** call next-action with `wakeup_notify`;
    /// instead, read `message.jobStatus` to get the real status, then call next-action again with
    /// that as `--jobStatus` to resume the script for the current status. See the WakeupNotify arm
    /// in flow.rs for details.
    WakeupNotify,

    /// An event name returned by the backend that this enum does not recognize (also used to carry
    /// user-instruction pseudo events: dispute_raise / agree_refund / dispute_evidence / close / set_public).
    Other(String),
}

impl Event {
    pub fn parse(s: &str) -> Self {
        match s {
            // Main task flow
            "job_created"               => Event::JobCreated,
            "provider_applied"          => Event::ProviderApplied,
            "job_accepted"              => Event::JobAccepted,
            "job_submitted"             => Event::JobSubmitted,
            "job_completed"             => Event::JobCompleted,
            "job_refused"               => Event::JobRefused,
            "dispute_approved"          => Event::DisputeApproved,
            "job_disputed"              => Event::JobDisputed,
            "job_refunded"              => Event::JobRefunded,
            "dispute_resolved"          => Event::DisputeResolved,
            "job_expired"               => Event::JobExpired,
            "job_closed"                => Event::JobClosed,
            "job_visibility_changed"    => Event::JobVisibilityChanged,
            "job_payment_mode_changed"  => Event::JobPaymentModeChanged,
            // Arbitration lifecycle
            "evaluator_selected"        => Event::EvaluatorSelected,
            "reveal_started"            => Event::RevealStarted,
            "vote_committed"            => Event::VoteCommitted,
            "vote_revealed"             => Event::VoteRevealed,
            "round_failed"              => Event::RoundFailed,
            "slashed"                   => Event::Slashed,
            // Staking lifecycle (first-time / additional both map to Staked — the real backend only emits one `staked` event)
            "staked"                    => Event::Staked,
            "unstake_requested"         => Event::UnstakeRequested,
            "unstake_claimed"           => Event::UnstakeClaimed,
            "unstake_cancelled"         => Event::UnstakeCancelled,
            "reward_claimed"            => Event::RewardClaimed,
            // Timeouts
            "submit_expired"            => Event::SubmitExpired,
            "refuse_expired"            => Event::RefuseExpired,
            "review_expired"            => Event::ReviewExpired,
            // Auto-complete / auto-refund tx receipts
            "job_auto_completed"        => Event::JobAutoCompleted,
            "job_auto_refunded"         => Event::JobAutoRefunded,
            // Reminders
            "submit_deadline_warn"      => Event::SubmitDeadlineWarn,
            "review_deadline_warn"      => Event::ReviewDeadlineWarn,
            // Extra evaluator lifecycle
            "stake_stopped"             => Event::StakeStopped,
            "cooldown_entered"          => Event::CooldownEntered,
            // Buyer terms-change tx receipts
            "task_token_budget_change"  => Event::TaskTokenBudgetChange,
            "task_provider_change"      => Event::TaskProviderChange,
            // User-session pseudo events
            "switch_provider"           => Event::SwitchProvider,
            // Attachment relay (buyer-local dispatch)
            "attachment_added"          => Event::AttachmentAdded,
            // Negotiation relay (buyer-local dispatch)
            "negotiate_reply"           => Event::NegotiateReply,
            "negotiate_ack"             => Event::NegotiateAck,
            "negotiate_counter"         => Event::NegotiateCounter,
            // Network / restart recovery
            "wakeup_notify"             => Event::WakeupNotify,
            other                       => Event::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Event::JobCreated             => "job_created",
            Event::ProviderApplied        => "provider_applied",
            Event::JobAccepted            => "job_accepted",
            Event::JobSubmitted           => "job_submitted",
            Event::JobCompleted           => "job_completed",
            Event::JobRefused             => "job_refused",
            Event::DisputeApproved        => "dispute_approved",
            Event::JobDisputed            => "job_disputed",
            Event::JobRefunded            => "job_refunded",
            Event::DisputeResolved        => "dispute_resolved",
            Event::JobExpired             => "job_expired",
            Event::JobClosed              => "job_closed",
            Event::JobVisibilityChanged   => "job_visibility_changed",
            Event::JobPaymentModeChanged  => "job_payment_mode_changed",
            Event::EvaluatorSelected      => "evaluator_selected",
            Event::RevealStarted          => "reveal_started",
            Event::VoteCommitted          => "vote_committed",
            Event::VoteRevealed           => "vote_revealed",
            Event::RoundFailed            => "round_failed",
            Event::Slashed                => "slashed",
            Event::Staked                 => "staked",
            Event::UnstakeRequested       => "unstake_requested",
            Event::UnstakeClaimed         => "unstake_claimed",
            Event::UnstakeCancelled       => "unstake_cancelled",
            Event::RewardClaimed          => "reward_claimed",
            Event::SubmitExpired          => "submit_expired",
            Event::RefuseExpired          => "refuse_expired",
            Event::ReviewExpired          => "review_expired",
            Event::JobAutoCompleted       => "job_auto_completed",
            Event::JobAutoRefunded        => "job_auto_refunded",
            Event::SubmitDeadlineWarn     => "submit_deadline_warn",
            Event::ReviewDeadlineWarn     => "review_deadline_warn",
            Event::StakeStopped           => "stake_stopped",
            Event::CooldownEntered        => "cooldown_entered",
            Event::TaskTokenBudgetChange  => "task_token_budget_change",
            Event::TaskProviderChange    => "task_provider_change",
            Event::SwitchProvider         => "switch_provider",
            Event::AttachmentAdded        => "attachment_added",
            Event::NegotiateReply         => "negotiate_reply",
            Event::NegotiateAck           => "negotiate_ack",
            Event::NegotiateCounter       => "negotiate_counter",
            Event::WakeupNotify           => "wakeup_notify",
            Event::Other(s)               => s.as_str(),
        }
    }

    pub fn failure_label(&self) -> &'static str {
        match self {
            Event::JobAutoRefunded    => "auto-refund failed",
            Event::JobClosed          => "close failed",
            Event::JobVisibilityChanged  => "visibility toggle failed",
            Event::JobPaymentModeChanged => "payment mode switch failed",
            Event::TaskTokenBudgetChange => "payment terms change failed",
            Event::TaskProviderChange    => "provider change failed",
            Event::JobAutoCompleted   => "auto-complete failed",
            Event::RewardClaimed      => "reward claim failed",
            Event::DisputeApproved    => "dispute initiation failed",
            Event::Staked             => "staking failed",
            Event::UnstakeRequested   => "unstake failed",
            Event::UnstakeClaimed     => "unstake claim failed",
            Event::UnstakeCancelled   => "unstake cancellation failed",
            Event::StakeStopped       => "stop staking failed",
            Event::CooldownEntered    => "cooldown entry failed",
            Event::Slashed            => "slash transaction failed",
            _                         => "transaction failed",
        }
    }
}

// ─── Bidirectional mapping ───────────────────────────────────────────────

/// Which status the task is in when the event fires.
///
/// `provider_applied` does not change status — it occurs in the created state;
/// `dispute_resolved` depends on the verdict (buyer-wins → refunded; seller-wins → completed),
/// which cannot be determined from the event alone; this returns `Completed` by default,
/// and callers should prefer calling `agent status` to fetch the real status.
pub fn status_when_event(e: &Event) -> Status {
    match e {
        // Main flow
        Event::JobCreated | Event::ProviderApplied
        | Event::TaskTokenBudgetChange | Event::TaskProviderChange
        | Event::SwitchProvider
        | Event::NegotiateReply | Event::NegotiateAck | Event::NegotiateCounter => Status::Created,
        Event::JobAccepted                                                  => Status::Accepted,
        Event::JobSubmitted                                                 => Status::Submitted,
        Event::JobRefused | Event::RefuseExpired                             => Status::Rejected,
        // submit_expired: provider did not submit; status is still accepted (never entered submitted)
        Event::SubmitExpired                                                => Status::Accepted,
        // dispute_approved is a pass-through event; status is still rejected (dispute phase 1, not yet truly disputed)
        Event::DisputeApproved                                              => Status::Rejected,
        Event::JobDisputed                                                  => Status::Disputed,
        // review_expired only means the review window has ended; task is still submitted —
        // must wait for the provider's claimAutoComplete to enter completed
        Event::ReviewExpired                                                => Status::Submitted,
        // Backend TaskStatusEnum: 6=COMPLETE (funds released to provider), 9=FAILED (funds returned to buyer).
        // The two terminal states are distinguished directly by the event.
        Event::JobCompleted | Event::JobAutoCompleted                       => Status::Completed,
        Event::JobRefunded | Event::JobAutoRefunded                         => Status::Failed,
        // DisputeResolved depends on the verdict (buyer-wins → Failed; seller-wins → Completed);
        // not determinable from the event alone — default to Completed and callers should prefer `agent status`.
        Event::DisputeResolved  => Status::Completed,
        // Arbitration sub state machine: all events fire while task=disputed
        Event::EvaluatorSelected | Event::VoteCommitted
        | Event::RevealStarted | Event::VoteRevealed
        | Event::Slashed | Event::CooldownEntered | Event::RoundFailed      => Status::Disputed,
        // Reminder class (no status change; task stays in its current status)
        Event::SubmitDeadlineWarn                                           => Status::Accepted,
        Event::ReviewDeadlineWarn                                           => Status::Submitted,
        Event::JobExpired                                                   => Status::Expired,
        Event::JobClosed                                                    => Status::Close,
        // visibility/paymentMode are pass-through events that do not change status; not allowed outside of created, so expect Created
        Event::JobVisibilityChanged | Event::JobPaymentModeChanged         => Status::Created,
        // Staking / slashing / reward lifecycle is decoupled from task status
        Event::Staked
        | Event::UnstakeRequested | Event::UnstakeClaimed | Event::UnstakeCancelled
        | Event::StakeStopped                                               => Status::Other("staking".to_string()),
        Event::RewardClaimed                                                     => Status::Other("reward_claimed".to_string()),
        // attachment_added is dispatched by the user session; can fire at Created or Accepted —
        // multi-status, so freshness check is skipped via PSEUDO_EVENTS; placeholder here.
        Event::AttachmentAdded                                                  => Status::Other("attachment".to_string()),
        // wake-up is a pass-through event; the real status lives in envelope.message.jobStatus.
        // Return a placeholder status here — agents must not drive next-action with wakeup_notify.
        Event::WakeupNotify                                                 => Status::Other("wakeup".to_string()),
        Event::Other(_)                                                     => Status::Other("unknown".to_string()),
    }
}

/// The **canonical** entry event that drove the task into this status.
/// - Status::Completed canonical = JobCompleted (happy-path acceptance / arbitration seller-wins)
/// - Status::Failed canonical = JobRefunded (refund / arbitration buyer-wins)
/// - DisputeResolved is not canonical (the same event may land on either Completed or Failed)
pub fn entry_event(s: &Status) -> Option<Event> {
    match s {
        Status::Init         => None,
        Status::Created         => Some(Event::JobCreated),
        Status::Accepted     => Some(Event::JobAccepted),
        Status::Submitted    => Some(Event::JobSubmitted),
        Status::Rejected     => Some(Event::JobRefused),
        Status::Disputed     => Some(Event::JobDisputed),
        Status::AdminStopped => None,
        Status::Completed    => Some(Event::JobCompleted),
        Status::Close        => Some(Event::JobClosed),
        Status::Expired      => Some(Event::JobExpired),
        Status::Failed       => Some(Event::JobRefunded),
        Status::Other(_)     => None,
    }
}

/// Given a string (which may be either a status or an event), parse it as an event first.
/// On failure (i.e. Event::Other) fall back to status parsing and run it back through entry_event.
/// Used as the compatibility entry for `next-action --jobStatus <X>` — historical callers pass either event or status names.
pub fn parse_status_or_event(s: &str) -> Event {
    let evt = Event::parse(s);
    if !matches!(evt, Event::Other(_)) {
        return evt;
    }
    let status = Status::parse(s);
    entry_event(&status).unwrap_or(Event::Other(s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_event_roundtrip() {
        // entry_event(s) → e ; status_when_event(e) must round-trip back to s.
        // Status::AdminStopped has no client-side entry event (entry_event returns None); skip.
        // Status::Completed → JobCompleted; Status::Failed → JobRefunded (buyer-wins / refund).
        for s in [
            Status::Created, Status::Accepted, Status::Submitted, Status::Rejected,
            Status::Disputed, Status::Completed, Status::Close, Status::Expired,
            Status::Failed,
        ] {
            let e = entry_event(&s).expect("non-Other status should have entry event");
            assert_eq!(status_when_event(&e), s, "entry_event/status_when_event mismatch for {:?}", s);
        }
    }

    #[test]
    fn parse_status_or_event_handles_both() {
        assert_eq!(parse_status_or_event("provider_applied"), Event::ProviderApplied);
        assert_eq!(parse_status_or_event("created"), Event::JobCreated);
        assert_eq!(parse_status_or_event("open"), Event::JobCreated); // backend compatibility
        assert_eq!(parse_status_or_event("submitted"), Event::JobSubmitted);
    }

    #[test]
    fn provider_applied_keeps_status_created() {
        // Pass-through event does not change status.
        assert_eq!(status_when_event(&Event::ProviderApplied), Status::Created);
    }
}
