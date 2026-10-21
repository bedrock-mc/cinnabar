//! The core's optional Cinnabar API connection: identity, visibility and player lookups.
#[cfg(test)]
use crate::valid_cinnabar_uuid;
use crate::valid_cinnabar_xuid;
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
    uuids: &'a [String],
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

/// Returns which `identities` (at most [`crate::contract::CINNABAR_API_MAX_LOOKUP_XUIDS`] XUIDs and
/// UUIDs combined) are visible Cinnabar players.
pub async fn lookup_cinnabar_players(
    socket_dir: &Path,
    identities: &[String],
) -> Result<CinnabarLookup, BridgeError> {
    let (xuids, uuids) = split_identities(identities);
    let body: CinnabarLookup = call(
        socket_dir,
        crate::contract::METHOD_CINNABAR_LOOKUP,
        Some(LookupParams {
            xuids: &xuids,
            uuids: &uuids,
        }),
    )
    .await?;
    Ok(body)
}

/// Splits lookup keys into the XUID and UUID arrays the API answers separately.
fn split_identities(identities: &[String]) -> (Vec<String>, Vec<String>) {
    identities
        .iter()
        .cloned()
        .partition(|id| valid_cinnabar_xuid(id))
}

/// Exchanges nearby watches and local playback with the core's nonblocking API connection.
pub async fn sync_cinnabar_players(
    socket_dir: &Path,
    state: &crate::contract::CinnabarPlayerState,
) -> Result<crate::contract::CinnabarPlayerUpdates, BridgeError> {
    call(
        socket_dir,
        crate::contract::METHOD_CINNABAR_PLAYERS,
        Some(state),
    )
    .await
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

    /// Mixed rosters send XUID keys and UUID keys in their own arrays.
    #[test]
    fn mixed_identities_split_by_kind() {
        let uuid = "00112233-4455-6677-8899-aabbccddeeff";
        let (xuids, uuids) = split_identities(&["11".into(), uuid.into(), "22".into()]);
        assert_eq!(
            (xuids, uuids),
            (vec!["11".into(), "22".into()], vec![uuid.into()])
        );
    }

    /// Local validation follows the pinned API schema's identifier patterns.
    #[test]
    fn identifier_validation_matches_the_pinned_schema() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../core/cinnabarapi/schema/v1.json")).unwrap();
        assert_eq!(schema["$defs"]["Xuid"]["pattern"], "^[0-9]{1,20}$");
        assert_eq!(
            schema["$defs"]["Uuid"]["pattern"],
            "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"
        );
        assert!(valid_cinnabar_xuid("12345678901234567890"));
        assert!(!valid_cinnabar_xuid("123456789012345678901") && !valid_cinnabar_xuid(""));
        assert!(valid_cinnabar_uuid("00112233-4455-6677-8899-aabbccddeeff"));
        assert!(!valid_cinnabar_uuid("00112233-4455-6677-8899-AABBCCDDEEFF"));
        assert!(!valid_cinnabar_uuid("00112233445566778899aabbccddeeff0000"));
    }
}
