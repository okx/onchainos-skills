//! Pure data models shared across the identity module. Contains serde
//! structs for agent card / service payloads and a few constants. The
//! pre-transaction unsigned-tx payload is the wallet-shared
//! `crate::wallet_api::UnsignedInfoResponse` — identity does not maintain
//! its own copy.

use serde::{Deserialize, Serialize};

pub(super) const XLAYER_CHAIN_INDEX: &str = "196";
pub(super) const XLAYER_CHAIN_INDEX_NUM: u64 = 196;
pub(super) const XLAYER_CHAIN_NAME: &str = "XLayer";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct AgentService {
    #[serde(rename = "id", default, skip_serializing_if = "Option::is_none")]
    pub(super) id: Option<String>,
    #[serde(rename = "servicedescription")]
    pub(super) service_description: String,
    #[serde(rename = "name")]
    pub(super) service_name: String,
    #[serde(rename = "fee", default)]
    pub(super) fee: String,
    #[serde(rename = "servicetype")]
    pub(super) service_type: String,
    #[serde(rename = "endpoint", default, skip_serializing_if = "Option::is_none")]
    pub(super) endpoint: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct AgentCard {
    #[serde(rename = "Role")]
    pub(super) role: String,
    #[serde(rename = "name")]
    pub(super) name: String,
    #[serde(rename = "image")]
    pub(super) profile_picture: String,
    #[serde(rename = "ProfileDescription")]
    pub(super) profile_description: String,
    #[serde(
        rename = "CommunicationAddress",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) communication_address: Option<String>,
    #[serde(rename = "services")]
    pub(super) services: Vec<AgentService>,
}

