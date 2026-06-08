//! Prompt-generation functions for the negotiation / matching phase.

pub(super) mod designated;
mod match_provider;
mod events;

pub(super) use match_provider::{job_created, switch_provider, provider_conversation};
pub(super) use events::{job_visibility_changed, job_payment_mode_changed, negotiate_reply, negotiate_ack, negotiate_counter};
