//! The actor facts animation reads, which actor authority supplies for each live actor.
use std::{collections::HashMap, sync::Arc};

use protocol::{ActorAttribute, ActorKind, ActorMetadataValue, GameModeUpdate};

/// Actors one store animates; actor authority tracks no more than this.
pub const MAX_ANIMATED_ACTORS: usize = 8_192;

/// One live actor's authoritative state, as animation queries and clocks read it.
pub trait AnimatedActor: Send + Sync + std::fmt::Debug {
    fn runtime_id(&self) -> u64;
    /// Distinguishes a respawned actor from the one that held its runtime id before.
    fn spawn_revision(&self) -> u64;
    fn kind(&self) -> &ActorKind;
    fn position(&self) -> [f32; 3];
    /// Position at the previous completed tick.
    fn previous_position(&self) -> [f32; 3];
    fn velocity(&self) -> [f32; 3];
    /// Vanilla velocity in blocks per tick, for tick-driven engine animation.
    fn native_velocity(&self) -> [f32; 3];
    fn pitch(&self) -> f32;
    fn yaw(&self) -> f32;
    fn head_yaw(&self) -> f32;
    fn body_yaw(&self) -> f32;
    /// Unknown ground contact reads as airborne.
    fn on_ground(&self) -> bool;
    fn player_game_mode(&self) -> Option<GameModeUpdate>;
    fn metadata(&self) -> &HashMap<u32, ActorMetadataValue>;
    fn health(&self) -> Option<&ActorAttribute>;
    fn int_properties(&self) -> &HashMap<u32, i32>;
    fn float_properties(&self) -> &HashMap<u32, f32>;
    /// One actor flag bit from the primary or overflow flag word.
    fn flag(&self, bit: u32) -> bool;
    fn is_creeper(&self) -> bool;
    fn is_horse(&self) -> bool;
    fn is_on_fire(&self) -> bool;
    fn is_sleeping(&self) -> bool;
    fn is_using_item(&self) -> bool;
    fn is_dead(&self) -> bool;
    /// Arrows and similar projectiles animate their full yaw through the target queries.
    fn target_rotation_is_absolute(&self) -> bool;
    /// Height the server adds above the feet position this actor reports.
    fn network_position_offset(&self) -> f32;
    fn render_scale(&self) -> f32;
    /// Creeper swell in `0..=1` at `partial_tick` between the last two ticks.
    fn creeper_swell_amount(&self, partial_tick: f32) -> f32;
    fn creeper_swell_changes(&self) -> bool;
    fn creeper_swelling_direction(&self) -> f32;
    /// Swell ticks at which a streamed swell reads as fully swollen.
    fn swell_full_ticks(&self) -> f32;
    /// Red damage overlay alpha; zero while no overlay shows.
    fn overlay_alpha(&self) -> f32;
    fn hurt_time(&self) -> u8;
    fn shake_time(&self) -> i32;
    fn hurt_direction(&self) -> Option<f32>;
    /// Completed ticks since a confirmed kinetic hit in the current item use.
    fn kinetic_hit_ticks(&self) -> Option<u32>;
    fn death_ticks(&self) -> u16;
    /// Bed orientation in degrees under a sleeping actor.
    fn sleep_rotation(&self) -> Option<f32>;
    /// Body `(water, lava)` contact; `None` before the first world sample.
    fn fluid(&self) -> Option<(bool, bool)>;
    /// The dragon's wing flap phase; `None` for every other actor.
    fn dragon_flap_phase(&self) -> Option<f32>;
    /// The dragon's `[yaw, height]` `offset` completed ticks ago, while that history is retained.
    fn dragon_historical_frame(&self, offset: usize) -> Option<[f32; 2]>;
    /// An owned copy that frame sampling reads after this tick's actor state has moved on.
    fn to_shared(&self) -> Arc<dyn AnimatedActor>;
}

/// Live actors by runtime id.
pub trait AnimatedActors: Sync {
    fn animated_actor(&self, runtime_id: u64) -> Option<&dyn AnimatedActor>;
}

impl<A: AnimatedActor> AnimatedActors for HashMap<u64, A> {
    fn animated_actor(&self, runtime_id: u64) -> Option<&dyn AnimatedActor> {
        self.get(&runtime_id)
            .map(|actor| actor as &dyn AnimatedActor)
    }
}
