use assets::facts::entity::{EntityFact, EntityFacts, NativeEntityClass};
use hashbrown::HashMap as FastHashMap;
use protocol_ids::{ActorDataKey, ActorFlag, PlayerFlag};
use std::collections::{HashMap, HashSet};

#[cfg(test)]
use protocol::ITEM_ACTOR_NETWORK_OFFSET;

mod aim_assist;
mod animated;
mod appearance_preparation;
mod colliders;
mod effect_members;
pub use colliders::ActorCollisionBounds;
mod death_visibility;
mod local_presentation;
mod native_facts;
#[cfg(test)]
pub(crate) use appearance_preparation::MAX_APPEARANCES_PUBLISHED_PER_FRAME;

use protocol::{
    ActorAttribute, ActorEvent, ActorKind, ActorLinkEvent, ActorLinkType, ActorMetadataValue,
    ActorMoveEvent, ActorPositionOrigin, ActorProperty, ActorSpawnEvent, EquipmentEvent,
    ItemActorEvent, MAX_ACTOR_ATTRIBUTES, MAX_ACTOR_METADATA_ENTRIES, MAX_ACTOR_PROPERTIES,
    MAX_PLAYER_LIST_SKIN_BYTES, MovePlayerEvent, MovePlayerMode, PLAYER_NETWORK_OFFSET,
    PlayerListEntry, PlayerSkin, PlayerSkinUnavailable,
};

use crate::{
    action::{ActorSourceTick, MAX_ACTION_EVENTS_PER_TICK, RemoteActionStore},
    item::ItemStateStore,
};
use actor_animation::{ActorAnimationStore, ActorLifetimeId, MAX_ANIMATED_ACTORS};

/// Every tracked actor keeps an animation rig.
pub(crate) const MAX_TRACKED_ACTORS: usize = MAX_ANIMATED_ACTORS;
pub(crate) const MAX_TRACKED_PLAYERS: usize = 4_096;
pub(crate) const MAX_TRACKED_ACTOR_LINKS: usize = MAX_TRACKED_ACTORS;
pub(crate) const MAX_TRACKED_PLAYER_SKIN_BYTES: usize = MAX_PLAYER_LIST_SKIN_BYTES;

const DEFAULT_ACTOR_COLLISION_WIDTH: f32 = 0.6;
const DEFAULT_ACTOR_COLLISION_HEIGHT: f32 = 1.8;

/// A sleeping player's offset from its feet to its native actor-state position (its eye).
pub const SLEEPING_PLAYER_NETWORK_OFFSET: f32 = 0.2;
/// A vanilla falling block's network offset, for its vanilla box height.
const FALLING_BLOCK_HEIGHT: f32 = 0.98;

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
pub(crate) const ACTOR_INTERPOLATION_TICKS: u32 = 3;

/// Vertical origin contributions; default values describe collision-feet coordinates.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PoseYOrigin {
    spawn: f32,
    network: f32,
    /// Exact network height before the conversion to collision feet.
    native_y: Option<f32>,
}
impl PoseYOrigin {
    const SPAWN: Self = Self {
        spawn: 1.0,
        network: 0.0,
        native_y: None,
    };
    const NETWORK: Self = Self {
        spawn: 0.0,
        network: 1.0,
        native_y: None,
    };

