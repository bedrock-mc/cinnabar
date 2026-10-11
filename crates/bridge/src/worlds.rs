//! The core's side of local worlds: where it routes the launcher's open world, and friend invites.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::BridgeError;

/// Game mode a world starts players in.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GameMode {
    #[default]
    Survival,
    Creative,
    Adventure,
}

/// Server backend a world runs on; fixed when the world is created.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// Bedrock Dedicated Server: vanilla terrain and mobs.
    Bds,
    /// Built-in Dragonfly server with flat or natural terrain.
    #[default]
    Dragonfly,
}

/// Lifecycle of the open local world.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldState {
    #[default]
    Idle,
    Starting,
    Running,
    Stopping,
    /// Kept until the world is closed, so a failed open never falls through to another target.
    Failed,
}

/// How the core dials the open world's server.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum Transport {
    #[serde(rename = "raknet")]
    RakNet,
    /// BDS's loopback HTTP signaling.
    #[serde(rename = "nethernet")]
    NetherNet,
    /// BDS found through its loopback LAN discovery advertisement.
    #[serde(rename = "nethernet-lan")]
    NetherNetLan,
}

/// The open world's loopback server.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LocalTarget {
    pub address: String,
    pub transport: Transport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lan_address: Option<String>,
    /// The BDS level its LAN advertisement must name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level_name: Option<String>,
}

/// The open world as the core advertises it to Xbox friends.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LocalWorldInfo {
    pub id: String,
    pub name: String,
    pub game_mode: GameMode,
    pub backend: Backend,
}

/// The launcher's open local world as `local_world.v1` reports it.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct LocalWorld {
    pub state: WorldState,
    /// Set unless idle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub world: Option<LocalWorldInfo>,
    /// Set while running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<LocalTarget>,
    /// Path-free reason while failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct Acknowledged {}

#[derive(Serialize)]
struct InviteParams<'a> {
    xuid: &'a str,
}

/// Tells the core about the open local world; while one is open it outranks `-upstream` for the
/// next session, and a world that begins opening clears any pending selection or transfer.
pub async fn set_local_world(socket_dir: &Path, world: &LocalWorld) -> Result<(), BridgeError> {
    crate::account::call::<Acknowledged, _>(socket_dir, "local_world.v1", Some(world)).await?;
    Ok(())
}

/// Sends the Xbox friend `xuid` an invite to the open world; the core refuses it while no
/// world is hosted for friends.
pub async fn invite_to_world(socket_dir: &Path, xuid: &str) -> Result<(), BridgeError> {
    crate::account::call::<Acknowledged, _>(
        socket_dir,
        "world_invite.v1",
        Some(InviteParams { xuid }),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_reports_only_the_state() {
        let idle = serde_json::to_value(LocalWorld::default()).expect("encode");
        assert_eq!(idle, serde_json::json!({"state": "idle"}));
    }

    #[test]
    fn running_reports_name_the_world_and_its_transport() {
        let running = LocalWorld {
            state: WorldState::Running,
            world: Some(LocalWorldInfo {
                id: "0123456789abcdef".into(),
                name: "Home".into(),
                game_mode: GameMode::Creative,
                backend: Backend::Bds,
            }),
            target: Some(LocalTarget {
                address: "127.0.0.1:5000".into(),
                transport: Transport::NetherNetLan,
                lan_address: Some("127.0.0.1:7551".into()),
                level_name: Some("0123456789abcdef".into()),
            }),
            error: None,
        };
        assert_eq!(
            serde_json::to_value(&running).expect("encode"),
            serde_json::json!({
                "state": "running",
                "world": {"id": "0123456789abcdef", "name": "Home", "game_mode": "creative", "backend": "bds"},
                "target": {"address": "127.0.0.1:5000", "transport": "nethernet-lan",
                           "lan_address": "127.0.0.1:7551", "level_name": "0123456789abcdef"},
            })
        );
        let raknet = serde_json::to_value(Transport::RakNet).expect("encode");
        assert_eq!(raknet, serde_json::json!("raknet"));
        let http = serde_json::to_value(Transport::NetherNet).expect("encode");
        assert_eq!(http, serde_json::json!("nethernet"));
    }

    #[test]
    fn invite_params_name_the_friend() {
        let invite = serde_json::to_value(InviteParams {
            xuid: "2535400000000001",
        })
        .expect("encode");
        assert_eq!(invite, serde_json::json!({"xuid": "2535400000000001"}));
    }

    #[test]
    fn acknowledgements_surface_rpc_errors() {
        let error =
            br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"invalid local world report"}}"#;
        assert!(matches!(
            crate::account::parse_response::<Acknowledged>(error),
            Err(BridgeError::ControlRpc { code: -32602, .. })
        ));
        let ok = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1}}"#;
        assert!(crate::account::parse_response::<Acknowledged>(ok).is_ok());
    }
}
