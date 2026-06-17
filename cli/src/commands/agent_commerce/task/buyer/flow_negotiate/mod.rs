//! Prompt-generation functions for the negotiation / matching phase.

pub(super) mod designated;
mod match_provider;
mod events;

pub(super) use match_provider::{job_created, job_created_cli, provider_conversation, provider_conversation_cli, provider_conversation_pick_cli};
pub(super) use events::{job_visibility_changed, job_payment_mode_changed, negotiate_reply, provider_reject};
