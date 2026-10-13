//! The core's optional Cinnabar API connection: identity, visibility and player lookups.
use crate::{BridgeError, account::call};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// JSON-RPC code the core returns when the API refused a lookup over its rate limit.
pub const RATE_LIMITED: i64 = -32024;

/// The hello identity and live visibility the core sends to the API.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CinnabarApiConfig {
    pub client_version: String,
    pub channel: String,
    pub visible: bool,
}

/// A release the API announced on the client's channel.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CinnabarRelease {
    pub channel: String,
    pub version: String,
}

#[derive(Deserialize)]
struct Accepted {}

#[derive(Serialize)]
struct LookupParams<'a> {
    xuids: &'a [String],
}

#[derive(Deserialize)]
struct LookupBody {
    cinnabar: Vec<String>,
}

/// Applies the identity and visibility; the core sends visibility live and reconnects on identity changes.
pub async fn configure_cinnabar_api(
    socket_dir: &Path,
    config: &CinnabarApiConfig,
) -> Result<(), BridgeError> {
    call::<Accepted, _>(socket_dir, "cinnabar_api.v1", Some(config)).await?;
    Ok(())
}

/// Returns the subset of `xuids` (at most [`crate::contract::CINNABAR_API_MAX_LOOKUP_XUIDS`]) that are visible Cinnabar players.
pub async fn lookup_cinnabar_players(
    socket_dir: &Path,
    xuids: &[String],
) -> Result<Vec<String>, BridgeError> {
    let body: LookupBody = call(
        socket_dir,
        "cinnabar_lookup.v1",
        Some(LookupParams { xuids }),
    )
    .await?;
    Ok(body.cinnabar)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_uses_the_core_field_names() {
        let config = CinnabarApiConfig {
            client_version: "0.1.18".into(),
            channel: "beta".into(),
            visible: true,
        };
        assert_eq!(
            serde_json::to_value(config).unwrap(),
            serde_json::json!({"client_version": "0.1.18", "channel": "beta", "visible": true})
        );
    }
}
