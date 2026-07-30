//! Pure data models shared across the identity module. Contains serde
//! structs for agent card / service payloads and a few constants. The
//! pre-transaction unsigned-tx payload is the wallet-shared
//! `crate::wallet_api::UnsignedInfoResponse` — identity does not maintain
//! its own copy.

use serde::{Deserialize, Serialize};

pub(super) const XLAYER_CHAIN_INDEX: &str = "196";
pub(super) const XLAYER_CHAIN_INDEX_NUM: u64 = 196;
pub(super) const XLAYER_CHAIN_NAME: &str = "XLayer";

/// Per-service write directive carried in cardJson as `services[].operation`.
/// Tags whether the entry should be created / updated / deleted on the next
/// register/update broadcast. Optional — omitted when the caller does not set
/// it (e.g. existing services fetched for an update back-fill). Wire form is
/// lowercase (`create` / `update` / `delete`). Usage semantics are driven by
/// the skill layer.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(super) enum ServiceOperation {
    Create,
    Update,
    Delete,
}

/// One subscription pricing tier carried in a service's `subscription[]`.
/// Only A2A services may have them; A2MCP never does. `interval` is currently
/// restricted to `"month"` (the only billing period the product supports today)
/// and `fee` is a plain number string (USDT implied, ≤6 decimals) — same fee
/// contract as the single-purchase `fee`. Field names mirror the Agent Card
/// JSON 1:1 so a fetched service deserializes directly.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct SubscriptionTier {
    #[serde(rename = "interval")]
    pub(super) interval: String,
    #[serde(rename = "fee")]
    pub(super) fee: String,
}

/// A single agent service. Field names mirror the `agentic/agent/services`
/// response 1:1 so a fetched service deserializes directly (no manual mapping),
/// and the `--service` CLI input uses the SAME camelCase keys (`serviceName` /
/// `serviceDescription` / `serviceType`) — one schema everywhere, no aliases.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct AgentService {
    #[serde(rename = "id", default, skip_serializing_if = "Option::is_none")]
    pub(super) id: Option<String>,
    #[serde(rename = "serviceName")]
    pub(super) service_name: String,
    #[serde(rename = "serviceDescription")]
    pub(super) service_description: String,
    /// Single-purchase price. Always serialized. A plain number string when a
    /// one-off price applies; an EMPTY string (`""`) when the service is
    /// subscription-priced (no single-purchase price). A2MCP always carries a
    /// real number here. For A2A this and `subscription` are mutually
    /// exclusive — exactly one carries a real price.
    #[serde(rename = "fee", default)]
    pub(super) fee: String,
    #[serde(rename = "serviceType")]
    pub(super) service_type: String,
    /// Subscription pricing tiers (A2A only). Always serialized: an empty `[]`
    /// means "no subscription" (and, on an update, clears any existing
    /// subscription). For A2A a service must carry EXACTLY ONE billing model —
    /// a single-purchase `fee` XOR a non-empty `subscription`, never both.
    /// Defaults to empty so a fetched service that predates this field
    /// deserializes cleanly.
    #[serde(rename = "subscription", default)]
    pub(super) subscription: Vec<SubscriptionTier>,
    /// `freeTrial` — duration in HOURS (A2A subscription services only). Optional —
    /// only meaningful alongside a NON-EMPTY `subscription`. A positive integer
    /// number of hours as a string (e.g. `"72"` = 3 days) sets a trial. An ABSENT
    /// key and an EMPTY string `""` are EQUIVALENT — both mean "no trial"; the
    /// field is then omitted from the payload (a missing `freeTrial` is read as
    /// no trial). Setting or clearing the trial does not affect `subscription`.
    /// Forbidden (must be absent/empty) on single-purchase A2A and on A2MCP.
    #[serde(rename = "freeTrial", default, skip_serializing_if = "Option::is_none")]
    pub(super) free_trial: Option<String>,
    #[serde(rename = "operation", default, skip_serializing_if = "Option::is_none")]
    pub(super) operation: Option<ServiceOperation>,
    #[serde(rename = "endpoint", default, skip_serializing_if = "Option::is_none")]
    pub(super) endpoint: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct AgentCard {
    #[serde(rename = "role")]
    pub(super) role: String,
    #[serde(rename = "name")]
    pub(super) name: String,
    #[serde(rename = "image")]
    pub(super) profile_picture: String,
    #[serde(rename = "profileDescription")]
    pub(super) profile_description: String,
    // CommunicationAddress is intentionally left as-is (not renamed).
    #[serde(
        rename = "CommunicationAddress",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) communication_address: Option<String>,
    #[serde(rename = "services")]
    pub(super) services: Vec<AgentService>,
}

