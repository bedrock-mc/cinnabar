use std::collections::{HashMap, HashSet};

use protocol::{
    ActorAttribute, ActorEvent, ActorKind, ActorLinkEvent, ActorLinkType, ActorMetadataValue,
    ActorMoveEvent, ActorPositionOrigin, ActorProperty, ActorSpawnEvent, EquipmentEvent,
    ITEM_ACTOR_NETWORK_OFFSET, ItemActorEvent, MAX_ACTOR_ATTRIBUTES, MAX_ACTOR_METADATA_ENTRIES,
    MAX_ACTOR_PROPERTIES, MAX_PLAYER_LIST_SKIN_BYTES, MovePlayerEvent, MovePlayerMode,
    PLAYER_NETWORK_OFFSET, PlayerListEntry, PlayerSkin, PlayerSkinUnavailable,
};

use crate::{
    action::{ActorSourceTick, MAX_ACTION_EVENTS_PER_TICK, RemoteActionStore},
    actor_animation::{ActorAnimationStore, ActorLifetimeId},
    item::ItemStateStore,
};

pub(crate) const MAX_TRACKED_ACTORS: usize = 8_192;
pub(crate) const MAX_TRACKED_PLAYERS: usize = 4_096;
pub(crate) const MAX_TRACKED_ACTOR_LINKS: usize = MAX_TRACKED_ACTORS;
pub(crate) const MAX_TRACKED_PLAYER_SKIN_BYTES: usize = MAX_PLAYER_LIST_SKIN_BYTES;

// Protocol 1001 metadata keys retained verbatim by ActorSnapshot.
const PLAYER_FLAGS_METADATA_KEY: u32 = 26;
const SCALE_METADATA_KEY: u32 = 38;
const NAMETAG_METADATA_KEY: u32 = 4;
const BOUNDING_BOX_WIDTH_METADATA_KEY: u32 = 53;
const BOUNDING_BOX_HEIGHT_METADATA_KEY: u32 = 54;
/// `minecraft:collision_box` in the vanilla `player.json` definition.
const PLAYER_COLLISION_WIDTH: f32 = 0.6;
const PLAYER_COLLISION_HEIGHT: f32 = 1.8;
pub(crate) const EXTENDED_FLAGS_METADATA_KEY: u32 = 92;
pub(crate) const FUSE_TIME_METADATA_KEY: u32 = 55;
const PLAYER_FLAGS_SLEEPING: u8 = 1 << 1;
/// Actor flag bits follow gophertunnel v1.61.0 `EntityDataFlag*` (iota from zero); bits from
/// 64 live in the overflow flag word.
pub(crate) const ACTOR_FLAG_SLEEPING: u32 = 76;
const ACTOR_FLAG_SNEAKING: u32 = 1;
const ACTOR_FLAG_INVISIBLE: u32 = 5;
const ACTOR_FLAG_SWIMMING: u32 = 57;
const ACTOR_FLAG_USING_ITEM: u32 = 4;
const ACTOR_FLAG_SPRINTING: u32 = 3;
const ACTOR_FLAG_GLIDING: u32 = 32;
const ACTOR_FLAG_CRAWLING: u32 = 114;

const SLEEPING_PLAYER_NETWORK_OFFSET: f32 = 0.2;
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

/// Steps a remote actor takes to reach each absolute movement target.
pub(crate) const ACTOR_INTERPOLATION_TICKS: u8 = 3;

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
    pub source_tick: Option<u64>,
    pub metadata: HashMap<u32, ActorMetadataValue>,
    pub attributes: HashMap<std::sync::Arc<str>, ActorAttribute>,
    pub int_properties: HashMap<u32, i32>,
    pub float_properties: HashMap<u32, f32>,
    pub status: ActorStatus,
}

impl ActorSnapshot {
    /// These actors animate their full yaw through the target-rotation queries.
    #[must_use]
    pub fn target_rotation_is_absolute(&self) -> bool {
        matches!(&self.kind, ActorKind::Entity { identifier } if matches!(identifier.as_ref(),
            "minecraft:arrow" | "minecraft:fireworks_rocket" | "minecraft:wither_skull" | "minecraft:wither_skull_dangerous"))
    }

