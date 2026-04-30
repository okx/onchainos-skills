//! 任务系统状态机 single source of truth。
//!
//! 把散落在 `available_actions` / `provider/flow.rs` / `buyer/flow.rs` /
//! `evaluator/flow.rs` 里的字符串 `"open"` / `"provider_applied"` 收拢到这里，
//! 提供 `Status` / `Event` / `Role` enum 加上 status<->event 互转，
//! 让所有 match 都走 enum，杜绝字符串拼写漂移。
//!
//! 设计上**事件视图**与**状态视图**互通：
//! - `entry_event(status)` —— 把任务推进到此 status 的入口事件
//! - `status_when_event(event)` —— 事件触发时任务处于哪个 status（包括 `provider_applied`
//!   这种"过场事件"——发生在 open 状态下，不改变 status）

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

/// 任务在状态机里此刻的真实状态。后端 spec：响应回 `status: int`，本地用 [`Status::from_int`] 派生。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Open,
    Accepted,
    Submitted,
    Refused,
    Disputed,
    Completed,
    Refunded,
    /// 后端返回的、当前枚举不认识的状态字符串（容错保留原值）
    Other(String),
}

impl Status {
    /// 字符串解析（用于 CLI `--jobStatus` 参数 / event 名解析），spec 字段是 int 应走 [`Self::from_int`]。
    pub fn parse(s: &str) -> Self {
        match s {
            "open"                   => Status::Open,
            "accepted"               => Status::Accepted,
            "submitted"              => Status::Submitted,
            "refused"                => Status::Refused,
            "disputed"               => Status::Disputed,
            "completed" | "complete" => Status::Completed,
            "refunded"               => Status::Refunded,
            other                    => Status::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Status::Open      => "open",
            Status::Accepted  => "accepted",
            Status::Submitted => "submitted",
            Status::Refused   => "refused",
            Status::Disputed  => "disputed",
            Status::Completed => "completed",
            Status::Refunded  => "refunded",
            Status::Other(s)  => s.as_str(),
        }
    }

    /// 后端 spec 的 `status` int 映射：
    /// 0=open / 1=accepted / 2=submitted / 3=refused / 4=disputed / 5=complete /
    /// 6=refunded / 7=close。其他取值按 `status_<n>` 兜底。
    pub fn from_int(n: i32) -> Self {
        match n {
            0 => Status::Open,
            1 => Status::Accepted,
            2 => Status::Submitted,
            3 => Status::Refused,
            4 => Status::Disputed,
            5 => Status::Completed,
            6 => Status::Refunded,
            other => Status::Other(format!("status_{other}")),
        }
    }
}

// ─── Event ──────────────────────────────────────────────────────────────

