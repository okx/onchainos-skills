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
    #[serde(rename = "fee", default)]
    pub(super) fee: String,
    #[serde(rename = "serviceType")]
    pub(super) service_type: String,
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

