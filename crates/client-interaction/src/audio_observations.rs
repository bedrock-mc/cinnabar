//! Borrows interaction authority for audio publication.
use client_audio::observations::{CollisionLookup, MiningObservation, PhysicsObservation};

impl PhysicsObservation for crate::movement::LocalPhysicsController {
    /// Borrows the latest completed simulation state.
    fn state(&self) -> Option<&sim::PlayerState> {
        std::ops::Deref::deref(self).state()
    }
    /// Forwards every completed tick so water entry survives frames with several physics ticks.
    fn visit_motion_ticks(
        &self,
        after: Option<u64>,
        visit: &mut dyn FnMut(u64, client_audio::audio::local::MotionSample),
    ) {
        self.visit_completed_ticks(after, &mut |sample, environment, entry_velocity| {
            let position = sample.position.map(f64::from);
            visit(
                sample.tick,
                client_audio::audio::local::MotionSample {
                    position: [
                        position[0],
                        position[1] - f64::from(protocol::PLAYER_NETWORK_OFFSET),
                        position[2],
                    ],
                    velocity_y: f64::from(sample.velocity[1]),
                    entry_velocity,
                    movement: sample.movement,
                    on_ground: sample.grounded_after_tick,
                    sneaking: sample.sneaking,
                    in_water: environment.in_water,
                },
            );
        });
    }
}
impl CollisionLookup for crate::movement::PhysicsCollisionRegistries {
    /// Borrows the existing registry without duplicating its ownership.
    fn registry(&self, mode: assets::NetworkIdMode) -> &sim::CollisionRegistry {
        std::ops::Deref::deref(self).registry(mode)
    }
    /// Resolves the existing block-name fact for presentation.
    fn block_identifier(&self, mode: assets::NetworkIdMode, runtime_id: u32) -> Option<&str> {
        std::ops::Deref::deref(self).block_identifier(mode, runtime_id)
    }
}
impl MiningObservation for crate::survival_mining::SurvivalMiningRuntime {
    /// Observes the admitted target without advancing mining.
    fn destroying_target(&self) -> Option<([i32; 3], u8)> {
        self.destroying_target()
    }
}
