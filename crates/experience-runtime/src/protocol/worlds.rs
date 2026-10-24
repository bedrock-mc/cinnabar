//! World and private-state contracts. WIT owns guest types; adapter fixtures check Go encoding.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldHandle {
    pub id: String,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionType {
    Overworld,
    Nether,
    End,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Generator {
    Empty,
    Flat,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldSettings {
    pub generator: Generator,
    pub dimension: DimensionType,
    pub persistent: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldDefinition {
    pub id: String,
    pub settings: WorldSettings,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldInstance {
    pub id: String,
    pub definition: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldRegistration {
    pub definitions: Vec<WorldDefinition>,
    pub startup: Vec<WorldInstance>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerLocation {
    pub world: WorldHandle,
    pub epoch: u64,
    pub position: Position,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlayerEvent {
    Join {
        location: PlayerLocation,
    },
    Leave {
        location: PlayerLocation,
    },
    WorldChange {
        before: PlayerLocation,
        after: PlayerLocation,
    },
    Move {
        before: PlayerLocation,
        after: PlayerLocation,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreScope {
    Player,
    Instance,
    Mod,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreKey {
    pub scope: StoreScope,
    pub schema: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateRecord {
    pub key: StoreKey,
    pub revision: u64,
    pub data: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldContext {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<super::EntitySnapshot>,
    pub current: WorldHandle,
    pub player: Option<PlayerLocation>,
    pub instances: Vec<WorldHandle>,
    pub stores: Vec<StateRecord>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorldOp {
    Create {
        instance: WorldInstance,
    },
    Unload {
        instance: WorldHandle,
    },
    Transfer {
        player: String,
        instance: WorldHandle,
        position: Position,
    },
}
