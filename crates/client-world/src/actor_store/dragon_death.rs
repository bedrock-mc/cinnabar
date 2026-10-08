//! Frame state for the dragon's death rays, separate from authored dissolve layers.

use protocol::ActorMetadataValue;

use super::{ActorStore, BOUNDING_BOX_HEIGHT_METADATA_KEY, dragon_particles::DRAGON_DEATH_TICKS};

const RAY_SEED: u32 = 432;
// The vanilla dragon definition supplies this height before metadata overrides it.
const DRAGON_COLLISION_HEIGHT: f32 = 4.0;

/// One dying dragon's interpolated body and the renderer component's retained parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragonDeathView {
    pub runtime_id: u64,
    pub owner_position: [f32; 3],
    pub center: [f32; 3],
    pub death_ticks: u16,
    pub partial_tick: f32,
    pub seed: u32,
    pub duration_ticks: f32,
}

impl ActorStore {
    pub(crate) fn dragon_death_rays(&self, partial_tick: f32) -> Vec<DragonDeathView> {
        if !partial_tick.is_finite() {
            return Vec::new();
        }
        let partial_tick = partial_tick.clamp(0.0, 1.0);
        let mut rays = self
            .actors
            .values()
            .filter_map(|actor| {
                if !actor.is_dying_dragon() || actor.status.death_ticks() < 2 {
                    return None;
                }
                let owner_position = actor.interpolated_position(partial_tick)?;
                let height = match actor.metadata.get(&BOUNDING_BOX_HEIGHT_METADATA_KEY) {
                    Some(ActorMetadataValue::Float(height))
                        if height.is_finite() && *height > 0.0 =>
                    {
                        *height
                    }
                    _ => DRAGON_COLLISION_HEIGHT,
                };
                let mut center = owner_position;
                center[1] += height * 0.5;
                center
                    .iter()
                    .all(|value| value.is_finite())
                    .then_some(DragonDeathView {
                        runtime_id: actor.runtime_id,
                        owner_position,
                        center,
                        death_ticks: actor.status.death_ticks(),
                        partial_tick,
                        seed: RAY_SEED,
                        duration_ticks: f32::from(DRAGON_DEATH_TICKS),
                    })
            })
            .collect::<Vec<_>>();
        rays.sort_unstable_by_key(|ray| ray.runtime_id);
        rays
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{ActorEvent, ActorKind, ActorMetadata, ActorStatusEvent, ActorStatusKind};
    use std::sync::Arc;

    fn spawn(identifier: &str, height: Option<f32>) -> ActorEvent {
        let ActorEvent::Spawn(mut actor) = super::super::tests::spawn(7, 7) else {
            unreachable!()
        };
        actor.kind = ActorKind::Entity {
            identifier: identifier.into(),
        };
        actor.metadata = height.map_or_else(
            || Arc::<[ActorMetadata]>::from([]),
            |height| {
                Arc::from([ActorMetadata {
                    key: BOUNDING_BOX_HEIGHT_METADATA_KEY,
                    value: ActorMetadataValue::Float(height),
                }])
            },
        );
        ActorEvent::Spawn(actor)
    }

    fn kill(store: &mut ActorStore) {
        store.apply(
            1,
            2,
            ActorEvent::Status(ActorStatusEvent {
                runtime_id: 7,
                kind: ActorStatusKind::Death,
                data: 0,
            }),
        );
    }

    #[test]
    fn ray_view_begins_after_the_death_event_and_tracks_lift_with_partial_pose() {
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, spawn("minecraft:ender_dragon", Some(10.0)));
        assert!(store.dragon_death_rays(0.5).is_empty());
        kill(&mut store);
        assert!(store.dragon_death_rays(0.5).is_empty());
        store.advance_interpolation_ticks(1);
        let ray = store.dragon_death_rays(0.5)[0];
        assert_eq!(ray.death_ticks, 2);
        assert!((ray.owner_position[1] - 2.05).abs() < 1e-5);
        assert!((ray.center[1] - 7.05).abs() < 1e-5);
        assert_eq!(ray.center[0], ray.owner_position[0]);
        assert_eq!(ray.center[2], ray.owner_position[2]);
        assert_eq!(ray.partial_tick, 0.5);
        assert_eq!(ray.duration_ticks, f32::from(DRAGON_DEATH_TICKS));
        assert_eq!(ray.seed, RAY_SEED);
        store.reset_dimension(1, 3, 2);
        assert!(store.dragon_death_rays(0.5).is_empty());
    }

    #[test]
    fn only_dragon_death_uses_the_effect_and_invalid_height_uses_its_definition() {
        let mut ordinary = ActorStore::new(1, 0);
        ordinary.apply(1, 1, spawn("minecraft:bee", Some(10.0)));
        kill(&mut ordinary);
        ordinary.advance_interpolation_ticks(2);
        assert!(ordinary.dragon_death_rays(0.5).is_empty());
        for height in [None, Some(f32::NAN), Some(-1.0)] {
            let mut dragon = ActorStore::new(1, 0);
            dragon.apply(1, 1, spawn("minecraft:ender_dragon", height));
            kill(&mut dragon);
            dragon.advance_interpolation_ticks(1);
            let ray = dragon.dragon_death_rays(0.0)[0];
            assert_eq!(
                ray.center[1] - ray.owner_position[1],
                DRAGON_COLLISION_HEIGHT * 0.5
            );
            assert!(dragon.dragon_death_rays(f32::NAN).is_empty());
        }
    }
}
