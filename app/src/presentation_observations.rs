//! Implements presentation's borrowed queries on the existing gameplay owners.

use client_presentation::observations::{
    CollisionLookup, ItemUseObservation, MiningObservation, ParticleAudioObservation,
    PhysicsObservation,
};

impl PhysicsObservation for crate::movement::LocalPhysicsController {
    /// Borrows the completed physics state at the presentation boundary.
    fn state(&self) -> Option<&sim::PlayerState> {
        std::ops::Deref::deref(self).state()
    }
    /// Borrows tick-owned sneak and sprint flags.
    fn latest_sneak_sprint(&self) -> Option<(bool, bool)> {
        self.latest_sneak_sprint()
    }
    /// Borrows the collision frontier used by the completed tick.
    fn last_world_identity(&self) -> Option<&sim::WorldCollisionIdentity> {
        std::ops::Deref::deref(self).last_world_identity()
    }
    /// Reports gameplay's current ownership of translation.
    fn is_active(&self) -> bool {
        std::ops::Deref::deref(self).is_active()
    }
}
impl CollisionLookup for crate::movement::PhysicsCollisionRegistries {
    /// Borrows the existing registry without duplicating its ownership.
    fn registry(&self, mode: assets::NetworkIdMode) -> &sim::CollisionRegistry {
        std::ops::Deref::deref(self).registry(mode)
    }
    /// Borrows the canonical state used by actor surface observations.
    fn block_canonical_state(&self, mode: assets::NetworkIdMode, runtime_id: u32) -> Option<&str> {
        std::ops::Deref::deref(self).block_canonical_state(mode, runtime_id)
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
impl ItemUseObservation for crate::item_use::ItemUseRuntime {
    /// Observes whether item use remains admitted.
    fn is_using(&self) -> bool {
        self.is_using()
    }
}
impl ParticleAudioObservation for crate::particles::ParticleInbox {
    /// Drains only audio's existing copy of actor status notices.
    fn take_status_audio(&mut self) -> Vec<client_world::ActorStatusNotice> {
        self.take_status_audio()
    }
    /// Drains only audio's existing copy of level events.
    fn take_level_audio(&mut self) -> Vec<(i32, [f32; 3], i32)> {
        self.take_level_audio()
    }
}
