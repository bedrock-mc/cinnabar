use super::*;

#[test]
fn button_inventory_uses_the_unpressed_west_wall_plate() {
    let root = tempfile::tempdir().unwrap();
    let blocks = IconBlocks::read(root.path()).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for record in blocks
        .records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Button)
    {
        let visual = BlockVisualId(record.sequential_id);
        let pose = &blocks.records[blocks.icon_state(visual).0 as usize];
        let state: serde_json::Value = serde_json::from_str(&pose.canonical_state).unwrap();
        assert_eq!(pose.name, record.name);
        assert_eq!(state["facing_direction"]["value"], 5, "{}", record.name);
        assert_eq!(state["button_pressed_bit"]["value"], 0, "{}", record.name);
        seen.insert(record.name.as_ref());
    }
    assert!(seen.contains("minecraft:stone_button"));
    assert!(seen.len() >= 10);
}
