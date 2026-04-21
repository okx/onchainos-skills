//! Pure data models shared across the identity module. Contains serde
//! structs for agent card / service / unsigned-tx payloads plus a few
//! constants and the `null_string` deserializer used by the unsigned-tx
//! response.

use serde::{Deserialize, Serialize};
use serde_json::Value;

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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ExistingAgentCard {
    pub(super) role: Option<String>,
    pub(super) name: Option<String>,
    pub(super) profile_picture: Option<String>,
    pub(super) profile_description: Option<String>,
    pub(super) communication_address: Option<String>,
    pub(super) services: Option<Vec<AgentService>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AgentUnsignedTx {
    #[serde(default, deserialize_with = "null_string")]
    pub(super) hash: String,
    #[serde(default, deserialize_with = "null_string")]
    pub(super) auth_hash_for7702: String,
    #[serde(default, deserialize_with = "null_string")]
    pub(super) uop_hash: String,
    #[serde(default, deserialize_with = "null_string")]
    pub(super) sign_type: String,
    #[serde(default, deserialize_with = "null_string")]
    pub(super) encoding: String,
    #[serde(default, deserialize_with = "null_string")]
    pub(super) unsigned_tx_hash: String,
    #[serde(default, deserialize_with = "null_string")]
    pub(super) unsigned_tx: String,
    #[serde(default)]
    pub(super) extra_data: Value,
}

/// Serde deserializer that turns `null` / number / bool into an empty or
/// stringified `String`, used by `AgentUnsignedTx` fields because some
/// backend responses send `null` where we expect a hex string.
fn null_string<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Null => Ok(String::new()),
        Value::String(text) => Ok(text),
        Value::Number(number) => Ok(number.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "expected string or null, got {other}"
        ))),
    }
}
