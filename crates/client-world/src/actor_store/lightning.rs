use assets::facts::entity::NativeEntityClass;

use super::ActorStore;

/// A live lightning-bolt actor; `unique_id` is stable across frames and seeds its shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightningBoltView {
    pub unique_id: i64,
    pub position: [f32; 3],
    pub age_ticks: u32,
}

impl ActorStore {
    /// Every lightning-bolt actor, ordered by unique id.
    pub(crate) fn lightning_bolts(&self) -> Vec<LightningBoltView> {
        let mut bolts = self
            .actors
            .values()
            .filter(|actor| actor.entity_fact.native_class == NativeEntityClass::LightningBolt)
            .map(|actor| LightningBoltView {
                unique_id: actor.unique_id,
                position: actor.position,
                age_ticks: actor.status.age_ticks,
            })
            .collect::<Vec<_>>();
        bolts.sort_unstable_by_key(|bolt| bolt.unique_id);
        bolts
    }
}
