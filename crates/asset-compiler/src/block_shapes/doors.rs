use assets::RegistryRecord;
use assets::block_shapes::CompiledDoor;

/// Decodes paired-door properties from the same record that supplies the runtime ID.
pub(super) fn state(record: &RegistryRecord) -> Option<CompiledDoor> {
    if !record.name.ends_with("_door") {
        return None;
    }
    let state: serde_json::Value = serde_json::from_str(&record.canonical_state).ok()?;
    let value = |name: &str| state.get(name)?.get("value");
    let facing = match value("minecraft:cardinal_direction").and_then(|v| v.as_str()) {
        Some("south") => 0,
        Some("west") => 1,
        Some("north") => 2,
        Some("east") => 3,
        _ => match value("direction")?.as_u64()? {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => 3,
            _ => return None,
        },
    };
    Some(CompiledDoor {
        family: world::block_state_identity::state_identity(&record.name, []),
        facing,
        upper: value("upper_block_bit")?.as_i64()? != 0,
        open: value("open_bit")?.as_i64()? != 0,
        hinge_right: value("door_hinge_bit")?.as_i64()? != 0,
    })
}
