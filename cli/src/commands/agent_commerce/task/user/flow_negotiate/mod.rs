//! Prompt-generation functions for the negotiation / matching phase.

pub(super) mod designated;
mod match_provider;
mod events;

pub(super) use match_provider::{job_created, provider_conversation_auto_consume, provider_conversation_pick_cli, provider_conversation_reject_cli};
pub(super) use events::{job_visibility_changed, job_payment_mode_changed, negotiate_reply, provider_reject};
