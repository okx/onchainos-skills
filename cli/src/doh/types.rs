use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A proxy node returned by okx-pilot binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DohNode {
    pub ip: String,
    pub host: String,
    pub ttl: u64,
}

/// A node that failed to connect, with expiry tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedNode {
    pub ip: String,
    pub failed_at: u64, // unix timestamp in milliseconds
}

/// Routing mode: proxy through DoH node, or direct connection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DohMode {
    Proxy,
    Direct,
}

/// Cache entry for a single domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DohCacheEntry {
    pub mode: DohMode,
    pub node: Option<DohNode>,
    pub failed_nodes: Vec<FailedNode>,
    pub updated_at: u64,
}

/// Cache file format: domain → cache entry.
pub type DohCacheFile = HashMap<String, DohCacheEntry>;

/// Checksum manifest from CDN (`checksum.json`).
#[derive(Debug, Deserialize)]
pub struct DohChecksum {
    pub sha256: String,
}

/// Parsed stdout from okx-pilot binary.
#[derive(Debug, Deserialize)]
pub struct DohBinaryResponse {
    pub code: i32,
    pub data: DohBinaryData,
}

#[derive(Debug, Deserialize)]
pub struct DohBinaryData {
    pub ip: String,
    pub host: String,
    pub ttl: u64,
}
