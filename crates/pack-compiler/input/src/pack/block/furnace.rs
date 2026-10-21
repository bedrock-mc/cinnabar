//! Furnace world faces select the pack's side, front and top/bottom slots.

use assets::{BlockFace, RegistryRecord};
use serde::Deserialize;

/// Selects the furnace pair without changing other containers or custom blocks.
pub(super) fn is_record(record: &RegistryRecord) -> bool {
    matches!(
        record.name.as_ref(),
        "minecraft:furnace" | "minecraft:lit_furnace"
    )
}

/// Maps a furnace's world face to its authored texture slot, without rotating UVs.
pub(super) fn source_face(record: &RegistryRecord, face: BlockFace) -> Option<BlockFace> {
    #[derive(Deserialize)]
    struct State {
        #[serde(rename = "minecraft:cardinal_direction")]
        direction: TaggedDirection,
    }
    #[derive(Deserialize)]
    struct TaggedDirection {
        #[serde(rename = "type")]
        kind: Box<str>,
        value: Box<str>,
    }
    let state: State = serde_json::from_str(&record.canonical_state).ok()?;
    if state.direction.kind.as_ref() != "string" {
        return None;
    }
    let front = match state.direction.value.as_ref() {
        "south" => BlockFace::South,
        "west" => BlockFace::West,
        "north" => BlockFace::North,
        "east" => BlockFace::East,
        _ => return None,
    };
    Some(match face {
        BlockFace::Down | BlockFace::Up => face,
        _ if face != front => BlockFace::North,
        _ if record.name.as_ref() == "minecraft:lit_furnace" => BlockFace::East,
        _ => BlockFace::South,
    })
}
