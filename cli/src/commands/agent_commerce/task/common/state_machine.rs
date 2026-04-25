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
    Seller,
    Evaluator,
}

impl Role {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "buyer" | "client"            => Some(Role::Buyer),
            "seller" | "provider"         => Some(Role::Seller),
            "evaluator" | "arbitrator"    => Some(Role::Evaluator),
            _                             => None,
        }
    }

    pub fn as_canonical_str(&self) -> &'static str {
        match self {
            Role::Buyer     => "buyer",
            Role::Seller    => "seller",
            Role::Evaluator => "evaluator",
        }
    }
}

// ─── Status ─────────────────────────────────────────────────────────────

/// 任务在状态机里此刻的真实状态（mock-api `task.statusStr`）。
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
    pub fn parse(s: &str) -> Self {
        match s {
            "open"      => Status::Open,
            "accepted"  => Status::Accepted,
            "submitted" => Status::Submitted,
            "refused"   => Status::Refused,
            "disputed"  => Status::Disputed,
            "completed" => Status::Completed,
            "refunded"  => Status::Refunded,
            other       => Status::Other(other.to_string()),
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
}

// ─── Event ──────────────────────────────────────────────────────────────

/// 系统通知里的 `event` 字段——触发本通知的具体动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// 任务创建上链（status 进入 open）
    JobCreated,
    /// 卖家 apply 上链（status 仍是 open，过场事件）
    ProviderApplied,
    /// 买家 confirm-accept 上链（status 进入 accepted）
    JobAccepted,
    /// 卖家 deliver 上链（status 进入 submitted）
    JobSubmitted,
    /// 买家 complete 上链（status 进入 completed）
    JobCompleted,
    /// 买家 reject 上链（status 进入 refused）
    JobRefused,
    /// 任一方 dispute raise 上链（status 进入 disputed）
    JobDisputed,
    /// 卖家 agree-refund 上链（status 进入 refunded）
    ConfirmRefund,
    /// 仲裁结果上链（status 进入 completed 或 refunded，看裁决方）
    DisputeResolved,
    /// 任务超时（accept 截止前未接单 或 submit 截止前未提交）
    JobExpired,
    /// 关闭任务上链
    JobClosed,
    /// 买家切换公开/私有可见性上链
    JobVisibilityChanged,
    /// 买家切换支付模式上链（escrow / non_escrow / x402）
    JobPaymentModeChanged,
    /// 提交交付物截止时间已过（卖家未提交）
    SubmitExpired,
    /// 拒绝后仲裁截止时间已过（卖家未发起仲裁）
    RefuseExpired,
    /// 后端发的、当前枚举不认识的事件名（也用来承载 user-instruction 伪 event：
    /// dispute_raise / agree_refund / dispute_evidence / close / set_public）
    Other(String),
}

impl Event {
    pub fn parse(s: &str) -> Self {
        match s {
            "job_created"               => Event::JobCreated,
            "provider_applied"          => Event::ProviderApplied,
            "job_accepted"              => Event::JobAccepted,
            "job_submitted"             => Event::JobSubmitted,
            "job_completed"             => Event::JobCompleted,
            "job_refused"               => Event::JobRefused,
            "job_disputed"              => Event::JobDisputed,
            "confirm_refund"            => Event::ConfirmRefund,
            "dispute_resolved"          => Event::DisputeResolved,
            "job_expired"               => Event::JobExpired,
            "job_closed"                => Event::JobClosed,
            "job_visibility_changed"    => Event::JobVisibilityChanged,
            "job_payment_mode_changed"  => Event::JobPaymentModeChanged,
            "submit_expired"            => Event::SubmitExpired,
            "refuse_expired"            => Event::RefuseExpired,
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
            Event::JobDisputed            => "job_disputed",
            Event::ConfirmRefund          => "confirm_refund",
            Event::DisputeResolved        => "dispute_resolved",
            Event::JobExpired             => "job_expired",
            Event::JobClosed              => "job_closed",
            Event::JobVisibilityChanged   => "job_visibility_changed",
            Event::JobPaymentModeChanged  => "job_payment_mode_changed",
            Event::SubmitExpired          => "submit_expired",
            Event::RefuseExpired          => "refuse_expired",
            Event::Other(s)               => s.as_str(),
        }
    }
}

// ─── 双向 mapping ────────────────────────────────────────────────────────

/// 事件触发时任务处于哪个 status。
///
/// `provider_applied` 不改变 status —— 它发生在 open 状态下；
/// `dispute_resolved` 取决于裁决方（buyer-wins → refunded；seller-wins → completed），
/// 单从 event 不能确定，这里默认返回 `Completed`，调用方应优先用 mock-api 实时拉取的 status。
pub fn status_when_event(e: &Event) -> Status {
    match e {
        Event::JobCreated | Event::ProviderApplied                          => Status::Open,
        Event::JobAccepted                                                  => Status::Accepted,
        Event::JobSubmitted                                                 => Status::Submitted,
        Event::JobRefused | Event::SubmitExpired | Event::RefuseExpired     => Status::Refused,
        Event::JobDisputed                                                  => Status::Disputed,
        Event::JobCompleted                                                 => Status::Completed,
        Event::ConfirmRefund                                                => Status::Refunded,
        Event::DisputeResolved                                              => Status::Completed,
        // 任务级 housekeeping 事件没有清晰的状态映射，保守用 Other
        Event::JobExpired | Event::JobClosed
        | Event::JobVisibilityChanged | Event::JobPaymentModeChanged       => Status::Other("housekeeping".to_string()),
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
