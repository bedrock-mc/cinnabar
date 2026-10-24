//! Cardinal-facing blocks map each world face to an authored texture slot.

use assets::{
    BlockFace, RegistryRecord,
    facts::{
        BlockProperty, StateValue,
        pack::{FaceSlots, face_slots},
    },
};

/// Selects built-in facing blocks without changing other containers or custom blocks.
pub(super) fn is_record(record: &RegistryRecord) -> bool {
    face_slots(&record.name).is_some()
}

/// Maps a facing block's world face to its authored texture slot, without rotating UVs.
pub(super) fn source_face(record: &RegistryRecord, face: BlockFace) -> Option<BlockFace> {
    let slots = face_slots(&record.name)?;
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
    Some(match (slots, face) {
        (FaceSlots::Pumpkin, BlockFace::Down | BlockFace::Up) => BlockFace::Up,
        (_, BlockFace::Down | BlockFace::Up) => face,
        _ if face != front => BlockFace::North,
        (FaceSlots::Furnace { lit: true }, _) => BlockFace::East,
        _ => BlockFace::South,
    })
}
