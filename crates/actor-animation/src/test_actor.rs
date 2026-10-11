//! A plain actor double for unit tests; actor authority's own derivations are tested where they live.
use std::{collections::HashMap, sync::Arc};

use protocol::{ActorAttribute, ActorKind, ActorMetadataValue, GameModeUpdate};

use super::{AnimatedActor, query};

pub(super) const EXTENDED_FLAGS_METADATA_KEY: u32 = 92;
pub(super) const SCALE_METADATA_KEY: u32 = 38;

#[derive(Clone, Debug)]
pub(super) struct TestActor {
    pub unique_id: i64,
    pub runtime_id: u64,
    pub spawn_revision: u64,
    pub kind: ActorKind,
    pub position: [f32; 3],
    pub previous_position: [f32; 3],
    pub velocity: [f32; 3],
    pub pitch: f32,
    pub yaw: f32,
    pub head_yaw: f32,
    pub body_yaw: f32,
    pub on_ground: Option<bool>,
    pub player_game_mode: Option<GameModeUpdate>,
    pub metadata: HashMap<u32, ActorMetadataValue>,
    pub attributes: HashMap<Arc<str>, ActorAttribute>,
    pub int_properties: HashMap<u32, i32>,
    pub float_properties: HashMap<u32, f32>,
    pub status: TestStatus,
}

/// Derived actor facts a test sets directly instead of deriving them from packets.
#[derive(Clone, Debug)]
pub(super) struct TestStatus {
    pub native_velocity: [f32; 3],
    pub fluid: Option<(bool, bool)>,
    pub dead: bool,
    pub on_fire: bool,
    pub sleeping: bool,
    pub hurt_time: u8,
    pub shake_time: i32,
    pub hurt_direction: Option<f32>,
    pub kinetic_hit_ticks: Option<u32>,
    pub death_ticks: u16,
    pub sleep_rotation: Option<f32>,
    pub overlay_alpha: f32,
    pub network_position_offset: f32,
    /// Creeper swell amounts at the previous and current tick.
    pub creeper_swell: [f32; 2],
    pub creeper_swelling_direction: f32,
    pub dragon_flap_phase: Option<f32>,
    /// `[yaw, height]` frames by completed-tick offset.
    pub dragon_history: Vec<[f32; 2]>,
}

impl Default for TestStatus {
    fn default() -> Self {
        Self {
            native_velocity: [0.0; 3],
            fluid: None,
            dead: false,
            on_fire: false,
            sleeping: false,
            hurt_time: 0,
            shake_time: 0,
            hurt_direction: None,
            kinetic_hit_ticks: None,
            death_ticks: 0,
            sleep_rotation: None,
            overlay_alpha: 0.0,
            network_position_offset: 0.0,
            creeper_swell: [0.0; 2],
            creeper_swelling_direction: -1.0,
            dragon_flap_phase: None,
            dragon_history: Vec::new(),
        }
    }
}

impl Default for TestActor {
    fn default() -> Self {
        Self {
            unique_id: -1,
            runtime_id: 1,
            spawn_revision: 1,
            kind: ActorKind::Entity {
                identifier: "minecraft:test".into(),
            },
            position: [0.0; 3],
            previous_position: [0.0; 3],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            on_ground: Some(false),
            player_game_mode: None,
            metadata: HashMap::new(),
            attributes: HashMap::new(),
            int_properties: HashMap::new(),
            float_properties: HashMap::new(),
            status: TestStatus::default(),
        }
    }
}

impl TestActor {
    fn identifier(&self) -> &str {
        match &self.kind {
            ActorKind::Player { .. } => "minecraft:player",
            ActorKind::Entity { identifier } => identifier,
        }
    }
}

impl AnimatedActor for TestActor {
    fn runtime_id(&self) -> u64 {
        self.runtime_id
    }

    fn spawn_revision(&self) -> u64 {
        self.spawn_revision
    }

    fn kind(&self) -> &ActorKind {
        &self.kind
    }

    fn position(&self) -> [f32; 3] {
        self.position
    }

    fn previous_position(&self) -> [f32; 3] {
        self.previous_position
    }

    fn velocity(&self) -> [f32; 3] {
        self.velocity
    }

    fn native_velocity(&self) -> [f32; 3] {
        self.status.native_velocity
    }

