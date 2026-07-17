use std::collections::{HashMap, HashSet};

use protocol::{
    ActorAttribute, ActorEvent, ActorKind, ActorMetadataValue, ActorMoveEvent, ActorPositionOrigin,
    ActorProperty, ActorSpawnEvent, EquipmentEvent, ItemActorEvent, MAX_ACTOR_ATTRIBUTES,
    MAX_ACTOR_METADATA_ENTRIES, MAX_ACTOR_PROPERTIES, MAX_PLAYER_LIST_SKIN_BYTES, MovePlayerEvent,
    MovePlayerMode, PLAYER_NETWORK_OFFSET, PlayerListEntry, PlayerSkin, PlayerSkinUnavailable,
};

use crate::{
    action::{ActorSourceTick, MAX_ACTION_EVENTS_PER_TICK, RemoteActionStore},
    actor_animation::{ActorAnimationStore, ActorLifetimeId},
    item::ItemStateStore,
};

pub(crate) const MAX_TRACKED_ACTORS: usize = 8_192;
pub(crate) const MAX_TRACKED_PLAYERS: usize = 4_096;
pub(crate) const MAX_TRACKED_PLAYER_SKIN_BYTES: usize = MAX_PLAYER_LIST_SKIN_BYTES;

// Protocol 1001 metadata keys retained verbatim by ActorSnapshot.
const PLAYER_FLAGS_METADATA_KEY: i32 = 26;
const BOUNDING_BOX_HEIGHT_METADATA_KEY: i32 = 54;
const EXTENDED_FLAGS_METADATA_KEY: i32 = 92;
const PLAYER_FLAGS_SLEEPING: u8 = 1 << 1;
const EXTENDED_FLAGS_SLEEPING: u64 = 1 << 11;

const SLEEPING_PLAYER_NETWORK_OFFSET: f32 = 0.2;
const ITEM_ACTOR_NETWORK_OFFSET: f32 = 0.5;
const FALLING_BLOCK_NETWORK_OFFSET: f32 = 0.5;
const MINECART_NETWORK_OFFSET: f32 = 0.5;
const BOAT_NETWORK_OFFSET: f32 = 0.375;
const DEFAULT_PRIMED_TNT_NETWORK_OFFSET: f32 = 0.49;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActorApplyResult {
    Inserted,
    Replaced,
    Updated,
    Removed,
    Reset,
    MissingActor,
    CapacityRejected,
    StaleSession,
    StaleSequence,
    StaleDimension,
}

