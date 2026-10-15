use super::*;

impl ActorStore {
    /// Reads retained painting candidates without scanning unrelated actors.
    pub(crate) fn painting_actors(&self) -> impl Iterator<Item = &ActorSnapshot> {
        self.effect_members
            .paintings
            .iter()
            .filter_map(|runtime| self.actors.get(runtime))
            .filter(|actor| actor.painting.is_some())
    }
    /// Retains hanging-picture data only on the lifetime this spawn actually admitted.
    pub(super) fn apply_painting_spawn(
        &mut self,
        sequence: u64,
        spawn: ActorSpawnEvent,
        painting: protocol::PaintingData,
    ) -> ActorApplyResult {
        let runtime = spawn.runtime_id;
        let result = self.apply_spawn(sequence, spawn);
        if let Some(actor) = self.actors.get_mut(&runtime)
            && actor.spawn_revision == sequence
        {
            if assets::painting::motive(&painting.motive).title != painting.motive.as_ref() {
                self.painting_motive_fallbacks += 1;
                if self.painting_motive_fallbacks.is_power_of_two() {
                    tracing::warn!(
                        count = self.painting_motive_fallbacks,
                        "unknown painting motives use the default artwork"
                    );
                }
            }
            actor.painting = Some(painting);
            self.effect_members.collidable.update(actor);
        }
        result
    }
}