    fn pitch(&self) -> f32 {
        self.pitch
    }

    fn yaw(&self) -> f32 {
        self.yaw
    }

    fn head_yaw(&self) -> f32 {
        self.head_yaw
    }

    fn body_yaw(&self) -> f32 {
        self.body_yaw
    }

    fn on_ground(&self) -> bool {
        self.on_ground.unwrap_or(false)
    }

    fn player_game_mode(&self) -> Option<GameModeUpdate> {
        self.player_game_mode
    }

    fn metadata(&self) -> &HashMap<u32, ActorMetadataValue> {
        &self.metadata
    }

    fn health(&self) -> Option<&ActorAttribute> {
        self.attributes.get("minecraft:health")
    }

    fn int_properties(&self) -> &HashMap<u32, i32> {
        &self.int_properties
    }

    fn float_properties(&self) -> &HashMap<u32, f32> {
        &self.float_properties
    }

    fn flag(&self, bit: u32) -> bool {
        let (key, bit) = if bit < 64 {
            (0, bit)
        } else {
            (EXTENDED_FLAGS_METADATA_KEY, bit - 64)
        };
        matches!(
            self.metadata.get(&key),
            Some(ActorMetadataValue::Flags(flags) | ActorMetadataValue::FlagsExtended(flags))
                if flags & (1_u64 << bit) != 0
        )
    }

    fn is_creeper(&self) -> bool {
        self.identifier() == "minecraft:creeper"
    }

    fn is_horse(&self) -> bool {
        matches!(
            self.identifier(),
            "minecraft:horse"
                | "minecraft:donkey"
                | "minecraft:mule"
                | "minecraft:zombie_horse"
                | "minecraft:skeleton_horse"
        )
    }

    fn is_on_fire(&self) -> bool {
        self.status.on_fire
    }

    fn is_sleeping(&self) -> bool {
        self.status.sleeping
    }

    fn is_using_item(&self) -> bool {
        self.flag(query::FLAG_USING_ITEM)
    }

    fn is_dead(&self) -> bool {
        self.status.dead
    }

    fn target_rotation_is_absolute(&self) -> bool {
        matches!(
            self.identifier(),
            "minecraft:arrow"
                | "minecraft:fireworks_rocket"
                | "minecraft:wither_skull"
                | "minecraft:wither_skull_dangerous"
        )
    }

    fn network_position_offset(&self) -> f32 {
        self.status.network_position_offset
    }

    fn render_scale(&self) -> f32 {
        match self.metadata.get(&SCALE_METADATA_KEY) {
            Some(ActorMetadataValue::Float(scale)) if scale.is_finite() && *scale >= 0.0 => *scale,
            _ => 1.0,
        }
    }

    fn creeper_swell_amount(&self, partial_tick: f32) -> f32 {
        let [previous, current] = self.status.creeper_swell;
        previous + (current - previous) * partial_tick
    }

    fn creeper_swell_changes(&self) -> bool {
        self.status.creeper_swell[0] != self.status.creeper_swell[1]
    }

    fn creeper_swelling_direction(&self) -> f32 {
        self.status.creeper_swelling_direction
    }

    fn swell_full_ticks(&self) -> f32 {
        28.0
    }

    fn overlay_alpha(&self) -> f32 {
        self.status.overlay_alpha
    }

    fn hurt_time(&self) -> u8 {
        self.status.hurt_time
    }

    fn shake_time(&self) -> i32 {
        self.status.shake_time
    }

    fn hurt_direction(&self) -> Option<f32> {
        self.status.hurt_direction
    }

    fn kinetic_hit_ticks(&self) -> Option<u32> {
        self.status.kinetic_hit_ticks
    }

    fn death_ticks(&self) -> u16 {
        self.status.death_ticks
    }

    fn sleep_rotation(&self) -> Option<f32> {
        self.status.sleep_rotation
    }

    fn fluid(&self) -> Option<(bool, bool)> {
        self.status.fluid
    }

    fn dragon_flap_phase(&self) -> Option<f32> {
        self.status.dragon_flap_phase
    }

    fn dragon_historical_frame(&self, offset: usize) -> Option<[f32; 2]> {
        self.status.dragon_history.get(offset).copied()
    }

    fn to_shared(&self) -> Arc<dyn AnimatedActor> {
        Arc::new(self.clone())
    }
}