pub(crate) const PLAYER_POSITION_INTERPOLATION_TICKS: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorPose {
    pub position: [f32; 3],
    pub pitch: f32,
    pub yaw: f32,
    pub head_yaw: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActorSnapshot {
    pub unique_id: i64,
    pub runtime_id: u64,
    pub spawn_revision: u64,
    pub movement_revision: u64,
    pub kind: ActorKind,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub pitch: f32,
    pub yaw: f32,
    pub head_yaw: f32,
    pub previous_pose: ActorPose,
    pub received_pose: ActorPose,
    pub interpolation_ticks_remaining: u8,
    pub body_yaw: f32,
    pub on_ground: Option<bool>,
    pub teleported: bool,
    pub player_mode: Option<MovePlayerMode>,
    pub source_tick: Option<i64>,
    pub metadata: HashMap<i32, ActorMetadataValue>,
    pub attributes: HashMap<std::sync::Arc<str>, ActorAttribute>,
    pub int_properties: HashMap<i32, i32>,
    pub float_properties: HashMap<i32, f32>,
}

impl ActorSnapshot {
    fn from_spawn(spawn: ActorSpawnEvent, spawn_revision: u64) -> Self {
        let pose = ActorPose {
            position: spawn.position,
            pitch: spawn.pitch,
            yaw: spawn.yaw,
            head_yaw: spawn.head_yaw,
        };
        let mut snapshot = Self {
            unique_id: spawn.unique_id,
            runtime_id: spawn.runtime_id,
            spawn_revision,
            movement_revision: 0,
            kind: spawn.kind,
            position: spawn.position,
            velocity: spawn.velocity,
            pitch: spawn.pitch,
            yaw: spawn.yaw,
            head_yaw: spawn.head_yaw,
            previous_pose: pose,
            received_pose: pose,
            interpolation_ticks_remaining: 0,
            body_yaw: spawn.body_yaw,
            on_ground: None,
            teleported: false,
            player_mode: None,
            source_tick: None,
            metadata: HashMap::with_capacity(spawn.metadata.len()),
            attributes: HashMap::with_capacity(spawn.attributes.len()),
            int_properties: HashMap::new(),
            float_properties: HashMap::new(),
        };
        snapshot.apply_metadata(&spawn.metadata);
        snapshot.apply_attributes(&spawn.attributes);
        snapshot.apply_properties(&spawn.properties);
        snapshot
    }

    fn current_pose(&self) -> ActorPose {
        ActorPose {
            position: self.position,
            pitch: self.pitch,
            yaw: self.yaw,
            head_yaw: self.head_yaw,
        }
    }

    fn set_current_pose(&mut self, pose: ActorPose) {
        self.position = pose.position;
        self.pitch = pose.pitch;
        self.yaw = pose.yaw;
        self.head_yaw = pose.head_yaw;
    }

    fn network_position_offset(&self) -> f32 {
        match &self.kind {
            ActorKind::Player { .. } => {
                if self.player_is_sleeping() {
                    SLEEPING_PLAYER_NETWORK_OFFSET
                } else {
                    PLAYER_NETWORK_OFFSET
                }
            }
            ActorKind::Entity { identifier } => {
                let Some(path) = identifier.strip_prefix("minecraft:") else {
                    return 0.0;
                };
                match path {
                    "item" => ITEM_ACTOR_NETWORK_OFFSET,
                    "falling_block" => FALLING_BLOCK_NETWORK_OFFSET,
                    "tnt" => self.primed_tnt_network_offset(),
                    "minecart"
                    | "hopper_minecart"
                    | "tnt_minecart"
                    | "chest_minecart"
                    | "command_block_minecart" => MINECART_NETWORK_OFFSET,
                    "boat" => BOAT_NETWORK_OFFSET,
                    _ => 0.0,
                }
            }
        }
    }

    fn player_is_sleeping(&self) -> bool {
        let player_flags = self.metadata.get(&PLAYER_FLAGS_METADATA_KEY).is_some_and(
            |value| matches!(value, ActorMetadataValue::Byte(flags) if (*flags as u8) & PLAYER_FLAGS_SLEEPING != 0),
        );
        let extended_flags = self.metadata.get(&EXTENDED_FLAGS_METADATA_KEY).is_some_and(
            |value| matches!(value, ActorMetadataValue::FlagsExtended(flags) if flags & EXTENDED_FLAGS_SLEEPING != 0),
        );
        player_flags || extended_flags
    }

    fn primed_tnt_network_offset(&self) -> f32 {
        self.metadata
            .get(&BOUNDING_BOX_HEIGHT_METADATA_KEY)
            .and_then(|value| match value {
                ActorMetadataValue::Float(height) if height.is_finite() && *height > 0.0 => {
                    Some(*height * 0.5)
                }
                _ => None,
            })
            .unwrap_or(DEFAULT_PRIMED_TNT_NETWORK_OFFSET)
    }

    fn apply_metadata(&mut self, metadata: &[protocol::ActorMetadata]) -> bool {
        let mut rejected = false;
        for metadata in metadata {
            if self.metadata.len() >= MAX_ACTOR_METADATA_ENTRIES
                && !self.metadata.contains_key(&metadata.key)
            {
                rejected = true;
                continue;
            }
            self.metadata.insert(metadata.key, metadata.value.clone());
        }
        rejected
    }

    fn apply_attributes(&mut self, attributes: &[ActorAttribute]) -> bool {
        let mut rejected = false;
        for attribute in attributes {
            if self.attributes.len() >= MAX_ACTOR_ATTRIBUTES
                && !self.attributes.contains_key(&attribute.name)
            {
                rejected = true;
                continue;
            }
            self.attributes
                .insert(attribute.name.clone(), attribute.clone());
        }
        rejected
    }

    fn apply_properties(&mut self, properties: &[ActorProperty]) -> bool {
        let mut rejected = false;
        for property in properties {
            match *property {
                ActorProperty::Int { index, value } => {
                    if !self.int_properties.contains_key(&index)
                        && !self.float_properties.contains_key(&index)
                        && self.int_properties.len() + self.float_properties.len()
                            >= MAX_ACTOR_PROPERTIES
                    {
                        rejected = true;
                        continue;
                    }
                    self.float_properties.remove(&index);
                    self.int_properties.insert(index, value);
                }
                ActorProperty::Float { index, value } => {
                    if !self.float_properties.contains_key(&index)
                        && !self.int_properties.contains_key(&index)
                        && self.int_properties.len() + self.float_properties.len()
                            >= MAX_ACTOR_PROPERTIES
                    {
                        rejected = true;
                        continue;
                    }
                    self.int_properties.remove(&index);
                    self.float_properties.insert(index, value);
                }
            }
        }
        rejected
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerProfile {
    pub unique_id: i64,
    pub username: std::sync::Arc<str>,
    pub verified: bool,
    pub skin: PlayerSkin,
}

/// Sparse, session-scoped actor state. It owns no render or chunk-mesh state.
#[derive(Debug)]
pub(crate) struct ActorStore {
    session_id: u64,
    dimension: i32,
    latest_sequence: u64,
    max_actors: usize,
    max_players: usize,
    max_player_skin_bytes: usize,
    retained_player_skin_bytes: usize,
    actors: HashMap<u64, ActorSnapshot>,
    unique_to_runtime: HashMap<i64, u64>,
    players: HashMap<[u8; 16], PlayerProfile>,
    animation: ActorAnimationStore,
    items: ItemStateStore,
    actions: RemoteActionStore,
    remote_state_excluded_runtime_id: Option<u64>,
}

mod lifecycle;
mod query;

fn retained_skin_bytes(skin: &PlayerSkin) -> usize {
    match skin {
        PlayerSkin::Standard(skin) => skin.rgba8.len(),
        PlayerSkin::Unavailable(_) => 0,
    }
}

fn event_dimension(event: &ActorEvent) -> Option<i32> {
    match event {
        ActorEvent::Spawn(event) => Some(event.dimension),
        ActorEvent::Remove(event) => Some(event.dimension),
        ActorEvent::Move(event) => Some(event.dimension),
        ActorEvent::Metadata(event) => Some(event.dimension),
        ActorEvent::Attributes(event) => Some(event.dimension),
        ActorEvent::PlayerList(_) => None,
    }
}

#[cfg(test)]
mod tests;
