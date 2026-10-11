use assets::RegistryRecord;
use assets::block_shapes::ShapeBox;

/// Supplies native scaffold support geometry; the movement context gates its solidity.
pub(super) fn shapes(record: &RegistryRecord) -> Option<Vec<ShapeBox>> {
    if record.name.as_ref() != "minecraft:scaffolding" {
        return None;
    }
    let state: serde_json::Value = serde_json::from_str(&record.canonical_state).ok()?;
    let stability = state.get("stability")?.get("value")?.as_u64()?;
    // Vanilla rejects stability 7; other states translate the unit cube.
    Some(if stability == 7 {
        Vec::new()
    } else {
        vec![ShapeBox::new([0.0; 3], [1.0; 3])]
    })
}

/// Compiles the scaffold platform and four posts, independently of movement admission.
pub(super) fn selection(record: &RegistryRecord) -> Option<Box<[ShapeBox]>> {
    if record.name.as_ref() != "minecraft:scaffolding" {
        return None;
    }
    Some(Box::new([
        ShapeBox::new([0.0, 0.8125, 0.0], [1.0, 0.9375, 1.0]),
        ShapeBox::new([0.0; 3], [0.125, 1.0, 0.125]),
        ShapeBox::new([0.0, 0.0, 0.875], [0.125, 1.0, 1.0]),
        ShapeBox::new([0.875, 0.0, 0.0], [1.0, 1.0, 0.125]),
        ShapeBox::new([0.875, 0.0, 0.875], [1.0; 3]),
    ]))
}
