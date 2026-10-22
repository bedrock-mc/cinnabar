//! Implements presentation's borrowed queries on the existing gameplay owners.

use client_presentation::observations::{CollisionLookup, ItemUseObservation, PhysicsObservation};

impl PhysicsObservation for crate::movement::LocalPhysicsController {
    /// Borrows the completed physics state at the presentation boundary.
    fn state(&self) -> Option<&sim::PlayerState> {
        std::ops::Deref::deref(self).state()
    }
    /// Forwards every resolved tick, including frames containing several physics updates.
    fn visit_camera_ticks(
        &self,
        after: Option<u64>,
        visit: &mut dyn FnMut(client_presentation::observations::CameraMovementTick),
    ) {
        self.visit_completed_ticks(after, &mut |sample, _, _| {
            visit(client_presentation::observations::CameraMovementTick {
                tick: sample.tick,
                position: sample.position,
                movement: sample.movement,
                velocity: sample.velocity,
                on_ground: sample.grounded_after_tick,
                riding: sample.mode == sim::MovementMode::Riding,
                walks: sample.walks,
            });
        });
    }
    /// Borrows tick-owned sneak and sprint flags.
    fn latest_sneak_sprint(&self) -> Option<(bool, bool)> {
        self.latest_sneak_sprint()
    }
    fn mode(&self) -> sim::MovementMode {
        std::ops::Deref::deref(self).mode()
    }
    fn sneak_profile(&self) -> sim::SneakProfile {
        std::ops::Deref::deref(self).sneak_profile()
    }
    fn fall_fly_ticks(&self) -> u32 {
        std::ops::Deref::deref(self).fall_fly_ticks()
    }
    /// Borrows the collision frontier used by the completed tick.
    fn last_world_identity(&self) -> Option<&sim::WorldCollisionIdentity> {
        std::ops::Deref::deref(self).last_world_identity()
    }
    /// Reports gameplay's current ownership of translation.
    fn is_active(&self) -> bool {
        std::ops::Deref::deref(self).is_active()
    }
    fn tick_alpha(&self) -> f32 {
        std::ops::Deref::deref(self).tick_alpha()
    }
}
impl CollisionLookup for crate::movement::PhysicsCollisionRegistries {
    /// Borrows the existing registry without duplicating its ownership.
    fn registry(&self, mode: assets::NetworkIdMode) -> &sim::CollisionRegistry {
        std::ops::Deref::deref(self).registry(mode)
    }
    /// Borrows the heading compiled when this registry was admitted.
    fn bed_rotation_degrees(&self, mode: assets::NetworkIdMode, runtime_id: u32) -> Option<f32> {
        std::ops::Deref::deref(self).bed_rotation_degrees(mode, runtime_id)
    }
    /// Resolves the existing block-name fact for presentation.
    fn block_identifier(&self, mode: assets::NetworkIdMode, runtime_id: u32) -> Option<&str> {
        std::ops::Deref::deref(self).block_identifier(mode, runtime_id)
    }
}
impl ItemUseObservation for crate::item_use::ItemUseRuntime {
    /// Observes whether item use remains admitted.
    fn is_using(&self) -> bool {
        self.is_using()
    }
}
