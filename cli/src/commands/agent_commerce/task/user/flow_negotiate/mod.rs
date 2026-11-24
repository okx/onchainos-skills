//! Prompt-generation functions for the negotiation / matching phase.

pub(super) mod designated;
mod events;
mod match_provider;

pub(super) use events::{job_payment_mode_changed, negotiate_reply, provider_reject};
pub(super) use match_provider::job_created;
