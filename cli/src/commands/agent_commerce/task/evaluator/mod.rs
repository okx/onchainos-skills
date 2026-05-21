//! Evaluator-side task commands — handler module collection.
//!
//! Command name mapping (old → new, CLI invocation form):
//! - `agent evaluator claim`           → `agent arbitration-claim`
//! - `agent evaluator claimable`       → `agent arbitration-claimable`
//!
//! - `helpers.rs`         — disputeId parsing
//! - `flow.rs`            — state-machine prompt generator (used by `next-action --role evaluator`)

pub mod claim;
pub mod claimable;
pub mod commit;
pub mod decimal_str;
pub mod dispute_status;
pub mod flow;
pub mod helpers;
pub mod info;
pub mod my_stake;
pub mod record;
pub mod reveal;
pub mod stake;
pub mod staking_config;
pub mod staking_types;
pub mod unstake;
