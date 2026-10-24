//! Shared native product API identity and lookup data.

use serde::{Deserialize, Serialize};

/// The hello identity and live visibility the core sends to the API.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CinnabarApiConfig {
    pub client_version: String,
    pub client_build: u64,
    pub channel: String,
    pub visible: bool,
}

/// One lookup answer and the lifetime advertised by the connected API server.
#[derive(Deserialize)]
pub struct CinnabarLookup {
    pub cinnabar: Vec<String>,
    #[serde(default)]
    pub cinnabar_uuids: Vec<String>,
    pub resolved: Vec<String>,
    #[serde(default)]
    pub resolved_uuids: Vec<String>,
    pub cache_ttl_ms: u64,
}
