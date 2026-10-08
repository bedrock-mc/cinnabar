//! Vanilla fire smoke origins; the loaded particle pack defines their art and motion.

use assets::{BlockFace, BlockFlags};

use super::AmbientRandom;

pub const FIRE_SMOKE_EFFECT: &str = "minecraft:basic_smoke_particle";
const SUPPORTED_SMOKE_COUNT: usize = 3;
const ATTACHED_SMOKE_COUNT: usize = 2;
const ATTACHMENT_INSET: f32 = 0.1;

const ATTACHMENT_ORDER: [BlockFace; 5] = [
    BlockFace::West,
    BlockFace::East,
    BlockFace::North,
    BlockFace::South,
    BlockFace::Up,
];

/// Emits smoke origins from effective neighbor flags indexed by `BlockFace`.
/// Consumes the sampler's shared random stream in vanilla coordinate order.
pub fn emit_fire_smoke(
    block: [i32; 3],
    neighbors: [BlockFlags; BlockFace::ALL.len()],
    below_is_campfire: bool,
    random: &mut AmbientRandom,
    mut emit: impl FnMut([f32; 3]),
) {
    let below = neighbors[BlockFace::Down as usize];
    let supported = below.intersects(BlockFlags::FIRE_TOP_SUPPORT | BlockFlags::FIRE_FLAMMABLE);
    // Valid side-attached fire also takes the ordinary three-smoke branch.
    // Campfires reject placement before testing other neighbor support.
    let may_place = !below_is_campfire
        && (below.contains(BlockFlags::FIRE_TOP_SUPPORT)
            || neighbors
                .iter()
                .any(|flags| flags.contains(BlockFlags::FIRE_FLAMMABLE)));
    let base = block.map(|value| value as f32);
    if supported || may_place {
        for _ in 0..SUPPORTED_SMOKE_COUNT {
            let x = random.unit();
            let y = random.unit();
            let z = random.unit();
            emit([base[0] + x, (y * 0.5 + base[1]) + 0.5, base[2] + z]);
        }
        return;
    }
    for face in ATTACHMENT_ORDER {
        if !neighbors[face as usize].contains(BlockFlags::FIRE_FLAMMABLE) {
            continue;
        }
        for _ in 0..ATTACHED_SMOKE_COUNT {
            // Native consumes its floats in X, Y, Z order for every origin.
            let [x, y, z] = [random.unit(), random.unit(), random.unit()];
            let position = match face {
                BlockFace::West => [base[0] + x * ATTACHMENT_INSET, base[1] + y, base[2] + z],
                BlockFace::East => [
                    block[0].wrapping_add(1) as f32 - x * ATTACHMENT_INSET,
                    base[1] + y,
                    base[2] + z,
                ],
                BlockFace::North => [base[0] + x, base[1] + y, base[2] + z * ATTACHMENT_INSET],
                BlockFace::South => [
                    base[0] + x,
                    base[1] + y,
                    block[2].wrapping_add(1) as f32 - z * ATTACHMENT_INSET,
                ],
                BlockFace::Up => [
                    base[0] + x,
                    block[1].wrapping_add(1) as f32 - y * ATTACHMENT_INSET,
                    base[2] + z,
                ],
                BlockFace::Down => unreachable!("down is not an attachment face"),
            };
            emit(position);
        }
    }
}

#[cfg(test)]
#[path = "fire_tests.rs"]
mod tests;