/// 系统通知里的 `event` 字段——触发本通知的具体动作。
/// 完整对齐后端事件枚举（参见 task system 设计文档）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    // ── 任务主流程 ────────────────────────────────────────────────────
    /// 任务创建上链（status 进入 open；通知 buyer）
    JobCreated,
    /// 卖家 apply 上链（status 仍是 open，过场事件；通知刚 apply 的 provider）
    ProviderApplied,
    /// 买家 confirm-accept 上链（status 进入 accepted；通知 provider）
    JobAccepted,
    /// 卖家 deliver 上链（status 进入 submitted；通知 buyer 验收）
    JobSubmitted,
    /// 买家 complete 上链 / 仲裁 approve（status 进入 completed；通知 provider）
    JobCompleted,
    /// 买家 reject 上链（status 进入 refused；通知 provider 决策仲裁/退款）
    JobRefused,
    /// 仲裁第一阶段（approve）上链（status 仍 refused，过场事件；通知发起的 provider 走第二阶段 dispute confirm）
    DisputeApproved,
    /// 任一方 dispute raise 上链（status 进入 disputed；通知 buyer + provider 上传证据）
    JobDisputed,
    /// **后端无此 event** —— 保留作为内部别名（agree-refund 上链后没专门 event 名，
    /// 状态变更直接通过 broadcast 推到 refunded）。模型/skill 提到时按"卖家退款"语义处理
    ConfirmRefund,
    /// DisputeSettled 仲裁裁决（status 进入 completed 或 refunded；通知 buyer/provider/voters
    /// 调 /claimable + /claim 领取奖励）
    DisputeResolved,
    /// 任务超时（accept 截止前未接单 或 submit 截止前未提交；通知 buyer 关单回收资金）
    JobExpired,
    /// TaskMarket.close 上链 / Close tx 结果（通知发起人 client）
    JobClosed,
    /// TaskMarket.setVisibility 上链（通知发起人 client）
    JobVisibilityChanged,
    /// TaskMarket.setPaymentMode 上链（通知发起人 client）
    JobPaymentModeChanged,

    // ── 仲裁 lifecycle（evaluator 子状态机）────────────────────────────
    /// VotersSelected 选出本轮 evaluators（通知被选中的每个 evaluator 调 /vote 提 commit）
    EvaluatorSelected,
    /// RevealStarted 上链（commit 阶段结束，reveal 窗口开启；通知本轮已 commit 的 evaluators）
    RevealStarted,
    /// evaluator commit tx 上链 success（通知发起 commit 的 evaluator 本人，等 reveal 窗口）
    VoteCommitted,
    /// evaluator reveal tx 上链 success（通知发起 reveal 的 evaluator 本人，等 dispute_resolved）
    VoteRevealed,
    /// DisputeInvalidated 当前轮失效（票数不足/无人揭示等；通知 buyer/provider/本轮 evaluators 等下一轮）
    RoundFailed,
    /// VoterStaking.Slashed 上链被罚没（无 user tx 触发；通知被罚的 evaluator）
    Slashed,

    // ── 质押 lifecycle（evaluator）────────────────────────────────────
    /// VoterStaking.Staked 上链（首次质押或追加质押；通知发起 stake 的 evaluator）
    Staked,
    /// VoterStaking.UnstakeRequested 上链（进入冷却期；通知发起 unstake 的 evaluator）
    UnstakeRequested,
    /// VoterStaking.UnstakeClaimed 上链（冷却期满已提走；通知发起 claim 的 evaluator）
    UnstakeClaimed,
    /// VoterStaking.UnstakeCancelled 上链（冷却期内取消；通知发起 cancel 的 evaluator）
    UnstakeCancelled,
    /// claimRewards tx 上链结果（通知领取人 client/provider/evaluator）
    RewardClaimed,

    // ── 超时事件 ─────────────────────────────────────────────────────
    /// submit 超时未交付（通知 buyer 调 claimAutoRefund）
    SubmitExpired,
    /// refuse 后 provider 未发起仲裁超时（通知 buyer 调 claimAutoRefund）
    RefuseExpired,
    /// review 超时（provider 提交后买家未确认；通知 provider 调 claimAutoComplete）
    ReviewExpired,

    // ── 截止时间提醒（warn 类，不改 status）────────────────────────────
    /// 担保支付 accept→submit 快超时提醒（通知 provider 发起 submit）
    SubmitDeadlineWarn,
    /// 担保支付 submit→complete 快超时提醒（通知 buyer 发起 complete）
    ReviewDeadlineWarn,

    /// 后端发的、当前枚举不认识的事件名（也用来承载 user-instruction 伪 event：
    /// dispute_raise / agree_refund / dispute_evidence / close / set_public）
    Other(String),
}

impl Event {
    pub fn parse(s: &str) -> Self {
        match s {
            // 任务主流程
            "job_created"               => Event::JobCreated,
            "provider_applied"          => Event::ProviderApplied,
            "job_accepted"              => Event::JobAccepted,
            "job_submitted"             => Event::JobSubmitted,
            "job_completed"             => Event::JobCompleted,
            "job_refused"               => Event::JobRefused,
            "dispute_approved"          => Event::DisputeApproved,
            "job_disputed"              => Event::JobDisputed,
            "confirm_refund"            => Event::ConfirmRefund,
            "dispute_resolved"          => Event::DisputeResolved,
            "job_expired"               => Event::JobExpired,
            "job_closed"                => Event::JobClosed,
            "job_visibility_changed"    => Event::JobVisibilityChanged,
            "job_payment_mode_changed"  => Event::JobPaymentModeChanged,
            // 仲裁 lifecycle
            "evaluator_selected"        => Event::EvaluatorSelected,
            "reveal_started"            => Event::RevealStarted,
            "vote_committed"            => Event::VoteCommitted,
            "vote_revealed"             => Event::VoteRevealed,
            "round_failed"              => Event::RoundFailed,
            "slashed"                   => Event::Slashed,
            // 质押 lifecycle
            "staked"                    => Event::Staked,
            "stake_increased"           => Event::Staked, // legacy alias: backend now only sends "staked"
            "unstake_requested"         => Event::UnstakeRequested,
            "unstake_claimed"           => Event::UnstakeClaimed,
            "unstake_cancelled"         => Event::UnstakeCancelled,
            "reward_claimed"            => Event::RewardClaimed,
            // 超时
            "submit_expired"            => Event::SubmitExpired,
            "refuse_expired"            => Event::RefuseExpired,
            "review_expired"            => Event::ReviewExpired,
            // 提醒
            "submit_deadline_warn"      => Event::SubmitDeadlineWarn,
            "review_deadline_warn"      => Event::ReviewDeadlineWarn,
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
            Event::ConfirmRefund          => "confirm_refund",
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
            Event::SubmitDeadlineWarn     => "submit_deadline_warn",
            Event::ReviewDeadlineWarn     => "review_deadline_warn",
            Event::Other(s)               => s.as_str(),
        }
    }
}

