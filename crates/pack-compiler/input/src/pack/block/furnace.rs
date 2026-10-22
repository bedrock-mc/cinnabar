//! Furnace world faces select the pack's side, front and top/bottom slots.

use assets::{
    BlockFace, RegistryRecord,
    facts::{BlockProperty, StateValue, pack::furnace_lit},
};

/// Selects furnace-family cubes without changing other containers or custom blocks.
pub(super) fn is_record(record: &RegistryRecord) -> bool {
    furnace_lit(&record.name).is_some()
}

/// Maps a furnace's world face to its authored texture slot, without rotating UVs.
pub(super) fn source_face(record: &RegistryRecord, face: BlockFace) -> Option<BlockFace> {
    let lit = furnace_lit(&record.name)?;
    let StateValue::String(direction) =
        record.property(BlockProperty::MinecraftCardinalDirection)?
    else {
        return None;
    };
    let front = match direction.as_ref() {
        "south" => BlockFace::South,
        "west" => BlockFace::West,
        "north" => BlockFace::North,
        "east" => BlockFace::East,
        _ => return None,
    };
    Some(match face {
        BlockFace::Down | BlockFace::Up => face,
        _ if face != front => BlockFace::North,
        _ if lit => BlockFace::East,
        _ => BlockFace::South,
    })
}
