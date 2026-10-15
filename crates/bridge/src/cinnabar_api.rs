//! The core's optional Cinnabar API connection: identity, visibility and player lookups.
use crate::{BridgeError, account::call};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The hello identity and live visibility the core sends to the API.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CinnabarApiConfig {
    pub client_version: String,
    pub client_build: u64,
    pub channel: String,
    pub visible: bool,
}

#[derive(Deserialize)]
struct Accepted {}

#[derive(Serialize)]
struct LookupParams<'a> {
    xuids: &'a [String],
}

/// One lookup answer and the lifetime advertised by the connected API server.
#[derive(Deserialize)]
pub struct CinnabarLookup {
    pub cinnabar: Vec<String>,
    pub resolved: Vec<String>,
    pub cache_ttl_ms: u64,
}

/// Applies the identity and visibility; the core sends visibility live and reconnects on identity changes.
pub async fn configure_cinnabar_api(
    socket_dir: &Path,
    config: &CinnabarApiConfig,
) -> Result<(), BridgeError> {
    call::<Accepted, _>(
        socket_dir,
        crate::contract::METHOD_CINNABAR_API,
        Some(config),
    )
    .await?;
    Ok(())
}

/// Returns the subset of `xuids` (at most [`crate::contract::CINNABAR_API_MAX_LOOKUP_XUIDS`]) that are visible Cinnabar players.
pub async fn lookup_cinnabar_players(
    socket_dir: &Path,
    xuids: &[String],
) -> Result<CinnabarLookup, BridgeError> {
    let body: CinnabarLookup = call(
        socket_dir,
        crate::contract::METHOD_CINNABAR_LOOKUP,
        Some(LookupParams { xuids }),
    )
    .await?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_uses_the_core_field_names() {
        let config = CinnabarApiConfig {
            client_version: "0.1.18".into(),
            client_build: 42,
            channel: "beta".into(),
            visible: true,
        };
        assert_eq!(
            serde_json::to_value(config).unwrap(),
            serde_json::json!({"client_version": "0.1.18", "client_build": 42, "channel": "beta", "visible": true})
        );
    }
}
