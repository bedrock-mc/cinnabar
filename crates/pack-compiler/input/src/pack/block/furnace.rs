//! Furnace world faces select the pack's side, front and top/bottom slots.

use assets::{BlockFace, RegistryRecord, facts::pack::furnace_lit};
use serde::Deserialize;

/// Selects the furnace pair without changing other containers or custom blocks.
pub(super) fn is_record(record: &RegistryRecord) -> bool {
    furnace_lit(&record.name).is_some()
}

/// Maps a furnace's world face to its authored texture slot, without rotating UVs.
pub(super) fn source_face(record: &RegistryRecord, face: BlockFace) -> Option<BlockFace> {
    let lit = furnace_lit(&record.name)?;
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
        _ if lit => BlockFace::East,
        _ => BlockFace::South,
    })
}
