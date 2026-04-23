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
    #[serde(
        rename = "id",
        default,
        alias = "id",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) id: Option<String>,
    #[serde(
        rename = "ServiceDescription",
        alias = "ServiceDescription",
        alias = "serviceDescription"
    )]
    pub(super) service_description: String,
    #[serde(rename = "ServiceName", alias = "ServiceName", alias = "serviceName")]
    pub(super) service_name: String,
    #[serde(rename = "Fee", default, alias = "Fee", alias = "fee")]
    pub(super) fee: String,
    #[serde(rename = "ServiceType", alias = "ServiceType", alias = "serviceType")]
    pub(super) service_type: String,
    #[serde(
        rename = "Endpoint",
        default,
        alias = "Endpoint",
        alias = "endpoint",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) endpoint: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct AgentCard {
    #[serde(rename = "Role")]
    pub(super) role: String,
    #[serde(rename = "Name")]
    pub(super) name: String,
    #[serde(rename = "ProfilePicture")]
    pub(super) profile_picture: String,
    #[serde(rename = "ProfileDescription")]
    pub(super) profile_description: String,
    #[serde(
        rename = "CommunicationAddress",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) communication_address: Option<String>,
    #[serde(rename = "Service")]
    pub(super) services: Vec<AgentService>,
}

