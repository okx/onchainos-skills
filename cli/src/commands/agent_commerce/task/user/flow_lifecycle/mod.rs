//! Prompt generators for task execution + arbitration + terminal states.

mod core;
mod dispute;
mod manage;
pub(super) mod subscription;
mod terminal;

pub(super) use self::core::{
    approve_review, deliverable_received_cli, job_accepted, job_completed, job_submitted,
    provider_applied, reject_review,
};
pub(crate) use self::core::{resume_queued_subscription_delivery, try_recover_from_temp_file};
pub(super) use dispute::{dispute_resolved, job_disputed, job_rejected};
pub(super) use manage::{attachment_added_cli, create_task, upload_and_forward_all_attachments};
pub(super) use terminal::{
    close_task, job_auto_refunded, job_closed, job_expired, job_refunded, reject_expired,
    review_deadline_warn, reward_claimed, staked_and_unknown, submit_expired, wakeup_notify,
};