    /// The render position `alpha` of the way from the previous tick's pose to the current one,
    /// or `None` when a component is not finite.
    #[must_use]
    pub fn interpolated_position(&self, alpha: f32) -> Option<[f32; 3]> {
        let position = std::array::from_fn(|axis| {
            self.previous_pose.position[axis]
                + (self.position[axis] - self.previous_pose.position[axis]) * alpha
        });
        position
            .iter()
            .all(|value| value.is_finite())
            .then_some(position)
    }

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
            status: ActorStatus::default(),
        };
        snapshot.apply_metadata(&spawn.metadata);
        snapshot.apply_attributes(&spawn.attributes);
        snapshot.apply_properties(&spawn.properties);
        snapshot
    }

    /// Builds the client-owned local-player snapshot; `revision` seeds both the spawn and the
    /// initial movement revision so the rig identity is exact from the first presented frame.
    /// `uuid`/`username` are the resolved identity by which the skin is looked up.
    fn local_player(
        unique_id: i64,
        runtime_id: u64,
        revision: u64,
        uuid: [u8; 16],
        username: std::sync::Arc<str>,
        feed: &LocalPlayerFeed,
    ) -> Self {
        let pose = ActorPose {
            position: feed.position,
            pitch: feed.pitch,
            yaw: feed.yaw,
            head_yaw: feed.head_yaw,
        };
        let mut snapshot = Self {
            unique_id,
            runtime_id,
            spawn_revision: revision,
            movement_revision: revision,
            kind: ActorKind::Player { uuid, username },
            position: feed.position,
            velocity: feed.velocity,
            pitch: feed.pitch,
            yaw: feed.yaw,
            head_yaw: feed.head_yaw,
            previous_pose: pose,
            received_pose: pose,
            interpolation_ticks_remaining: 0,
            body_yaw: feed.yaw,
            on_ground: Some(feed.on_ground),
            teleported: feed.teleported,
            player_mode: None,
            source_tick: None,
            metadata: HashMap::new(),
            attributes: HashMap::new(),
            int_properties: HashMap::new(),
            float_properties: HashMap::new(),
            status: ActorStatus::default(),
        };
        snapshot.apply_local_flags(feed);
        snapshot
    }

    /// Overwrites the primary-word flags the client predicts itself: sneak, sprint, swim and
    /// predicted item use. Shield blocking stays server-owned.
    fn apply_local_flags(&mut self, feed: &LocalPlayerFeed) {
        self.set_flag(ACTOR_FLAG_SNEAKING, feed.sneaking);
        self.set_flag(ACTOR_FLAG_SPRINTING, feed.sprinting);
        // Sprinting in water is swimming; the water sample lags one frame.
        let in_water = self.status.fluid.is_some_and(|(water, _)| water);
        self.set_flag(ACTOR_FLAG_SWIMMING, feed.sprinting && in_water);
        match feed.item_use {
            LocalItemUse::Unpredicted => {}
            LocalItemUse::Idle => self.set_flag(ACTOR_FLAG_USING_ITEM, false),
            LocalItemUse::Using => self.set_flag(ACTOR_FLAG_USING_ITEM, true),
        }
    }

    /// Sets one actor flag bit in the primary or overflow word, creating the word when absent.
    fn set_flag(&mut self, bit: u32, on: bool) {
        let (key, bit, empty) = if bit < 64 {
            (0, bit, ActorMetadataValue::Flags(0))
        } else {
            (
                EXTENDED_FLAGS_METADATA_KEY,
                bit - 64,
                ActorMetadataValue::FlagsExtended(0),
            )
        };
        let entry = self.metadata.entry(key).or_insert(empty);
        if let ActorMetadataValue::Flags(flags) | ActorMetadataValue::FlagsExtended(flags) = entry {
            if on {
                *flags |= 1_u64 << bit;
            } else {
                *flags &= !(1_u64 << bit);
            }
        }
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

    /// Feet-anchored `(min, max)` box from the width and height metadata; a player
    /// missing either falls back to its definition's collision box.
    #[must_use]
    pub fn bounding_box(&self) -> Option<([f32; 3], [f32; 3])> {
        let player = matches!(self.kind, ActorKind::Player { .. });
        let dimension = |key, player_default| match self.metadata.get(&key) {
            Some(ActorMetadataValue::Float(value)) if value.is_finite() && *value > 0.0 => {
                Some(*value)
            }
            _ => player.then_some(player_default),
        };
        let half_width = dimension(BOUNDING_BOX_WIDTH_METADATA_KEY, PLAYER_COLLISION_WIDTH)? * 0.5;
        let height = dimension(BOUNDING_BOX_HEIGHT_METADATA_KEY, PLAYER_COLLISION_HEIGHT)?;
        let [x, y, z] = self.position;
        Some((
            [x - half_width, y, z - half_width],
            [x + half_width, y + height, z + half_width],
        ))
    }

    /// Samples 0.66 of the body height above interpolated feet (Lens 1.26.50.26 0x1c0e520).
    /// Network position offsets have already been removed by the actor store.
    pub fn brightness_sample_position(&self, mut feet: [f32; 3]) -> [f32; 3] {
        if let Some((min, max)) = self.bounding_box() {
            feet[1] += 0.66 * (max[1] - min[1]);
        }
        feet
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

    /// The server-set render scale (metadata `Scale`), multiplying the model's own scale; an
    /// absent, non-finite or non-positive value reads 1.
    #[must_use]
    pub fn render_scale(&self) -> f32 {
        match self.metadata.get(&SCALE_METADATA_KEY) {
            Some(ActorMetadataValue::Float(scale)) if scale.is_finite() && *scale > 0.0 => *scale,
            _ => 1.0,
        }
    }

    #[must_use]
    pub fn is_invisible(&self) -> bool {
        self.flag(ACTOR_FLAG_INVISIBLE)
    }

    #[must_use]
    pub fn is_sneaking(&self) -> bool {
        self.flag(ACTOR_FLAG_SNEAKING)
    }

    #[must_use]
    pub fn is_sleeping(&self) -> bool {
        self.player_is_sleeping()
    }

    pub(crate) fn player_is_sleeping(&self) -> bool {
        let player_flags = self.metadata.get(&PLAYER_FLAGS_METADATA_KEY).is_some_and(
            |value| matches!(value, ActorMetadataValue::Byte(flags) if (*flags as u8) & PLAYER_FLAGS_SLEEPING != 0),
        );
        player_flags || self.flag(ACTOR_FLAG_SLEEPING)
    }

    /// Whether the using-item flag is set; for the local player's food and drink it is the
    /// server's admission of the use.
    #[must_use]
    pub fn is_using_item(&self) -> bool {
        self.flag(ACTOR_FLAG_USING_ITEM)
    }

    /// Reads one actor flag bit from the primary or overflow flag word.
    pub(crate) fn flag(&self, bit: u32) -> bool {
        let (key, bit) = if bit < 64 {
            (0, bit)
        } else {
            (EXTENDED_FLAGS_METADATA_KEY, bit - 64)
        };
        match self.metadata.get(&key) {
            Some(ActorMetadataValue::Flags(flags) | ActorMetadataValue::FlagsExtended(flags)) => {
                flags & (1_u64 << bit) != 0
            }
            _ => false,
        }
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
            if metadata.key == FUSE_TIME_METADATA_KEY {
                self.status.fuse_age_ticks = self.status.age_ticks;
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

/// Movement-affecting flags one local-player metadata update carries; `None`
/// when the flag word holding it was absent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MovementFlagUpdate {
    pub sneaking: Option<bool>,
    pub sprinting: Option<bool>,
    pub gliding: Option<bool>,
    pub swimming: Option<bool>,
    pub crawling: Option<bool>,
}

impl MovementFlagUpdate {
    /// Reads the flag words from `metadata`; `None` when neither word is present.
    #[must_use]
    pub fn from_metadata(metadata: &[protocol::ActorMetadata]) -> Option<Self> {
        let word = |key| {
            metadata.iter().rev().find_map(|item| match item.value {
                ActorMetadataValue::Flags(flags) | ActorMetadataValue::FlagsExtended(flags)
                    if item.key == key =>
                {
                    Some(flags)
                }
                _ => None,
            })
        };
        let primary = word(0);
        let extended = word(EXTENDED_FLAGS_METADATA_KEY);
        if primary.is_none() && extended.is_none() {
            return None;
        }
        let bit = |bit: u32| {
            let (flags, bit) = if bit < 64 {
                (primary, bit)
            } else {
                (extended, bit - 64)
            };
            flags.map(|flags| flags & (1_u64 << bit) != 0)
        };
        Some(Self {
            sneaking: bit(ACTOR_FLAG_SNEAKING),
            sprinting: bit(ACTOR_FLAG_SPRINTING),
            gliding: bit(ACTOR_FLAG_GLIDING),
            swimming: bit(ACTOR_FLAG_SWIMMING),
            crawling: bit(ACTOR_FLAG_CRAWLING),
        })
    }
}

/// Client-authored identity and pose for the local player's own third-person rig, which the
/// server never spawns as an actor. When the player list carries no self entry, the skin backs
/// a synthetic profile keyed by `uuid`; a real echo overrides it.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalPlayerFeed {
    pub uuid: [u8; 16],
    pub username: std::sync::Arc<str>,
    /// The client's own skin, uploaded at login and shown on the local body and HUD paperdoll.
    pub skin: PlayerSkin,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub on_ground: bool,
    /// Look-input yaw driving the body target, not the camera boom.
    pub yaw: f32,
    pub head_yaw: f32,
    pub pitch: f32,
    /// Identifiers of the client-owned main-hand and off-hand items.
    pub main_hand: Option<std::sync::Arc<str>>,
    pub off_hand: Option<std::sync::Arc<str>>,
    /// Snaps the pose and resets the rig instead of interpolating.
    pub teleported: bool,
    /// The camera renders from the player's eyes; selects the first-person render controller.
    pub first_person: bool,
    /// Predicted movement state; overrides the streamed sneak and sprint flags on the local rig.
    pub sneaking: bool,
    pub sprinting: bool,
    pub item_use: LocalItemUse,
}

/// Predicted use of the held item; only items the client can animate without the server.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LocalItemUse {
    /// The held item is not predicted; the streamed flags stand.
    #[default]
    Unpredicted,
    /// A predicted item is held but not in use.
    Idle,
    /// A predicted item is in use.
    Using,
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
    rider_to_ridden: HashMap<i64, i64>,
    max_actor_links: usize,
    players: HashMap<[u8; 16], PlayerProfile>,
    /// Appearances of spawned players removed from the roster, retained until despawn.
    unlisted_players: HashMap<[u8; 16], PlayerProfile>,
    animation: ActorAnimationStore,
    items: ItemStateStore,
    actions: RemoteActionStore,
    remote_state_excluded_runtime_id: Option<u64>,
    /// Key of the synthetic local-player profile, present only while the player list carries no
    /// self entry; cleared when a real echo takes over or the actor set is reset.
    synthetic_local_uuid: Option<[u8; 16]>,
    /// Monotonic spawn/movement revision for the client-fed local player actor.
    synthetic_local_revision: u64,
    /// Whether the local player's own rig should render first-person; set by each pose feed.
    local_first_person: bool,
    /// Held items of the client-fed local player, which the item store never tracks.
    local_hands: [Option<std::sync::Arc<str>>; 2],
    /// View `[pitch, yaw]` in degrees, sampled into each animation tick.
    camera_rotation: [f32; 2],
    /// View world position, sampled into each animation tick.
    camera_position: [f32; 3],
    /// Rigs outside this view hold their pose instead of animating.
    animation_view: Option<crate::actor_animation::ActorAnimationView>,
    /// Seat layouts for mounts whose riders stream no seat offset.
    seat_defaults: std::sync::Arc<SeatDefaults>,
    property_registry: properties::PropertyRegistry,
    /// Latest local-player knockback `(sequence, [x, z])`, for hurt direction inference.
    local_knockback: Option<(u64, [f32; 2])>,
    /// Status events awaiting a particle or sound consumer.
    status_notices: Vec<ActorStatusNotice>,
}

mod dropped;
mod entities;
mod hurt;
mod lifecycle;
mod lightning;
mod placement;
mod projectile;
pub(crate) mod properties;
mod query;

pub use dropped::{DroppedItemView, MAX_DROPPED_ITEM_COPIES, dropped_item_copy_count};
pub use entities::{BlockEntityKind, BlockEntityView, RopeKind, RopeView, tnt_presentation};
pub use hurt::{
    ActorPickup, ActorStatus, ActorStatusNotice, DEATH_DURATION_TICKS, HURT_DURATION_TICKS,
    HURT_OVERLAY_ALPHA, MAX_STATUS_NOTICES, PICKUP_DURATION_TICKS,
};
pub use lightning::LightningBoltView;
pub use placement::{RideSeat, SeatDefaults, SeatRequirement};
pub use properties::PropertyDefault;

fn retained_skin_bytes(skin: &PlayerSkin) -> usize {
    match skin {
        PlayerSkin::Standard(skin) => {
            skin.rgba8.len()
                + skin.cape.as_ref().map_or(0, |cape| cape.rgba8.len())
                + skin
                    .geometry
                    .as_ref()
                    .map_or(0, |geometry| geometry.byte_len())
        }
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
        ActorEvent::PlayerList(_)
        | ActorEvent::Skin { .. }
        | ActorEvent::Status(_)
        | ActorEvent::TakeItem(_) => None,
    }
}

#[cfg(test)]
mod local_tests;
mod player_appearance;
#[cfg(test)]
mod riding_tests;
mod skin_update;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod projectile_tests;