// ─── 双向 mapping ────────────────────────────────────────────────────────

/// 事件触发时任务处于哪个 status。
///
/// `provider_applied` 不改变 status —— 它发生在 open 状态下；
/// `dispute_resolved` 取决于裁决方（buyer-wins → refunded；seller-wins → completed），
/// 单从 event 不能确定，这里默认返回 `Completed`，调用方应优先调 `agent status` 拉取真实 status。
pub fn status_when_event(e: &Event) -> Status {
    match e {
        // 主流程
        Event::JobCreated | Event::ProviderApplied                          => Status::Open,
        Event::JobAccepted                                                  => Status::Accepted,
        Event::JobSubmitted                                                 => Status::Submitted,
        Event::JobRefused | Event::SubmitExpired | Event::RefuseExpired     => Status::Refused,
        // dispute_approved 是过场事件，status 仍为 refused（dispute 阶段 1，未真正进入 disputed）
        Event::DisputeApproved                                              => Status::Refused,
        Event::JobDisputed                                                  => Status::Disputed,
        Event::JobCompleted | Event::ReviewExpired                          => Status::Completed,
        Event::ConfirmRefund                                                => Status::Refunded,
        Event::DisputeResolved                                              => Status::Completed,
        // 仲裁子状态机：所有事件都发生在 task=disputed 状态下
        Event::EvaluatorSelected | Event::RevealStarted
        | Event::VoteCommitted | Event::VoteRevealed
        | Event::RoundFailed                                                => Status::Disputed,
        // 提醒类（不改 status，task 还在原状态）
        Event::SubmitDeadlineWarn                                           => Status::Accepted,
        Event::ReviewDeadlineWarn                                           => Status::Submitted,
        // 任务级 housekeeping 事件没有清晰的状态映射，保守用 Other
        Event::JobExpired | Event::JobClosed
        | Event::JobVisibilityChanged | Event::JobPaymentModeChanged       => Status::Other("housekeeping".to_string()),
        // 质押 / 罚没 / 奖励 lifecycle 跟 task status 解耦
        Event::Staked
        | Event::UnstakeRequested | Event::UnstakeClaimed | Event::UnstakeCancelled
        | Event::RewardClaimed | Event::Slashed                             => Status::Other("staking".to_string()),
        Event::Other(_)                                                     => Status::Other("unknown".to_string()),
    }
}

/// 把任务推进到此 status 的入口事件（每个非 Other status 都有唯一 entry event）。
pub fn entry_event(s: &Status) -> Option<Event> {
    match s {
        Status::Open      => Some(Event::JobCreated),
        Status::Accepted  => Some(Event::JobAccepted),
        Status::Submitted => Some(Event::JobSubmitted),
        Status::Refused   => Some(Event::JobRefused),
        Status::Disputed  => Some(Event::JobDisputed),
        Status::Completed => Some(Event::JobCompleted),
        Status::Refunded  => Some(Event::ConfirmRefund),
        Status::Other(_)  => None,
    }
}

/// 收到一个字符串（可能是 status 也可能是 event），优先按 event 解析。
/// 失败时（即 Event::Other）尝试按 status 解析、走 entry_event 反查。
/// 用于 `next-action --jobStatus <X>` 的兼容入口——历史调用既传 event 名也传 status 名。
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
        for s in [
            Status::Open, Status::Accepted, Status::Submitted, Status::Refused,
            Status::Disputed, Status::Completed, Status::Refunded,
        ] {
            let e = entry_event(&s).expect("non-Other status should have entry event");
            // entry_event 应该能再反推回相同 status（除 DisputeResolved 这种 default 模糊）
            assert_eq!(status_when_event(&e), s, "entry_event/status_when_event mismatch for {:?}", s);
        }
    }

    #[test]
    fn parse_status_or_event_handles_both() {
        assert_eq!(parse_status_or_event("provider_applied"), Event::ProviderApplied);
        assert_eq!(parse_status_or_event("open"), Event::JobCreated);
        assert_eq!(parse_status_or_event("submitted"), Event::JobSubmitted);
    }

    #[test]
    fn provider_applied_keeps_status_open() {
        // 过场事件不改 status
        assert_eq!(status_when_event(&Event::ProviderApplied), Status::Open);
    }
}
