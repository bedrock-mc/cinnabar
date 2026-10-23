use super::*;

#[test]
fn lightning_rod_inventory_uses_the_unpowered_upright_pose() {
    let root = tempfile::tempdir().unwrap();
    let blocks = IconBlocks::read(root.path()).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for record in blocks
        .records
        .iter()
        .filter(|record| pack_compile_input::lightning_rod::is_record(record))
    {
        let visual = BlockVisualId(record.sequential_id);
        let pose = &blocks.records[blocks.icon_state(visual).0 as usize];
        assert_eq!(pose.name, record.name);
        assert_eq!(
            pack_compile_input::lightning_rod::state(pose),
            Some((1, 0)),
            "{} {}",
            record.name,
            record.canonical_state
        );
        seen.insert(record.name.as_ref());
    }
    assert_eq!(seen.len(), 8);
}
