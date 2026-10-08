//! Live block facts and spawn wiring for vanilla fire smoke.

use assets::{BlockFace, BlockFlags};
use chunk_pipeline::WorldStream;
use particles::{
    ParticleSystem, SpawnRequest,
    ambient::{FIRE_SMOKE_EFFECT, emit_fire_smoke},
};

use super::AmbientParticles;
use crate::{movement::PhysicsCollisionRegistries, particles::world_adapter::StreamParticleWorld};

const NEIGHBORS: [(BlockFace, [i32; 3]); 6] = [
    (BlockFace::West, [-1, 0, 0]),
    (BlockFace::East, [1, 0, 0]),
    (BlockFace::Down, [0, -1, 0]),
    (BlockFace::Up, [0, 1, 0]),
    (BlockFace::North, [0, 0, -1]),
    (BlockFace::South, [0, 0, 1]),
];
impl AmbientParticles {
    pub(super) fn try_fire(
        &mut self,
        block: [i32; 3],
        stream: &WorldStream,
        world: &StreamParticleWorld<'_>,
        collisions: &PhysicsCollisionRegistries,
        system: &mut ParticleSystem,
    ) {
        let Some(runtime_id) = world.block_runtime_id(block) else {
            return;
        };
        let mode = stream.network_id_mode();
        if collisions.block_identifier(mode, runtime_id) != Some("minecraft:fire") {
            return;
        }
        let mut neighbors = [BlockFlags::empty(); BlockFace::ALL.len()];
        let mut below_is_campfire = false;
        for (face, offset) in NEIGHBORS {
            let Some(position) = offset_position(block, offset) else {
                continue;
            };
            let Some(id) = world.block_runtime_id(position) else {
                continue;
            };
            let neighbor = stream.runtime_assets().resolve(mode, id);
            if neighbor.is_known() {
                neighbors[face as usize] = neighbor.flags();
            }
            if face == BlockFace::Down {
                below_is_campfire = matches!(
                    collisions.block_identifier(mode, id),
                    Some("minecraft:campfire" | "minecraft:soul_campfire")
                );
            }
        }
        emit_fire_smoke(
            block,
            neighbors,
            below_is_campfire,
            &mut self.random,
            |position| {
                system.spawn(&SpawnRequest {
                    effect: FIRE_SMOKE_EFFECT.to_owned(),
                    position,
                    // The native callback supplies Vec3::ZERO. The pack's
                    // initial direction/speed produces the rising smoke itself.
                    inherit_velocity: Some([0.0; 3]),
                    manual_count: Some(1),
                    ..SpawnRequest::default()
                });
            },
        );
    }
}

fn offset_position(block: [i32; 3], offset: [i32; 3]) -> Option<[i32; 3]> {
    Some([
        block[0].checked_add(offset[0])?,
        block[1].checked_add(offset[1])?,
        block[2].checked_add(offset[2])?,
    ])
}

#[cfg(test)]
#[path = "fire_tests.rs"]
mod tests;
