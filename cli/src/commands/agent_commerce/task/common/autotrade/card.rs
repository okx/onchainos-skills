//! Notification-only contract for signals that cannot execute.

use serde::Serialize;

const NOTIFY_TEMPLATE: &str =
    "[Auto Copy-Trade] The provider's signal for job <jobId> was not executed (<reason>). \
The deliverable is saved for manual review and may still be executed manually with any available tool.";

/// Emitted on ANY degrade path.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NotifyOnly {
    /// `true` when a signal was present but degraded; `false` for ordinary delivery.
    pub auto_trade: bool,
    pub executed: bool,
    /// Allowed here (notify-only path).
    pub saved_path: String,
    /// Machine-readable degrade reason (stable; matches audit action).
    pub reason: String,
    /// Fill-in template for the agent-side fallback. Empty (and omitted from the
    /// wire) once the CLI delivered the notice itself (`notification_pushed`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub notification_template: String,
    /// `true` when the CLI already took care of the user notification for this
    /// degrade — delivered it (`notify::push_degrade_notice`), or deliberately
    /// suppressed it (latch-dedup `replay_skip`). The consuming agent must NOT
    /// notify again. Omitted from the wire when false (fallback: agent notifies).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub notification_pushed: bool,
    /// In-band instruction for the consuming agent (set by `push_degrade_notice`)
    /// — overrides any stale playbook text a resumed session may still hold.
    /// Omitted when empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub guidance: String,
}

/// Build a [`NotifyOnly`] from a degrade reason + saved path.
pub fn make_notify_only(saved_path: &str, reason: &str) -> NotifyOnly {
    NotifyOnly {
        auto_trade: true,
        executed: false,
        saved_path: saved_path.to_string(),
        reason: reason.to_string(),
        notification_template: NOTIFY_TEMPLATE.to_string(),
        notification_pushed: false,
        guidance: String::new(),
    }
}
