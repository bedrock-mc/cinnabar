//! Facing blocks map each world face to an authored slot by their native block class.

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
    if slots == FaceSlots::FaceDirectional {
        return Some(face_directional_slot(face, horizontal_facing(record)?));
    }
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

/// The six-way facing (down, up, north, south, west, east) of a horizontal `direction` block.
pub fn horizontal_facing(record: &RegistryRecord) -> Option<usize> {
    let StateValue::Int(direction) = record.property(BlockProperty::Direction)? else {
        return None;
    };
    // Directions run south, west, north, east.
    [3, 4, 2, 5].get(usize::try_from(*direction).ok()?).copied()
}

/// The authored slot a north-authored block draws on `face` when turned to six-way `facing`.
pub fn face_directional_slot(face: BlockFace, facing: usize) -> BlockFace {
    use BlockFace::{Down, East, North, South, Up, West};
    // Rows by facing; columns in world-face order west, east, down, up, north, south.
    const SLOTS: [[BlockFace; 6]; 6] = [
        [West, East, North, South, Up, Down],
        [West, East, South, North, Down, Up],
        BlockFace::ALL,
        [East, West, Down, Up, South, North],
        [North, South, Down, Up, East, West],
        [South, North, Down, Up, West, East],
    ];
    SLOTS[facing][face as usize]
}
