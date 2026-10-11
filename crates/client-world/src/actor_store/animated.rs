//! Supplies each tracked actor's authoritative state to the animation runtime.
use std::{collections::HashMap, sync::Arc};

use protocol::{ActorAttribute, ActorKind, ActorMetadataValue, GameModeUpdate};

use super::{ActorSnapshot, HURT_OVERLAY_ALPHA, creeper::SWELL_FULL_TICKS, dragon_animation};
use crate::actor_animation::AnimatedActor;

impl AnimatedActor for ActorSnapshot {
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
        self.previous_pose.position
    }

    fn velocity(&self) -> [f32; 3] {
        self.velocity
    }

    fn native_velocity(&self) -> [f32; 3] {
        ActorSnapshot::native_velocity(self)
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
        ActorSnapshot::flag(self, bit)
    }

    fn is_creeper(&self) -> bool {
        ActorSnapshot::is_creeper(self)
    }

    fn is_horse(&self) -> bool {
        ActorSnapshot::is_horse(self)
    }

    fn is_on_fire(&self) -> bool {
        ActorSnapshot::is_on_fire(self)
    }

    fn is_sleeping(&self) -> bool {
        ActorSnapshot::is_sleeping(self)
    }

    fn is_using_item(&self) -> bool {
        ActorSnapshot::is_using_item(self)
    }

    fn is_dead(&self) -> bool {
        self.status.dead
    }

    fn target_rotation_is_absolute(&self) -> bool {
        ActorSnapshot::target_rotation_is_absolute(self)
    }

    fn network_position_offset(&self) -> f32 {
        ActorSnapshot::network_position_offset(self)
    }

    fn render_scale(&self) -> f32 {
        ActorSnapshot::render_scale(self)
    }

    fn creeper_swell_amount(&self, partial_tick: f32) -> f32 {
        ActorSnapshot::creeper_swell_amount(self, partial_tick)
    }

    fn creeper_swell_changes(&self) -> bool {
        ActorSnapshot::creeper_swell_changes(self)
    }

    fn creeper_swelling_direction(&self) -> f32 {
        ActorSnapshot::creeper_swelling_direction(self)
    }

    fn swell_full_ticks(&self) -> f32 {
        SWELL_FULL_TICKS
    }

    fn overlay_alpha(&self) -> f32 {
        if self.hurt_overlay_active() {
            HURT_OVERLAY_ALPHA
        } else {
            0.0
        }
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
        self.status.death_ticks()
    }

    fn sleep_rotation(&self) -> Option<f32> {
        self.status.sleep_rotation
    }

    fn fluid(&self) -> Option<(bool, bool)> {
        self.status.fluid
    }

    fn dragon_flap_phase(&self) -> Option<f32> {
        self.dragon_animation.as_ref().map(|state| state.flap_phase)
    }

    fn dragon_historical_frame(&self, offset: usize) -> Option<[f32; 2]> {
        if offset >= dragon_animation::HISTORICAL_VARIABLES {
            return None;
        }
        let state = self.dragon_animation.as_ref()?;
        let dead = self.status.dead
            || self
                .attributes
                .get("minecraft:health")
                .is_some_and(|health| health.current <= 0.0);
        Some(state.historical_frame(offset, dead))
    }

    fn to_shared(&self) -> Arc<dyn AnimatedActor> {
        Arc::new(self.clone())
    }
}
