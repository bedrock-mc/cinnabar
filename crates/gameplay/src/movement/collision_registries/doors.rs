use assets::RegistryRecord;
use sim::{DoorFacing, DoorState};

/// Decodes paired-door properties from the same record that supplies the runtime ID.
pub(super) fn state(record: &RegistryRecord) -> Option<DoorState> {
    if !record.name.ends_with("_door") {
        return None;
    }
    let state: serde_json::Value = serde_json::from_str(&record.canonical_state).ok()?;
    let value = |name: &str| state.get(name)?.get("value");
    let facing = match value("minecraft:cardinal_direction").and_then(|v| v.as_str()) {
        Some("south") => DoorFacing::South,
        Some("west") => DoorFacing::West,
        Some("north") => DoorFacing::North,
        Some("east") => DoorFacing::East,
        _ => match value("direction")?.as_u64()? {
            0 => DoorFacing::South,
            1 => DoorFacing::West,
            2 => DoorFacing::North,
            3 => DoorFacing::East,
            _ => return None,
        },
    };
    Some(DoorState {
        family: record.name.clone().into(),
        facing,
        upper: value("upper_block_bit")?.as_i64()? != 0,
        open: value("open_bit")?.as_i64()? != 0,
        hinge_right: value("door_hinge_bit")?.as_i64()? != 0,
    })
}
