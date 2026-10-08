use assets::RegistryRecord;
use sim::{Aabb, Vec3};

/// Supplies native scaffold support geometry; the movement context gates its solidity.
pub(super) fn shapes(record: &RegistryRecord) -> Option<Vec<Aabb>> {
    if record.name.as_ref() != "minecraft:scaffolding" {
        return None;
    }
    let state: serde_json::Value = serde_json::from_str(&record.canonical_state).ok()?;
    let stability = state.get("stability")?.get("value")?.as_u64()?;
    // Vanilla rejects stability 7; other states translate the unit cube.
    Some(if stability == 7 {
        Vec::new()
    } else {
        vec![Aabb::new(Vec3::ZERO, Vec3::ONE)]
    })
}
