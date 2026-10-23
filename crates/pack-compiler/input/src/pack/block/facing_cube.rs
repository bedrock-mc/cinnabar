//! Horizontal face-directional cubes (the loom) turn every pack face toward their direction.

use assets::{
    BlockFace, RegistryRecord,
    facts::{BlockProperty, BlockRenderKind, StateValue, block_render_fact},
};

/// Horizontal facing of each `direction` value, in the registry's south, west, north, east order.
const DIRECTION_FACING: [BlockFace; 4] = [
    BlockFace::South,
    BlockFace::West,
    BlockFace::North,
    BlockFace::East,
];

/// Selects the face-directional cube route without changing other blocks.
pub(super) fn is_record(record: &RegistryRecord) -> bool {
    block_render_fact(&record.name).kind == BlockRenderKind::FacingCube
}

/// The pack face drawn on a facing cube's world `face` and that face's native UV flip
/// (1 and 2 are opposite quarter turns, 3 a half turn), or `None` for other blocks and
/// malformed directions.
pub fn facing_cube_face(record: &RegistryRecord, face: BlockFace) -> Option<(BlockFace, u8)> {
    if !is_record(record) {
        return None;
    }
    let &StateValue::Int(direction) = record.property(BlockProperty::Direction)? else {
        return None;
    };
    let facing = *DIRECTION_FACING.get(usize::try_from(direction).ok()?)?;
    Some(match (facing, face) {
        (_, BlockFace::Down | BlockFace::Up) => (face, vertical_flip(facing, face)),
        (BlockFace::South, side) => (opposite(side), 0),
        (BlockFace::West, BlockFace::North) => (BlockFace::East, 0),
        (BlockFace::West, BlockFace::South) => (BlockFace::West, 0),
        (BlockFace::West, BlockFace::West) => (BlockFace::North, 0),
        (BlockFace::West, BlockFace::East) => (BlockFace::South, 0),
        (BlockFace::East, BlockFace::North) => (BlockFace::West, 0),
        (BlockFace::East, BlockFace::South) => (BlockFace::East, 0),
        (BlockFace::East, BlockFace::West) => (BlockFace::South, 0),
        (BlockFace::East, BlockFace::East) => (BlockFace::North, 0),
        (_, side) => (side, 0),
    })
}

/// Native top and bottom flip for a horizontal facing; the north-facing top is a half turn.
fn vertical_flip(facing: BlockFace, face: BlockFace) -> u8 {
    match (facing, face) {
        (BlockFace::North, BlockFace::Up) | (BlockFace::South, BlockFace::Down) => 3,
        (BlockFace::West, _) => 1,
        (BlockFace::East, _) => 2,
        _ => 0,
    }
}

fn opposite(face: BlockFace) -> BlockFace {
    match face {
        BlockFace::West => BlockFace::East,
        BlockFace::East => BlockFace::West,
        BlockFace::Down => BlockFace::Up,
        BlockFace::Up => BlockFace::Down,
        BlockFace::North => BlockFace::South,
        BlockFace::South => BlockFace::North,
    }
}