    /// Blends origin contributions with the same fraction as the vertical position.
    fn interpolate(self, target: Self, fraction: f32) -> Self {
        Self {
            spawn: self.spawn + (target.spawn - self.spawn) * fraction,
            network: self.network + (target.network - self.network) * fraction,
            native_y: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorPose {
    /// Retains which vertical offsets contributed to this pose.
    pub y_origin: PoseYOrigin,
    pub position: [f32; 3],
    pub pitch: f32,
    pub yaw: f32,
    pub head_yaw: f32,
}

impl ActorPose {
    /// Applies only the offset deltas that contributed to this pose's vertical coordinate.
    fn shift_y(&mut self, network_delta: f32, spawn_delta: f32, network_offset: f32) {
        let delta = self.y_origin.network * network_delta + self.y_origin.spawn * spawn_delta;
        if let Some(native_y) = self.y_origin.native_y {
            let native_y = native_y + (delta - network_delta);
            self.y_origin.native_y = Some(native_y);
            self.position[1] = native_y - network_offset;
        } else if delta != 0.0 {
            self.position[1] += delta;
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActorSnapshot {
    /// Dedicated hanging-picture state; ordinary AddActor spawns do not supply it.
    pub painting: Option<protocol::PaintingData>,
    pub unique_id: i64,
    pub runtime_id: u64,
    pub spawn_revision: u64,
    pub movement_revision: u64,
    pub kind: ActorKind,
    /// Native facts copied at actor admission from its declared constructor.
    pub entity_fact: EntityFact,
    /// Resolved at packet admission; frame and ray paths read these numeric dispositions.
    pub interaction_traits: assets::interactions::EntityInteractionTraits,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub pitch: f32,
    pub yaw: f32,
    pub head_yaw: f32,
    pub previous_pose: ActorPose,
    pub received_pose: ActorPose,
    pub interpolation_ticks_remaining: u32,
    pub body_yaw: f32,
    pub on_ground: Option<bool>,
    pub teleported: bool,
    pub player_mode: Option<MovePlayerMode>,
    pub player_game_mode: Option<protocol::GameModeUpdate>,
    pub source_tick: Option<u64>,
    /// Accepted actor metadata, independent of local animation flags.
    pub metadata: HashMap<u32, ActorMetadataValue>,
    /// Client-derived rendering flags, retired by the next received primary word.
    local_presentation_flags: Option<u64>,
    pub attributes: HashMap<std::sync::Arc<str>, ActorAttribute>,
    pub int_properties: HashMap<u32, i32>,
    pub float_properties: HashMap<u32, f32>,
    pub status: ActorStatus,
    pub(crate) dragon_animation: Option<Box<dragon_animation::State>>,
    /// Accumulated [`ActorDataKey::HitBox`] boxes; empty geometry uses the collision box.
    pub(crate) hitboxes: Option<std::sync::Arc<hitbox::State>>,
}

impl ActorSnapshot {
    /// Reads the server's packed spell RGB color as an opaque particle tint.
    #[must_use]
    pub fn spell_color(&self) -> [f32; 4] {
        let rgb = match self
            .metadata
            .get(&ActorDataKey::DataSpellCastingColor.wire_value())
        {
            Some(ActorMetadataValue::Int(value)) => *value as u32,
            _ => 0,
        };
        [
            ((rgb >> 16) & 0xff) as f32 / 255.0,
            ((rgb >> 8) & 0xff) as f32 / 255.0,
            (rgb & 0xff) as f32 / 255.0,
            1.0,
        ]
    }

    /// Whether damage or ordinary mob death applies the red actor overlay.
    #[must_use]
    pub fn hurt_overlay_active(&self) -> bool {
        if self.is_dying_dragon() {
            self.status.hurt_time > 0 && !self.status.skip_red_flash
        } else {
            self.status.overlay_active()
        }
    }

    /// Dragons retain their authored death pose while ordinary mobs tip onto their side.
    #[must_use]
    pub fn death_rotation_progress(&self, partial_tick: f32) -> Option<f32> {
        if self.is_dying_dragon() {
            None
        } else {
            self.status.death_progress(partial_tick)
        }
    }

    pub(crate) fn is_dying_dragon(&self) -> bool {
        self.status.dead && self.entity_fact.native_class == NativeEntityClass::EnderDragon
    }

    /// Vanilla velocity units (blocks/tick) for tick-driven engine animation.
    #[must_use]
    pub fn native_velocity(&self) -> [f32; 3] {
        self.status.native_velocity
    }

    /// The render position `alpha` of the way from the previous tick's pose to the current one,
    /// or `None` when a component is not finite.
    #[must_use]
    pub fn interpolated_position(&self, alpha: f32) -> Option<[f32; 3]> {
        let mut position = std::array::from_fn(|axis| {
            self.previous_pose.position[axis]
                + (self.position[axis] - self.previous_pose.position[axis]) * alpha
        });
        if self.previous_pose.y_origin.native_y.is_some()
            || self
                .status
                .movement_interpolation
                .current_y_origin
                .native_y
                .is_some()
        {
            let offset = self.network_position_offset();
            let from = self.previous_pose.native_position(offset)[1];
            let to = self.native_position()[1];
            position[1] = from + (to - from) * alpha - offset;
        }
        position
            .iter()
            .all(|value| value.is_finite())
            .then_some(position)
    }

    /// Builds retained state using the constructor facts resolved at admission.
    fn from_spawn(
        spawn: ActorSpawnEvent,
        spawn_revision: u64,
        entity_fact: EntityFact,
        hitbox_budget: &mut hitbox::Budget,
    ) -> Self {
        let terrain_interlock = terrain_interlock::TerrainInterlock::attached(
            entity_fact.native_class,
            web_time::Instant::now(),
        );
        let position = spawn.position;
        let pitch = movement_interpolation::signed_pitch(spawn.pitch);
        let pose = ActorPose {
            y_origin: PoseYOrigin::SPAWN,
            position,
            pitch,
            yaw: spawn.yaw,
            head_yaw: spawn.head_yaw,
        };
        let mut snapshot = Self {
            painting: None,
            unique_id: spawn.unique_id,
            runtime_id: spawn.runtime_id,
            spawn_revision,
            movement_revision: 0,
            interaction_traits: assets::interactions::EntityInteractionTraits::Unresolved,
            kind: spawn.kind,
            entity_fact,
            position,
            velocity: spawn.velocity,
            pitch,
            yaw: spawn.yaw,
            head_yaw: spawn.head_yaw,
            previous_pose: pose,
            received_pose: pose,
            interpolation_ticks_remaining: 0,
            body_yaw: spawn.body_yaw,
            on_ground: None,
            teleported: false,
            player_mode: None,
            player_game_mode: None,
            source_tick: None,
            metadata: HashMap::with_capacity(spawn.metadata.len()),
            local_presentation_flags: None,
            attributes: HashMap::with_capacity(spawn.attributes.len()),
            int_properties: HashMap::new(),
            float_properties: HashMap::new(),
            status: ActorStatus {
                native_velocity: spawn.velocity,
                terrain_interlock,
                ..ActorStatus::default()
            },
            dragon_animation: None,
            hitboxes: None,
        };
        snapshot.set_current_pose(pose);
        // Falling-block and boat spawn positions carry their network offset, as movement does;
        // a falling block's box sets it and `apply_metadata` rebases onto the spawn's box.
        let spawn_offset = snapshot.spawn_network_offset();
        if spawn_offset != 0.0 {
            snapshot.shift_poses_y(-spawn_offset, -spawn_offset);
        }
        snapshot.apply_metadata(&spawn.metadata, hitbox_budget);
        snapshot.apply_attributes(&spawn.attributes);
        snapshot.sync_status_from_health();
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
            y_origin: PoseYOrigin::default(),
            position: feed.position,
            pitch: feed.pitch,
            yaw: feed.yaw,
            head_yaw: feed.head_yaw,
        };
        let mut snapshot = Self {
            painting: None,
            unique_id,
            runtime_id,
            spawn_revision: revision,
            movement_revision: revision,
            interaction_traits: assets::interactions::EntityInteractionTraits::GENERIC_MOB,
            kind: ActorKind::Player { uuid, username },
            entity_fact: EntityFacts::pinned().player(),
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
            player_game_mode: None,
            source_tick: None,
            metadata: HashMap::new(),
            local_presentation_flags: None,
            attributes: HashMap::new(),
            int_properties: HashMap::new(),
            float_properties: HashMap::new(),
            status: ActorStatus {
                native_velocity: feed.velocity,
                fall_fly_ticks: feed.fall_fly_ticks,
                ..ActorStatus::default()
            },
            dragon_animation: None,
            hitboxes: None,
        };
        // Retain the visible-name default until the server replaces it.
        snapshot.metadata.insert(
            ActorDataKey::Flags.wire_value(),
            ActorMetadataValue::Flags(1 << ActorFlag::ShowName.wire_value()),
        );
        snapshot.apply_local_flags(feed);
        snapshot
    }

    fn current_pose(&self) -> ActorPose {
        ActorPose {
            y_origin: self.status.movement_interpolation.current_y_origin,
            position: self.position,
            pitch: self.pitch,
            yaw: self.yaw,
            head_yaw: self.head_yaw,
        }
    }

    fn set_current_pose(&mut self, pose: ActorPose) {
        self.status.movement_interpolation.current_y_origin = pose.y_origin;
        self.position = pose.position;
        self.pitch = pose.pitch;
        self.yaw = pose.yaw;
        self.head_yaw = pose.head_yaw;
    }

    /// The server-set render scale (metadata `Scale`), multiplying the model's own scale; an
    /// absent, non-finite or negative value reads 1. Zero hides the body while retaining its actor.
    #[must_use]
    pub fn render_scale(&self) -> f32 {
        match self.metadata.get(&ActorDataKey::Scale.wire_value()) {
            Some(ActorMetadataValue::Float(scale)) if scale.is_finite() && *scale >= 0.0 => *scale,
            _ => 1.0,
        }
    }

    #[must_use]
    pub fn is_invisible(&self) -> bool {
        self.flag(ActorFlag::Invisible.wire_value())
    }

    #[must_use]
    pub fn is_sneaking(&self) -> bool {
        self.flag(ActorFlag::Sneaking.wire_value())
    }

    #[must_use]
    pub fn is_gliding(&self) -> bool {
        self.flag(ActorFlag::Gliding.wire_value())
    }

    /// Whether this actor is in its Riptide spin pose.
    pub fn is_spinning(&self) -> bool {
        self.flag(ActorFlag::DamageNearbyMobs.wire_value())
    }

    #[must_use]
    pub fn is_sleeping(&self) -> bool {
        self.player_is_sleeping()
    }

    pub(crate) fn player_is_sleeping(&self) -> bool {
        let player_flags = self.metadata.get(&ActorDataKey::PlayerFlags.wire_value()).is_some_and(
            |value| matches!(value, ActorMetadataValue::Byte(flags) if (*flags as u8) & PlayerFlag::Sleeping.wire_value() != 0),
        );
        player_flags || self.flag(ActorFlag::Sleeping.wire_value())
    }

    /// Whether the rendered actor uses its held item, including a local visual pose.
    #[must_use]
    pub fn is_using_item(&self) -> bool {
        self.flag(ActorFlag::UsingItem.wire_value())
    }

    /// Reads a rendering flag, including the local rig's visual primary word.
    pub(crate) fn flag(&self, bit: u32) -> bool {
        if bit < u64::BITS
            && let Some(flags) = self.local_presentation_flags
        {
            return flags & (1_u64 << bit) != 0;
        }
        let (key, bit) = if bit < 64 {
            (ActorDataKey::Flags.wire_value(), bit)
        } else if bit < 128 {
            (ActorDataKey::FlagsTwo.wire_value(), bit - 64)
        } else if bit < 192 {
            (ActorDataKey::FlagsThird.wire_value(), bit - 128)
        } else {
            return false;
        };
        match self.metadata.get(&key) {
            Some(ActorMetadataValue::Flags(flags) | ActorMetadataValue::FlagsExtended(flags)) => {
                flags & (1_u64 << bit) != 0
            }
            _ => false,
        }
    }

    /// A falling block's network offset follows its box as the Scale metadata sizes it: half a
    /// block for the vanilla box, none for an empty box or a near-zero scale, so a text host
    /// built on a falling block stays at the position its server sends.
    fn falling_block_network_offset(&self) -> f32 {
        let height = match self.metadata.get(&ActorDataKey::Height.wire_value()) {
            Some(ActorMetadataValue::Float(height)) if height.is_finite() && *height >= 0.0 => {
                *height
            }
            _ => FALLING_BLOCK_HEIGHT,
        };
        let offset =
            self.entity_fact.network_y_offset * height * self.render_scale() / FALLING_BLOCK_HEIGHT;
        if offset.is_finite() {
            offset
        } else {
            self.entity_fact.network_y_offset
        }
    }

    fn primed_tnt_network_offset(&self) -> f32 {
        self.metadata
            .get(&ActorDataKey::Height.wire_value())
            .and_then(|value| match value {
                ActorMetadataValue::Float(height) if height.is_finite() && *height > 0.0 => {
                    Some(*height * 0.5)
                }
                _ => None,
            })
            .unwrap_or(self.entity_fact.network_y_offset)
    }

    fn is_falling_block(&self) -> bool {
        self.entity_fact.native_class == NativeEntityClass::FallingBlock
    }

    /// Spawn packets need vertical normalization only for falling-block and boat constructors.
    fn spawn_network_offset(&self) -> f32 {
        if self.is_falling_block() || self.entity_fact.native_class == NativeEntityClass::Boat {
            self.network_position_offset()
        } else {
            0.0
        }
    }

    fn apply_metadata(
        &mut self,
        metadata: &[protocol::ActorMetadata],
        hitbox_budget: &mut hitbox::Budget,
    ) -> bool {
        let previous_offset = self.network_position_offset();
        let mut rejected = false;
        for metadata in metadata {
            // The sentinel preserves the last effective always-show value.
            if metadata.key == ActorDataKey::AlwaysShowNameTag.wire_value()
                && matches!(metadata.value, ActorMetadataValue::Byte(-1))
            {
                continue;
            }
            if self.metadata.len() >= MAX_ACTOR_METADATA_ENTRIES
                && !self.metadata.contains_key(&metadata.key)
            {
                rejected = true;
                continue;
            }
            if metadata.key == ActorDataKey::FuseTime.wire_value() {
                self.status.fuse_age_ticks = self.status.age_ticks;
            }
            if metadata.key == ActorDataKey::HitBox.wire_value() {
                rejected |= hitbox_budget.apply(&mut self.hitboxes, &metadata.value);
            }
            if metadata.key == ActorDataKey::Flags.wire_value() {
                self.local_presentation_flags = None;
            }
            self.metadata.insert(metadata.key, metadata.value.clone());
        }
        // Falling-block size changes keep the network origin fixed. Other actors retain
        // their existing feet anchors.
        let offset = self.network_position_offset();
        if offset != previous_offset {
            if self.is_falling_block() {
                self.shift_poses_y(previous_offset - offset, previous_offset - offset);
            } else {
                self.clear_native_heights();
            }
        }
        let burning = self.is_on_fire();
        self.status.fire.observe(burning, self.status.age_ticks);
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
    /// Decimal Xbox user ID, empty when the server sent none.
    pub xuid: std::sync::Arc<str>,
    pub skin: PlayerSkin,
}

/// Movement-affecting flags one local-player metadata update carries; `None`
/// when the flag word holding it was absent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MovementFlagUpdate {
    pub immobile: Option<bool>,
    pub sneaking: Option<bool>,
    pub sprinting: Option<bool>,
    pub gliding: Option<bool>,
    pub swimming: Option<bool>,
    pub crawling: Option<bool>,
    pub has_gravity: Option<bool>,
    pub uniform_air_drag: Option<bool>,
}

impl MovementFlagUpdate {
    /// Reads the flag words from `metadata`; `None` when no word is present.
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
        let words = [
            word(ActorDataKey::Flags.wire_value()),
            word(ActorDataKey::FlagsTwo.wire_value()),
            word(ActorDataKey::FlagsThird.wire_value()),
        ];
        if words.iter().all(Option::is_none) {
            return None;
        }
        let bit =
            |bit: u32| words[(bit / 64) as usize].map(|flags| flags & (1_u64 << (bit % 64)) != 0);
        Some(Self {
            immobile: bit(ActorFlag::NoAI.wire_value()),
            sneaking: bit(ActorFlag::Sneaking.wire_value()),
            sprinting: bit(ActorFlag::Sprinting.wire_value()),
            gliding: bit(ActorFlag::Gliding.wire_value()),
            swimming: bit(ActorFlag::Swimming.wire_value()),
            crawling: bit(ActorFlag::Crawling.wire_value()),
            has_gravity: bit(ActorFlag::HasGravity.wire_value()),
            uniform_air_drag: bit(ActorFlag::UsesUniformAirDrag.wire_value()),
        })
    }
}

/// Client-authored identity and pose for the local rig. The skin backs a synthetic profile;
/// a server appearance takes priority unless `prefer_client_skin` explicitly overrides it.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalPlayerFeed {
    /// Committed local mode used by authored player render controllers.
    pub game_mode: Option<protocol::PlayerGameMode>,
    pub uuid: [u8; 16],
    pub username: std::sync::Arc<str>,
    /// The client's own skin, uploaded at login and shown on the local body and HUD paperdoll.
    pub skin: PlayerSkin,
    /// Uses the client appearance locally while retaining any server profile for restoration.
    pub prefer_client_skin: bool,
    pub position: [f32; 3],
    /// Native simulation displacement per tick, passed through from `sim::PlayerState`.
    pub velocity: [f32; 3],
    pub on_ground: bool,
    /// Active flight from the client's completed movement mode, rather than server abilities.
    pub flying: bool,
    /// Predicted elytra glide; overrides the streamed gliding flag on the local rig.
    pub gliding: bool,
    /// Predicted Riptide spin pose, independent of swimming and gliding.
    pub spinning: bool,
    /// Consecutive completed gliding ticks, which ease the body into its glide tilt.
    pub fall_fly_ticks: u32,
    /// Look-input yaw driving the body target, not the camera boom.
    pub yaw: f32,
    pub head_yaw: f32,
    pub pitch: f32,
    /// Identifiers of the client-owned main-hand and off-hand items.
    pub main_hand: Option<std::sync::Arc<str>>,
    pub off_hand: Option<std::sync::Arc<str>>,
    /// Occupied local armor-container slots, independent of item admission and hands.
    pub equipment_count: u8,
    /// Numerical registry identities for the main hand and off hand; zero means empty.
    pub held_network_ids: [i32; 2],
    /// The main-hand stack's data value.
    pub main_hand_metadata: u32,
    /// Main-hand and off-hand stack facts whose change re-equips the item.
    pub held_variants: [actor_animation::StackVariant; 2],
    /// Positive server identity for the selected stack, if one is available.
    pub main_hand_stack_id: Option<i32>,
    /// Selected hotbar slot; equal stacks in different slots still re-equip.
    pub main_hand_slot: u8,
    /// Current Bedrock swing duration, recalculated from the active effects each tick.
    pub bedrock_swing_ticks: i32,
    /// Current Java swing duration, which excludes Conduit Power.
    pub java_swing_ticks: i32,
    /// Snaps the pose and resets the rig instead of interpolating.
    pub teleported: bool,
    /// The camera renders from the player's eyes; selects the first-person render controller.
    pub first_person: bool,
    /// View-bobbing preference driving the local player's authored hand animations.
    pub view_bobbing: bool,
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
    /// Gameplay's selected-stack clock, including charge-duration modifiers and loaded state.
    Predicted {
        elapsed_ticks: Option<u32>,
        max_ticks: u32,
        charged: bool,
    },
}

/// Sparse session state, including immutable prepared appearances; GPU resources stay in presentation.
#[derive(Debug)]
pub(crate) struct ActorStore {
    session_id: u64,
    dimension: i32,
    latest_sequence: u64,
    max_actors: usize,
    max_players: usize,
    max_player_skin_bytes: usize,
    retained_player_skin_bytes: usize,
    ignored_movement_components: u64,
    actor_identifier_skips: u64,
    painting_motive_fallbacks: u64,
    player_game_mode_skips: u64,
    world_default_game_mode: Option<protocol::PlayerGameMode>,
    entity_assets: Option<std::sync::Arc<assets::RuntimeEntityAssets>>,
    custom_actor_facts: HashMap<std::sync::Arc<str>, EntityFact>,
    actors: HashMap<u64, ActorSnapshot>,
    hitbox_budget: hitbox::Budget,
    effect_members: effect_members::EffectMembers,
    unique_to_runtime: FastHashMap<i64, u64>,
    rider_to_ridden: FastHashMap<i64, i64>,
    max_actor_links: usize,
    players: HashMap<[u8; 16], PlayerProfile>,
    /// Appearances of spawned players removed from the roster, retained until despawn.
    unlisted_players: HashMap<[u8; 16], PlayerProfile>,
    animation: ActorAnimationStore,
    ready_appearances: appearance_preparation::ReadyAppearances,
    items: ItemStateStore,
    actions: RemoteActionStore,
    remote_state_excluded_runtime_id: Option<u64>,
    /// Health admitted before the client-fed local actor is created.
    pending_local_health: Option<ActorAttribute>,
    /// Arm swings the server sent for the local player, waiting for its swing tracker.
    local_server_swings: Vec<Option<std::sync::Arc<str>>>,
    pending_local_damage: Option<local_health::PendingLocalDamage>,
    local_health_skips: u64,
    /// The local player's last-death data held while its actor is absent (a dimension change),
    /// since servers need not resend it.
    pending_local_death: Vec<protocol::ActorMetadata>,
    /// Server PlayerSpawn has enabled local health-drop animations in this session.
    local_player_spawned: bool,
    /// Key of the synthetic local-player profile, present only while the player list carries no
    /// self entry; cleared when a real echo takes over or the actor set is reset.
    synthetic_local_uuid: Option<[u8; 16]>,
    /// Last client-fed appearance, separate from server replacements of its profile.
    synthetic_local_skin: Option<[u8; 32]>,
    /// Avoids rehashing an unchanged client-fed skin on every pose feed.
    local_skin_fingerprints: profiles::SkinFingerprintMemo,
    /// A client skin that could not fit the profile budget is retried on later feeds.
    synthetic_local_skin_pending: bool,
    /// Monotonic spawn/movement revision for the client-fed local player actor.
    synthetic_local_revision: u64,
    /// Whether the local player's own rig should render first-person; set by each pose feed.
    local_first_person: bool,
    local_view_dirty: bool,
    /// Remote motion this frame's due ticks reach, sorted by runtime id and read by picks until
    /// the frame advances the live actors; reused across frames.
    pick_states: Vec<(u64, movement_interpolation::MotionState)>,
    picks_ahead: bool,
    local_view_bobbing: bool,
    local_flying: bool,
    /// Held items of the client-fed local player, which the item store never tracks.
    local_hands: [Option<std::sync::Arc<str>>; 2],
    local_equipment_count: u8,
    local_held_network_ids: [i32; 2],
    local_main_metadata: u32,
    local_held_variants: [actor_animation::StackVariant; 2],
    local_main_stack_id: Option<i32>,
    local_main_slot: u8,
    local_item_use: LocalItemUse,
    local_bedrock_swing_ticks: i32,
    local_java_swing_ticks: i32,
    /// View `[pitch, yaw]` in degrees, sampled into each animation tick.
    camera_rotation: [f32; 2],
    /// View world position, sampled into each animation tick.
    camera_position: [f32; 3],
    /// Rigs outside this view hold their pose instead of animating.
    animation_view: Option<actor_animation::ActorAnimationView>,
    /// Seat descriptor per mount, for riders that stream no seat offset.
    seat_bindings: FastHashMap<u64, Option<u32>>,
    seat_bases: HashMap<std::sync::Arc<str>, Option<u32>>,
    property_registry: properties::PropertyRegistry,
    /// Latest local-player knockback `(sequence, [x, z])`, for hurt direction inference.
    local_knockback: Option<(u64, [f32; 2])>,
    /// Status events awaiting a particle or sound consumer.
    status_notices: Vec<ActorStatusNotice>,
    pickup_visuals: Vec<dropped::PickupVisual>,
    particle_effects: std::collections::VecDeque<crate::CommittedParticleEvent>,
    synchronized_audio: synchronized_audio::SynchronizedAudio,
}

pub use synchronized_audio::ROCKET_LAUNCH_EVENT;

mod cloud_particles;
pub(crate) mod creeper;
mod crystal_beam;
pub use crystal_beam::CrystalBeamView;
pub(crate) mod dragon_animation;
mod dragon_beam;
mod dragon_death;
pub use dragon_death::DragonDeathView;
mod dragon_fireball_particles;
mod dragon_particles;
mod dropped;
mod entities;
mod fire;
mod hitbox;
mod hurt;
mod last_death;
mod local_health;
pub use local_health::DEFAULT_PLAYER_HEALTH;
mod lifecycle;
mod lightning;
mod mount;
mod movement_interpolation;
#[cfg(test)]
mod painting_tests;
mod paintings;
mod placement;
mod projectile;
pub(crate) mod properties;
mod query;
mod shadow;
mod spatial;
mod synchronized_audio;
mod terrain_interlock;

pub use dropped::{DroppedItemView, MAX_DROPPED_ITEM_COPIES, dropped_item_copy_count};
pub use entities::{
    BlockEntityCandidate, BlockEntityKind, BlockEntityView, RopeKind, RopeView, tnt_presentation,
};
pub use fire::FIRE_FADE_TICKS;
pub use hitbox::ActorHitBoxes;
pub use hurt::{
    ActorDamageState, ActorPickup, ActorStatus, ActorStatusNotice, DEATH_DURATION_TICKS,
    HURT_DURATION_TICKS, HURT_OVERLAY_ALPHA, MAX_STATUS_NOTICES, PICKUP_DURATION_TICKS,
};
pub use lightning::LightningBoltView;

pub use placement::{ActorFluidProbe, RideSeat};
pub use properties::PropertyDefault;
pub use shadow::ActorShadowCaster;

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
        ActorEvent::Spawn(event)
        | ActorEvent::PlayerSpawn { spawn: event, .. }
        | ActorEvent::PaintingSpawn { spawn: event, .. } => Some(event.dimension),
        ActorEvent::Remove(event) => Some(event.dimension),
        ActorEvent::Move(event) => Some(event.dimension),
        ActorEvent::Metadata(event) => Some(event.dimension),
        ActorEvent::Attributes(event) => Some(event.dimension),
        ActorEvent::PlayerList(_)
        | ActorEvent::Skin { .. }
        | ActorEvent::Status(_)
        | ActorEvent::TakeItem(_)
        | ActorEvent::Identifiers(_) => None,
    }
}

#[cfg(test)]
mod animation_tests;
#[cfg(test)]
mod local_presentation_tests;
#[cfg(test)]
mod local_tests;
mod player_appearance;
mod profiles;
#[cfg(test)]
mod riding_tests;
#[cfg(test)]
mod roster_identity_tests;
mod skin_update;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod projectile_tests;

#[cfg(test)]
mod review_tests;

#[cfg(test)]
mod terrain_interlock_tests;

#[cfg(test)]
mod velocity_tests;

#[cfg(test)]
mod movement_duration_tests;

#[cfg(test)]
mod scale_tests;

#[cfg(test)]
mod sleep_tests;

#[cfg(test)]
mod bounds_tests;

#[cfg(test)]
mod hitbox_tests;

#[cfg(test)]
mod movement_flags_tests;

mod display;
pub use display::{DisplayActorStore, MAX_DISPLAY_ACTORS};
