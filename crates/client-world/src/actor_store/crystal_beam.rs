//! End crystal effects are driven by the target in actor metadata, outside the JSON rig.

use protocol::{ActorKind, ActorMetadataValue};

use super::ActorStore;

// EnderCrystal defines this BlockPos with the zero position as its sentinel.
const CRYSTAL_TARGET_METADATA_KEY: u32 = 47;

/// End crystal beam endpoints and animation age, in world blocks and game ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrystalBeamView {
    pub runtime_id: u64,
    pub target: [f32; 3],
    pub crystal: [f32; 3],
    pub age_ticks: f32,
}

impl ActorStore {
    /// Publishes the additional beam for crystals with a nonzero block target.
    pub(crate) fn crystal_beams(&self, partial_tick: f32) -> Vec<CrystalBeamView> {
        let alpha = partial_tick.clamp(0.0, 1.0);
        let mut beams = self.actors.values().filter_map(|actor| {
            if !matches!(&actor.kind, ActorKind::Entity { identifier } if identifier.as_ref() == "minecraft:ender_crystal") {
                return None;
            }
            let ActorMetadataValue::BlockPosition(target) = actor.metadata.get(&CRYSTAL_TARGET_METADATA_KEY)? else {
                return None;
            };
            if *target == [0; 3] {
                return None;
            }
            // Vanilla beam rendering offsets only the target Y by one
            // and ends at the interpolated actor origin.
            Some(CrystalBeamView {
                runtime_id: actor.runtime_id,
                target: [target[0] as f32, target[1] as f32 + 1.0, target[2] as f32],
                crystal: actor.interpolated_position(alpha)?,
                age_ticks: actor.status.age_ticks as f32 + alpha,
            })
        }).collect::<Vec<_>>();
        beams.sort_unstable_by_key(|beam| beam.runtime_id);
        beams
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use protocol::{ActorEvent, ActorMetadata};

    use super::*;

    /// Creates a streamed entity with the crystal's target metadata.
    fn spawn(identifier: &str, target: ActorMetadataValue) -> ActorEvent {
        let ActorEvent::Spawn(mut actor) = super::super::tests::spawn(42, 42) else {
            unreachable!();
        };
        actor.kind = ActorKind::Entity {
            identifier: identifier.into(),
        };
        actor.metadata = Arc::from([ActorMetadata {
            key: CRYSTAL_TARGET_METADATA_KEY,
            value: target,
        }]);
        ActorEvent::Spawn(actor)
    }

    #[test]
    fn target_metadata_publishes_the_beam_and_zero_or_wrong_type_suppresses_it() {
        for (identifier, target, present) in [
            (
                "minecraft:ender_crystal",
                ActorMetadataValue::BlockPosition([7, 80, -2]),
                true,
            ),
            (
                "minecraft:ender_crystal",
                ActorMetadataValue::BlockPosition([0; 3]),
                false,
            ),
            ("minecraft:ender_crystal", ActorMetadataValue::Int(7), false),
            (
                "minecraft:bee",
                ActorMetadataValue::BlockPosition([7, 80, -2]),
                false,
            ),
        ] {
            let mut store = ActorStore::new(1, 0);
            store.apply(1, 1, spawn(identifier, target));
            let beams = store.crystal_beams(0.5);
            assert_eq!(beams.len(), usize::from(present));
            if present {
                assert_eq!(beams[0].target, [7.0, 81.0, -2.0]);
                assert_eq!(beams[0].crystal, [1.0, 2.0, 3.0]);
                assert_eq!(beams[0].age_ticks, 0.5);
            }
        }
    }

    #[test]
    fn clearing_the_target_or_changing_dimension_removes_the_beam() {
        let mut store = ActorStore::new(1, 0);
        store.apply(
            1,
            1,
            spawn(
                "minecraft:ender_crystal",
                ActorMetadataValue::BlockPosition([7, 80, -2]),
            ),
        );
        assert_eq!(store.crystal_beams(0.0).len(), 1);
        store.apply(
            1,
            2,
            ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 42,
                metadata: Arc::from([ActorMetadata {
                    key: CRYSTAL_TARGET_METADATA_KEY,
                    value: ActorMetadataValue::BlockPosition([0; 3]),
                }]),
                properties: Arc::from([]),
                tick: 0,
            }),
        );
        assert!(store.crystal_beams(0.0).is_empty());
        store.apply(
            1,
            3,
            spawn(
                "minecraft:ender_crystal",
                ActorMetadataValue::BlockPosition([7, 80, -2]),
            ),
        );
        assert_eq!(store.crystal_beams(0.0).len(), 1);
        store.reset_dimension(1, 4, 1);
        assert!(store.crystal_beams(0.0).is_empty());
    }
}
